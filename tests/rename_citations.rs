//! A rename reaches every report that cites the function (`Name` (`VA`)), including chunk
//! and architecture syntheses, and keeps their completion; structural changes still reopen.
use serde_json::Value;
use sha2::{Digest, Sha256};
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
    /// Architecture 1: chunk 2 holds F3 (0x1000, VA 00401000); chunk 4 holds F5 (0x2000),
    /// which cites F3 and mentions it once more by bare name; chunk 4's and the architecture's
    /// syntheses cite F3 too. Everything is complete.
    fn new() -> Self {
        let home = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-workspaces")
            .join(format!("citations-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&home).unwrap();
        let f = Self { home };
        f.ok(&["project", "create", "fixture", "--name", "Fixture", "--binary",
            "Build 1; SHA-256 aa63a5750d60ef16746c686b3d5e26876d98953eab08b1c026cd0faf78e88cb8; image base 0x00400000"]);
        f.ok(&[
            "node", "create", "fixture", "--parent", "0", "--name", "Arch",
        ]);
        f.ok(&[
            "node", "create", "fixture", "--parent", "1", "--name", "Cache",
        ]);
        f.ok(&[
            "node",
            "create",
            "fixture",
            "--parent",
            "2",
            "--name",
            "FUN_00401000",
            "--rva",
            "1000",
        ]);
        f.ok(&[
            "node", "create", "fixture", "--parent", "1", "--name", "Users",
        ]);
        f.ok(&[
            "node", "create", "fixture", "--parent", "4", "--name", "Caller", "--rva", "2000",
        ]);
        f.save("3", "Denies the item.");
        f.complete("3");
        f.save("2", "Cache synthesis.");
        f.complete("2");
        f.save(
            "5",
            "Calls `FUN_00401000` (`00401000`) on failure; FUN_00401000 also runs at exit.",
        );
        f.complete("5");
        f.save("4", "The users call `FUN_00401000` (`0x00401000`).");
        f.complete("4");
        f.save(
            "1",
            "Architecture synthesis: `FUN_00401000` (`00401000`) is the denial path.",
        );
        f.complete("1");
        f
    }
    fn ok(&self, args: &[&str]) -> Value {
        let o = std::process::Command::new(env!("CARGO_BIN_EXE_research"))
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
    fn show(&self, id: &str) -> Value {
        self.ok(&["node", "show", "fixture", id])
    }
    fn save(&self, id: &str, body: &str) {
        let b = self.home.join(format!("body-{id}.md"));
        let e = self.home.join(format!("evidence-{id}.md"));
        fs::write(&b, body).unwrap();
        fs::write(&e, "Fixture evidence.").unwrap();
        let token = self.show(id)["token"].as_str().unwrap().to_string();
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
        let token = self.show(id)["completion_token"]
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
    fn batch(&self, update: Value) -> Value {
        let binary = self.ok(&["project", "revision", "fixture"])["binary"].clone();
        let path = self.home.join("batch.json");
        fs::write(
            &path,
            serde_json::json!({"schema":2,"binary":binary,"update":update}).to_string(),
        )
        .unwrap();
        self.ok(&["node", "batch", "fixture", "--file", path.to_str().unwrap()])
    }
    fn revision_of(&self, id: &str) -> u64 {
        self.show(id)["node"]["revision"].as_u64().unwrap()
    }
}

#[test]
fn a_rename_reaches_every_citation_and_keeps_completion() {
    let f = Fixture::new();
    let caller = f.show("5")["report"].as_str().unwrap().to_string();
    let digest = |path: &str| format!("{:x}", Sha256::digest(fs::read(path).unwrap()));
    let before = digest(&caller);
    let ledger = f
        .home
        .join("projects/fixture/tools/profiles/annotations.json");
    fs::create_dir_all(ledger.parent().unwrap()).unwrap();
    fs::write(&ledger, format!(
        "{{\"schema\": 1, \"annotations\": [{{\"report\": \"architectures/000001/000004/00002000/research.md\", \"report_sha256\": \"{before}\"}}]}}\n"
    )).unwrap();
    let rev = f.revision_of("3");
    let result =
        f.batch(serde_json::json!([{"id":3,"name":"DenyItem","expect_node_revision":rev}]));
    for id in ["1", "2", "3", "4", "5"] {
        assert_eq!(
            f.show(id)["effective_status"],
            "Complete",
            "node {id} stays complete"
        );
    }
    assert!(
        f.show("5")["body"]
            .as_str()
            .unwrap()
            .contains("Calls `DenyItem` (`00401000`)")
    );
    assert!(
        f.show("4")["body"]
            .as_str()
            .unwrap()
            .contains("`DenyItem` (`0x00401000`)")
    );
    assert!(
        f.show("1")["body"]
            .as_str()
            .unwrap()
            .contains("`DenyItem` (`00401000`) is the denial path")
    );
    let header = fs::read_to_string(f.show("3")["report"].as_str().unwrap()).unwrap();
    assert!(header.starts_with("# DenyItem"), "{header}");
    let cited: Vec<u64> = result["citations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_u64().unwrap())
        .collect();
    for id in [1, 4, 5] {
        assert!(cited.contains(&id), "report {id} rewritten: {result}");
    }
    let bare = &result["unrewritten"].as_array().unwrap()[0];
    assert_eq!(
        (bare["id"].as_u64(), bare["mentions"].as_u64()),
        (Some(5), Some(1)),
        "{result}"
    );
    assert!(
        result["kept_complete"].as_array().unwrap().len() >= 4,
        "{result}"
    );
    let pinned = fs::read_to_string(&ledger).unwrap();
    assert!(
        pinned.contains(&digest(&caller)) && !pinned.contains(&before),
        "{pinned}"
    );
    let summary = f.ok(&["status", "fixture", "--summary"]);
    assert_eq!(
        f.ok(&["review-queue", "fixture"])["entries"]
            .as_array()
            .map_or(0, Vec::len),
        0,
        "{summary}"
    );
}

#[test]
fn a_move_in_the_same_batch_still_reopens() {
    let f = Fixture::new();
    let rev3 = f.revision_of("3");
    let rev5 = f.revision_of("5");
    f.batch(serde_json::json!([
        {"id":3,"name":"DenyItem","expect_node_revision":rev3},
        {"id":5,"parent":2,"expect_node_revision":rev5}
    ]));
    assert_eq!(
        f.show("5")["effective_status"],
        "InProgress",
        "the moved record is reviewed"
    );
    assert_eq!(
        f.show("2")["effective_status"],
        "InProgress",
        "its new chunk is reviewed"
    );
    assert_eq!(
        f.show("4")["effective_status"],
        "InProgress",
        "its old chunk is reviewed"
    );
    assert_eq!(
        f.show("3")["effective_status"],
        "Complete",
        "the rename alone keeps completion"
    );
    assert!(
        f.show("5")["body"]
            .as_str()
            .unwrap()
            .contains("`DenyItem` (`00401000`)")
    );
}

#[test]
fn a_create_in_the_same_batch_reopens_its_parent() {
    let f = Fixture::new();
    let binary = f.ok(&["project", "revision", "fixture"])["binary"].clone();
    let rev3 = f.revision_of("3");
    let path = f.home.join("batch.json");
    fs::write(
        &path,
        serde_json::json!({"schema":2,"binary":binary,
            "create":[{"parent":2,"name":"FUN_00403000","rva":12288}],
            "update":[{"id":3,"name":"DenyItem","expect_node_revision":rev3}]})
        .to_string(),
    )
    .unwrap();
    f.ok(&["node", "batch", "fixture", "--file", path.to_str().unwrap()]);
    assert_ne!(
        f.show("2")["node"]["status"],
        "Complete",
        "the chunk with a new member is reviewed, not just waiting on the member"
    );
    assert_eq!(
        f.show("5")["effective_status"],
        "Complete",
        "a report changed only by the citation stays complete"
    );
}
