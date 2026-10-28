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
