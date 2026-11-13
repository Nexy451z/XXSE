//! The update check, on the host: no console, no card, no network.
//!
//! Everything the console would have to get right is a rule here - what the
//! release document means, which version is newer, where the download is staged
//! and what the swap does when its second rename fails - so that the only part
//! that cannot be tested here is the socket itself.

use std::collections::BTreeMap;

use mhnx_core::json::{self, Json};
use mhnx_core::update::{
    activate_staged, already_installed, app_install_plan, banner_text, c_path, device_path, dir_of,
    install_plan, missing_staged_message, note_lines, parse_release, percent, post_download_message,
    staged_is_complete, staged_state, CardDir, DirEntry, InstallError, Release, RenameFs, Settings,
    StagedState, UpdateError, Version, APP_DIR, ASSET_NAME, RELEASES_LATEST_URL, SD_DEVICE,
    SETTINGS_FILE,
};

/// A real answer from the release feed, trimmed to the members that matter.
fn release_json(tag: &str, asset_name: &str, url: &str) -> String {
    format!(
        r##"{{
  "url": "https://api.github.com/repos/Nexy451z/XXSE/releases/1",
  "html_url": "https://github.com/Nexy451z/XXSE/releases/tag/{tag}",
  "assets_url": "https://api.github.com/repos/Nexy451z/XXSE/releases/1/assets",
  "tag_name": "{tag}",
  "target_commitish": "main",
  "name": "XXSE {tag}",
  "draft": false,
  "prerelease": false,
  "id": 1234567,
  "body": "変更点\n- アイテムボックスの一括操作を直した\n- 護石の並び順\n",
  "assets": [
    {{
      "name": "{asset_name}",
      "size": 4823040,
      "download_count": 71,
      "content_type": "application/octet-stream",
      "browser_download_url": "{url}"
    }}
  ]
}}"##
    )
}

// ---------------------------------------------------------------------------
// JSON
// ---------------------------------------------------------------------------

#[test]
fn a_release_document_is_read_whole() {
    let text = release_json(
        "v0.3.1",
        "XXSE.nro",
        "https://github.com/Nexy451z/XXSE/releases/download/v0.3.1/XXSE.nro",
    );
    let doc = json::parse(&text).expect("the feed is valid JSON");
    assert_eq!(doc.str_at("tag_name"), "v0.3.1");
    assert_eq!(doc.u64_at("id"), 1234567);
    assert_eq!(doc.get("draft").and_then(Json::as_bool), Some(false));
    let assets = doc.get("assets").and_then(Json::as_array).expect("assets array");
    assert_eq!(assets.len(), 1);
    assert_eq!(assets[0].u64_at("size"), 4823040);

    let release = parse_release(&text).expect("a usable release");
    assert_eq!(release.tag, "v0.3.1");
    assert_eq!(release.name, "XXSE v0.3.1");
    assert_eq!(release.assets.len(), 1);
    assert_eq!(release.assets[0].url, assets[0].str_at("browser_download_url"));
}

#[test]
fn escapes_and_japanese_survive_the_reader() {
    // \u3042 is あ, and a surrogate pair is one character, not two.
    let text = r#"{"body": "\u3042\u3044 \ud83d\ude00 \"quoted\" \\ slash\/","n": -12.5e3}"#;
    let doc = json::parse(text).expect("valid");
    assert_eq!(doc.str_at("body"), "あい \u{1F600} \"quoted\" \\ slash/");
    assert_eq!(doc.get("n").and_then(Json::as_u64), None, "a fraction is not an integer");
}

#[test]
fn a_broken_document_is_refused_not_half_read() {
    let cases = [
        "",
        "{",
        "{\"tag_name\": }",
        "{\"tag_name\": \"v1.0.0\"} trailing",
        "{\"tag_name\": \"v1.0.0\",}",
        "[1, 2,]",
        "{\"a\": 01}",
        "{\"a\": \"unterminated}",
        // A raw control character inside a string is not JSON; the escaped
        // form (\u0001) is, and the reader above accepts it.
        "{\"a\": \"\u{1}\"}",
        "nope",
    ];
    for case in cases {
        assert!(json::parse(case).is_err(), "should refuse: {case:?}");
    }
    // A nest deeper than the limit is refused rather than overflowing the stack.
    let deep = "[".repeat(json::MAX_DEPTH + 8);
    assert!(json::parse(&deep).is_err());
}

#[test]
fn a_non_json_answer_is_an_error_and_not_a_panic() {
    // What a captive portal or a 404 page looks like.
    let html = "<!DOCTYPE html><html><body>Maintenance</body></html>";
    assert_eq!(parse_release(html).unwrap_err(), UpdateError::Json);
    // Valid JSON that is not a release.
    assert_eq!(
        parse_release(r#"{"message": "Not Found"}"#).unwrap_err(),
        UpdateError::Missing("tag_name")
    );
}

// ---------------------------------------------------------------------------
// Versions
// ---------------------------------------------------------------------------

#[test]
fn the_newer_version_is_the_one_semver_orders_higher() {
    let cases = [
        ("v0.3.1", "0.3.0", true),
        ("v0.3.1", "0.3.1", false),
        ("v0.3.1", "v0.4.0", false),
        ("v0.4", "0.3.9", true),
        ("v1.0.0", "0.99.99", true),
        ("v0.3.1-alpha", "0.3.1", false),
        ("v0.3.2-alpha", "0.3.1", true),
        ("v0.3.1-rc.1", "0.3.1-alpha", true),
        ("v0.3.1-alpha.2", "0.3.1-alpha.10", false),
        ("v0.3.1+build.9", "0.3.1", false),
    ];
    for (latest, running, expected) in cases {
        let latest = Version::parse(latest).expect("version");
        let running = Version::parse(running).expect("version");
        assert_eq!(
            latest.is_newer_than(&running),
            expected,
            "{latest} vs {running}"
        );
    }
}

#[test]
fn a_version_is_read_or_refused() {
    let v = Version::parse("V0.3.1-rc.2+sha.abc").expect("readable");
    assert_eq!((v.major, v.minor, v.patch), (0, 3, 1));
    assert_eq!(v.pre, "rc.2");
    assert_eq!(v.to_string(), "0.3.1-rc.2");

    for bad in ["", "v", "v1.2.3.4", "latest", "v1.x.0", "1.2.3-"] {
        assert!(Version::parse(bad).is_none(), "should refuse: {bad:?}");
    }
}

#[test]
fn the_running_version_is_read_from_the_crate() {
    // The number the banner compares against is the one compiled into the .nro,
    // so it is the crate's own version and not a string typed twice.
    let compiled = env!("CARGO_PKG_VERSION");
    assert!(Version::parse(compiled).is_some(), "{compiled}");
}

// ---------------------------------------------------------------------------
// Picking the asset
// ---------------------------------------------------------------------------

#[test]
fn the_asset_is_the_named_nro_and_https_is_required() {
    let release = parse_release(&release_json(
        "v0.3.2",
        ASSET_NAME,
        "https://github.com/Nexy451z/XXSE/releases/download/v0.3.2/XXSE.nro",
    ))
    .expect("release");
    let plan = release.plan().expect("a downloadable plan");
    assert_eq!(plan.version.to_string(), "0.3.2");
    assert_eq!(plan.name, ASSET_NAME);
    assert_eq!(plan.size, 4823040);

    // A release that renames the file is still updatable.
    let renamed = parse_release(&release_json(
        "v0.3.3",
        "XXSE-0.3.3.nro",
        "https://example.invalid/x.nro",
    ))
    .expect("release");
    assert_eq!(renamed.asset().map(|a| a.name.as_str()), Some("XXSE-0.3.3.nro"));

    // An http address is refused rather than downgraded.
    let insecure = parse_release(&release_json(
        "v0.3.2",
        ASSET_NAME,
        "http://example.invalid/XXSE.nro",
    ))
    .expect("release");
    assert_eq!(insecure.plan().unwrap_err(), UpdateError::InsecureUrl);

    // No .nro in the release at all.
    let empty = Release {
        tag: String::from("v0.3.2"),
        name: String::new(),
        page_url: String::new(),
        body: String::new(),
        prerelease: false,
        assets: vec![],
    };
    assert_eq!(empty.plan().unwrap_err(), UpdateError::NoAsset);
    assert!(empty.plan().unwrap_err().message().contains(ASSET_NAME));
}

// ---------------------------------------------------------------------------
// Where the bytes go
// ---------------------------------------------------------------------------

#[test]
fn the_download_is_staged_beside_the_running_nro() {
    let plan = app_install_plan();
    assert_eq!(plan.target, "/switch/XXSE/XXSE.nro");
    assert_eq!(plan.staged, "/switch/XXSE/XXSE.nro.new");
    assert_eq!(plan.backup, "/switch/XXSE/XXSE.nro.bak");
    assert_eq!(
        RELEASES_LATEST_URL,
        "https://api.github.com/repos/Nexy451z/XXSE/releases/latest"
    );

    // A trailing slash in the folder does not produce a doubled separator.
    let plan = install_plan("/switch/XXSE/", "XXSE.nro");
    assert_eq!(plan.staged, "/switch/XXSE/XXSE.nro.new");
    // The staged name always sits under the real one, so a stray .new can never
    // be mistaken for the application by hbmenu.
    assert!(plan.staged.starts_with(&plan.target));
}

/// A card in memory, with the one failure a test needs it to have.
struct FakeFs {
    files: BTreeMap<String, u32>,
    fail_rename_to: Option<String>,
    fail_remove: bool,
}

impl FakeFs {
    fn with(files: &[(&str, u32)]) -> FakeFs {
        FakeFs {
            files: files.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
            fail_rename_to: None,
            fail_remove: false,
        }
    }
}

impl RenameFs for FakeFs {
    fn exists(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    fn remove(&mut self, path: &str) -> bool {
        if self.fail_remove {
            return false;
        }
        self.files.remove(path);
        !self.files.contains_key(path)
    }

    fn rename(&mut self, from: &str, to: &str) -> bool {
        if self.fail_rename_to.as_deref() == Some(to) {
            return false;
        }
        match self.files.remove(from) {
            Some(bytes) => {
                self.files.insert(to.to_string(), bytes);
                true
            }
            None => false,
        }
    }
}

/// A card whose second write to the target name is the one that fails: that is
/// the staged -> target move, with the old file already at .bak.
struct SecondWriteFails {
    files: BTreeMap<String, u32>,
    target: String,
    writes: u32,
}

impl RenameFs for SecondWriteFails {
    fn exists(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    fn remove(&mut self, path: &str) -> bool {
        self.files.remove(path);
        true
    }

    fn rename(&mut self, from: &str, to: &str) -> bool {
        if to == self.target && from != self.target {
            self.writes += 1;
            if self.writes == 1 {
                return false;
            }
        }
        match self.files.remove(from) {
            Some(bytes) => {
                self.files.insert(to.to_string(), bytes);
                true
            }
            None => false,
        }
    }
}

#[test]
fn the_swap_replaces_the_nro_and_keeps_the_old_one() {
    let plan = app_install_plan();
    let mut fs = FakeFs::with(&[(plan.target.as_str(), 1), (plan.staged.as_str(), 2)]);
    activate_staged(&mut fs, &plan).expect("the swap succeeds");
    assert_eq!(fs.files.get(&plan.target), Some(&2), "the new bytes are live");
    assert_eq!(fs.files.get(&plan.backup), Some(&1), "the old bytes are kept");
    assert!(!fs.files.contains_key(&plan.staged), "the staged name is gone");
}

#[test]
fn a_failed_swap_leaves_a_runnable_file_on_the_card() {
    let plan = app_install_plan();

    // 1. Nothing was downloaded.
    let mut fs = FakeFs::with(&[(plan.target.as_str(), 1)]);
    assert_eq!(activate_staged(&mut fs, &plan), Err(InstallError::MissingStaged));
    assert_eq!(fs.files.get(&plan.target), Some(&1));

    // 2. The old file cannot be moved aside: the target is untouched and the
    //    download is still there to try again with.
    let mut fs = FakeFs::with(&[(plan.target.as_str(), 1), (plan.staged.as_str(), 2)]);
    fs.fail_rename_to = Some(plan.backup.clone());
    assert_eq!(activate_staged(&mut fs, &plan), Err(InstallError::BackupFailed));
    assert_eq!(fs.files.get(&plan.target), Some(&1), "still runnable");
    assert_eq!(fs.files.get(&plan.staged), Some(&2), "the download is still there");

    // 3. The staged file cannot be moved in: the old one is put back under its
    //    own name by the restore.
    let mut fs = SecondWriteFails {
        files: [(plan.target.clone(), 1u32), (plan.staged.clone(), 2u32)]
            .into_iter()
            .collect(),
        target: plan.target.clone(),
        writes: 0,
    };
    assert_eq!(activate_staged(&mut fs, &plan), Err(InstallError::ActivateFailed));
    assert_eq!(fs.files.get(&plan.target), Some(&1), "the old file is back in place");

    // 4. An old .bak that cannot be removed stops the swap before anything moves.
    let mut fs = FakeFs::with(&[
        (plan.target.as_str(), 1),
        (plan.staged.as_str(), 2),
        (plan.backup.as_str(), 9),
    ]);
    fs.fail_remove = true;
    assert_eq!(activate_staged(&mut fs, &plan), Err(InstallError::BackupFailed));
    assert_eq!(fs.files.get(&plan.target), Some(&1));
    assert_eq!(fs.files.get(&plan.staged), Some(&2));
    assert_eq!(fs.files.get(&plan.backup), Some(&9));
}

#[test]
fn a_swap_that_cannot_restore_says_where_the_old_bytes_are() {
    // The worst case: the staged file cannot be moved in AND the restore cannot
    // move it back either. The old bytes are still on the card under .bak, and
    // the message is the only thing that says so.
    struct NoWriteAtAll {
        files: BTreeMap<String, u32>,
        target: String,
    }
    impl RenameFs for NoWriteAtAll {
        fn exists(&self, path: &str) -> bool {
            self.files.contains_key(path)
        }
        fn remove(&mut self, path: &str) -> bool {
            self.files.remove(path);
            true
        }
        fn rename(&mut self, from: &str, to: &str) -> bool {
            if to == self.target {
                return false;
            }
            match self.files.remove(from) {
                Some(bytes) => {
                    self.files.insert(to.to_string(), bytes);
                    true
                }
                None => false,
            }
        }
    }
    let plan = app_install_plan();
    let mut fs = NoWriteAtAll {
        files: [(plan.target.clone(), 1u32), (plan.staged.clone(), 2u32)]
            .into_iter()
            .collect(),
        target: plan.target.clone(),
    };
    let err = activate_staged(&mut fs, &plan).unwrap_err();
    assert_eq!(err, InstallError::ActivateFailed);
    assert!(err.message(&plan).contains(&plan.backup), "{}", err.message(&plan));
    assert_eq!(fs.files.get(&plan.backup), Some(&1), "the old bytes are still on the card");
}

#[test]
fn a_second_update_replaces_the_old_backup() {
    let plan = app_install_plan();
    let mut fs = FakeFs::with(&[
        (plan.target.as_str(), 1),
        (plan.staged.as_str(), 2),
        (plan.backup.as_str(), 0),
    ]);
    activate_staged(&mut fs, &plan).expect("the swap succeeds");
    assert_eq!(fs.files.get(&plan.target), Some(&2));
    assert_eq!(fs.files.get(&plan.backup), Some(&1), "the newest old file wins");
}

// ---------------------------------------------------------------------------
// The names as the C library sees them
// ---------------------------------------------------------------------------

#[test]
fn every_path_the_updater_opens_names_the_card() {
    // The three names the swap uses and the settings file all carry the
    // device: fsdev resolves a path without one against whatever device is the
    // default, and "the default" is not a promise this code can keep.
    let plan = app_install_plan();
    for path in [&plan.target, &plan.staged, &plan.backup] {
        assert_eq!(device_path(path), format!("sdmc:{}", path));
    }
    assert_eq!(device_path(SETTINGS_FILE), "sdmc:/switch/XXSE/update.cfg");
    assert_eq!(SD_DEVICE, "sdmc:");

    // A path that already names one is not given a second name.
    assert_eq!(
        device_path("sdmc:/switch/XXSE/XXSE.nro"),
        "sdmc:/switch/XXSE/XXSE.nro"
    );
    // A colon inside a file name is not a device name.
    assert_eq!(device_path("/switch/XXSE/a:b.nro"), "sdmc:/switch/XXSE/a:b.nro");
    // Another device is left alone: the updater never opens one, but the
    // helper must not turn "save:/x" into "sdmc:save:/x".
    assert_eq!(device_path("save:/system"), "save:/system");
}

#[test]
fn the_c_path_ends_in_a_nul_so_fopen_sees_one_name() {
    let plan = app_install_plan();
    let c = c_path(&plan.staged);
    assert_eq!(c.last(), Some(&0), "a C string ends in a NUL");
    assert_eq!(&c[..c.len() - 1], b"sdmc:/switch/XXSE/XXSE.nro.new");
    assert!(!c[..c.len() - 1].contains(&0), "no NUL inside the name");

    // The bug this guards: a Vec<u8> built from a String has no terminator, so
    // fopen reads the allocator's padding as part of the name - the download
    // then writes a file the swap cannot find.
    let bare = plan.staged.clone().into_bytes();
    assert_ne!(bare.last(), Some(&0), "a String is not a C string");
    assert_eq!(c.len(), bare.len() + SD_DEVICE.len() + 1);
}

#[test]
fn the_folder_of_a_name_is_the_folder_it_sits_in() {
    let plan = app_install_plan();
    assert_eq!(dir_of(&plan.staged), APP_DIR);
    assert_eq!(dir_of("/switch/XXSE/XXSE.nro"), "/switch/XXSE");
    assert_eq!(dir_of("sdmc:/switch/XXSE/XXSE.nro"), "sdmc:/switch/XXSE");
    assert_eq!(dir_of("/XXSE.nro"), "/");
    assert_eq!(dir_of("XXSE.nro"), "");
}

// ---------------------------------------------------------------------------
// What the card holds
// ---------------------------------------------------------------------------

#[test]
fn the_card_listing_says_what_is_there() {
    let empty = CardDir::new(vec![]);
    assert_eq!(empty.describe(6), "（ファイルがありません）");
    assert_eq!(empty.size_of("/switch/XXSE/XXSE.nro"), None);

    let card = CardDir::new(vec![
        DirEntry { name: String::from("XXSE.nro"), size: 4823040 },
        DirEntry { name: String::from("XXSE.nro.bak"), size: 4700000 },
        DirEntry { name: String::from("update.cfg"), size: 62 },
    ]);
    let line = card.describe(6);
    assert!(line.contains("XXSE.nro 4823040 バイト"), "{line}");
    assert!(line.contains("XXSE.nro.bak 4700000 バイト"), "{line}");
    assert!(line.contains("update.cfg 62 バイト"), "{line}");
    assert!(!line.contains('\n'), "one line: a dialog draws one line");

    // The size is found by the last component, with or without the device.
    assert_eq!(card.size_of("/switch/XXSE/XXSE.nro"), Some(4823040));
    assert_eq!(card.size_of("sdmc:/switch/XXSE/XXSE.nro"), Some(4823040));
    assert_eq!(card.size_of("/switch/XXSE/XXSE.nro.new"), None);

    // Capped, and the cap says how many were left out.
    let many = CardDir::new(
        (0u64..10)
            .map(|i| DirEntry { name: format!("f{i}"), size: i })
            .collect(),
    );
    let line = many.describe(3);
    assert!(line.contains("f0 0 バイト"), "{line}");
    assert!(!line.contains("f3"), "{line}");
    assert!(line.contains("ほか 7 件"), "{line}");
}

#[test]
fn the_staged_file_is_ready_only_at_the_declared_size() {
    assert_eq!(staged_state(None, 100), StagedState::Missing);
    assert_eq!(staged_state(Some(100), 100), StagedState::Ready(100));
    assert_eq!(staged_state(Some(99), 100), StagedState::WrongSize { found: 99 });
    assert_eq!(staged_state(Some(101), 100), StagedState::WrongSize { found: 101 });
    // No declared size: a non-empty file is all that can be asked.
    assert_eq!(staged_state(Some(1), 0), StagedState::Ready(1));
    assert_eq!(staged_state(Some(0), 0), StagedState::WrongSize { found: 0 });
}

#[test]
fn a_missing_download_is_explained_with_the_folder() {
    let plan = app_install_plan();
    let card = CardDir::new(vec![
        DirEntry { name: String::from("XXSE.nro"), size: 4700000 },
        DirEntry { name: String::from("XXSE.nro.new"), size: 1024 },
    ]);
    let message = missing_staged_message(&plan, 4823040, &card);
    assert!(message.contains(&plan.staged), "{message}");
    assert!(message.contains(APP_DIR), "{message}");
    assert!(message.contains("XXSE.nro.new 1024 バイト"), "{message}");
    assert!(!message.contains("更新済み"), "{message}");
    assert!(!message.contains('\n'), "one line: a dialog draws one line");

    // An empty folder is its own answer.
    let message = missing_staged_message(&plan, 4823040, &CardDir::new(vec![]));
    assert!(message.contains("（ファイルがありません）"), "{message}");

    // The same sentence is what the swap's own error says now.
    let err = InstallError::MissingStaged;
    assert_eq!(err.message_with_card(&plan, 4823040, &card), missing_staged_message(&plan, 4823040, &card));
    assert_eq!(err.message(&plan), format!("ダウンロードしたファイルがありません（{}）", plan.staged));
}

#[test]
fn a_missing_download_that_is_already_installed_asks_for_a_restart() {
    let plan = app_install_plan();
    let card = CardDir::new(vec![DirEntry { name: String::from("XXSE.nro"), size: 4823040 }]);
    assert!(already_installed(&plan, 4823040, &card));
    let message = missing_staged_message(&plan, 4823040, &card);
    assert!(message.contains("更新済み"), "{message}");
    assert!(message.contains("再起動"), "{message}");
    assert!(message.contains("4823040"), "{message}");

    // A target of another size is not "already installed".
    let older = CardDir::new(vec![DirEntry { name: String::from("XXSE.nro"), size: 4700000 }]);
    assert!(!already_installed(&plan, 4823040, &older));
    assert!(!missing_staged_message(&plan, 4823040, &older).contains("更新済み"));
    // A release that declares no size cannot be called installed.
    assert!(!already_installed(&plan, 0, &card));
}

#[test]
fn the_post_download_check_names_the_size_the_card_kept() {
    let plan = app_install_plan();
    let card = CardDir::new(vec![DirEntry { name: String::from("XXSE.nro.new"), size: 4096 }]);

    // The card agrees: nothing to say, the swap goes ahead.
    assert_eq!(
        post_download_message(&plan, 4823040, StagedState::Ready(4823040), &card),
        None
    );

    // The file is there but short: the message names both sizes.
    let message = post_download_message(&plan, 4823040, StagedState::WrongSize { found: 4096 }, &card)
        .expect("a short file is a failure");
    assert!(message.contains("4096"), "{message}");
    assert!(message.contains("4823040"), "{message}");
    assert!(message.contains(&plan.staged), "{message}");

    // The file is not there at all: the message lists the folder.
    let message = post_download_message(&plan, 4823040, StagedState::Missing, &card)
        .expect("a missing file is a failure");
    assert!(message.contains("XXSE.nro.new 4096 バイト"), "{message}");
    assert!(message.contains(&plan.staged), "{message}");
}

#[test]
fn a_download_is_complete_only_at_the_declared_size() {
    assert!(staged_is_complete(4823040, 4823040));
    assert!(!staged_is_complete(4823039, 4823040));
    assert!(!staged_is_complete(4823041, 4823040));
    // No size in the release: a non-empty file is all that can be said.
    assert!(staged_is_complete(1, 0));
    assert!(!staged_is_complete(0, 0));
}

// ---------------------------------------------------------------------------
// Progress and settings
// ---------------------------------------------------------------------------

#[test]
fn progress_is_a_whole_percent_that_never_runs_past_a_hundred() {
    assert_eq!(percent(0, 100), 0);
    assert_eq!(percent(50, 100), 50);
    assert_eq!(percent(99, 100), 99);
    assert_eq!(percent(100, 100), 100);
    assert_eq!(percent(101, 100), 100);
    assert_eq!(percent(1, 3), 33);
    // An unknown total is not a division by zero.
    assert_eq!(percent(10, 0), 0);
    // A 4.8 MB file, which is the size a release is.
    assert_eq!(percent(4823040 / 2, 4823040), 50);
}

#[test]
fn the_start_up_check_can_be_turned_off_and_the_file_is_forgiving() {
    let on = Settings::default();
    assert!(on.check_on_start, "on by default");
    assert_eq!(Settings::parse(&on.render()), on);

    let off = Settings { check_on_start: false };
    assert_eq!(Settings::parse(&off.render()), off);

    // Anything a user might have typed, and anything a corrupted file holds.
    assert!(!Settings::parse("check_on_start=0\n").check_on_start);
    assert!(!Settings::parse("  check_on_start = off  \n").check_on_start);
    assert!(Settings::parse("# comment only\n").check_on_start);
    assert!(Settings::parse("").check_on_start);
    assert!(Settings::parse("garbage").check_on_start);
    assert!(!Settings::parse("check_on_start=0\r\n").check_on_start, "CRLF");
}

// ---------------------------------------------------------------------------
// What the screen says
// ---------------------------------------------------------------------------

#[test]
fn the_banner_is_one_quiet_line() {
    let v = Version::parse("v0.3.2").unwrap();
    assert_eq!(banner_text(&v), "新しいバージョン v0.3.2 があります");
    // No key in the line: [A] opens the dialog on the start screen only, and a
    // banner that names it lies on the other twenty screens.
    assert!(
        !banner_text(&v).contains("[A]"),
        "the banner must not name a key it cannot keep on every screen"
    );
    // One line: the guide strip draws exactly one.
    assert!(!banner_text(&v).contains('\n'));
}

#[test]
fn the_release_notes_become_plain_lines() {
    let body = "# 変更点\n\n- アイテムボックスの一括操作を直した\n* [護石](https://example.invalid)の並び順\n\n```\ncode\n```\n<!-- hidden -->\n";
    let lines = note_lines(body, 8);
    assert!(lines.iter().any(|l| l == "・アイテムボックスの一括操作を直した"), "{lines:?}");
    assert!(lines.iter().any(|l| l == "・護石の並び順"), "{lines:?}");
    assert!(!lines.iter().any(|l| l.contains("http")), "no addresses: {lines:?}");
    assert!(!lines.iter().any(|l| l.contains("code")), "no code fences: {lines:?}");
    assert!(!lines.iter().any(|l| l.contains("hidden")), "no comments: {lines:?}");

    // Capped, with a marker saying so.
    let long = (0..40).map(|i| format!("行 {i}")).collect::<Vec<_>>().join("\n");
    let lines = note_lines(&long, 5);
    assert_eq!(lines.len(), 6);
    assert_eq!(lines.last().map(|s| s.as_str()), Some("…"));
    // An empty body is no lines at all.
    assert!(note_lines("", 5).is_empty());
}
