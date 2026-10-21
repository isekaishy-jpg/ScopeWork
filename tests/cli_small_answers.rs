//! `project revision` and the `status` / `node list` filters return only what was asked for.
use serde_json::Value;
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
    fn new() -> Self {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-workspaces");
        let home = base.join(format!("answers-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&home).unwrap();
        let f = Self { home };
        for args in [
            &[
                "project",
                "create",
                "fixture",
                "--name",
                "Fixture",
                "--binary",
                "synthetic",
            ][..],
            &[
                "node", "create", "fixture", "--parent", "0", "--name", "Arch",
            ],
            &[
                "node", "create", "fixture", "--parent", "1", "--name", "Chunk",
            ],
            &[
                "node", "create", "fixture", "--parent", "2", "--name", "F1", "--rva", "1000",
            ],
            &[
                "node", "create", "fixture", "--parent", "2", "--name", "F2", "--rva", "2000",
            ],
        ] {
            f.ok(args);
        }
        f
    }
    fn ok(&self, args: &[&str]) -> Value {
        let o = std::process::Command::new(env!("CARGO_BIN_EXE_research"))
            // These tests cover direct mode; tests/coordinator.rs covers the coordinator.
            .env("SCOPEWORK_COORDINATOR", "off")
            .arg("--home")
            .arg(&self.home)
            .args(args)
            .output()
            .unwrap();
        assert!(
            o.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        serde_json::from_slice(&o.stdout).unwrap()
    }
}

#[test]
fn revision_omits_the_catalog() {
    let f = Fixture::new();
    let rev = f.ok(&["project", "revision", "fixture"]);
    let show = f.ok(&["project", "show", "fixture"]);
    assert_eq!(rev["revision"], show["revision"]);
    assert_eq!(rev["next_id"], show["next_id"]);
    assert_eq!(rev["binary"], "synthetic");
    assert!(rev.get("nodes").is_none());
}

#[test]
fn status_and_list_filters() {
    let f = Fixture::new();
    let functions = f.ok(&["status", "fixture", "--kind", "function"])["records"].clone();
    assert_eq!(functions.as_array().unwrap().len(), 2);
    let children = f.ok(&["node", "list", "fixture", "--parent", "2"]);
    assert_eq!(children.as_array().unwrap().len(), 2);
    // Nothing is written yet, so nothing is ready for review (a record needs research to review).
    let ready = f.ok(&["status", "fixture", "--ready"])["records"].clone();
    assert!(ready.as_array().unwrap().is_empty());
    let summary = f.ok(&["status", "fixture", "--summary"]);
    assert!(summary.get("records").is_none());
    assert_eq!(summary["counts"]["Function"]["NotStarted"], 2);
    assert_eq!(summary["ready_for_review"], 0);
    assert_eq!(summary["ready_for_synthesis"], 0);
    let complete = f.ok(&["node", "list", "fixture", "--status", "complete"]);
    assert!(complete.as_array().unwrap().is_empty());
}

impl Fixture {
    fn write_report(&self, id: &str) {
        let body = self.home.join(format!("body-{id}.md"));
        let evidence = self.home.join(format!("evidence-{id}.md"));
        fs::write(&body, format!("Report for {id}.")).unwrap();
        fs::write(&evidence, "Fixture evidence.").unwrap();
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
            body.to_str().unwrap(),
            "--evidence-file",
            evidence.to_str().unwrap(),
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
}

#[test]
fn readiness_needs_written_research_and_parents_wait_for_synthesis() {
    let f = Fixture::new();
    f.write_report("3");
    let ready = f.ok(&["status", "fixture", "--ready"])["records"].clone();
    let ids: Vec<_> = ready
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_u64().unwrap())
        .collect();
    assert_eq!(
        ids,
        [3],
        "only the function with a written report is ready for review"
    );
    f.write_report("4");
    f.complete("3");
    f.complete("4");
    // Both functions are complete and the chunk's own report is empty: ready for synthesis, not review.
    let summary = f.ok(&["status", "fixture", "--summary"]);
    assert_eq!(summary["ready_for_review"], 0);
    assert_eq!(summary["ready_for_synthesis"], 1);
    let synthesis = f.ok(&["status", "fixture", "--ready-for-synthesis"])["records"].clone();
    let ids: Vec<_> = synthesis
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids, [2]);
    assert_eq!(synthesis[0]["ready_for_review"], false);
    // Once the chunk's report is written it is ready for review instead.
    f.write_report("2");
    let summary = f.ok(&["status", "fixture", "--summary"]);
    assert_eq!(summary["ready_for_review"], 1);
    assert_eq!(summary["ready_for_synthesis"], 0);
}
