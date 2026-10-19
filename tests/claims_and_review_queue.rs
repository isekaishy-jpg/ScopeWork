//! Claims record who works on a branch; a move queues every completed record it reopens until
//! that record is complete again, and reports whose claimed work it touched.
use serde_json::{Value, json};
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
        let home = base.join(format!("claims-{}", uuid::Uuid::new_v4()));
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
        for (parent, name, rva) in [
            ("0", "Arch", None),             // 1
            ("1", "Source", None),           // 2
            ("2", "Moved", Some("1000")),    // 3
            ("2", "Stays", Some("2000")),    // 4
            ("1", "Destination", None),      // 5
            ("5", "Resident", Some("3000")), // 6
        ] {
            let mut args = vec![
                "node", "create", "fixture", "--parent", parent, "--name", name,
            ];
            if let Some(rva) = rva {
                args.extend(["--rva", rva]);
            }
            f.ok(&args);
        }
        for id in [3, 4, 6, 2, 5, 1] {
            f.research(id);
        }
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
    fn research(&self, id: u64) {
        let id_text = id.to_string();
        let r = self.ok(&["node", "show", "fixture", &id_text]);
        let (b, e) = (self.home.join("body.md"), self.home.join("evidence.md"));
        fs::write(&b, format!("Research for {id}")).unwrap();
        fs::write(&e, "Synthetic evidence").unwrap();
        self.ok(&[
            "node",
            "save",
            "fixture",
            &id_text,
            "--expect",
            r["token"].as_str().unwrap(),
            "--body-file",
            b.to_str().unwrap(),
            "--evidence-file",
            e.to_str().unwrap(),
        ]);
        let r = self.ok(&["node", "show", "fixture", &id_text]);
        self.ok(&[
            "node",
            "complete",
            "fixture",
            &id_text,
            "--expect",
            r["completion_token"].as_str().unwrap(),
            "--reviewed",
        ]);
    }
}

#[test]
fn claims_conflict_and_release() {
    let f = Fixture::new();
    f.ok(&[
        "claim",
        "set",
        "fixture",
        "2",
        "--owner",
        "alpha",
        "--note",
        "chunk work",
    ]);
    let conflict = f.run(&["claim", "set", "fixture", "2", "--owner", "beta"]);
    assert!(!conflict.status.success());
    assert!(String::from_utf8_lossy(&conflict.stderr).contains("claimed by alpha"));
    let listed = f.ok(&["claim", "list", "fixture"]);
    assert_eq!(listed["claims"][0]["claim"]["owner"], "alpha");
    let function = f.run(&["claim", "set", "fixture", "3", "--owner", "alpha"]);
    assert!(!function.status.success(), "functions are not claimable");
    // Status shows the covering claim on the chunk's members.
    let status = f.ok(&["status", "fixture", "--parent", "2"]);
    for r in status["records"].as_array().unwrap() {
        assert_eq!(r["claimed_by"]["owner"], "alpha");
    }
    let refused = f.run(&["claim", "release", "fixture", "2", "--owner", "beta"]);
    assert!(!refused.status.success());
    assert_eq!(
        f.ok(&["claim", "release", "fixture", "2", "--owner", "alpha"])["released"],
        true
    );
    assert!(
        f.ok(&["claim", "list", "fixture"])["claims"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn moves_queue_reopened_records_until_completed_again() {
    let f = Fixture::new();
    f.ok(&["claim", "set", "fixture", "5", "--owner", "beta"]);
    let p = f.ok(&["project", "revision", "fixture"]);
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
    let moved = f.ok(&[
        "node",
        "batch",
        "fixture",
        "--file",
        request.to_str().unwrap(),
    ]);
    let reopened: Vec<u64> = moved["reopened"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["node"].as_u64().unwrap())
        .collect();
    for id in [1, 2, 3, 5] {
        assert!(
            reopened.contains(&id),
            "node {id} should be queued: {moved}"
        );
    }
    assert!(!reopened.contains(&4) && !reopened.contains(&6));
    let destination = moved["reopened"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["node"] == 5)
        .unwrap();
    assert_eq!(destination["claimed_by"]["owner"], "beta");
    let queue = f.ok(&["review-queue", "fixture"]);
    assert_eq!(queue["count"], 4);
    assert!(
        queue["entries"][0]["reasons"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("moving node(s) 3")
    );
    // Re-reviewing the moved function and its new chunk clears their entries.
    f.research(3);
    f.research(5);
    let queue = f.ok(&["review-queue", "fixture"]);
    let open: Vec<u64> = queue["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["node"].as_u64().unwrap())
        .collect();
    assert_eq!(open, vec![1, 2]);
    assert_eq!(f.ok(&["status", "fixture", "--summary"])["review_queue"], 2);
}
