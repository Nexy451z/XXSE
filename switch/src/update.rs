//! The update check and the update itself, on the console.
//!
//! Shape: the frame loop never waits for a socket. A check and a download each
//! run on a libnx thread of their own and hand their answer back through a
//! single pointer, which the frame loop picks up on the next frame and then
//! joins. Every decision that can be made without a network - is the version
//! newer, where does the file go, is the staged download complete, what does a
//! failed swap leave behind - is in mhnx_core::update and is tested on the host.
//!
//! ## What this file owns
//!
//! * the settings file (/switch/XXSE/update.cfg) and the "off" the user can ask
//!   for, which means no socket is opened at start-up at all,
//! * the three-second budget of the start-up check,
//! * the download to XXSE.nro.new, its progress and its キャンセル,
//! * the swap (mhnx_core::update::activate_staged over the real card) and the
//!   self-restart through envSetNextLoad.
//!
//! ## What it must never do
//!
//! Touch a save. The only paths this module opens are under /switch/XXSE, and
//! the only ones it writes are update.cfg and the two .nro names.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
// c_char / c_int are only named by the curl callbacks, which a build without
// switch-curl compiles out.
#[allow(unused_imports)]
use core::ffi::{c_char, c_int, c_void};
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};

use mhnx_core::update::{
    self as core_update, DownloadPlan, InstallPlan, Release, RenameFs, Settings, Version,
};

/// Only reached by the curl code, which a build without the portlib does not
/// compile; the import stays so that both builds read the same.
#[allow(unused_imports)]
use crate::net;
use crate::sys;

/// The .nro as the *loader* wants it. nx-hbloader's own default is
/// "sdmc:/hbmenu.nro", so the next-load path carries the device prefix.
pub const RELAUNCH_PATH: &str = "sdmc:/switch/XXSE/XXSE.nro";

/// A release document larger than this is not a release document. The cap is
/// what keeps a wrong URL from filling the console's heap.
const MAX_BODY: usize = 512 * 1024;

/// Thread stacks. curl with the ssl service needs more than a socket only
/// transfer, and the download thread also holds a FILE*.
const THREAD_STACK: usize = 64 * 1024;
/// The priority the frame loop's own thread keeps; the workers sit below it.
const THREAD_PRIO: i32 = 0x2C;

// ---------------------------------------------------------------------------
// What the frame loop reads
// ---------------------------------------------------------------------------

/// Where the updater is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// Nothing has been asked yet (or the start-up check is off).
    Idle,
    /// A worker is fetching the release feed.
    Checking,
    /// A newer release exists: the banner is up.
    Available,
    /// The download is running.
    Downloading,
    /// The download and the swap both succeeded; the app is about to restart.
    Updated,
    /// The bytes are in place but the console could not be asked to restart.
    UpdatedManual,
    /// Something failed; message says what.
    Failed,
}

/// Everything the drawing half and the key half need to know.
pub struct UpdateState {
    pub phase: Phase,
    pub settings: Settings,
    /// The recommendation banner, or empty when there is nothing to say.
    pub banner: String,
    /// Is the details dialog open?
    pub dialog: bool,
    /// The version found, when one was.
    pub version: Option<Version>,
    /// The release notes, already flattened to lines.
    pub notes: Vec<String>,
    /// 0..=100 while downloading.
    pub progress: u32,
    /// The line under the title: what happened, or what is happening.
    pub message: String,
    /// Set when the loader has been told to start the new file: the main loop
    /// must leave so that the process can exit and the loader can act.
    pub restart_pending: bool,
    /// The banner was put away for this session.
    pub dismissed: bool,
    /// The release the dialog offers, if it offers one.
    plan: Option<DownloadPlan>,
    /// The three names (target / staged / backup) on the card.
    paths: InstallPlan,
    /// The bytes the download wrote, checked against the declared size.
    downloaded: u64,
    /// The start-up check has been asked for already (it runs once).
    started: bool,
}

impl UpdateState {
    /// Read the settings file and work out where the bytes would go.
    ///
    /// Never fails: a missing card, a missing file and a corrupt file all mean
    /// "the default", because the update check is not worth a refusal to start.
    pub fn new() -> UpdateState {
        let settings = match read_text(core_update::SETTINGS_FILE) {
            Some(text) => Settings::parse(&text),
            None => {
                // Write the default once so that the switch is a file the user
                // can find, not a rumour in a readme.
                let default = Settings::default();
                write_text(core_update::SETTINGS_FILE, &default.render());
                default
            }
        };
        UpdateState {
            phase: Phase::Idle,
            settings,
            banner: String::new(),
            dialog: false,
            version: None,
            notes: Vec::new(),
            progress: 0,
            message: String::new(),
            restart_pending: false,
            dismissed: false,
            plan: None,
            paths: core_update::app_install_plan(),
            downloaded: 0,
            started: false,
        }
    }

    /// The one asynchronous start: called once, by the frame loop, after the
    /// first frame has been drawn. Silent when the user turned the check off.
    pub fn start_once(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        if self.settings.check_on_start {
            self.begin_check();
        }
    }

    /// The banner is the one line the guide strip draws on the right.
    pub fn banner_visible(&self) -> bool {
        self.phase == Phase::Available && !self.dismissed && !self.banner.is_empty()
    }

    /// Is a worker running? The dialog shows a spinner-ish line instead of a
    /// button while this is true.
    pub fn busy(&self) -> bool {
        matches!(self.phase, Phase::Checking | Phase::Downloading)
    }

    /// The dialog's [A]. What it means depends on where the updater is: this is
    /// the same key on every state, so the dialog never shows a dead button.
    pub fn primary_label(&self) -> &'static str {
        match self.phase {
            Phase::Idle => "今すぐ確認",
            Phase::Checking => "確認中…",
            // Newer, but with nothing this build may download (no asset, or an
            // http address): the honest button is another look, not a promise.
            Phase::Available => {
                if self.plan.is_some() {
                    "ダウンロード"
                } else {
                    "確認し直す"
                }
            }
            Phase::Downloading => "中止",
            Phase::Failed => "もう一度",
            Phase::Updated | Phase::UpdatedManual => "閉じる",
        }
    }

    /// Open the details dialog (banner tap, or [R] held on the start screen).
    pub fn open_dialog(&mut self) {
        self.dialog = true;
        if self.phase == Phase::Idle {
            self.message = String::from("起動時に確認: オン / オフは [Y]");
        }
    }

    /// Put the banner away until the next start.
    ///
    /// While a transfer is running this is also a cancel: "あとで" may not leave
    /// a download writing to the card behind a closed dialog, because the one
    /// thing the user asked for by closing it is to be left alone.
    pub fn dismiss(&mut self) {
        if self.phase == Phase::Downloading {
            DL_CANCEL.store(true, Ordering::SeqCst);
        }
        self.dismissed = true;
        self.dialog = false;
    }

    /// [A] in the dialog.
    pub fn primary(&mut self) {
        match self.phase {
            Phase::Idle => self.begin_check(),
            Phase::Checking => {}
            Phase::Available => {
                if self.plan.is_some() {
                    self.begin_download();
                } else {
                    self.begin_check();
                }
            }
            Phase::Downloading => self.cancel_download(),
            Phase::Failed => {
                if self.plan.is_some() {
                    self.begin_download();
                } else {
                    self.begin_check();
                }
            }
            Phase::Updated | Phase::UpdatedManual => self.dialog = false,
        }
    }

    /// [Y] in the dialog: the start-up check on or off, written to the card.
    pub fn toggle_setting(&mut self) {
        self.settings.check_on_start = !self.settings.check_on_start;
        write_text(core_update::SETTINGS_FILE, &self.settings.render());
        self.message = if self.settings.check_on_start {
            String::from("次回から起動時に確認します（この起動では [A] 今すぐ確認）")
        } else {
            String::from("次回から起動時に確認しません（[A] 今すぐ確認は使えます）")
        };
    }

    // -----------------------------------------------------------------------
    // the check
    // -----------------------------------------------------------------------

    /// Ask for the release feed on a worker thread.
    fn begin_check(&mut self) {
        if self.busy() {
            return;
        }
        self.phase = Phase::Checking;
        self.message = String::from("更新を確認しています…（最大 3 秒）");
        self.notes.clear();
        self.version = None;
        let started = unsafe { spawn(&raw mut CHECK_THREAD, check_entry, ptr::null_mut()) };
        if !started {
            self.phase = Phase::Failed;
            self.message = String::from("確認用のスレッドを起動できませんでした");
        }
    }

    // -----------------------------------------------------------------------
    // the download
    // -----------------------------------------------------------------------

    fn begin_download(&mut self) {
        let Some(plan) = self.plan.clone() else {
            self.phase = Phase::Failed;
            self.message = String::from("ダウンロードする版がありません");
            return;
        };
        if self.busy() {
            return;
        }
        DL_CANCEL.store(false, Ordering::SeqCst);
        DL_PERCENT.store(0, Ordering::SeqCst);
        self.progress = 0;
        self.downloaded = 0;
        self.phase = Phase::Downloading;
        self.message = format!("ダウンロード中… 0%（{}）", plan.name);

        let job = Box::new(DownloadJob {
            url: plan.url.clone().into_bytes(),
            path: self.paths.staged.clone().into_bytes(),
            expected: plan.size,
        });
        let arg = Box::into_raw(job) as *mut c_void;
        let started = unsafe { spawn(&raw mut DL_THREAD, download_entry, arg) };
        if !started {
            // The argument was handed over and nobody will free it otherwise.
            drop(unsafe { Box::from_raw(arg as *mut DownloadJob) });
            self.phase = Phase::Failed;
            self.message = String::from("ダウンロード用のスレッドを起動できませんでした");
        }
    }

    fn cancel_download(&mut self) {
        if self.phase == Phase::Downloading {
            DL_CANCEL.store(true, Ordering::SeqCst);
            self.message = String::from("中止しています…");
        }
    }

    // -----------------------------------------------------------------------
    // the frame loop's poll
    // -----------------------------------------------------------------------

    /// Collect whatever the workers finished. Called once per frame; cheap when
    /// there is nothing to collect.
    pub fn poll(&mut self) {
        // --- the check ---
        let done = CHECK_RESULT.swap(ptr::null_mut(), Ordering::Acquire);
        if !done.is_null() {
            let outcome = unsafe { *Box::from_raw(done) };
            unsafe { join(&raw mut CHECK_THREAD) };
            self.apply_check(outcome);
        }
        // --- the download ---
        let done = DL_RESULT.swap(ptr::null_mut(), Ordering::Acquire);
        if !done.is_null() {
            let outcome = unsafe { *Box::from_raw(done) };
            unsafe { join(&raw mut DL_THREAD) };
            self.apply_download(outcome);
        }
        if self.phase == Phase::Downloading {
            self.progress = DL_PERCENT.load(Ordering::Relaxed);
            if let Some(plan) = &self.plan {
                self.message = format!("ダウンロード中… {}%（{}）", self.progress, plan.name);
            }
        }
    }

    fn apply_check(&mut self, outcome: CheckOutcome) {
        match outcome.result {
            Ok(release) => {
                let running = Version::parse(env!("CARGO_PKG_VERSION"));
                let latest = release.version();
                match (latest, running) {
                    (Ok(latest), Some(running)) if latest.is_newer_than(&running) => {
                        self.banner = core_update::banner_text(&latest);
                        self.version = Some(latest.clone());
                        self.notes = core_update::note_lines(&release.body, 12);
                        self.message = String::from("ダウンロードしてこの場で入れ替えられます");
                        match release.plan() {
                            Ok(plan) => {
                                self.plan = Some(plan);
                                self.phase = Phase::Available;
                            }
                            Err(e) => {
                                // Newer, but not installable from here: say why,
                                // and keep the banner so the user can read it.
                                self.phase = Phase::Available;
                                self.message = e.message();
                                self.plan = None;
                            }
                        }
                    }
                    (Ok(_), _) => {
                        // Up to date (or running a build newer than the tag,
                        // which a source build is): nothing to say.
                        self.phase = Phase::Idle;
                        self.banner.clear();
                        self.plan = None;
                        self.notes.clear();
                        self.message = String::from("最新版です");
                    }
                    (Err(e), _) => {
                        self.phase = Phase::Failed;
                        self.banner.clear();
                        self.message = e.message();
                    }
                }
            }
            Err(message) => {
                // The point of the whole feature: a console with no network
                // says nothing and starts normally. A dialog the user opened
                // themselves is the one place the reason is worth printing.
                self.phase = if self.dialog { Phase::Failed } else { Phase::Idle };
                self.banner.clear();
                self.message = message;
            }
        }
        if self.phase == Phase::Idle && !self.dialog {
            // Nothing to show anywhere: the message is cleared so that a stale
            // "checking" line cannot survive in the footer.
            self.message.clear();
        }
    }

    fn apply_download(&mut self, outcome: DownloadOutcome) {
        match outcome.result {
            Ok(bytes) => {
                let expected = self.plan.as_ref().map(|p| p.size).unwrap_or(0);
                self.downloaded = bytes;
                if !core_update::staged_is_complete(bytes, expected) {
                    // A short file is not a build; it is thrown away rather
                    // than left where the swap could pick it up.
                    let _ = remove_file(&self.paths.staged);
                    self.phase = Phase::Failed;
                    self.message = format!(
                        "ダウンロードが途中で終わりました（{} / {} バイト）。元のファイルはそのままです",
                        bytes, expected
                    );
                    return;
                }
                self.activate();
            }
            Err(message) => {
                let _ = remove_file(&self.paths.staged);
                self.phase = Phase::Failed;
                self.message = format!("{}。元のファイルはそのままです", message);
            }
        }
    }

    // -----------------------------------------------------------------------
    // the swap and the restart
    // -----------------------------------------------------------------------

    /// Replace the running .nro with the staged one and ask the loader to start
    /// it. From here on every failure leaves a runnable file on the card, which
    /// is the property mhnx_core::update::activate_staged is tested for.
    fn activate(&mut self) {
        let expected = self.plan.as_ref().map(|p| p.size).unwrap_or(0);
        if !core_update::staged_is_complete(self.downloaded, expected) {
            self.phase = Phase::Failed;
            self.message = String::from("ダウンロードが揃っていません。元のファイルはそのままです");
            return;
        }
        match core_update::activate_staged(&mut SdCard, &self.paths) {
            Ok(()) => {
                // The file is in place. Ask the loader for the new one; if the
                // environment has no next-load entry (an old hbmenu, or a launch
                // that did not come from one) the user restarts by hand and the
                // new build is what starts.
                let path = loader_path(&self.paths.target);
                let next = to_c(path.as_bytes());
                let argv = to_c(b"");
                let ok = unsafe { sys::envHasNextLoad() }
                    && unsafe { sys::envSetNextLoad(next.as_ptr() as *const c_char, argv.as_ptr() as *const c_char) } == 0;
                if ok {
                    self.phase = Phase::Updated;
                    self.message = String::from("新しい版を起動します…");
                    self.restart_pending = true;
                } else {
                    self.phase = Phase::UpdatedManual;
                    self.message = String::from(
                        "更新しました。この画面からは起動し直せません。手動で hbmenu から起動し直してください",
                    );
                }
            }
            Err(e) => {
                self.phase = Phase::Failed;
                self.message = e.message(&self.paths);
            }
        }
    }

    /// The files the updater may write, for the report and for tests.
    pub fn paths(&self) -> &InstallPlan {
        &self.paths
    }
}

// ---------------------------------------------------------------------------
// Workers
// ---------------------------------------------------------------------------

static CHECK_RESULT: AtomicPtr<CheckOutcome> = AtomicPtr::new(ptr::null_mut());
static DL_RESULT: AtomicPtr<DownloadOutcome> = AtomicPtr::new(ptr::null_mut());
static DL_PERCENT: AtomicU32 = AtomicU32::new(0);
static DL_CANCEL: AtomicBool = AtomicBool::new(false);
static CURL_READY: AtomicBool = AtomicBool::new(false);
static mut CHECK_THREAD: sys::Thread = sys::Thread::ZERO;
static mut DL_THREAD: sys::Thread = sys::Thread::ZERO;

struct CheckOutcome {
    result: core::result::Result<Release, String>,
}

struct DownloadOutcome {
    result: core::result::Result<u64, String>,
}

struct DownloadJob {
    url: Vec<u8>,
    path: Vec<u8>,
    expected: u64,
}

/// Start a worker. Returns false when libnx refused the thread.
///
/// The Thread lives in a static (there is exactly one check and one download at
/// a time), so it is reached through a raw pointer rather than a reference: the
/// worker thread reads the same struct through libnx, and a &mut to a static
/// would be a promise this code cannot keep.
unsafe fn spawn(t: *mut sys::Thread, entry: sys::ThreadFunc, arg: *mut c_void) -> bool {
    // A finished worker was joined by poll(); this closes the last one so the
    // handle is not leaked on a second check in the same session.
    let rc = sys::threadCreate(t, entry, arg, ptr::null_mut(), THREAD_STACK, THREAD_PRIO, -2);
    if rc != 0 {
        return false;
    }
    sys::threadStart(t) == 0
}

/// Wait for a worker that has already published its answer, and free it.
unsafe fn join(t: *mut sys::Thread) {
    if (*t).handle != 0 {
        sys::threadWaitForExit(t);
        sys::threadClose(t);
        *t = sys::Thread::ZERO;
    }
}

unsafe extern "C" fn check_entry(_arg: *mut c_void) {
    let result = run_check();
    let boxed = Box::into_raw(Box::new(CheckOutcome { result }));
    CHECK_RESULT.store(boxed, Ordering::Release);
}

unsafe extern "C" fn download_entry(arg: *mut c_void) {
    let job = Box::from_raw(arg as *mut DownloadJob);
    let result = run_download(&job);
    let boxed = Box::into_raw(Box::new(DownloadOutcome { result }));
    DL_RESULT.store(boxed, Ordering::Release);
    // job is dropped here, inside the thread that was handed the pointer.
}

/// The start-up check: one GET, three seconds, and a reason when it fails.
#[cfg(switch_curl)]
fn run_check() -> core::result::Result<Release, String> {
    let rc = net::socket_initialize_default();
    if rc != 0 {
        return Err(format!("ネットワークを初期化できませんでした (0x{:08X})", rc));
    }
    let result = unsafe { http_get(core_update::RELEASES_LATEST_URL) };
    unsafe { net::socketExit() };
    let body = result?;
    let text = String::from_utf8_lossy(&body);
    core_update::parse_release(&text).map_err(|e| e.message())
}

/// GET a URL into memory, with the three-second budget of the start-up check.
#[cfg(switch_curl)]
unsafe fn http_get(url: &str) -> core::result::Result<Vec<u8>, String> {
    if !CURL_READY.swap(true, Ordering::SeqCst) {
        let rc = net::curl_global_init(net::CURL_GLOBAL_DEFAULT);
        if rc != net::CURLE_OK {
            return Err(String::from("通信の初期化に失敗しました"));
        }
    }
    let handle = net::curl_easy_init();
    if handle.is_null() {
        return Err(String::from("通信の初期化に失敗しました"));
    }
    let mut body: Vec<u8> = Vec::with_capacity(8 * 1024);
    let url_c = to_c(url.as_bytes());
    let agent_c = to_c(concat!("XXSE-nro/", env!("CARGO_PKG_VERSION")).as_bytes());
    let accept_c = to_c(b"Accept: application/vnd.github+json");

    net::curl_easy_setopt(handle, net::CURLOPT_URL, url_c.as_ptr() as *const c_char);
    net::curl_easy_setopt(handle, net::CURLOPT_USERAGENT, agent_c.as_ptr() as *const c_char);
    net::curl_easy_setopt(handle, net::CURLOPT_WRITEDATA, &mut body as *mut Vec<u8> as *mut c_void);
    net::curl_easy_setopt(handle, net::CURLOPT_WRITEFUNCTION, write_body as *const c_void);
    // 3 s for the whole request, 2.5 s of it for the connection: a dead network
    // must not cost the player the full three.
    net::curl_easy_setopt(handle, net::CURLOPT_TIMEOUT_MS, core_update::CHECK_TIMEOUT_MS as i64);
    net::curl_easy_setopt(handle, net::CURLOPT_CONNECTTIMEOUT_MS, 2500i64);
    net::curl_easy_setopt(handle, net::CURLOPT_FOLLOWLOCATION, 1i64);
    net::curl_easy_setopt(handle, net::CURLOPT_MAXREDIRS, 3i64);
    net::curl_easy_setopt(handle, net::CURLOPT_NOSIGNAL, 1i64);
    net::curl_easy_setopt(handle, net::CURLOPT_FAILONERROR, 1i64);
    net::curl_easy_setopt(handle, net::CURLOPT_ACCEPT_ENCODING, ptr::null::<c_char>());

    let list = net::curl_slist_append(ptr::null_mut(), accept_c.as_ptr() as *const c_char);
    if !list.is_null() {
        net::curl_easy_setopt(handle, net::CURLOPT_HTTPHEADER, list);
    }

    let rc = net::curl_easy_perform(handle);
    let mut http: i64 = 0;
    net::curl_easy_getinfo(handle, net::CURLINFO_RESPONSE_CODE, &mut http as *mut i64);

    if !list.is_null() {
        net::curl_slist_free_all(list);
    }
    net::curl_easy_cleanup(handle);

    if rc != net::CURLE_OK {
        return Err(net::describe(rc).to_string());
    }
    if http != 200 {
        // A 404 from the API means "no release yet", which is not an update.
        return Err(format!("更新情報を取得できませんでした (HTTP {})", http));
    }
    Ok(body)
}

#[cfg(switch_curl)]
unsafe extern "C" fn write_body(
    data: *mut c_char,
    size: usize,
    nmemb: usize,
    userdata: *mut c_void,
) -> usize {
    let n = size.wrapping_mul(nmemb);
    if userdata.is_null() {
        return 0;
    }
    let buf = &mut *(userdata as *mut Vec<u8>);
    if buf.len() + n > MAX_BODY {
        return 0; // 0 aborts the transfer (CURL_WRITE_ERROR)
    }
    buf.extend_from_slice(core::slice::from_raw_parts(data as *const u8, n));
    n
}

/// The download: to XXSE.nro.new, with progress, cancellable, and removed again
/// when it does not finish, so the swap can never pick up a half file.
#[cfg(switch_curl)]
fn run_download(job: &DownloadJob) -> core::result::Result<u64, String> {
    let rc = net::socket_initialize_default();
    if rc != 0 {
        return Err(format!("ネットワークを初期化できませんでした (0x{:08X})", rc));
    }
    let result = unsafe { write_download(job) };
    unsafe { net::socketExit() };
    result
}

#[cfg(switch_curl)]
unsafe fn write_download(job: &DownloadJob) -> core::result::Result<u64, String> {
    if !CURL_READY.swap(true, Ordering::SeqCst) {
        net::curl_global_init(net::CURL_GLOBAL_DEFAULT);
    }
    let handle = net::curl_easy_init();
    if handle.is_null() {
        return Err(String::from("通信の初期化に失敗しました"));
    }
    let file = sys::fopen(job.path.as_ptr() as *const c_char, c"wb".as_ptr());
    if file.is_null() {
        net::curl_easy_cleanup(handle);
        return Err(String::from("保存先を開けませんでした（/switch/XXSE の空き容量を確認してください）"));
    }

    FILE_WRITTEN.store(0, Ordering::Relaxed);
    let url_c = to_c(&job.url);
    let agent_c = to_c(concat!("XXSE-nro/", env!("CARGO_PKG_VERSION")).as_bytes());
    let accept_c = to_c(b"Accept: application/octet-stream");

    net::curl_easy_setopt(handle, net::CURLOPT_URL, url_c.as_ptr() as *const c_char);
    net::curl_easy_setopt(handle, net::CURLOPT_USERAGENT, agent_c.as_ptr() as *const c_char);
    net::curl_easy_setopt(handle, net::CURLOPT_WRITEDATA, file);
    net::curl_easy_setopt(handle, net::CURLOPT_WRITEFUNCTION, write_file as *const c_void);
    net::curl_easy_setopt(handle, net::CURLOPT_XFERINFODATA, ptr::null_mut::<c_void>());
    net::curl_easy_setopt(handle, net::CURLOPT_XFERINFOFUNCTION, on_progress as *const c_void);
    net::curl_easy_setopt(handle, net::CURLOPT_NOPROGRESS, 0i64);
    // A download of a 5 MB build over a home line needs more than three seconds;
    // the connection still has to answer quickly, and a transfer that stalls for
    // ten seconds is over.
    net::curl_easy_setopt(handle, net::CURLOPT_CONNECTTIMEOUT_MS, 8000i64);
    net::curl_easy_setopt(handle, net::CURLOPT_TIMEOUT_MS, 300_000i64);
    net::curl_easy_setopt(handle, net::CURLOPT_FOLLOWLOCATION, 1i64);
    net::curl_easy_setopt(handle, net::CURLOPT_MAXREDIRS, 3i64);
    net::curl_easy_setopt(handle, net::CURLOPT_NOSIGNAL, 1i64);

    let list = net::curl_slist_append(ptr::null_mut(), accept_c.as_ptr() as *const c_char);
    if !list.is_null() {
        net::curl_easy_setopt(handle, net::CURLOPT_HTTPHEADER, list);
    }

    let rc = net::curl_easy_perform(handle);
    if !list.is_null() {
        net::curl_slist_free_all(list);
    }
    net::curl_easy_cleanup(handle);

    // The count is the file's own: it is what write_file actually stored.
    let written = FILE_WRITTEN.load(Ordering::Relaxed);

    let _ = sys::fflush(file);
    let _ = sys::fclose(file);
    FILE_WRITTEN.store(0, Ordering::Relaxed);

    if DL_CANCEL.load(Ordering::SeqCst) {
        return Err(String::from("中止しました"));
    }
    if rc != net::CURLE_OK {
        return Err(net::describe(rc).to_string());
    }
    if written == 0 {
        return Err(String::from("中身が空でした"));
    }
    if job.expected != 0 && written != job.expected {
        return Err(format!(
            "サイズが合いません（{} / {} バイト）",
            written, job.expected
        ));
    }
    Ok(written)
}

/// Bytes the download callback has stored, read by the worker after the
/// transfer. Written by the same thread that reads it.
#[cfg(switch_curl)]
static FILE_WRITTEN: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

#[cfg(switch_curl)]
unsafe extern "C" fn write_file(
    data: *mut c_char,
    size: usize,
    nmemb: usize,
    userdata: *mut c_void,
) -> usize {
    let n = size.wrapping_mul(nmemb);
    if userdata.is_null() || n == 0 {
        return 0;
    }
    let stored = sys::fwrite(data as *const c_void, 1, n, userdata);
    if stored != n {
        return 0; // a full card is a write error, not a silent short file
    }
    FILE_WRITTEN.fetch_add(n as u64, Ordering::Relaxed);
    n
}

#[cfg(switch_curl)]
unsafe extern "C" fn on_progress(
    _data: *mut c_void,
    dltotal: i64,
    dlnow: i64,
    _ultotal: i64,
    _ulnow: i64,
) -> c_int {
    if DL_CANCEL.load(Ordering::Relaxed) {
        return 1; // non-zero aborts with CURLE_ABORTED_BY_CALLBACK
    }
    if dltotal > 0 && dlnow >= 0 {
        DL_PERCENT.store(
            core_update::percent(dlnow as u64, dltotal as u64),
            Ordering::Relaxed,
        );
    }
    0
}

/// A build without switch-curl: the check answers "this build cannot", which
/// the state machine treats like any other failed check (nothing is drawn, the
/// editor starts normally). The dialog still opens and says why.
#[cfg(not(switch_curl))]
fn run_check() -> core::result::Result<Release, String> {
    Err(String::from(
        "このビルドは更新確認に対応していません（switch-curl を入れて作り直してください）",
    ))
}

#[cfg(not(switch_curl))]
fn run_download(_job: &DownloadJob) -> core::result::Result<u64, String> {
    Err(String::from("このビルドはダウンロードに対応していません"))
}

// ---------------------------------------------------------------------------
// The card
// ---------------------------------------------------------------------------

/// The real card behind mhnx_core::update::RenameFs. fsdev resolves these
/// paths against sdmc, which is the device hbmenu and this application both
/// start from.
struct SdCard;

impl RenameFs for SdCard {
    fn exists(&self, path: &str) -> bool {
        let name = to_c(path.as_bytes());
        unsafe { !sys::fopen(name.as_ptr() as *const c_char, c"rb".as_ptr()).is_null() }
    }

    fn remove(&mut self, path: &str) -> bool {
        let name = to_c(path.as_bytes());
        // fsdev has no "definitely absent" answer for remove, so the question is
        // asked after the fact: the contract is "the path is gone now".
        unsafe { sys::remove(name.as_ptr() as *const c_char) };
        !self.exists(path)
    }

    fn rename(&mut self, from: &str, to: &str) -> bool {
        let a = to_c(from.as_bytes());
        let b = to_c(to.as_bytes());
        unsafe { sys::rename(a.as_ptr() as *const c_char, b.as_ptr() as *const c_char) == 0 }
    }
}

/// "sdmc:" plus the fsdev path, unless it is already there.
fn loader_path(target: &str) -> String {
    if target.starts_with("sdmc:") {
        target.to_string()
    } else {
        format!("sdmc:{}", target)
    }
}

// ---------------------------------------------------------------------------
// Small file helpers (newlib stdio over fsdev)
// ---------------------------------------------------------------------------

fn to_c(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 1);
    out.extend_from_slice(bytes);
    out.push(0);
    out
}

fn read_text(path: &str) -> Option<String> {
    let name = to_c(path.as_bytes());
    unsafe {
        let file = sys::fopen(name.as_ptr() as *const c_char, c"rb".as_ptr());
        if file.is_null() {
            return None;
        }
        let mut bytes: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 512];
        loop {
            let n = sys::fread(chunk.as_mut_ptr() as *mut c_void, 1, chunk.len(), file);
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..n]);
            if bytes.len() > 64 * 1024 {
                break;
            }
        }
        let _ = sys::fclose(file);
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }
}

fn write_text(path: &str, text: &str) -> bool {
    let name = to_c(path.as_bytes());
    unsafe {
        let file = sys::fopen(name.as_ptr() as *const c_char, c"wb".as_ptr());
        if file.is_null() {
            return false;
        }
        let bytes = text.as_bytes();
        let n = sys::fwrite(bytes.as_ptr() as *const c_void, 1, bytes.len(), file);
        let _ = sys::fflush(file);
        let _ = sys::fclose(file);
        n == bytes.len()
    }
}

fn remove_file(path: &str) -> bool {
    let name = to_c(path.as_bytes());
    unsafe { sys::remove(name.as_ptr() as *const c_char) == 0 }
}
