//! The project store follows the change receipts beside the authoritative files.
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
    /// A project with an architecture (1), a chunk (2) and two functions (3, 4).
    fn new() -> Self {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-workspaces");
        let home = base.join(format!("store-{}", uuid::Uuid::new_v4()));
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
        f
    }
    fn run(&self, args: &[&str]) -> std::process::Output {
        std::process::Command::new(env!("CARGO_BIN_EXE_research"))
            .env("SCOPEWORK_COORDINATOR", "off")
            // Small segments, so a short history reaches checkpoints inside it.
            .env("SCOPEWORK_STORE_SEGMENT_BYTES", "1500")
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
    fn fails(&self, args: &[&str]) -> String {
        let o = self.run(args);
        assert!(!o.status.success(), "{args:?} succeeded");
        String::from_utf8_lossy(&o.stderr).into_owned()
    }
    fn project(&self) -> PathBuf {
        self.home.join("projects/fixture")
    }
    fn revision(&self) -> u64 {
        self.ok(&["project", "revision", "fixture"])["revision"]
            .as_u64()
            .unwrap()
    }
    fn file(&self, name: &str, text: &str) -> String {
        let path = self.home.join(name);
        fs::write(&path, text).unwrap();
        path.to_string_lossy().into_owned()
    }
    /// Saves a report for `id`: a commit with report texts.
    fn save(&self, id: &str, text: &str) {
        let body = self.file("body.md", text);
        let evidence = self.file("evidence.md", "Fixture evidence.");
        let token = self.ok(&["node", "show", "fixture", id])["token"]
            .as_str()
            .unwrap()
            .to_string();
        self.ok(&[
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
        ]);
    }
    fn logs(&self) -> Vec<PathBuf> {
        let mut logs: Vec<PathBuf> = fs::read_dir(self.project().join("store"))
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "log"))
            .collect();
        logs.sort();
        logs
    }
    fn verified(&self) -> Value {
        let verify = self.ok(&["store", "verify", "fixture"]);
        assert_eq!(verify["ok"], true, "{verify}");
        verify
    }
}

#[test]
fn init_imports_the_receipt_history_and_sync_follows_new_commits() {
    let f = Fixture::new();
    assert!(f.fails(&["store", "sync", "fixture"]).contains("no_store"));
    for i in 0..6 {
        f.save("3", &format!("Draft {i} of the first function."));
    }
    let revision = f.revision();
    let init = f.ok(&["store", "init", "fixture"]);
    assert_eq!(init["revision"], revision);
    assert!(init["commits_added"].as_u64().unwrap() >= 6, "{init}");
    // Small segments: checkpoints inside the history, each verified when written.
    assert!(
        init["segments_started"].as_array().unwrap().len() > 1,
        "{init}"
    );
    let status = f.ok(&["store", "status", "fixture"]);
    assert_eq!(status["current"], true, "{status}");
    assert_eq!(status["behind"], 0);
    assert!(status["gaps"].as_array().unwrap().is_empty());
    let verify = f.verified();
    assert_eq!(verify["current"], true);
    assert!(verify["texts_checked"].as_u64().unwrap() >= 6);
    assert!(
        f.fails(&["store", "init", "fixture"])
            .contains("already has a store")
    );

    // New commits through direct mode; the store is behind until it syncs.
    f.save("4", "The second function.");
    f.save("4", "The second function, revised.");
    assert_eq!(f.ok(&["store", "status", "fixture"])["behind"], 2);
    let sync = f.ok(&["store", "sync", "fixture"]);
    assert_eq!(sync["commits_added"], 2, "{sync}");
    assert!(sync["gap"].is_null(), "{sync}");
    assert_eq!(f.ok(&["store", "sync", "fixture"])["up_to_date"], true);
    assert_eq!(f.verified()["current"], true);
}

#[test]
fn a_torn_tail_is_repaired_and_damage_is_reported_without_changing_the_files() {
    let f = Fixture::new();
    f.save("3", "First.");
    f.ok(&["store", "init", "fixture"]);
    f.save("3", "Second.");
    f.ok(&["store", "sync", "fixture"]);
    // An interrupted append: a frame header with only part of its payload.
    let last = f.logs().pop().unwrap();
    let mut bytes = fs::read(&last).unwrap();
    let clean = bytes.len();
    bytes.extend_from_slice(&100u32.to_le_bytes());
    bytes.extend_from_slice(&[0, 0, 0, 0, b'{']);
    fs::write(&last, &bytes).unwrap();
    let verify = f.ok(&["store", "verify", "fixture"]);
    assert_eq!(verify["ok"], true, "{verify}");
    assert_eq!(verify["notes"].as_array().unwrap().len(), 1, "{verify}");
    f.save("3", "Third.");
    let sync = f.ok(&["store", "sync", "fixture"]);
    assert_eq!(sync["commits_added"], 1, "{sync}");
    assert!(fs::read(&last).unwrap().len() > clean);
    assert!(f.verified()["notes"].as_array().unwrap().is_empty());

    // A changed byte inside a frame: verify reports it, sync refuses, the catalog is untouched.
    let first = f.logs()[0].clone();
    let mut bytes = fs::read(&first).unwrap();
    let at = bytes.len() / 2;
    bytes[at] ^= 0x20;
    fs::write(&first, &bytes).unwrap();
    let catalog = fs::read(f.project().join("project.json")).unwrap();
    let verify = f.ok(&["store", "verify", "fixture"]);
    assert_eq!(verify["ok"], false, "{verify}");
    f.save("4", "After the damage.");
    let catalog_after_save = fs::read(f.project().join("project.json")).unwrap();
    assert_ne!(
        catalog, catalog_after_save,
        "commits go on with a damaged store"
    );
    assert!(
        f.fails(&["store", "sync", "fixture"])
            .contains("store_damaged")
    );
}

#[test]
fn a_change_without_a_receipt_is_a_gap_and_the_next_commits_connect_again() {
    let f = Fixture::new();
    f.ok(&["store", "init", "fixture"]);
    // A creation-only batch runs as an import, whose publication leaves no catalog receipt.
    let request = json!({"schema":1,"binary":"synthetic","expect_revision":f.revision(),
        "create":[{"parent":2,"name":"Imported","rva":12288}],"update":[],"reorder":[]});
    let file = f.file("import.json", &request.to_string());
    f.ok(&["node", "batch", "fixture", "--file", &file]);
    let imported = f.revision();
    let sync = f.ok(&["store", "sync", "fixture"]);
    assert_eq!(sync["revision"], imported);
    assert!(!sync["gap"].is_null(), "{sync}");
    let status = f.ok(&["store", "status", "fixture"]);
    assert_eq!(status["gaps"].as_array().unwrap().len(), 1, "{status}");
    assert_eq!(status["current"], true);
    f.save("3", "After the import.");
    let sync = f.ok(&["store", "sync", "fixture"]);
    assert_eq!(sync["commits_added"], 1, "{sync}");
    assert!(sync["gap"].is_null(), "{sync}");
    f.verified();
}

#[test]
fn history_show_and_catalog_answer_for_any_stored_revision() {
    let f = Fixture::new();
    f.save("3", "First understanding.");
    let first = f.revision();
    let first_catalog = fs::read(f.project().join("project.json")).unwrap();
    f.save("3", "Second understanding.");
    let token = f.ok(&["node", "show", "fixture", "3"])["structural_token"]
        .as_str()
        .unwrap()
        .to_string();
    f.ok(&["deps", "set", "fixture", "3", "--expect", &token, "2000"]);
    let revision = f.revision();
    f.ok(&["store", "init", "fixture"]);

    let history = f.ok(&["store", "history", "fixture", "3"]);
    assert_eq!(history["backend"], "store");
    let revisions = history["revisions"].as_array().unwrap();
    // Newest first: the prerequisite, the second save, the first save, the creation.
    assert_eq!(revisions[0]["revision"], revision, "{history}");
    assert_eq!(revisions[0]["prerequisites"]["added"], json!([4]));
    assert!(
        revisions[1]["report"]["after_sha256"].is_string(),
        "{history}"
    );
    assert_eq!(revisions.last().unwrap()["created"], true, "{history}");

    // The record and report as they were after the first save.
    let shown = f.ok(&[
        "store",
        "show",
        "fixture",
        "3",
        "--revision",
        &first.to_string(),
    ]);
    assert_eq!(shown["record"]["status"], "InProgress");
    assert_eq!(shown["report"]["source"], "store");
    let text = shown["report"]["text"].as_str().unwrap();
    assert!(text.contains("First understanding."), "{text}");
    // Function 4's report never changed: the current file holds it.
    let unchanged = f.ok(&[
        "store",
        "show",
        "fixture",
        "4",
        "--revision",
        &first.to_string(),
    ]);
    assert_eq!(unchanged["report"]["source"], "current_file");

    // The whole catalog at that revision, byte for byte, into a new file only.
    let out = f.home.join("catalog-at-first.json");
    let catalog = f.ok(&[
        "store",
        "catalog",
        "fixture",
        "--revision",
        &first.to_string(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert_eq!(catalog["verified"], true);
    assert_eq!(fs::read(&out).unwrap(), first_catalog);
    assert!(
        f.fails(&[
            "store",
            "catalog",
            "fixture",
            "--revision",
            &first.to_string(),
            "--out",
            out.to_str().unwrap(),
        ])
        .contains("Cannot create")
    );
    let ahead = (revision + 5).to_string();
    assert!(
        f.fails(&["store", "show", "fixture", "3", "--revision", &ahead])
            .contains("store sync")
    );
}
