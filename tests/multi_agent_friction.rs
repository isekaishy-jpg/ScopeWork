//! Several agents on one project: moves leave unrelated reports alone, the annotation ledger is
//! repathed in place, record-level batch preconditions, blocked recovery, the fair lock queue
//! and reclaiming duplicated sidecar history.
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Output,
    time::Duration,
};

struct Fixture {
    home: PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.home);
    }
}
const LEDGER: &str = "tools/profiles/annotations.json";
impl Fixture {
    /// App 0 → architecture 1 → chunks 2, 5 and 7; functions 3 (0x1000) and 4 (0x2000) in
    /// chunk 2, 6 (0x3000) in chunk 5 and 8 (0x4000) in chunk 7.
    fn new() -> Self {
        let home = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-workspaces")
            .join(format!("agents-{}", uuid::Uuid::new_v4()));
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
            ("1", "Source chunk", None),
            ("2", "Moved", Some("1000")),
            ("2", "Stays", Some("2000")),
            ("1", "Destination chunk", None),
            ("5", "Resident", Some("3000")),
            ("1", "Unrelated chunk", None),
            ("7", "Unrelated", Some("4000")),
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
    fn dir(&self) -> PathBuf {
        self.home.join("projects/fixture")
    }
    fn show(&self, id: u64) -> Value {
        self.ok(&["node", "show", "fixture", &id.to_string()])
    }
    fn report(&self, id: u64) -> PathBuf {
        PathBuf::from(self.show(id)["report"].as_str().unwrap())
    }
    fn save(&self, id: u64, body: &str) {
        let r = self.show(id);
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
    }
    fn complete(&self, id: u64) {
        let r = self.show(id);
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
    fn status(&self, id: u64) -> String {
        self.show(id)["effective_status"].as_str().unwrap().into()
    }
    fn batch(&self, request: Value) -> Output {
        let path = self.home.join("batch.json");
        fs::write(&path, serde_json::to_vec(&request).unwrap()).unwrap();
        self.run(&["node", "batch", "fixture", "--file", path.to_str().unwrap()])
    }
    fn revision(&self) -> (Value, Value) {
        let p = self.ok(&["project", "revision", "fixture"]);
        (p["binary"].clone(), p["revision"].clone())
    }
    fn move_node(&self, id: u64, parent: u64) -> Value {
        let (binary, revision) = self.revision();
        let o = self.batch(json!({"schema":1,"binary":binary,"expect_revision":revision,"update":[{"id":id,"parent":parent}]}));
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        serde_json::from_slice(&o.stdout).unwrap()
    }
}

fn read(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap()
}

#[test]
fn a_move_changes_only_reports_that_reference_moved_records() {
    let f = Fixture::new();
    // Original source strings are evidence and must survive other sessions' moves byte for byte.
    f.save(
        8,
        r"Diagnostic tags `.\Movement_C.cpp`, .\MovementShared.cpp and `..\src\fmod_file_cdda.cpp`; see [stays](../../000002/00002000/research.md).",
    );
    f.save(6, "Calls [moved](../../000002/00001000/research.md).");
    for id in [3, 4, 2, 6, 5, 8, 7] {
        if id != 6 && id != 8 {
            f.save(id, "Synthetic research");
        }
        f.complete(id);
    }
    let unrelated = read(&f.report(8));
    let moved = f.move_node(3, 5);
    assert_eq!(
        moved["moved"],
        json!([{"node":3,"from":"architectures/000001/000002/00001000/research.md","to":"architectures/000001/000005/00001000/research.md"}])
    );
    assert_eq!(read(&f.report(8)), unrelated, "unrelated report rewritten");
    assert_eq!(f.status(8), "Complete");
    assert_eq!(
        f.status(7),
        "Complete",
        "unrelated completed chunk reopened"
    );
    assert_eq!(f.status(4), "Complete");
    // The one reference to the moved report follows it, and that report needs review.
    assert_eq!(f.show(6)["body"], "Calls [moved](../00001000/research.md).");
    assert_eq!(f.status(6), "InProgress");
    let resolved = f.ok(&[
        "node",
        "resolve",
        "fixture",
        "architectures/000001/000002/00001000/data/capture.bin",
    ]);
    assert_eq!(resolved["paths"][0]["node"], 3);
    assert_eq!(
        resolved["paths"][0]["current"],
        "architectures/000001/000005/00001000/data/capture.bin"
    );
    assert_eq!(resolved["paths"][0]["moved"], true);
}

#[test]
fn the_annotation_ledger_is_repathed_in_place_and_heals_reverted_rows() {
    let f = Fixture::new();
    let ledger = f.dir().join(LEDGER);
    let original = concat!(
        "{\r\n  \"annotations\": [\r\n",
        "    {\r\n      \"kind\": \"plate-comment\",\r\n      \"report\": \"architectures/000001/000002/00001000/research.md\",\r\n      \"report_sha256\": \"pinned\"\r\n    },\r\n",
        "    {\r\n      \"note\": \"source .\\\\Movement_C.cpp\",\r\n      \"report\": \"architectures/000001/000002/00002000/research.md\"\r\n    }\r\n",
        "  ],\r\n  \"schema\": 1\r\n}\r\n"
    );
    fs::write(&ledger, original).unwrap();
    let moved = f.move_node(3, 5);
    assert_eq!(moved["ledger_rows_repathed"], 1);
    let expected = original.replace("000002/00001000", "000005/00001000");
    assert_eq!(
        fs::read_to_string(&ledger).unwrap(),
        expected,
        "only the path changes"
    );
    // The receipt records the move, not copies of the ledger.
    let receipt = f.dir().join(format!(
        "tools/workspaces/catalog-transactions-v4/revision-{}/journal.json",
        moved["revision"]
    ));
    let receipt: Value = serde_json::from_slice(&read(&receipt)).unwrap();
    assert_eq!(receipt["ledger"], true);
    assert!(
        receipt["sidecars"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["path"] != LEDGER)
    );
    // A tool that read the ledger before the move writes the old path back; the next move heals it.
    fs::write(&ledger, original).unwrap();
    let next = f.move_node(4, 7);
    assert_eq!(next["ledger_rows_repathed"], 2);
    assert_eq!(
        fs::read_to_string(&ledger).unwrap(),
        expected.replace("000002/00002000", "000007/00002000")
    );
}

#[test]
fn schema_two_batches_check_records_not_the_project_revision() {
    let f = Fixture::new();
    let (binary, _) = f.revision();
    let node = f.show(3)["node"]["revision"].clone();
    // Another session saves an unrelated report: the global revision moves on.
    f.save(8, "Unrelated work");
    let o = f.batch(json!({"schema":2,"binary":binary,"update":[{"id":3,"parent":5,"expect_node_revision":node}]}));
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    // A stale record precondition is refused, and so is an update without one.
    let o = f.batch(json!({"schema":2,"binary":binary,"update":[{"id":3,"parent":2,"expect_node_revision":node}]}));
    assert!(String::from_utf8_lossy(&o.stderr).contains("record_conflict"));
    let o = f.batch(json!({"schema":2,"binary":binary,"update":[{"id":4,"name":"Renamed"}]}));
    assert!(!o.status.success());
}

#[test]
fn removal_queues_reopened_records_and_drops_their_coordination_entries() {
    let f = Fixture::new();
    for id in [3, 4, 2] {
        f.save(id, "Synthetic research");
        f.complete(id);
    }
    f.ok(&["claim", "set", "fixture", "2", "--owner", "agent-a"]);
    let request = f.home.join("delete.json");
    let (binary, revision) = f.revision();
    fs::write(
        &request,
        serde_json::to_vec(
            &json!({"schema":1,"binary":binary,"expect_revision":revision,"ids":[3]}),
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
    let queue = f.ok(&["review-queue", "fixture"]);
    let queued: Vec<u64> = queue["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["node"].as_u64().unwrap())
        .collect();
    assert!(
        queued.contains(&2),
        "the reopened chunk waits for review: {queue}"
    );
    assert_eq!(
        f.status(4),
        "Complete",
        "the untouched sibling keeps its completion"
    );
    // Deleting the claimed chunk itself drops its claim with it.
    let (binary, revision) = f.revision();
    fs::write(
        &request,
        serde_json::to_vec(
            &json!({"schema":1,"binary":binary,"expect_revision":revision,"ids":[2]}),
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
    assert_eq!(f.ok(&["claim", "list", "fixture"])["claims"], json!([]));
}

#[test]
fn reads_continue_while_a_pending_transaction_is_blocked() {
    if !cfg!(debug_assertions) {
        eprintln!("skipped: needs debug-only fault injection");
        return;
    }
    let f = Fixture::new();
    // Interrupt a report save right after its journal is written.
    let r = f.show(3);
    fs::write(f.home.join("body.md"), "Interrupted").unwrap();
    fs::write(f.home.join("evidence.md"), "Evidence").unwrap();
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_research"))
        // These tests cover direct mode; tests/coordinator.rs covers the coordinator.
        .env("SCOPEWORK_COORDINATOR", "off")
        .env("SCOPEWORK_CATALOG_FAIL_AT", "journal")
        .arg("--home")
        .arg(&f.home)
        .args([
            "node",
            "save",
            "fixture",
            "3",
            "--expect",
            r["token"].as_str().unwrap(),
        ])
        .arg("--body-file")
        .arg(f.home.join("body.md"))
        .arg("--evidence-file")
        .arg(f.home.join("evidence.md"))
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(99));
    // Someone edits the report outside the app, so the journal can no longer resume.
    fs::write(
        f.dir()
            .join("architectures/000001/000002/00001000/research.md"),
        "outside edit",
    )
    .unwrap();
    let o = f.run(&["node", "show", "fixture", "4"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stderr).contains("recovery_blocked"));
    let o = f.run(&[
        "project",
        "set-scope",
        "fixture",
        "--scope",
        "x",
        "--expect-revision",
        "0",
    ]);
    let error = String::from_utf8_lossy(&o.stderr);
    assert!(!o.status.success());
    assert!(
        error.contains("Report changed during batch") && error.contains("pending"),
        "{error}"
    );
}

#[test]
fn waiting_writers_go_before_later_readers_and_dead_tickets_are_ignored() {
    let f = Fixture::new();
    let locks = f.home.join("projects/.project-locks");
    // A waiter that died long ago leaves its ticket behind; it must not block anyone.
    let waiters = locks.join("fixture.waiters");
    fs::create_dir_all(&waiters).unwrap();
    let dead = waiters.join("00000000000000000001-1-w-deadbeef.lock");
    fs::write(&dead, b"").unwrap();
    let before = f.ok(&["project", "revision", "fixture"])["revision"].clone();
    assert!(!dead.exists(), "dead ticket removed");

    let held = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(locks.join("fixture.lock"))
        .unwrap();
    held.lock().unwrap();
    let spawn = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_research"))
            // These tests cover direct mode; tests/coordinator.rs covers the coordinator.
            .env("SCOPEWORK_COORDINATOR", "off")
            .arg("--home")
            .arg(&f.home)
            .args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap()
    };
    let writer = spawn(&[
        "node", "create", "fixture", "--parent", "1", "--name", "Queued",
    ]);
    std::thread::sleep(Duration::from_millis(600));
    let reader = spawn(&["project", "revision", "fixture"]);
    std::thread::sleep(Duration::from_millis(600));
    let queue = f.ok(&["--wait", "0", "locks", "fixture"]);
    assert_eq!(queue["waiting"][0]["write"], true, "{queue}");
    assert_eq!(queue["waiting"][0]["position"], 1);
    held.unlock().unwrap();
    let writer = writer.wait_with_output().unwrap();
    let reader = reader.wait_with_output().unwrap();
    assert!(writer.status.success() && reader.status.success());
    let revision: Value = serde_json::from_slice(&reader.stdout).unwrap();
    assert_eq!(
        revision["revision"],
        before.as_u64().unwrap() + 1,
        "the reader ran after the earlier writer"
    );
}

#[test]
fn large_sidecar_copies_in_completed_receipts_can_be_reclaimed() {
    let f = Fixture::new();
    let rows: Vec<Value> = (0..20_000)
        .map(|i| json!({"report": format!("architectures/000001/000002/{i:08X}/research.md"), "kind": "plate-comment"}))
        .collect();
    let before = serde_json::to_string_pretty(&json!({"schema":1,"annotations":rows})).unwrap();
    let after = before.replacen("000002/00000007", "000005/00000007", 1);
    let revision = f.revision().1;
    let receipt = f
        .dir()
        .join("tools/workspaces/catalog-transactions-v4/revision-999/journal.json");
    fs::create_dir_all(receipt.parent().unwrap()).unwrap();
    fs::write(
        &receipt,
        serde_json::to_vec(&json!({"schema":1,"kind":"completed-catalog-delta",
            "before":{"id":"fixture","revision":998},"after":{"id":"fixture","revision":999},
            "changes":{"nodes":{"before":[],"after":[]},"dependencies":{"before":[],"after":[]}},
            "edits":[],"moves":[],"dependencies":[],"sidecars":[{"path":LEDGER,"before":before,"after":after}]}))
        .unwrap(),
    )
    .unwrap();
    let size = fs::metadata(&receipt).unwrap().len();
    let inspected = f.ok(&["journal", "fixture"]);
    assert!(inspected["large_sidecar_bytes"].as_u64().unwrap() > 1 << 20);
    assert_eq!(
        fs::metadata(&receipt).unwrap().len(),
        size,
        "inspection never writes"
    );
    f.ok(&["journal", "fixture", "--compact-sidecars"]);
    let compacted: Value = serde_json::from_slice(&read(&receipt)).unwrap();
    let sidecar = &compacted["sidecars"][0];
    assert_eq!(
        sidecar["changes"],
        json!([{"path":"/annotations/7/report","before":"architectures/000001/000002/00000007/research.md","after":"architectures/000001/000005/00000007/research.md"}])
    );
    assert!(sidecar.get("before").is_none());
    assert!(fs::metadata(&receipt).unwrap().len() < size / 100);
    assert_eq!(f.revision().1, revision, "the catalog is untouched");
}

/// `deps set` conflicts only on the function's own prerequisite list: unrelated prerequisite
/// changes, moving or renaming the function and saving its report leave its token valid.
#[test]
fn prerequisite_tokens_conflict_only_on_the_functions_own_list() {
    let f = Fixture::new();
    let token = |id: u64| f.show(id)["structural_token"].as_str().unwrap().to_string();
    let deps = |id: u64, expect: &str, rvas: &[&str]| {
        let mut args = vec!["deps", "set", "fixture"];
        let id = id.to_string();
        args.extend([id.as_str(), "--expect", expect]);
        args.extend(rvas);
        f.run(&args)
    };
    let held = token(3);
    assert!(held.starts_with("s5:"), "{held}");
    // Another function's prerequisites, a move and a rename of 3, and a save of its report.
    let other = token(4);
    assert!(deps(4, &other, &["3000"]).status.success());
    f.move_node(3, 5);
    let (binary, revision) = f.revision();
    let o = f.batch(
        json!({"schema":1,"binary":binary,"expect_revision":revision,
        "update":[{"id":3,"name":"Renamed"}]}),
    );
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    f.save(3, "Revised research");
    let o = deps(3, &held, &["2000"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    // The same token after that change to 3's own list is refused.
    let o = deps(3, &held, &["4000"]);
    assert!(!o.status.success());
    assert!(
        String::from_utf8_lossy(&o.stderr).contains("record_conflict"),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert_eq!(f.show(3)["dependency_rvas"], "0x00002000");
}
