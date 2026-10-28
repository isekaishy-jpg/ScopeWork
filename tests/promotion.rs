//! Promotion: the store becomes authoritative for a project, its commits write the store, and
//! no receipts are written.
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

struct Fixture {
    home: PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.home);
    }
}
impl Fixture {
    /// A project with an architecture (1), a chunk (2) and two functions (3 at 0x1000, 4 at
    /// 0x2000), and a store.
    fn new() -> Self {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-workspaces");
        let home = base.join(format!("promotion-{}", uuid::Uuid::new_v4()));
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
        f.ok(&[
            "node", "create", "fixture", "--parent", "2", "--name", "F1", "--rva", "1000",
        ]);
        f.ok(&[
            "node", "create", "fixture", "--parent", "2", "--name", "F2", "--rva", "2000",
        ]);
        f.ok(&["store", "init", "fixture"]);
        f
    }
    fn command(&self, args: &[&str]) -> std::process::Command {
        let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_research"));
        c.env("SCOPEWORK_COORDINATOR", "off")
            .env("SCOPEWORK_STORE_SEGMENT_BYTES", "3000")
            .arg("--home")
            .arg(&self.home)
            .args(args);
        c
    }
    fn ok(&self, args: &[&str]) -> Value {
        let o = self.command(args).output().unwrap();
        assert!(
            o.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        serde_json::from_slice(&o.stdout).unwrap()
    }
    fn fails(&self, args: &[&str]) -> String {
        let o = self.command(args).output().unwrap();
        assert!(!o.status.success(), "{args:?} succeeded");
        String::from_utf8_lossy(&o.stderr).into_owned()
    }
    fn project(&self) -> PathBuf {
        self.home.join("projects/fixture")
    }
    fn revision(&self) -> String {
        self.ok(&["project", "revision", "fixture"])["revision"].to_string()
    }
    fn file(&self, name: &str, text: &str) -> String {
        let path = self.home.join(name);
        fs::write(&path, text).unwrap();
        path.to_string_lossy().into_owned()
    }
    fn save_args(&self, id: &str, text: &str) -> Vec<String> {
        let body = self.file("body.md", text);
        let evidence = self.file("evidence.md", "Fixture evidence.");
        let token = self.ok(&["node", "show", "fixture", id])["token"]
            .as_str()
            .unwrap()
            .to_string();
        [
            "node",
            "save",
            "fixture",
            id,
            "--body-file",
            &body,
            "--evidence-file",
            &evidence,
            "--expect",
            &token,
        ]
        .map(String::from)
        .to_vec()
    }
    fn save(&self, id: &str, text: &str) {
        let args = self.save_args(id, text);
        self.ok(&args.iter().map(String::as_str).collect::<Vec<_>>());
    }
    fn complete(&self, id: &str) {
        let token = self.ok(&["node", "show", "fixture", id])["completion_token"]
            .as_str()
            .unwrap()
            .to_string();
        self.ok(&[
            "node",
            "complete",
            "fixture",
            id,
            "--expect",
            &token,
            "--reviewed",
        ]);
    }
    fn effective(&self, id: &str) -> Value {
        self.ok(&["node", "show", "fixture", id])["effective_status"].clone()
    }
    fn ledger_rows(&self) -> Vec<Value> {
        let path = self.project().join("tools/profiles/annotations.json");
        let ledger: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        ledger["annotations"].as_array().unwrap().clone()
    }
    fn receipts(&self) -> usize {
        fs::read_dir(
            self.project()
                .join("tools/workspaces/catalog-transactions-v4"),
        )
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("revision-"))
        .count()
    }
    fn ledger(&self, rows: Value) {
        let path = self.project().join("tools/profiles/annotations.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            serde_json::to_vec(&json!({"schema": 1, "annotations": rows, "types": []})).unwrap(),
        )
        .unwrap();
    }
    fn promote(&self) -> Value {
        let r = self.revision();
        self.ok(&["store", "promote", "fixture", "--expect-revision", &r])
    }
    /// The store verifies and its head is the current catalog.
    fn consistent(&self) {
        let verify = self.ok(&["store", "verify", "fixture"]);
        assert_eq!(verify["ok"], true, "{verify}");
        assert_eq!(verify["current"], true, "{verify}");
        assert!(
            !self
                .project()
                .join("tools/workspaces/catalog-transactions-v4/pending")
                .exists()
        );
    }
}

#[test]
fn promotion_refuses_until_the_store_is_current_verified_and_free_of_conflicts() {
    let f = Fixture::new();
    assert!(
        f.fails(&["store", "promote", "fixture", "--expect-revision", "1"])
            .contains("Project changed")
    );
    f.ledger(
        json!([{"kind": "function-name", "rva": "0x00001000", "value": "Other", "reason": "r"}]),
    );
    let r = f.revision();
    let refused = f.fails(&["store", "promote", "fixture", "--expect-revision", &r]);
    assert!(refused.contains("conflicts"), "{refused}");
    f.ledger(json!([{"kind": "function-name", "rva": "0x00001000", "value": "F1", "reason": "r"}]));
    // A commit since the last sync: promotion syncs first.
    f.save("3", "Before promotion.");
    let promoted = f.promote();
    assert_eq!(promoted["layout_version"], 8, "{promoted}");
    assert!(f.project().join("store/promotion/project.json").is_file());
    assert!(
        f.project()
            .join("store/promotion/annotations.json")
            .is_file()
    );
    let r = f.revision();
    assert!(
        f.fails(&["store", "promote", "fixture", "--expect-revision", &r])
            .contains("already promoted")
    );
    f.consistent();
    // A promoted project's store is written by its commits; there is nothing to follow.
    assert_eq!(f.ok(&["store", "sync", "fixture"])["up_to_date"], true);
}

#[test]
fn a_promoted_project_commits_through_its_store_without_receipts() {
    let f = Fixture::new();
    f.promote();
    let receipts = f.receipts();

    f.save("3", "Saved after promotion.");
    let token = f.ok(&["node", "show", "fixture", "3"])["structural_token"]
        .as_str()
        .unwrap()
        .to_string();
    f.ok(&["deps", "set", "fixture", "3", "--expect", &token, "2000"]);
    let r = f.revision();
    f.ok(&[
        "project",
        "set-scope",
        "fixture",
        "--scope",
        "Rendering",
        "--expect-revision",
        &r,
    ]);
    // A batch that renames and moves.
    f.ok(&[
        "node", "create", "fixture", "--parent", "1", "--name", "Other",
    ]);
    let r: u64 = f.revision().parse().unwrap();
    let batch = f.file(
        "batch.json",
        &json!({
            "schema": 1, "binary": "synthetic", "expect_revision": r,
            "update": [{"id": 4, "name": "F2Renamed", "parent": 5}],
        })
        .to_string(),
    );
    f.ok(&["node", "batch", "fixture", "--file", &batch]);
    // Many commits: the store starts new segments as its log grows.
    for i in 0..6 {
        f.save(
            "3",
            &format!("Revision {i} of the understanding, long enough to grow the log."),
        );
    }
    assert_eq!(f.receipts(), receipts, "no receipt after promotion");
    f.consistent();
    let status = f.ok(&["store", "status", "fixture"]);
    assert!(status["segments"].as_u64().unwrap() > 1, "{status}");

    // History answers from the store.
    let history = f.ok(&[
        "history", "show", "fixture", "--only", "catalog", "--limit", "3",
    ]);
    let newest = &history["entries"][0];
    assert_eq!(newest["revision"].to_string(), f.revision(), "{history}");
    let node = f.ok(&["history", "show", "fixture", "--node", "4"]);
    assert!(
        node["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e.to_string().contains("F2Renamed")),
        "{node}"
    );
    let stored = f.ok(&["store", "history", "fixture", "4"]);
    assert_eq!(
        stored["revisions"][0]["name"]["after"], "F2Renamed",
        "{stored}"
    );

    // An import publication is a commit of the store too.
    let r: u64 = f.revision().parse().unwrap();
    let import = f.file(
        "import.json",
        &json!({
            "schema": 1, "binary": "synthetic", "expect_revision": r,
            "create": [{"parent": 2, "name": "Imported", "rva": 12288}],
        })
        .to_string(),
    );
    f.ok(&["node", "batch", "fixture", "--file", &import]);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !f
        .ok(&["project", "show", "fixture"])
        .to_string()
        .contains("Imported")
    {
        assert!(
            std::time::Instant::now() < deadline,
            "import never published"
        );
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    assert_eq!(f.receipts(), receipts);
    f.consistent();
}

#[test]
fn an_interrupted_promoted_commit_is_finished_by_the_next_open() {
    let f = Fixture::new();
    f.promote();
    for point in ["journal", "store", "catalog"] {
        let before: u64 = f.revision().parse().unwrap();
        let args = f.save_args("3", &format!("Interrupted at {point}."));
        let failed = f
            .command(&args.iter().map(String::as_str).collect::<Vec<_>>())
            .env("SCOPEWORK_CATALOG_FAIL_AT", point)
            .output()
            .unwrap();
        assert_eq!(failed.status.code(), Some(99), "{point}");
        // The next command recovers the pending commit: the save is there, once.
        let shown = f.ok(&["node", "show", "fixture", "3"]);
        assert!(
            shown["body"]
                .as_str()
                .unwrap()
                .contains(&format!("Interrupted at {point}.")),
            "{point}: {shown}"
        );
        assert_eq!(f.revision().parse::<u64>().unwrap(), before + 1, "{point}");
        f.consistent();
    }
}

fn sha(text: &str) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(text.as_bytes()))
}
const F1: &str = "architectures/000001/000002/00001000/research.md";
const F2: &str = "architectures/000001/000002/00002000/research.md";

#[test]
fn promotion_converts_acceptance_to_the_research_body_and_its_ledger_pins() {
    let f = Fixture::new();
    f.save("3", "Researched.");
    f.complete("3");
    f.save("4", "Also researched.");
    f.complete("4");
    // Function 4's report is edited outside Scopework: its acceptance is stale.
    let f2 = f.project().join(F2);
    let edited = fs::read_to_string(&f2)
        .unwrap()
        .replace("Also researched.", "Edited outside.");
    fs::write(&f2, edited).unwrap();
    assert_eq!(f.effective("3"), "Complete");
    assert_ne!(f.effective("4"), "Complete");
    let f1_text = fs::read_to_string(f.project().join(F1)).unwrap();
    let marker = f1_text.find("<!-- research-content -->").unwrap();
    f.ledger(json!([
        {"kind": "function-name", "rva": "0x00001000", "value": "F1", "report": F1, "report_sha256": sha(&f1_text), "reason": "r"},
        {"kind": "plate-comment", "rva": "0x00002000", "value": "Note", "report": F2, "report_sha256": "0".repeat(64), "reason": "r"},
    ]));

    let promoted = f.promote();
    assert_eq!(promoted["acceptances_converted"], 1, "{promoted}");
    assert_eq!(promoted["acceptances_stale"], 1, "{promoted}");
    assert_eq!(f.effective("3"), "Complete");
    assert_ne!(f.effective("4"), "Complete");
    let rows = f.ledger_rows();
    assert_eq!(
        rows[0]["report_sha256"],
        sha(&f1_text[marker..]),
        "the pin follows the mapping"
    );
    assert_eq!(
        rows[1]["report_sha256"],
        "0".repeat(64),
        "a stale pin stays stale"
    );
    let kept: Value = serde_json::from_slice(
        &fs::read(f.project().join("store/promotion/acceptance.json")).unwrap(),
    )
    .unwrap();
    assert!(kept["reports"].as_array().unwrap().len() >= 5, "{kept}");

    // A rename rewrites function 3's header, not its research: it stays complete, and its pin
    // still matches.
    let r: u64 = f.revision().parse().unwrap();
    let batch = f.file(
        "rename.json",
        &json!({"schema": 1, "binary": "synthetic", "expect_revision": r,
                "update": [{"id": 3, "name": "F1Renamed"}]})
        .to_string(),
    );
    f.ok(&["node", "batch", "fixture", "--file", &batch]);
    assert!(
        fs::read_to_string(f.project().join(F1))
            .unwrap()
            .contains("# F1Renamed")
    );
    assert_eq!(f.effective("3"), "Complete");
    let stale = f.ok(&["annotate", "stale", "fixture"]);
    assert_eq!(stale["acceptance"], "body");
    assert_eq!(stale["stale"], 1, "{stale}");
    assert_eq!(stale["listed"][0]["report"], F2);

    // Annotations change through Scopework, each change recorded in the store with its author.
    let set = f.ok(&[
        "annotate",
        "set",
        "fixture",
        "--kind",
        "label",
        "--rva",
        "2000",
        "--value",
        "Table",
        "--report",
        F2,
        "--reason",
        "Seen in the dump.",
    ]);
    assert_eq!(set["change"]["set"]["rva"], "0x00002000", "{set}");
    assert_eq!(set["store"]["rows"]["added"], 1, "{set}");
    let history = f.ok(&["store", "annotations", "fixture", "2000", "--kind", "label"]);
    assert_eq!(history["current"][0]["value"], "Table", "{history}");
    f.ok(&[
        "annotate",
        "remove",
        "fixture",
        "--kind",
        "plate-comment",
        "--rva",
        "0x2000",
    ]);
    assert!(
        f.fails(&[
            "annotate",
            "remove",
            "fixture",
            "--kind",
            "plate-comment",
            "--rva",
            "2000"
        ])
        .contains("No plate-comment row")
    );
    assert!(
        f.fails(&[
            "annotate",
            "set",
            "fixture",
            "--kind",
            "label",
            "--rva",
            "2000",
            "--value",
            "X",
            "--report",
            "nowhere.md",
            "--reason",
            "r",
        ])
        .contains("not a record's report")
    );
    assert_eq!(f.ok(&["annotate", "stale", "fixture"])["stale"], 0);

    // A tool's own rewrite is recorded at the next sync.
    let mut rows = f.ledger_rows();
    rows.push(json!({"kind": "label", "rva": "0x00003000", "value": "ByTool", "reason": "tool"}));
    f.ledger(json!(rows));
    let sync = f.ok(&["store", "sync", "fixture"]);
    assert_eq!(sync["ledger"]["rows"]["added"], 1, "{sync}");
    f.consistent();
}

#[test]
fn annotations_change_through_the_project_tools_until_promotion() {
    let f = Fixture::new();
    let refused = f.fails(&[
        "annotate", "set", "fixture", "--kind", "label", "--rva", "1000", "--value", "X",
        "--report", F1, "--reason", "r",
    ]);
    assert!(refused.contains("not_promoted"), "{refused}");
    f.ledger(json!([{"kind": "function-name", "rva": "0x00001000", "value": "F1", "report": F1, "report_sha256": "0".repeat(64), "reason": "r"}]));
    let stale = f.ok(&["annotate", "stale", "fixture"]);
    assert_eq!(stale["acceptance"], "whole_file");
    assert_eq!(stale["stale"], 1);
}
