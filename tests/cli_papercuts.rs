//! Small CLI answers that used to leave agents guessing: why `node prune` skipped a node, a
//! revision precondition on `project set-binary`, and identity amendments after enrolment.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};

const SHA: &str = "aa63a5750d60ef16746c686b3d5e26876d98953eab08b1c026cd0faf78e88cb8";

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
        let home = base.join(format!("papercuts-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&home).unwrap();
        let f = Self { home };
        let binary = format!("Build 1; SHA-256 {SHA}; image base 0x00400000");
        f.ok(&[
            "project", "create", "fixture", "--name", "Fixture", "--binary", &binary,
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
        f
    }
    fn run(&self, args: &[&str]) -> std::process::Output {
        std::process::Command::new(env!("CARGO_BIN_EXE_research"))
            .env("SCOPEWORK_COORDINATOR", "off")
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
    fn err(&self, args: &[&str]) -> String {
        let o = self.run(args);
        assert!(!o.status.success(), "{args:?} unexpectedly succeeded");
        String::from_utf8_lossy(&o.stderr).into_owned()
    }
    fn project(&self) -> PathBuf {
        self.home.join("projects/fixture")
    }
    fn revision(&self) -> String {
        self.ok(&["project", "revision", "fixture"])["revision"].to_string()
    }
    fn save(&self, id: &str, body: &str) {
        let b = self.home.join(format!("body-{id}.md"));
        let e = self.home.join(format!("evidence-{id}.md"));
        fs::write(&b, body).unwrap();
        fs::write(&e, "Fixture evidence.").unwrap();
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
            b.to_str().unwrap(),
            "--evidence-file",
            e.to_str().unwrap(),
            "--expect",
            &token,
        ]);
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
    fn prune_preview(&self, ids: &[u64], recursive: bool) -> Value {
        let binary = self.ok(&["project", "revision", "fixture"])["binary"].clone();
        let request = serde_json::json!({"schema":1,"binary":binary,"expect_revision":self.revision().parse::<u64>().unwrap(),
            "ids":ids,"recursive":recursive});
        let path = self.home.join("prune.json");
        fs::write(&path, request.to_string()).unwrap();
        self.ok(&["node", "prune", "fixture", "--file", path.to_str().unwrap()])
    }
}

fn excluded(preview: &Value, id: u64) -> Value {
    preview["excluded"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == id)
        .cloned()
        .unwrap_or(Value::Null)
}

#[test]
fn prune_preview_says_why_requested_nodes_were_skipped() {
    let f = Fixture::new();
    let preview = f.prune_preview(&[1, 2, 3], false);
    assert!(preview["candidates"].as_array().unwrap().is_empty());
    assert_eq!(excluded(&preview, 1)["reason"], "has-children");
    assert_eq!(excluded(&preview, 1)["children"], serde_json::json!([2]));
    assert_eq!(excluded(&preview, 2)["reason"], "has-children");
    assert_eq!(excluded(&preview, 3)["reason"], "kind");
    // An empty chunk with an authored report is protected and says so.
    f.ok(&[
        "node", "create", "fixture", "--parent", "1", "--name", "Written",
    ]);
    f.save("4", "Synthesis text.");
    let preview = f.prune_preview(&[4], false);
    assert_eq!(excluded(&preview, 4)["reason"], "authored-content");
    // An empty, unwritten chunk is a candidate and is not listed as excluded.
    f.ok(&[
        "node", "create", "fixture", "--parent", "1", "--name", "Empty",
    ]);
    let preview = f.prune_preview(&[5], false);
    assert_eq!(preview["candidates"][0]["id"], 5);
    assert!(excluded(&preview, 5).is_null());
}

#[test]
fn set_binary_honours_the_revision_precondition() {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-workspaces");
    let home = base.join(format!("papercuts-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&home).unwrap();
    let f = Fixture { home };
    f.ok(&["project", "create", "fixture", "--name", "Fixture"]);
    let current = f.revision();
    let stale = (current.parse::<u64>().unwrap() + 7).to_string();
    let error = f.err(&[
        "project",
        "set-binary",
        "fixture",
        "--binary",
        "Build 2",
        "--expect-revision",
        &stale,
    ]);
    assert!(error.contains("Project changed"), "{error}");
    f.ok(&[
        "project",
        "set-binary",
        "fixture",
        "--binary",
        "Build 2",
        "--expect-revision",
        &current,
    ]);
    assert_eq!(
        f.ok(&["project", "revision", "fixture"])["binary"],
        "Build 2"
    );
}

#[test]
fn identity_amendments_keep_completion_and_ledger_pins() {
    let f = Fixture::new();
    f.save("3", "Function behaviour.");
    f.complete("3");
    // A second function whose prerequisite is the first: its stamp must survive the amendment.
    f.ok(&[
        "node", "create", "fixture", "--parent", "2", "--name", "F2", "--rva", "2000",
    ]);
    let structural = f.ok(&["node", "show", "fixture", "4"])["structural_token"]
        .as_str()
        .unwrap()
        .to_string();
    f.ok(&[
        "deps",
        "set",
        "fixture",
        "4",
        "1000",
        "--expect",
        &structural,
    ]);
    f.save("4", "Uses F1.");
    f.complete("4");
    let report = f
        .project()
        .join("architectures/000001/000002/00001000/research.md");
    let digest = |path: &PathBuf| format!("{:x}", Sha256::digest(fs::read(path).unwrap()));
    let before_hash = digest(&report);
    let ledger = f.project().join("tools/profiles/annotations.json");
    fs::create_dir_all(ledger.parent().unwrap()).unwrap();
    fs::write(&ledger, format!(
        "{{\n  \"schema\": 1,\n  \"annotations\": [\n    {{\n      \"report\": \"architectures/000001/000002/00001000/research.md\",\n      \"report_sha256\": \"{before_hash}\",\n      \"kind\": \"function-name\"\n    }}\n  ]\n}}\n"
    )).unwrap();
    // Without a precondition, or dropping a pinned value, the identity stays locked.
    let error = f.err(&[
        "project",
        "set-binary",
        "fixture",
        "--binary",
        "Build 1; image base 0x00400000",
    ]);
    assert!(error.contains("--expect-revision"), "{error}");
    let revision = f.revision();
    let error = f.err(&[
        "project",
        "set-binary",
        "fixture",
        "--binary",
        "Build 1; image base 0x00400000",
        "--expect-revision",
        &revision,
    ]);
    assert!(
        error.contains(SHA),
        "the refusal names the missing value: {error}"
    );
    // A wording fix plus a new mapping is an amendment: completion and the authored text survive.
    let amended = format!(
        "WoW build 1; SHA-256 {SHA}; PE32 x86; image base 0x00400000; front end mapped at 0x00800000"
    );
    f.ok(&[
        "project",
        "set-binary",
        "fixture",
        "--binary",
        &amended,
        "--expect-revision",
        &revision,
    ]);
    let shown = f.ok(&["node", "show", "fixture", "3"]);
    assert_eq!(shown["effective_status"], "Complete");
    let dependent = f.ok(&["node", "show", "fixture", "4"]);
    assert_eq!(
        dependent["effective_status"], "Complete",
        "prerequisite stamp carried over"
    );
    assert_eq!(
        shown["body"].as_str().unwrap().trim(),
        "Function behaviour."
    );
    let text = fs::read_to_string(&report).unwrap();
    assert!(
        text.contains(&amended),
        "the header shows the amended identity"
    );
    let after_hash = digest(&report);
    assert_ne!(before_hash, after_hash);
    let pinned = fs::read_to_string(&ledger).unwrap();
    assert!(
        pinned.contains(&after_hash) && !pinned.contains(&before_hash),
        "ledger row re-pinned: {pinned}"
    );
    assert!(
        pinned.contains("\"kind\": \"function-name\""),
        "the tool's formatting is kept"
    );
}

#[test]
fn header_text_and_unchanged_identities_are_handled() {
    let home = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/test-workspaces")
        .join(format!("header-text-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&home).unwrap();
    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_research"))
            .env("SCOPEWORK_COORDINATOR", "off")
            .arg("--home")
            .arg(&home)
            .args(args)
            .output()
            .unwrap()
    };
    let marker = "build 1\n<!-- research-content -->\nsha 00";
    for args in [
        vec![
            "project", "create", "bad", "--name", "Bad", "--binary", marker,
        ],
        vec!["project", "create", "bad", "--name", "Two\nlines"],
    ] {
        let o = run(&args);
        assert!(!o.status.success(), "{args:?}");
        assert!(String::from_utf8_lossy(&o.stderr).contains("one line"));
    }
    assert!(
        run(&["project", "create", "p", "--name", "P", "--binary", "b"])
            .status
            .success()
    );
    let o = run(&["project", "set-binary", "p", "--binary", marker]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("one line"));
    std::fs::remove_dir_all(&home).unwrap();
}
