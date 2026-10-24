//! A commit whose final file replacement is blocked by another process's open handle either
//! succeeds after the hold ends or answers `commit_pending` (recorded, applied by recovery);
//! it never answers a plain failure for a change that will land.
#![cfg(windows)]
use serde_json::Value;
use std::{fs, os::windows::fs::OpenOptionsExt, path::PathBuf, process::Output, time::Duration};

const FILE_SHARE_READ: u32 = 1;

struct Fixture {
    home: PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.home);
    }
}
impl Fixture {
    fn new() -> Self {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-workspaces");
        let home = base.join(format!("outcomes-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&home).unwrap();
        let f = Self { home };
        let o = f.run(&["project", "create", "fixture", "--name", "Fixture"], &[]);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        f
    }
    fn run(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_research"));
        command.env("SCOPEWORK_COORDINATOR", "off");
        for (k, v) in env {
            command.env(k, v);
        }
        command
            .arg("--home")
            .arg(&self.home)
            .args(args)
            .output()
            .unwrap()
    }
    /// Holds project.json open the way a plain reader does (no delete or write sharing).
    fn hold_catalog(&self, for_how_long: Duration) -> std::thread::JoinHandle<()> {
        let path = self.home.join("projects/fixture/project.json");
        let file = fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(path)
            .unwrap();
        std::thread::spawn(move || {
            std::thread::sleep(for_how_long);
            drop(file);
        })
    }
}

#[test]
fn a_briefly_held_catalog_still_commits() {
    let f = Fixture::new();
    let holder = f.hold_catalog(Duration::from_millis(1500));
    let o = f.run(
        &[
            "node", "create", "fixture", "--parent", "0", "--name", "Arch",
        ],
        &[],
    );
    holder.join().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

#[test]
fn a_catalog_held_past_the_retries_answers_commit_pending_and_lands_later() {
    let f = Fixture::new();
    let holder = f.hold_catalog(Duration::from_secs(9));
    let o = f.run(
        &[
            "node", "create", "fixture", "--parent", "0", "--name", "Arch",
        ],
        &[("SCOPEWORK_COMMIT_RETRY_MS", "200")],
    );
    let stderr = String::from_utf8_lossy(&o.stderr).into_owned();
    assert!(
        !o.status.success(),
        "the held catalog could not be replaced"
    );
    let error: Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(error["kind"], "commit_pending", "{stderr}");
    holder.join().unwrap();
    // The next command recovers the journal: the node exists without repeating the request.
    let o = f.run(&["node", "list", "fixture", "--parent", "0"], &[]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let children: Value = serde_json::from_slice(&o.stdout).unwrap();
    let names: Vec<_> = children
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["node"]["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, ["Arch"]);
}

#[test]
fn a_failed_receipt_write_after_the_rename_still_reports_success() {
    let f = Fixture::new();
    let o = f.run(
        &[
            "node", "create", "fixture", "--parent", "0", "--name", "Arch",
        ],
        &[
            ("SCOPEWORK_RECEIPT_FAIL_TIMES", "1"),
            ("SCOPEWORK_COMMIT_RETRY_MS", "2000"),
        ],
    );
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let revision = serde_json::from_slice::<Value>(&o.stdout).unwrap()["revision"]
        .as_u64()
        .unwrap();
    let receipts = f
        .home
        .join("projects/fixture/tools/workspaces/catalog-transactions-v4");
    assert!(!receipts.join("pending").exists());
    let journal: Value = serde_json::from_slice(
        &fs::read(receipts.join(format!("revision-{revision}/journal.json"))).unwrap(),
    )
    .unwrap();
    assert_eq!(journal["kind"], "completed-catalog-delta");
    assert!(journal["at_unix"].is_u64(), "{journal}");
}
