//! Proof records (docs/proposal-proof-records.md): the per-project requirement, the completion
//! gate, repository path checks, batches, unproven listings and removal.
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
    /// App 0 → architecture 1 → chunk 2 → functions 3 (0x1000) and 4 (0x2000).
    fn new() -> Self {
        let home = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-workspaces")
            .join(format!("proofs-{}", uuid::Uuid::new_v4()));
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
            ("0", "Architecture", None),
            ("1", "Chunk", None),
            ("2", "F1", Some("1000")),
            ("2", "F2", Some("2000")),
        ] {
            let mut args = vec![
                "node", "create", "fixture", "--parent", parent, "--name", name,
            ];
            if let Some(rva) = rva {
                args.extend(["--rva", rva]);
            }
            f.ok(&args);
        }
        f
    }
    fn run(&self, args: &[&str]) -> Output {
        std::process::Command::new(env!("CARGO_BIN_EXE_research"))
            .env("SCOPEWORK_COORDINATOR", "off")
            .env("SCOPEWORK_SUBMITTER", "tester")
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
    fn fails(&self, args: &[&str], kind: &str) {
        let o = self.run(args);
        assert!(!o.status.success(), "{args:?} succeeded");
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(err.contains(kind), "{args:?}: {err}");
    }
    fn show(&self, id: u64) -> Value {
        self.ok(&["node", "show", "fixture", &id.to_string()])
    }
    fn save(&self, id: u64) {
        let r = self.show(id);
        let (b, e) = (self.home.join("body.md"), self.home.join("evidence.md"));
        fs::write(&b, "Research").unwrap();
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
    }
    fn complete_args(&self, id: u64) -> Vec<String> {
        let token = self.show(id)["completion_token"]
            .as_str()
            .unwrap()
            .to_string();
        [
            "node",
            "complete",
            "fixture",
            &id.to_string(),
            "--expect",
            &token,
            "--reviewed",
        ]
        .map(String::from)
        .to_vec()
    }
    fn complete(&self, id: u64) {
        let args = self.complete_args(id);
        self.ok(&args.iter().map(String::as_str).collect::<Vec<_>>());
    }
    fn require(&self, required: bool, repository: Option<&str>) {
        let flag = required.to_string();
        let mut args = vec!["project", "set-proof", "fixture", "--required", &flag];
        if let Some(repository) = repository {
            args.extend(["--repository", repository]);
        }
        self.ok(&args);
    }
    fn prove(&self, id: u64, recreation: &str, test: &str) -> Output {
        self.run(&[
            "node",
            "proof",
            "fixture",
            &id.to_string(),
            "--recreation",
            recreation,
            "--test",
            test,
        ])
    }
}

#[test]
fn without_the_requirement_completion_is_unchanged_and_proofs_are_listed() {
    let f = Fixture::new();
    f.save(3);
    f.complete(3);
    assert!(f.prove(4, "src/f2.rs::f2", "tests/f2.rs").status.success());
    let shown = f.show(4);
    assert_eq!(shown["proof"]["recreation"], "src/f2.rs::f2");
    assert_eq!(shown["proof"]["by"], "tester");
    assert_eq!(shown["unproven"], false);
    let unproven = f.ok(&["node", "unproven", "fixture"]);
    assert_eq!(unproven["required"], false);
    assert_eq!(unproven["unproven"].as_array().unwrap().len(), 1);
    assert_eq!(unproven["unproven"][0]["id"], 3);
}

#[test]
fn the_requirement_gates_function_completion_only() {
    let f = Fixture::new();
    f.require(true, None);
    assert_eq!(
        f.ok(&["project", "show", "fixture"])["proof"]["required"],
        true
    );
    for id in [3, 4] {
        f.save(id);
    }
    assert_eq!(f.show(3)["unproven"], true);
    let args = f.complete_args(3);
    f.fails(
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
        "proof_required",
    );
    let ready = f.ok(&["status", "fixture", "--summary"]);
    assert_eq!(
        ready["ready_for_review"], 0,
        "unproven is not ready for review"
    );
    for id in [3, 4] {
        assert!(f.prove(id, "src/f.rs", "tests/f.rs").status.success());
        f.complete(id);
    }
    // The chunk synthesizes; it takes no proof.
    f.save(2);
    f.complete(2);
    assert_eq!(f.show(2)["effective_status"], "Complete");
    f.fails(
        &[
            "node",
            "proof",
            "fixture",
            "2",
            "--recreation",
            "x",
            "--test",
            "y",
        ],
        "not a function",
    );
}

#[test]
fn a_repository_checks_that_proof_files_exist() {
    let f = Fixture::new();
    let repo = f.home.join("repo");
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::create_dir_all(repo.join("tests")).unwrap();
    fs::write(repo.join("src/heap.rs"), "").unwrap();
    fs::write(repo.join("tests/heap.rs"), "").unwrap();
    f.require(false, Some(repo.to_str().unwrap()));
    let missing = f.prove(3, "src/missing.rs::alloc", "tests/heap.rs");
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("proof_path_missing"));
    let missing = f.prove(3, "src/heap.rs::alloc", "tests/nope.rs");
    assert!(!missing.status.success());
    assert!(
        f.prove(3, "src/heap.rs::alloc", "tests/heap.rs")
            .status
            .success()
    );
    f.ok(&["node", "proof", "fixture", "3", "--clear"]);
    assert!(f.show(3)["proof"].is_null());
}

#[test]
fn records_completed_before_the_requirement_stay_complete_and_show_as_unproven() {
    let f = Fixture::new();
    f.save(3);
    f.complete(3);
    f.require(true, None);
    assert_eq!(f.show(3)["effective_status"], "Complete");
    assert_eq!(f.show(3)["unproven"], true);
    let unproven = f.ok(&["node", "unproven", "fixture", "--complete"]);
    let ids: Vec<u64> = unproven["unproven"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids, [3]);
    assert_eq!(
        f.ok(&["status", "fixture", "--summary"])["unproven_complete"],
        1
    );
}

#[test]
fn proof_batches_are_all_or_nothing_and_removal_drops_proofs() {
    let f = Fixture::new();
    let file = f.home.join("proofs.json");
    let batch = |value: Value| {
        fs::write(&file, serde_json::to_vec(&value).unwrap()).unwrap();
        f.run(&[
            "node",
            "proof-batch",
            "fixture",
            "--file",
            file.to_str().unwrap(),
        ])
    };
    let refused = batch(json!({"proofs":[
        {"id":3,"recreation":"src/a.rs","tests":["tests/a.rs"]},
        {"id":2,"recreation":"src/c.rs","tests":["tests/c.rs"]}]}));
    assert!(!refused.status.success(), "a chunk takes no proof");
    assert!(f.show(3)["proof"].is_null(), "nothing recorded");
    let refused = batch(json!({"proofs":[{"id":3,"recreation":"src/a.rs","tests":[]}]}));
    assert!(!refused.status.success(), "a proof needs a test");
    let o = batch(json!({"proofs":[
        {"id":3,"recreation":"src/a.rs","tests":["tests/a.rs"]},
        {"id":4,"recreation":"src/b.rs","tests":["tests/b.rs","tests/b2.rs"]}]}));
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(
        f.show(4)["proof"]["tests"],
        json!(["tests/b.rs", "tests/b2.rs"])
    );
    assert_eq!(
        f.ok(&["project", "show", "fixture"])["proof"]["recorded"],
        2
    );
    let request = f.home.join("delete.json");
    let p = f.ok(&["project", "revision", "fixture"]);
    fs::write(
        &request,
        serde_json::to_vec(
            &json!({"schema":1,"binary":p["binary"],"expect_revision":p["revision"],"ids":[3]}),
        )
        .unwrap(),
    )
    .unwrap();
    let preview = f.ok(&[
        "node",
        "delete",
        "fixture",
        "--file",
        request.to_str().unwrap(),
    ]);
    f.ok(&[
        "node",
        "delete",
        "fixture",
        "--file",
        request.to_str().unwrap(),
        "--expect",
        preview["token"].as_str().unwrap(),
    ]);
    let stored: Value =
        serde_json::from_slice(&fs::read(f.home.join("projects/fixture/proofs.json")).unwrap())
            .unwrap();
    assert!(stored["proofs"].get("3").is_none());
    assert!(stored["proofs"].get("4").is_some());
}

#[test]
fn a_damaged_proofs_file_keeps_reads_and_refuses_proof_writes_and_function_completion() {
    let f = Fixture::new();
    f.save(3);
    f.complete(3);
    fs::write(f.home.join("projects/fixture/proofs.json"), "{not json").unwrap();
    // Reads answer as if there were no proofs.
    assert_eq!(f.show(3)["effective_status"], "Complete");
    assert!(f.show(3)["proof"].is_null());
    f.ok(&["status", "fixture", "--summary"]);
    f.ok(&["node", "unproven", "fixture"]);
    f.ok(&["project", "show", "fixture"]);
    // Proof writes and function completion fail closed; parents are unaffected.
    f.fails(
        &[
            "node",
            "proof",
            "fixture",
            "4",
            "--recreation",
            "a.rs",
            "--test",
            "t.rs",
        ],
        "proofs_unreadable",
    );
    f.save(4);
    let args = f.complete_args(4);
    f.fails(
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
        "proofs_unreadable",
    );
}

#[test]
fn writes_that_change_nothing_leave_no_sidecar_files() {
    let f = Fixture::new();
    let dir = f.home.join("projects/fixture");
    // The default proof policy, a flag that does not exist and a claim nobody holds.
    f.ok(&["project", "set-proof", "fixture", "--required", "false"]);
    f.ok(&["node", "unflag", "fixture", "3"]);
    f.ok(&["claim", "release", "fixture", "2", "--owner", "tester"]);
    for name in ["proofs.json", "review-flags.json", "claims.json"] {
        assert!(!dir.join(name).exists(), "{name} was created");
    }
    // A real change creates the file in its transaction.
    f.ok(&["project", "set-proof", "fixture", "--required", "true"]);
    assert!(dir.join("proofs.json").is_file());
}
