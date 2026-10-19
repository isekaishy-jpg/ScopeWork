//! Moving a function between completed chunks reopens both chunks and their ancestors while keeping research and data.
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

struct Fixture {
    home: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-workspaces");
        let home = base.join(format!("move-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&home).unwrap();
        Self { home }
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
    fn status(&self, id: u64) -> String {
        self.ok(&["node", "show", "fixture", &id.to_string()])["node"]["status"]
            .as_str()
            .unwrap()
            .to_string()
    }
    fn research(&self, id: u64, body: &str) {
        let r = self.ok(&["node", "show", "fixture", &id.to_string()]);
        let (b, e) = (self.home.join("body.md"), self.home.join("evidence.md"));
        fs::write(&b, body).unwrap();
        fs::write(&e, "Synthetic evidence").unwrap();
        self.ok(&[
            "node",
            "save",
            "fixture",
            &id.to_string(),
            "--expect",
            r["token"].as_str().unwrap(),
            "--body-file",
            b.to_str().unwrap(),
            "--evidence-file",
            e.to_str().unwrap(),
        ]);
        let r = self.ok(&["node", "show", "fixture", &id.to_string()]);
        self.ok(&[
            "node",
            "complete",
            "fixture",
            &id.to_string(),
            "--expect",
            r["completion_token"].as_str().unwrap(),
            "--reviewed",
        ]);
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
fn moving_a_function_reopens_both_chunks_and_keeps_research() {
    let f = Fixture::new();
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
        "node",
        "create",
        "fixture",
        "--parent",
        "1",
        "--name",
        "Source chunk",
    ]); // 2
    f.ok(&[
        "node", "create", "fixture", "--parent", "2", "--name", "Moved", "--rva", "1000",
    ]); // 3
    f.ok(&[
        "node", "create", "fixture", "--parent", "2", "--name", "Stays", "--rva", "2000",
    ]); // 4
    f.ok(&[
        "node",
        "create",
        "fixture",
        "--parent",
        "1",
        "--name",
        "Destination chunk",
    ]); // 5
    f.ok(&[
        "node", "create", "fixture", "--parent", "5", "--name", "Resident", "--rva", "3000",
    ]); // 6
    for (id, body) in [
        (3, "Moved research"),
        (4, "Stays research"),
        (6, "Resident research"),
        (2, "Source synthesis"),
        (5, "Destination synthesis"),
        (1, "Architecture synthesis"),
    ] {
        f.research(id, body);
    }
    for id in [1, 2, 3, 4, 5, 6] {
        assert_eq!(f.status(id), "Complete", "node {id} should start complete");
    }
    let before = f.ok(&["node", "show", "fixture", "3"]);
    let data = PathBuf::from(before["data"].as_str().unwrap());
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("capture.bin"), b"evidence bytes").unwrap();

    let p = f.ok(&["project", "show", "fixture"]);
    let request = f.home.join("move.json");
    fs::write(
        &request,
        serde_json::to_vec(
            &json!({"schema":1,"binary":p["binary"],"expect_revision":p["revision"],
        "update":[{"id":3,"parent":5}]}),
        )
        .unwrap(),
    )
    .unwrap();
    f.ok(&[
        "node",
        "batch",
        "fixture",
        "--file",
        request.to_str().unwrap(),
    ]);

    // Both affected chunks and their architecture need review again; the untouched sibling keeps its completion.
    assert_eq!(f.status(2), "InProgress");
    assert_eq!(f.status(5), "InProgress");
    assert_eq!(f.status(1), "InProgress");
    assert_eq!(f.status(4), "Complete");
    // Current rule: the moved function's own report is rewritten with its new parent, so it also needs re-review.
    assert_eq!(f.status(3), "InProgress");
    // The moved function keeps its research text and supporting data at its new canonical path.
    let after = f.ok(&["node", "show", "fixture", "3"]);
    assert_eq!(after["body"], "Moved research");
    let moved = PathBuf::from(after["data"].as_str().unwrap());
    assert_ne!(moved, data);
    assert_eq!(
        fs::read(moved.join("capture.bin")).unwrap(),
        b"evidence bytes"
    );
}
