//! Quarantined records count as complete, are sealed against structural changes unless the
//! human directed them, keep editable text, and return to their own status when lifted.
use serde_json::Value;
use std::{fs, path::PathBuf, time::Duration, time::Instant};

struct Fixture {
    home: PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.run_in("auto", &["coordinator", "stop"]);
        let deadline = Instant::now() + Duration::from_secs(10);
        while fs::remove_dir_all(&self.home).is_err() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}
impl Fixture {
    /// Architecture 1 with chunk 2 (function 3, 0x1000, not started) and chunk 4 (function 5,
    /// 0x2000, completed).
    fn new() -> Self {
        let home = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-workspaces")
            .join(format!("quarantine-{}", uuid::Uuid::new_v4()));
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
            "node", "create", "fixture", "--parent", "1", "--name", "Voice",
        ]);
        f.ok(&[
            "node", "create", "fixture", "--parent", "2", "--name", "F1", "--rva", "1000",
        ]);
        f.ok(&[
            "node", "create", "fixture", "--parent", "1", "--name", "Other",
        ]);
        f.ok(&[
            "node", "create", "fixture", "--parent", "4", "--name", "F2", "--rva", "2000",
        ]);
        f.save("5", "F2 behaviour.");
        f.complete("5");
        f.save("4", "Other synthesis.");
        f.complete("4");
        f
    }
    fn run_in(&self, mode: &str, args: &[&str]) -> std::process::Output {
        std::process::Command::new(env!("CARGO_BIN_EXE_research"))
            .env("SCOPEWORK_COORDINATOR", mode)
            .arg("--home")
            .arg(&self.home)
            .args(args)
            .output()
            .unwrap()
    }
    fn ok(&self, args: &[&str]) -> Value {
        let o = self.run_in("off", args);
        assert!(
            o.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        serde_json::from_slice(&o.stdout).unwrap()
    }
    fn refused(&self, args: &[&str]) -> String {
        let o = self.run_in("off", args);
        assert!(!o.status.success(), "{args:?} unexpectedly succeeded");
        String::from_utf8_lossy(&o.stderr).into_owned()
    }
    fn status(&self, id: &str) -> String {
        self.ok(&["node", "show", "fixture", id])["effective_status"]
            .as_str()
            .unwrap()
            .to_string()
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
    fn batch(&self, request: Value, directed: Option<&str>) -> std::process::Output {
        let path = self.home.join("batch.json");
        fs::write(&path, request.to_string()).unwrap();
        let mut args = vec!["node", "batch", "fixture", "--file", path.to_str().unwrap()];
        if let Some(d) = directed {
            args.extend(["--directed-by", d]);
        }
        self.run_in("off", &args)
    }
    fn revision(&self) -> u64 {
        self.ok(&["project", "revision", "fixture"])["revision"]
            .as_u64()
            .unwrap()
    }
}

#[test]
fn a_quarantined_chunk_counts_as_complete_and_its_parent_can_complete() {
    let f = Fixture::new();
    f.save("1", "Architecture synthesis.");
    let token = f.ok(&["node", "show", "fixture", "1"])["completion_token"]
        .as_str()
        .unwrap()
        .to_string();
    let blocked = f.refused(&[
        "node",
        "complete",
        "fixture",
        "1",
        "--expect",
        &token,
        "--reviewed",
    ]);
    assert!(blocked.contains("required child"), "{blocked}");
    f.ok(&["node", "flag", "fixture", "2", "--reason", "Old audit flag"]);
    let shown = f.ok(&[
        "node",
        "quarantine",
        "fixture",
        "2",
        "--origin",
        "human",
        "--reason",
        "Voice code only",
        "--by",
        "tester",
    ]);
    assert_eq!(shown["effective_status"], "Complete");
    assert_eq!(shown["quarantine"]["origin"], "human");
    assert!(
        shown["review_flag"].is_null(),
        "quarantining clears the record's flag"
    );
    assert_eq!(f.status("3"), "Complete", "the chunk covers its function");
    assert_eq!(
        f.ok(&["node", "show", "fixture", "3"])["quarantine"]["node"],
        2
    );
    let summary = f.ok(&["status", "fixture", "--summary"]);
    assert_eq!(summary["quarantined"], 2);
    assert_eq!(
        summary["ready_for_review"], 1,
        "only the architecture's written report waits"
    );
    f.complete("1");
    assert_eq!(f.status("1"), "Complete");
    let listed = f.ok(&["node", "quarantined", "fixture"]);
    assert_eq!(listed["quarantined"][0]["id"], 2);
    assert_eq!(listed["quarantined"][0]["by"], "tester");
}

#[test]
fn structural_changes_need_the_humans_direction() {
    let f = Fixture::new();
    f.ok(&[
        "node",
        "quarantine",
        "fixture",
        "2",
        "--origin",
        "cyber-stop",
        "--reason",
        "Stopped by a safety classifier",
    ]);
    // Creating inside it.
    let error = f.refused(&[
        "node", "create", "fixture", "--parent", "2", "--name", "F3", "--rva", "3000",
    ]);
    assert!(
        error.contains("\"kind\":\"quarantined\"") && error.contains("--directed-by"),
        "{error}"
    );
    f.ok(&[
        "node",
        "create",
        "fixture",
        "--parent",
        "2",
        "--name",
        "F3",
        "--rva",
        "3000",
        "--directed-by",
        "The human: collect the ComSat helper here",
    ]);
    assert_eq!(
        f.status("6"),
        "Complete",
        "a record created in it is quarantined with it"
    );
    // Moving another function into it.
    let binary = f.ok(&["project", "revision", "fixture"])["binary"].clone();
    let rev5 = f.ok(&["node", "show", "fixture", "5"])["node"]["revision"]
        .as_u64()
        .unwrap();
    let request = serde_json::json!({"schema":2,"binary":binary,"update":[{"id":5,"parent":2,"expect_node_revision":rev5}]});
    let o = f.batch(request.clone(), None);
    assert!(!o.status.success() && String::from_utf8_lossy(&o.stderr).contains("quarantined"));
    let o = f.batch(request, Some("The human: F2 is voice code"));
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let history = f.ok(&["history", "show", "fixture", "--limit", "5"]);
    assert!(
        history["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["directed_by"] == "The human: F2 is voice code"),
        "the instruction is recorded with the change: {history}"
    );
    // Prerequisites touching it.
    let structural = f.ok(&["node", "show", "fixture", "3"])["structural_token"]
        .as_str()
        .unwrap()
        .to_string();
    let error = f.refused(&[
        "deps",
        "set",
        "fixture",
        "3",
        "2000",
        "--expect",
        &structural,
    ]);
    assert!(error.contains("quarantined"), "{error}");
    // A creation-only import into it.
    let request = serde_json::json!({"schema":1,"binary":binary,"expect_revision":f.revision(),
        "create":[{"parent":2,"name":"F4","rva":4096}]});
    let o = f.batch(request, None);
    assert!(
        !o.status.success() && String::from_utf8_lossy(&o.stderr).contains("quarantined"),
        "imports respect the seal: {}",
        String::from_utf8_lossy(&o.stderr)
    );
}

#[test]
fn text_stays_editable_and_lifting_restores_the_own_status() {
    let f = Fixture::new();
    f.ok(&[
        "node",
        "quarantine",
        "fixture",
        "2",
        "--origin",
        "human",
        "--reason",
        "Voice code only",
    ]);
    f.save("3", "Notes kept for the record.");
    assert_eq!(f.status("3"), "Complete", "editing text never reopens it");
    let error = f.refused(&["node", "flag", "fixture", "3", "--reason", "check"]);
    assert!(error.contains("quarantined"), "{error}");
    let token = f.ok(&["node", "show", "fixture", "3"])["completion_token"]
        .as_str()
        .unwrap()
        .to_string();
    let error = f.refused(&[
        "node",
        "complete",
        "fixture",
        "3",
        "--expect",
        &token,
        "--reviewed",
    ]);
    assert!(error.contains("already counts as complete"), "{error}");
    let error = f.refused(&["node", "unquarantine", "fixture", "3", "--reason", "x"]);
    assert!(error.contains("lift the chunk"), "{error}");
    f.ok(&[
        "node",
        "unquarantine",
        "fixture",
        "2",
        "--reason",
        "The human lifted it",
        "--by",
        "tester",
    ]);
    assert_eq!(
        f.status("3"),
        "InProgress",
        "back to its own status (a saved report)"
    );
    assert_eq!(f.status("2"), "InProgress");
    let file: Value = serde_json::from_str(
        &fs::read_to_string(f.home.join("projects/fixture/quarantine.json")).unwrap(),
    )
    .unwrap();
    assert!(file["records"].as_object().unwrap().is_empty());
    assert_eq!(file["lifted"][0]["node"], 2);
    assert_eq!(file["lifted"][0]["reason"], "The human lifted it");
    let error = f.refused(&[
        "node",
        "quarantine",
        "fixture",
        "1",
        "--origin",
        "human",
        "--reason",
        "x",
    ]);
    assert!(error.contains("Only chunks and functions"), "{error}");
}

#[test]
fn quarantine_works_through_the_coordinator() {
    let f = Fixture::new();
    let o = f.run_in(
        "required",
        &[
            "node",
            "quarantine",
            "fixture",
            "2",
            "--origin",
            "human",
            "--reason",
            "Voice code only",
        ],
    );
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let o = f.run_in("required", &["status", "fixture", "--summary"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let summary: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(summary["quarantined"], 2);
    assert_eq!(f.status("3"), "Complete");
}

#[test]
fn deleting_a_quarantined_chunk_drops_its_record() {
    let f = Fixture::new();
    f.ok(&[
        "node",
        "quarantine",
        "fixture",
        "2",
        "--origin",
        "human",
        "--reason",
        "Voice code only",
    ]);
    let binary = f.ok(&["project", "revision", "fixture"])["binary"].clone();
    let request =
        serde_json::json!({"schema":1,"binary":binary,"expect_revision":f.revision(),"ids":[2]});
    let path = f.home.join("delete.json");
    fs::write(&path, request.to_string()).unwrap();
    let path = path.to_str().unwrap();
    // Removal takes work out of the seal: it needs the human's direction.
    let error = f.refused(&["node", "delete", "fixture", "--file", path]);
    assert!(
        error.contains("\"kind\":\"quarantined\"") && error.contains("Removing"),
        "{error}"
    );
    let direction = "The human: drop the voice chunk";
    let preview = f.ok(&[
        "node",
        "delete",
        "fixture",
        "--file",
        path,
        "--directed-by",
        direction,
    ]);
    let token = preview["token"].as_str().unwrap().to_string();
    f.ok(&[
        "node",
        "delete",
        "fixture",
        "--file",
        path,
        "--expect",
        &token,
        "--directed-by",
        direction,
    ]);
    let listed = f.ok(&["node", "quarantined", "fixture"]);
    assert!(
        listed["quarantined"].as_array().unwrap().is_empty(),
        "{listed}"
    );
    let history = f.ok(&["history", "show", "fixture", "--limit", "1"]);
    assert_eq!(history["entries"][0]["directed_by"], direction, "{history}");
}

#[test]
fn deleting_a_function_of_a_quarantined_chunk_needs_direction() {
    let f = Fixture::new();
    f.ok(&[
        "node",
        "quarantine",
        "fixture",
        "2",
        "--origin",
        "human",
        "--reason",
        "Voice code only",
    ]);
    let binary = f.ok(&["project", "revision", "fixture"])["binary"].clone();
    let request =
        serde_json::json!({"schema":1,"binary":binary,"expect_revision":f.revision(),"ids":[3]});
    let path = f.home.join("delete.json");
    fs::write(&path, request.to_string()).unwrap();
    let error = f.refused(&[
        "node",
        "delete",
        "fixture",
        "--file",
        path.to_str().unwrap(),
    ]);
    assert!(error.contains("through chunk 2"), "{error}");
}

#[test]
fn prune_passes_over_quarantined_records() {
    let f = Fixture::new();
    f.ok(&[
        "node", "create", "fixture", "--parent", "1", "--name", "Empty",
    ]);
    f.ok(&[
        "node",
        "quarantine",
        "fixture",
        "6",
        "--origin",
        "human",
        "--reason",
        "Sealed placeholder",
    ]);
    let binary = f.ok(&["project", "revision", "fixture"])["binary"].clone();
    let request =
        serde_json::json!({"schema":1,"binary":binary,"expect_revision":f.revision(),"ids":[6]});
    let path = f.home.join("prune.json");
    fs::write(&path, request.to_string()).unwrap();
    let path = path.to_str().unwrap();
    let preview = f.ok(&["node", "prune", "fixture", "--file", path]);
    assert!(
        preview["candidates"].as_array().unwrap().is_empty(),
        "{preview}"
    );
    assert_eq!(preview["excluded"][0]["reason"], "quarantined", "{preview}");
    let directed = f.ok(&[
        "node",
        "prune",
        "fixture",
        "--file",
        path,
        "--directed-by",
        "The human: remove the placeholder",
    ]);
    assert_eq!(directed["candidates"][0]["id"], 6, "{directed}");
}

#[test]
fn a_quarantined_function_may_be_named_as_a_prerequisite() {
    let f = Fixture::new();
    f.ok(&[
        "node",
        "quarantine",
        "fixture",
        "2",
        "--origin",
        "human",
        "--reason",
        "Voice code only",
    ]);
    let structural = |id: &str| {
        f.ok(&["node", "show", "fixture", id])["structural_token"]
            .as_str()
            .unwrap()
            .to_string()
    };
    // F2 (outside the seal) builds on F1 (inside it): no direction needed, any spelling.
    for spelling in ["0X1000", "0x1000", "1000"] {
        f.ok(&[
            "deps",
            "set",
            "fixture",
            "5",
            spelling,
            "--expect",
            &structural("5"),
        ]);
        assert_eq!(
            f.ok(&["deps", "get", "fixture", "5"])["requires"][0],
            3,
            "{spelling}"
        );
        f.ok(&["deps", "set", "fixture", "5", "--expect", &structural("5")]);
    }
    // F1's own prerequisites stay sealed, however they are spelled.
    for spelling in ["2000", "0X2000"] {
        let error = f.refused(&[
            "deps",
            "set",
            "fixture",
            "3",
            spelling,
            "--expect",
            &structural("3"),
        ]);
        assert!(error.contains("quarantined"), "{spelling}: {error}");
    }
    f.ok(&[
        "deps",
        "set",
        "fixture",
        "3",
        "0X2000",
        "--expect",
        &structural("3"),
        "--directed-by",
        "The human: F1 calls F2",
    ]);
    let error = f.refused(&[
        "deps",
        "set",
        "fixture",
        "5",
        "0x1000 zz",
        "--expect",
        &structural("5"),
    ]);
    assert!(error.contains("Invalid hexadecimal RVA: zz"), "{error}");
}

#[test]
fn restating_a_sealed_records_values_needs_no_direction() {
    let f = Fixture::new();
    f.ok(&[
        "node",
        "quarantine",
        "fixture",
        "2",
        "--origin",
        "human",
        "--reason",
        "Voice code only",
    ]);
    let binary = f.ok(&["project", "revision", "fixture"])["binary"].clone();
    let revision = f.ok(&["node", "show", "fixture", "3"])["node"]["revision"]
        .as_u64()
        .unwrap();
    let o = f.batch(
        serde_json::json!({"schema":2,"binary":binary,
            "update":[{"id":3,"parent":2,"name":"F1","expect_node_revision":revision}],
            "reorder":[{"parent":2,"children":[3]}]}),
        None,
    );
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    // A real rename inside it still needs the human's direction.
    let o = f.batch(
        serde_json::json!({"schema":2,"binary":binary,
            "update":[{"id":3,"name":"Renamed","expect_node_revision":revision}]}),
        None,
    );
    assert!(!o.status.success() && String::from_utf8_lossy(&o.stderr).contains("quarantined"));
}

#[test]
fn a_status_reset_leaves_quarantined_records_alone() {
    let f = Fixture::new();
    f.save("3", "Started research.");
    f.ok(&[
        "node",
        "quarantine",
        "fixture",
        "2",
        "--origin",
        "human",
        "--reason",
        "Voice code only",
    ]);
    let binary = f.ok(&["project", "revision", "fixture"])["binary"].clone();
    let path = f.home.join("reset.json");
    for request in [
        serde_json::json!({"schema":1,"binary":binary,"expect_revision":f.revision(),"ids":[3]}),
        serde_json::json!({"schema":1,"binary":binary,"expect_revision":f.revision(),"all_in_progress":true}),
    ] {
        fs::write(&path, request.to_string()).unwrap();
        let o = f.run_in(
            "off",
            &[
                "node",
                "reset-status",
                "fixture",
                "--file",
                path.to_str().unwrap(),
            ],
        );
        if request["ids"].is_array() {
            assert!(
                !o.status.success() && String::from_utf8_lossy(&o.stderr).contains("quarantined")
            );
        } else {
            assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        }
    }
    f.ok(&["node", "unquarantine", "fixture", "2", "--reason", "lifted"]);
    assert_eq!(
        f.status("3"),
        "InProgress",
        "its own status survived the reset"
    );
}

#[test]
fn edits_around_a_quarantined_record_keep_its_own_status() {
    let f = Fixture::new();
    // F1 (3) complete, and F2 (5) completed again on top of it as its prerequisite.
    f.save("3", "F1 behaviour.");
    f.complete("3");
    let structural = f.ok(&["node", "show", "fixture", "5"])["structural_token"]
        .as_str()
        .unwrap()
        .to_string();
    f.ok(&[
        "deps",
        "set",
        "fixture",
        "5",
        "1000",
        "--expect",
        &structural,
    ]);
    f.complete("5");
    f.ok(&[
        "node",
        "quarantine",
        "fixture",
        "5",
        "--origin",
        "human",
        "--reason",
        "Sealed after review",
    ]);
    let stored = |id: &str| {
        f.ok(&["node", "show", "fixture", id])["node"]["status"]
            .as_str()
            .unwrap()
            .to_string()
    };
    // Editing the prerequisite outside the seal reopens nothing inside it.
    f.save("3", "F1 behaviour, revised.");
    assert_eq!(stored("5"), "Complete");
    f.ok(&["node", "unquarantine", "fixture", "5", "--reason", "Lifted"]);
    assert_eq!(
        stored("5"),
        "Complete",
        "lifting restores the stored status"
    );
    // Its own completion survives; its prerequisite did change, so it now needs review.
    assert_eq!(f.status("5"), "InProgress");
}

#[test]
fn an_edit_inside_a_completed_quarantined_chunk_leaves_the_chunk_complete() {
    let f = Fixture::new();
    f.ok(&[
        "node",
        "quarantine",
        "fixture",
        "4",
        "--origin",
        "human",
        "--reason",
        "Sealed chunk",
    ]);
    f.save("5", "F2 notes kept for the record.");
    let chunk = f.ok(&["node", "show", "fixture", "4"]);
    assert_eq!(chunk["node"]["status"], "Complete", "{chunk}");
    f.ok(&["node", "unquarantine", "fixture", "4", "--reason", "Lifted"]);
    assert_eq!(
        f.ok(&["node", "show", "fixture", "4"])["node"]["status"],
        "Complete"
    );
}
