//! history show summarises catalog receipts and tooling entries; history append writes the tooling log.
use serde_json::Value;
use std::{fs, path::PathBuf, process::Output};

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
        let home = base.join(format!("history-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&home).unwrap();
        let f = Self { home };
        f.ok(&[
            "project",
            "create",
            "fixture",
            "--name",
            "Fixture",
            "--binary",
            "synthetic",
        ]);
        f.ok(&[
            "node", "create", "fixture", "--parent", "0", "--name", "Arch",
        ]);
        f.ok(&[
            "node", "create", "fixture", "--parent", "1", "--name", "Chunk",
        ]);
        f
    }
    fn run(&self, args: &[&str]) -> Output {
        std::process::Command::new(env!("CARGO_BIN_EXE_research"))
            .arg("--home")
            .arg(&self.home)
            .args(args)
            .output()
            .unwrap()
    }
    fn ok(&self, args: &[&str]) -> Value {
        let o = self.run(args);
        assert!(
            o.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        serde_json::from_slice(&o.stdout).unwrap()
    }
}

#[test]
fn catalog_receipts_and_tooling_entries_read_as_one_history() {
    let f = Fixture::new();
    f.ok(&[
        "node",
        "flag",
        "fixture",
        "2",
        "--reason",
        "check membership",
    ]);
    let flagged = f.ok(&[
        "history", "show", "fixture", "--only", "catalog", "--limit", "1",
    ]);
    let newest = &flagged["entries"][0];
    assert_eq!(newest["kind"], "catalog");
    assert_eq!(newest["sidecars"][0], "review-flags.json");

    let diff = f.home.join("diff.json");
    fs::write(&diff, r#"{"rows_added":2,"rows_removed":0}"#).unwrap();
    let entry = f.ok(&[
        "history",
        "append",
        "fixture",
        "--kind",
        "ledger-change",
        "--summary",
        "Queued two signatures",
        "--node",
        "2",
        "--ref",
        "tools/profiles/annotations.json",
        "--diff-file",
        diff.to_str().unwrap(),
    ]);
    assert_eq!(entry["diff"]["rows_added"], 2);

    let tooling = f.ok(&["history", "show", "fixture", "--only", "tooling"]);
    assert_eq!(tooling["entries"].as_array().unwrap().len(), 1);
    assert_eq!(tooling["entries"][0]["entry_kind"], "ledger-change");

    let chunk = f.ok(&["history", "show", "fixture", "--node", "2"]);
    let kinds: Vec<&str> = chunk["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"tooling") && kinds.contains(&"catalog"));
    let created = chunk["entries"].as_array().unwrap().iter().find(|e| {
        e["kind"] == "catalog"
            && e["changes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["created"] == true)
    });
    assert!(
        created.is_some(),
        "the chunk's creation is in its history: {chunk}"
    );

    for bad in [["--kind", "catalog"], ["--kind", "Bad Kind"]] {
        let out = f.run(&[
            "history",
            "append",
            "fixture",
            bad[0],
            bad[1],
            "--summary",
            "x",
        ]);
        assert!(!out.status.success());
    }
}
