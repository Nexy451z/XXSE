//! The update check: everything about it that does not need a network.
//!
//! On the console this module answers three questions and nothing else:
//!
//!   * is the release GitHub answers with newer than the one running,
//!   * where do the new bytes go so that a failure cannot hurt the old ones,
//!   * what the banner and the dialog say.
//!
//! The socket, the TLS and the file I/O live in the caller (switch/src/net.rs,
//! switch/src/update.rs), which is what lets every rule here be tested on the
//! host with no console, no card and no wifi - including the swap that replaces
//! the running .nro, driven through the RenameFs trait.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::json::{self, Json};

// ---------------------------------------------------------------------------
// Where the answers come from, and where the bytes go
// ---------------------------------------------------------------------------

/// The release feed. The repository is the one the .nro is built from.
pub const RELEASES_LATEST_URL: &str = "https://api.github.com/repos/Nexy451z/XXSE/releases/latest";

/// The asset the console looks for in the release.
pub const ASSET_NAME: &str = "XXSE.nro";

/// The card folder the application lives in (fsdev path, i.e. "sdmc:" implied).
pub const APP_DIR: &str = "/switch/XXSE";

/// The name of the running application inside APP_DIR.
pub const NRO_NAME: &str = "XXSE.nro";

/// The settings file the optional start-up check reads.
pub const SETTINGS_FILE: &str = "/switch/XXSE/update.cfg";

/// How long the start-up check may take before it is abandoned. Short on
/// purpose: a missing network must cost the player three seconds, not thirty.
pub const CHECK_TIMEOUT_MS: u32 = 3000;

/// The first words of the banner's one line.
pub const BANNER_PREFIX: &str = "新しいバージョン";

// ---------------------------------------------------------------------------
// Version
// ---------------------------------------------------------------------------

/// A release version: major.minor.patch, with an optional pre-release tail.
///
/// Written here rather than taken from a crate because the comparison has to be
/// the semver one (a pre-release is OLDER than its release) and the crate would
/// have to be no_std as well.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// The part after '-' (alpha, beta.2, rc.1 ...), empty when there is none.
    pub pre: String,
}

impl Version {
    /// Read "v0.3.1", "0.3.1", "0.3.1-alpha" or "0.3.1-rc.2+build".
    ///
    /// One or two components are accepted and the missing ones are zero, so a
    /// tag of "v0.4" still compares the way a reader expects. Build metadata
    /// after '+' is ignored: semver says it takes no part in precedence.
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.trim();
        let text = text
            .strip_prefix('v')
            .or_else(|| text.strip_prefix('V'))
            .unwrap_or(text);
        if text.is_empty() {
            return None;
        }
        // Build metadata is not part of precedence.
        let text = match text.find('+') {
            Some(i) => &text[..i],
            None => text,
        };
        let (core, pre) = match text.find('-') {
            Some(i) => {
                let tail = &text[i + 1..];
                // A trailing '-' is a truncated version, not "no pre-release".
                if tail.is_empty() {
                    return None;
                }
                (&text[..i], tail)
            }
            None => (text, ""),
        };
        let mut parts = core.split('.');
        let major = parse_component(parts.next()?)?;
        let minor = match parts.next() {
            Some(p) => parse_component(p)?,
            None => 0,
        };
        let patch = match parts.next() {
            Some(p) => parse_component(p)?,
            None => 0,
        };
        // A fourth component means this is not a version this scheme covers
        // (a date-stamped tag, say): refuse rather than compare half of it.
        if parts.next().is_some() {
            return None;
        }
        Some(Version { major, minor, patch, pre: pre.to_string() })
    }

    /// Is this version newer than the one the console is running?
    ///
    /// Strictly newer: an equal version is not an update, and a pre-release of
    /// the same numbers is older than its release.
    pub fn is_newer_than(&self, running: &Version) -> bool {
        self.compare(running) == core::cmp::Ordering::Greater
    }

    /// The semver precedence order.
    pub fn compare(&self, other: &Version) -> core::cmp::Ordering {
        use core::cmp::Ordering;
        let numbers =
            (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch));
        if numbers != Ordering::Equal {
            return numbers;
        }
        match (self.pre.is_empty(), other.pre.is_empty()) {
            // A release is newer than any pre-release of the same numbers.
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => compare_pre(&self.pre, &other.pre),
        }
    }
}

fn parse_component(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut value: u64 = 0;
    for b in text.bytes() {
        value = value.checked_mul(10)?.checked_add((b - b'0') as u64)?;
    }
    Some(value)
}

/// Compare two pre-release tails, dot by dot, the way semver 11.4 says.
fn compare_pre(a: &str, b: &str) -> core::cmp::Ordering {
    use core::cmp::Ordering;
    let mut ai = a.split('.');
    let mut bi = b.split('.');
    loop {
        match (ai.next(), bi.next()) {
            (None, None) => return Ordering::Equal,
            // A shorter tail is lower: 1.0.0-alpha < 1.0.0-alpha.1.
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                let xn = !x.is_empty() && x.bytes().all(|c| c.is_ascii_digit());
                let yn = !y.is_empty() && y.bytes().all(|c| c.is_ascii_digit());
                let ord = match (xn, yn) {
                    (true, true) => {
                        let xv = parse_component(x).unwrap_or(0);
                        let yv = parse_component(y).unwrap_or(0);
                        xv.cmp(&yv)
                    }
                    // Numeric identifiers are lower than alphanumeric ones.
                    (true, false) => Ordering::Less,
                    (false, true) => Ordering::Greater,
                    (false, false) => x.cmp(y),
                };
                if ord != Ordering::Equal {
                    return ord;
                }
            }
        }
    }
}

impl core::fmt::Display for Version {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if !self.pre.is_empty() {
            write!(f, "-{}", self.pre)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// The release document
// ---------------------------------------------------------------------------

/// One downloadable file of a release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

/// The parts of GitHub's release answer the updater reads.
///
/// The fields the console never shows are not carried: a document with no tag
/// is refused rather than half used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// The tag as published, e.g. "v0.3.1".
    pub tag: String,
    /// The release title, which is what the dialog shows first.
    pub name: String,
    /// The browser page for the release.
    pub page_url: String,
    /// The release notes, markdown as GitHub stores them.
    pub body: String,
    pub prerelease: bool,
    pub assets: Vec<Asset>,
}

/// Why a release could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateError {
    /// The answer was not JSON at all (a captive portal, an HTML error page).
    Json,
    /// It was JSON but not a release this updater understands.
    Missing(&'static str),
    /// The release carries no .nro to download.
    NoAsset,
    /// The tag is not a version.
    BadVersion(String),
    /// The URL was not https, which is the only scheme this updater follows.
    InsecureUrl,
}

impl UpdateError {
    /// The sentence the dialog shows. Short: it is a line on a dialog.
    pub fn message(&self) -> String {
        match self {
            UpdateError::Json => String::from("更新情報を読めませんでした（応答が JSON ではありません）"),
            UpdateError::Missing(what) => format!("更新情報に {} がありません", what),
            UpdateError::NoAsset => format!("配布ファイル（{}）が見つかりません", ASSET_NAME),
            UpdateError::BadVersion(v) => format!("版番号を読めません: {}", v),
            UpdateError::InsecureUrl => String::from("配布先が https ではありません"),
        }
    }
}

impl core::fmt::Display for UpdateError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.message())
    }
}

/// Parse GitHub's "latest release" answer.
pub fn parse_release(text: &str) -> Result<Release, UpdateError> {
    let doc = json::parse(text).map_err(|_| UpdateError::Json)?;
    release_from_json(&doc)
}

/// The same, from an already parsed document (so a caller can cache the tree).
pub fn release_from_json(doc: &Json) -> Result<Release, UpdateError> {
    let tag = doc.str_at("tag_name");
    if tag.is_empty() {
        return Err(UpdateError::Missing("tag_name"));
    }
    let mut assets = Vec::new();
    if let Some(items) = doc.get("assets").and_then(Json::as_array) {
        for item in items {
            let name = item.str_at("name");
            if name.is_empty() {
                continue;
            }
            // GitHub calls it browser_download_url. A document that names the
            // asset but not its address is not usable for that asset.
            let url = item.str_at("browser_download_url");
            if url.is_empty() {
                continue;
            }
            assets.push(Asset {
                name: name.to_string(),
                url: url.to_string(),
                size: item.u64_at("size"),
            });
        }
    }
    Ok(Release {
        tag: tag.to_string(),
        name: doc.str_at("name").to_string(),
        page_url: doc.str_at("html_url").to_string(),
        body: doc.str_at("body").to_string(),
        prerelease: doc.get("prerelease").and_then(Json::as_bool).unwrap_or(false),
        assets,
    })
}

impl Release {
    /// The version this release is, if its tag is one.
    pub fn version(&self) -> Result<Version, UpdateError> {
        Version::parse(&self.tag).ok_or_else(|| UpdateError::BadVersion(self.tag.clone()))
    }

    /// The asset to download: the exact name first, then any .nro the release
    /// carries. A release that renames its file should still be updatable.
    pub fn asset(&self) -> Option<&Asset> {
        if let Some(exact) = self.assets.iter().find(|a| a.name == ASSET_NAME) {
            return Some(exact);
        }
        self.assets.iter().find(|a| a.name.to_ascii_lowercase().ends_with(".nro"))
    }

    /// The version, the asset and the address, checked together: this is what
    /// the dialog needs before it may offer [A] ダウンロード.
    pub fn plan(&self) -> Result<DownloadPlan, UpdateError> {
        let version = self.version()?;
        let asset = self.asset().ok_or(UpdateError::NoAsset)?;
        if !asset.url.starts_with("https://") {
            return Err(UpdateError::InsecureUrl);
        }
        Ok(DownloadPlan {
            version,
            url: asset.url.clone(),
            size: asset.size,
            name: asset.name.clone(),
        })
    }
}

/// What the download half of the updater is told to fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadPlan {
    pub version: Version,
    pub url: String,
    /// The size the release declares, or 0 when it does not say.
    pub size: u64,
    pub name: String,
}

// ---------------------------------------------------------------------------
// Where the bytes go
// ---------------------------------------------------------------------------

/// The three names the swap uses.
///
/// The running .nro is only ever touched by a rename, and only after the new
/// file is complete on the card. A download that dies half way leaves a .new
/// behind and nothing else; a swap that dies between its two renames leaves the
/// old bytes under .bak, which the next start can be pointed at by hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallPlan {
    /// The file the loader starts.
    pub target: String,
    /// Where the download is written first.
    pub staged: String,
    /// Where the old file is put aside during the swap.
    pub backup: String,
}

/// Work out the three names for an application folder and file name.
pub fn install_plan(dir: &str, nro_name: &str) -> InstallPlan {
    let base = dir.trim_end_matches('/');
    InstallPlan {
        target: format!("{}/{}", base, nro_name),
        staged: format!("{}/{}.new", base, nro_name),
        backup: format!("{}/{}.bak", base, nro_name),
    }
}

/// The plan for this application: /switch/XXSE/XXSE.nro and its two neighbours.
pub fn app_install_plan() -> InstallPlan {
    install_plan(APP_DIR, NRO_NAME)
}

/// The file operations the swap needs, so the swap itself can be tested on the
/// host with a fake card instead of a real one.
pub trait RenameFs {
    fn exists(&self, path: &str) -> bool;
    /// Remove a file. Returns true when the path no longer exists.
    fn remove(&mut self, path: &str) -> bool;
    /// Rename, replacing the destination if it exists.
    fn rename(&mut self, from: &str, to: &str) -> bool;
}

/// Why the swap stopped. In every one of these the old .nro is still on the
/// card: either in place, or under the .bak name the message names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallError {
    /// Nothing was downloaded, or the file vanished.
    MissingStaged,
    /// The old file could not be moved aside; nothing has changed yet.
    BackupFailed,
    /// The new file could not be moved into place. The old one is at .bak.
    ActivateFailed,
}

impl InstallError {
    pub fn message(&self, plan: &InstallPlan) -> String {
        match self {
            InstallError::MissingStaged => {
                format!("ダウンロードしたファイルがありません（{}）", plan.staged)
            }
            InstallError::BackupFailed => format!(
                "今のファイルを退避できませんでした。何も変更していません（{}）",
                plan.target
            ),
            InstallError::ActivateFailed => {
                format!("差し替えに失敗しました。元のファイルは {} にあります", plan.backup)
            }
        }
    }
}

/// Replace the running .nro with the downloaded one.
///
/// The order is the whole safety story: the new bytes are already complete
/// under .new before this is called, the old file is moved (not overwritten)
/// to .bak, and the new one takes its name. Each failure returns with the old
/// bytes still somewhere on the card, and the caller says where.
pub fn activate_staged(fs: &mut impl RenameFs, plan: &InstallPlan) -> Result<(), InstallError> {
    if !fs.exists(&plan.staged) {
        return Err(InstallError::MissingStaged);
    }
    // A backup from an earlier attempt is not a backup of this file.
    if fs.exists(&plan.backup) && !fs.remove(&plan.backup) {
        return Err(InstallError::BackupFailed);
    }
    if fs.exists(&plan.target) && !fs.rename(&plan.target, &plan.backup) {
        return Err(InstallError::BackupFailed);
    }
    if !fs.rename(&plan.staged, &plan.target) {
        // Put the old file back where the loader looks, so a failed swap is
        // not a broken install: the .bak is only a fallback.
        let _ = fs.rename(&plan.backup, &plan.target);
        return Err(InstallError::ActivateFailed);
    }
    // The backup is kept: it is the way back for a user whose new build will
    // not start, and it is replaced on the next successful update.
    Ok(())
}

/// Is a staged download complete? When the release declares no size, a
/// non-empty file is all that can be checked.
pub fn staged_is_complete(staged_size: u64, expected_size: u64) -> bool {
    if expected_size == 0 {
        return staged_size > 0;
    }
    staged_size == expected_size
}

// ---------------------------------------------------------------------------
// Progress
// ---------------------------------------------------------------------------

/// Whole percent of a download, clamped to 0..=100.
pub fn percent(done: u64, total: u64) -> u32 {
    if total == 0 {
        return 0;
    }
    if done >= total {
        return 100;
    }
    ((done as u128 * 100) / total as u128) as u32
}

// ---------------------------------------------------------------------------
// The optional start-up check
// ---------------------------------------------------------------------------

/// The user's answer to "may this start look for an update?".
///
/// Off is a real choice, not a hidden switch: with it off nothing opens a
/// socket at start-up, and the dialog's own [Y] can turn it back on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    pub check_on_start: bool,
}

impl Default for Settings {
    fn default() -> Self {
        // On by default: the point of the feature is that a player who never
        // reads a readme still gets the fix.
        Settings { check_on_start: true }
    }
}

impl Settings {
    /// The file's text. One key per line, '#' starts a comment.
    pub fn render(&self) -> String {
        format!(
            "# XXSE update check\n# 0 = 起動時に確認しない\ncheck_on_start={}\n",
            if self.check_on_start { 1 } else { 0 }
        )
    }

    /// Read the file's text. Anything unreadable falls back to the default,
    /// because a settings file is never a reason to refuse to start.
    pub fn parse(text: &str) -> Settings {
        let mut settings = Settings::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, value) = match line.split_once('=') {
                Some(pair) => pair,
                None => continue,
            };
            if key.trim() != "check_on_start" {
                continue;
            }
            match value.trim() {
                "0" | "false" | "off" | "no" => settings.check_on_start = false,
                "1" | "true" | "on" | "yes" => settings.check_on_start = true,
                _ => {}
            }
        }
        settings
    }
}

// ---------------------------------------------------------------------------
// What the screen says
// ---------------------------------------------------------------------------

/// The banner's one line: 新しいバージョン v0.3.2 があります [A] 詳細
pub fn banner_text(version: &Version) -> String {
    format!("{} v{} があります  [A] 詳細", BANNER_PREFIX, version)
}

/// The dialog's title.
pub fn dialog_title(version: &Version) -> String {
    format!("更新があります: v{}", version)
}

/// The release notes, cut down to lines a dialog can draw.
///
/// GitHub stores markdown; the console has no markdown, so the shapes that only
/// exist for a browser (headings, bullet markers, images, HTML) are flattened
/// to plain text and the list is capped.
pub fn note_lines(body: &str, max_lines: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    // Everything between two fence lines is code, and a console dialog has no
    // room for it.
    let mut in_fence = false;
    for raw in body.lines() {
        let trimmed = raw.trim_start();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if trimmed.starts_with("<!--") || trimmed.starts_with('<') {
            continue;
        }
        // Heading marks and bullet marks are noise on a console line.
        let mut chars = trimmed.chars().peekable();
        while let Some(c) = chars.peek().copied() {
            if c == '#' || c == '*' || c == '-' || c == '>' || c == '+' || c == ' ' {
                chars.next();
                continue;
            }
            break;
        }
        // A bullet that carried text keeps its text, with its own marker.
        let had_bullet = trimmed.starts_with("- ") || trimmed.starts_with("* ");
        let mut stripped = String::new();
        for c in chars {
            stripped.push(c);
        }
        // Turn the markdown link form into its label.
        let line = strip_links(&stripped).trim().to_string();
        if line.is_empty() {
            if out.last().map(|l| l.is_empty()).unwrap_or(true) {
                continue;
            }
            out.push(String::new());
            continue;
        }
        if had_bullet {
            out.push(format!("・{}", line));
        } else {
            out.push(line);
        }
        if out.len() >= max_lines {
            out.push(String::from("…"));
            break;
        }
    }
    while out.last().map(|l| l.is_empty()).unwrap_or(false) {
        out.pop();
    }
    out
}

/// [label](url) -> label
fn strip_links(text: &str) -> String {
    let mut out = String::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            if let Some(close) = text[i..].find(']') {
                let label = &text[i + 1..i + close];
                let after = i + close + 1;
                if text[after..].starts_with('(') {
                    if let Some(end) = text[after..].find(')') {
                        out.push_str(label);
                        i = after + end + 1;
                        continue;
                    }
                }
            }
        }
        let next = text[i..].chars().next().unwrap_or(' ');
        out.push(next);
        i += next.len_utf8();
    }
    out
}

#[cfg(test)]
mod inline_tests {
    use super::*;

    #[test]
    fn version_display_round_trips() {
        let v = Version::parse("v1.2.3-rc.1").unwrap();
        assert_eq!(v.to_string(), "1.2.3-rc.1");
    }
}
