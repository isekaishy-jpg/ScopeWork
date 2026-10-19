//! Evidence-only saves keep Not Started, status reset returns started records, and review flags gate completion.
use serde_json::{Value, json};
use std::{fs, path::PathBuf, process::Output};

struct Fixture {
    home: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-workspaces");
        let home = base.join(format!("flags-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&home).unwrap();
        let f = Self { home };
        f.ok(&[
            "project",
            "create",
            "fixture",
            "--name",
            "Fixture",
            "--binary",
            "synthetic-build",
        ]);
        f.ok(&[
            "node",
            "create",
            "fixture",
            "--parent",
            "0",
            "--name",
            "Architecture",
        ]); // 1
        f.ok(&[
            "node", "create", "fixture", "--parent", "1", "--name", "Chunk",
        ]); // 2
        f.ok(&[
            "node", "create", "fixture", "--parent", "2", "--name", "Function", "--rva", "1000",
        ]); // 3
        f
    }
    fn run(&self, args: &[&str]) -> Output {
        std::process::Command::new(env!("CARGO_BIN_EXE_research"))
            // These tests cover direct mode; tests/coordinator.rs covers the coordinator.
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
    fn status(&self, id: &str) -> String {
        self.ok(&["node", "show", "fixture", id])["node"]["status"]
            .as_str()
            .unwrap()
            .into()
    }
    fn save(&self, id: &str, body: &str, evidence_only: bool) -> Output {
        let token = self.ok(&["node", "show", "fixture", id])["token"]
            .as_str()
            .unwrap()
            .to_string();
        let (b, e) = (self.home.join("body.md"), self.home.join("evidence.md"));
        fs::write(&b, body).unwrap();
        fs::write(&e, "Synthetic evidence").unwrap();
        let mut args = vec![
            "node",
            "save",
            "fixture",
            id,
            "--expect",
            &token,
            "--body-file",
            b.to_str().unwrap(),
            "--evidence-file",
            e.to_str().unwrap(),
        ];
        if evidence_only {
            args.push("--evidence-only");
        }
        self.run(&args)
    }
    fn request(&self, name: &str, value: Value) -> String {
        let path = self.home.join(name);
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        path.to_str().unwrap().into()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-workspaces")
            .canonicalize()
            .unwrap();
        let target = self.home.canonicalize().unwrap();
        assert!(target.starts_with(&base) && target != base);
        fs::remove_dir_all(target).unwrap();
    }
}

#[test]
fn evidence_only_saves_and_status_reset_keep_research_not_started() {
    let f = Fixture::new();
    assert!(
        f.save("3", "Generated inventory evidence", true)
            .status
            .success()
    );
    assert_eq!(f.status("3"), "NotStarted");
    assert_eq!(
        f.ok(&["node", "show", "fixture", "3"])["body"],
        "Generated inventory evidence"
    );
    // An ordinary save still marks research started; reset returns it.
    assert!(
        f.save("3", "Authored understanding", false)
            .status
            .success()
    );
    assert_eq!(f.status("3"), "InProgress");
    let p = f.ok(&["project", "show", "fixture"]);
    let file = f.request("reset.json", json!({"schema":1,"binary":p["binary"],"expect_revision":p["revision"],"all_in_progress":true}));
    assert_eq!(
        f.ok(&["node", "reset-status", "fixture", "--file", &file])["reset"],
        json!([3])
    );
    assert_eq!(f.status("3"), "NotStarted");
    assert_eq!(
        f.ok(&["node", "show", "fixture", "3"])["body"],
        "Authored understanding"
    );
    // Both ids and all_in_progress together are rejected.
    let p = f.ok(&["project", "show", "fixture"]);
    let bad = f.request("bad.json", json!({"schema":1,"binary":p["binary"],"expect_revision":p["revision"],"ids":[3],"all_in_progress":true}));
    assert!(
        !f.run(&["node", "reset-status", "fixture", "--file", &bad])
            .status
            .success()
    );
}

#[test]
fn review_flags_block_completion_and_follow_deletion() {
    let f = Fixture::new();
    assert!(
        f.save("3", "Authored understanding", false)
            .status
            .success()
    );
    let shown = f.ok(&[
        "node",
        "flag",
        "fixture",
        "3",
        "--reason",
        "Check the second branch",
    ]);
    assert_eq!(shown["review_flag"]["reason"], "Check the second branch");
    assert_eq!(f.ok(&["node", "flagged", "fixture"])["flagged"][0]["id"], 3);
    let status = f.ok(&["status", "fixture"]);
    assert_eq!(status["review_flags"], 1);
    let token = f.ok(&["node", "show", "fixture", "3"])["completion_token"]
        .as_str()
        .unwrap()
        .to_string();
    let refused = f.run(&[
        "node",
        "complete",
        "fixture",
        "3",
        "--expect",
        &token,
        "--reviewed",
    ]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("flagged for review"));
    assert_eq!(f.status("3"), "InProgress");
    f.ok(&["node", "unflag", "fixture", "3"]);
    assert!(f.ok(&["node", "show", "fixture", "3"])["review_flag"].is_null());
    let token = f.ok(&["node", "show", "fixture", "3"])["completion_token"]
        .as_str()
        .unwrap()
        .to_string();
    f.ok(&[
        "node",
        "complete",
        "fixture",
        "3",
        "--expect",
        &token,
        "--reviewed",
    ]);
    assert_eq!(f.status("3"), "Complete");
    // Deleting a flagged branch removes its flags with it.
    f.ok(&[
        "node",
        "flag",
        "fixture",
        "2",
        "--reason",
        "Ownership doubtful",
    ]);
    let p = f.ok(&["project", "show", "fixture"]);
    let file = f.request(
        "remove.json",
        json!({"schema":1,"binary":p["binary"],"expect_revision":p["revision"],"ids":[2]}),
    );
    let preview = f.ok(&["node", "delete", "fixture", "--file", &file]);
    f.ok(&[
        "node",
        "delete",
        "fixture",
        "--file",
        &file,
        "--expect",
        preview["token"].as_str().unwrap(),
    ]);
    assert_eq!(f.ok(&["node", "flagged", "fixture"])["flagged"], json!([]));
}
