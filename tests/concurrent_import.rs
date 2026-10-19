use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    time::{Duration, Instant},
};
struct Fixture {
    home: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-workspaces");
        let home = base.join(format!("integration-{}", uuid::Uuid::new_v4()));
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
        ]);
        f.ok(&[
            "node", "create", "fixture", "--parent", "1", "--name", "Chunk",
        ]);
        f.ok(&[
            "node", "create", "fixture", "--parent", "2", "--name", "First", "--rva", "1000",
        ]);
        f.ok(&[
            "node", "create", "fixture", "--parent", "2", "--name", "Second", "--rva", "2000",
        ]);
        f
    }
    fn command(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_research"));
        // These tests cover direct mode; tests/coordinator.rs covers the coordinator.
        c.env("SCOPEWORK_COORDINATOR", "off");
        c.arg("--home").arg(&self.home).args(args);
        c
    }
    fn output(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }
    fn ok(&self, args: &[&str]) -> Value {
        let o = self.output(args);
        assert!(
            o.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        serde_json::from_slice(&o.stdout).unwrap()
    }
    fn input(&self, count: usize) -> PathBuf {
        assert!(
            count <= 100,
            "Use small fixtures; large on-disk inventories are prohibited"
        );
        let p = self.ok(&["project", "show", "fixture"]);
        let batch = json!({"schema":1,"binary":p["binary"],"expect_revision":p["revision"],"create":(0..count).map(|i|json!({"parent":2,"name":format!("Imported {i}"),"rva":0x10000+i*16})).collect::<Vec<_>>()});
        let path = self.home.join("request.json");
        fs::write(&path, serde_json::to_vec(&batch).unwrap()).unwrap();
        path
    }
    fn save(&self, id: &str, token: &str, body: &str) -> Output {
        let b = self.home.join(format!("body-{id}.md"));
        let e = self.home.join(format!("evidence-{id}.md"));
        fs::write(&b, body).unwrap();
        fs::write(&e, "Synthetic evidence").unwrap();
        self.output(&[
            "node",
            "save",
            "fixture",
            id,
            "--expect",
            token,
            "--body-file",
            b.to_str().unwrap(),
            "--evidence-file",
            e.to_str().unwrap(),
        ])
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
/// Fault and I/O injection only exist in debug builds; release test runs skip these recovery tests explicitly.
fn skipped_without_fault_injection() -> bool {
    if cfg!(debug_assertions) {
        return false;
    }
    eprintln!("skipped: requires debug-build fault injection (run without --release)");
    true
}
#[test]
fn unrelated_tokens_survive_and_completion_has_a_distinct_purpose() {
    let f = Fixture::new();
    let a = f.ok(&["node", "show", "fixture", "3"]);
    let b = f.ok(&["node", "show", "fixture", "4"]);
    assert!(
        f.save("3", a["token"].as_str().unwrap(), "first edit")
            .status
            .success()
    );
    assert!(
        f.save("4", b["token"].as_str().unwrap(), "independent edit")
            .status
            .success()
    );
    assert!(
        !f.save("3", a["token"].as_str().unwrap(), "stale overwrite")
            .status
            .success()
    );
    let a = f.ok(&["node", "show", "fixture", "3"]);
    assert!(
        !f.output(&[
            "node",
            "complete",
            "fixture",
            "3",
            "--expect",
            a["token"].as_str().unwrap(),
            "--reviewed"
        ])
        .status
        .success()
    );
    f.ok(&[
        "node",
        "complete",
        "fixture",
        "3",
        "--expect",
        a["completion_token"].as_str().unwrap(),
        "--reviewed",
    ]);
}
#[test]
fn existing_reports_are_editable_during_preparation_and_parent_text_survives() {
    let f = Fixture::new();
    let input = f.input(16);
    let mut child = f
        .command(&[
            "import",
            "start",
            "fixture",
            "--file",
            input.to_str().unwrap(),
        ])
        .env("SCOPEWORK_IMPORT_DELAY_MS", "1500")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let job;
    loop {
        let output = f.output(&["project", "show", "fixture"]);
        if output.status.success() {
            let p: Value = serde_json::from_slice(&output.stdout).unwrap();
            if let Some(id) = p["active_import"].as_str() {
                job = id.to_string();
                break;
            }
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    let before = f.ok(&["node", "show", "fixture", "3"]);
    assert!(
        f.save(
            "3",
            before["token"].as_str().unwrap(),
            "edited during import"
        )
        .status
        .success()
    );
    let parent = f.ok(&["node", "show", "fixture", "2"]);
    assert!(
        f.save(
            "2",
            parent["token"].as_str().unwrap(),
            "parent prose during import"
        )
        .status
        .success()
    );
    let p = f.ok(&["project", "show", "fixture"]);
    assert_eq!(p["nodes"].as_array().unwrap().len(), 5);
    assert!(
        !f.output(&[
            "node", "create", "fixture", "--parent", "2", "--name", "Blocked", "--rva", "3000"
        ])
        .status
        .success()
    );
    assert!(
        child.try_wait().unwrap().is_none(),
        "Preparation ended before concurrent edits were exercised"
    );
    let done = child.wait_with_output().unwrap();
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
    assert_eq!(
        f.ok(&["import", "status", "fixture", &job])["state"],
        "Committed"
    );
    assert_eq!(
        f.ok(&["node", "show", "fixture", "3"])["body"],
        "edited during import"
    );
    let p = f.ok(&["node", "show", "fixture", "2"]);
    assert_eq!(p["body"], "parent prose during import");
    assert_eq!(p["children"].as_array().unwrap().len(), 18);
}
#[test]
fn pause_cancel_and_reservation_never_reuse_ids() {
    let f = Fixture::new();
    let input = f.input(10);
    let j = f.ok(&[
        "import",
        "start",
        "fixture",
        "--file",
        input.to_str().unwrap(),
        "--paused",
    ]);
    let id = j["id"].as_str().unwrap();
    assert_eq!(j["state"], "Paused");
    let before = f.ok(&["node", "show", "fixture", "3"]);
    assert!(
        f.save(
            "3",
            before["token"].as_str().unwrap(),
            "available while paused"
        )
        .status
        .success()
    );
    assert_eq!(
        f.ok(&["import", "cancel", "fixture", id])["state"],
        "Cancelled"
    );
    let p = f.ok(&["project", "show", "fixture"]);
    assert_eq!(p["next_id"], 15);
    assert_eq!(p["nodes"].as_array().unwrap().len(), 5);
    let created = f.ok(&[
        "node",
        "create",
        "fixture",
        "--parent",
        "2",
        "--name",
        "After cancellation",
        "--rva",
        "3000",
    ]);
    assert_eq!(created["node"]["id"], 15);
}
#[test]
fn process_exit_at_each_publication_boundary_recovers_once() {
    if skipped_without_fault_injection() {
        return;
    }
    for point in [
        "segment_planned",
        "segment_done",
        "ready",
        "publication_journal",
        "parents_published",
        "catalog_published",
    ] {
        let f = Fixture::new();
        let input = f.input(4);
        let failed = f
            .command(&[
                "import",
                "start",
                "fixture",
                "--file",
                input.to_str().unwrap(),
            ])
            .env("SCOPEWORK_IMPORT_FAIL_AT", point)
            .output()
            .unwrap();
        assert_eq!(
            failed.status.code(),
            Some(99),
            "{point}: {}",
            String::from_utf8_lossy(&failed.stderr)
        );
        let p = f.ok(&["project", "show", "fixture"]);
        let id = p["active_import"]
            .as_str()
            .or_else(|| p["last_import"].as_str())
            .unwrap();
        let job = f.ok(&["import", "resume", "fixture", id]);
        assert_eq!(job["state"], "Committed", "{point}");
        let after = f.ok(&["project", "show", "fixture"]);
        assert_eq!(after["nodes"].as_array().unwrap().len(), 9);
        f.ok(&["import", "resume", "fixture", id]);
        assert_eq!(
            f.ok(&["project", "show", "fixture"])["revision"],
            after["revision"]
        );
    }
}
#[test]
fn cancellation_preserves_changed_and_foreign_files_and_recovers_release() {
    if skipped_without_fault_injection() {
        return;
    }
    let f = Fixture::new();
    let input = f.input(2);
    let failed = f
        .command(&[
            "import",
            "start",
            "fixture",
            "--file",
            input.to_str().unwrap(),
        ])
        .env("SCOPEWORK_IMPORT_FAIL_AT", "segment_done")
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(99));
    let p = f.ok(&["project", "show", "fixture"]);
    let id = p["active_import"].as_str().unwrap();
    let dir = f
        .home
        .join("projects/fixture/architectures/000001/000002/00010000");
    fs::write(dir.join("research.md"), "manually changed prepared report").unwrap();
    fs::write(dir.join("data/foreign.bin"), b"preserve evidence bytes").unwrap();
    let failed = f
        .command(&["import", "cancel", "fixture", id])
        .env("SCOPEWORK_IMPORT_FAIL_AT", "cancel_released")
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(99));
    let job = f.ok(&["import", "resume", "fixture", id]);
    assert_eq!(job["state"], "Cancelled");
    assert!(!job["warnings"].as_array().unwrap().is_empty());
    assert_eq!(
        fs::read(dir.join("data/foreign.bin")).unwrap(),
        b"preserve evidence bytes"
    );
    assert_eq!(
        fs::read_to_string(dir.join("research.md")).unwrap(),
        "manually changed prepared report"
    );
    assert!(f.ok(&["project", "show", "fixture"])["active_import"].is_null());
}
#[test]
fn collision_and_duplicate_rejection_never_adopt_foreign_files() {
    let f = Fixture::new();
    let input = f.input(1);
    let path = f
        .home
        .join("projects/fixture/architectures/000001/000002/00010000");
    fs::create_dir(&path).unwrap();
    fs::write(path.join("research.md"), "foreign").unwrap();
    assert!(
        !f.output(&[
            "import",
            "start",
            "fixture",
            "--file",
            input.to_str().unwrap()
        ])
        .status
        .success()
    );
    assert_eq!(
        fs::read_to_string(path.join("research.md")).unwrap(),
        "foreign"
    );
    let p = f.ok(&["project", "show", "fixture"]);
    f.ok(&[
        "import",
        "cancel",
        "fixture",
        p["active_import"].as_str().unwrap(),
    ]);
    assert_eq!(
        fs::read_to_string(path.join("research.md")).unwrap(),
        "foreign"
    );
    let input = f.input(1);
    let mut batch: Value = serde_json::from_slice(&fs::read(&input).unwrap()).unwrap();
    batch["create"][0]["rva"] = json!(0x1000);
    fs::write(&input, serde_json::to_vec(&batch).unwrap()).unwrap();
    let before = f.ok(&["project", "show", "fixture"]);
    assert!(
        !f.output(&[
            "import",
            "plan",
            "fixture",
            "--file",
            input.to_str().unwrap()
        ])
        .status
        .success()
    );
    assert_eq!(before, f.ok(&["project", "show", "fixture"]));
}
#[test]
fn completion_inputs_and_schema_two_batch_conflicts() {
    let f = Fixture::new();
    let a = f.ok(&["node", "show", "fixture", "3"]);
    let b = f.ok(&["node", "show", "fixture", "4"]);
    let p = f.ok(&["project", "show", "fixture"]);
    let path = f.home.join("save.json");
    fs::write(&path,serde_json::to_vec(&json!({"schema":2,"binary":p["binary"],"reports":[{"id":3,"expect":a["token"],"body":"batch first","evidence":"synthetic"},{"id":4,"expect":b["token"],"body":"batch second","evidence":"synthetic"}]})).unwrap()).unwrap();
    // An unrelated project label edit does not invalidate either draft.
    f.ok(&[
        "project",
        "set-scope",
        "fixture",
        "--scope",
        "Synthetic",
        "--expect-revision",
        &p["revision"].to_string(),
    ]);
    f.ok(&[
        "node",
        "save-batch",
        "fixture",
        "--file",
        path.to_str().unwrap(),
    ]);
    assert_eq!(
        f.ok(&["node", "show", "fixture", "3"])["body"],
        "batch first"
    );
    assert!(
        !f.output(&[
            "node",
            "save-batch",
            "fixture",
            "--file",
            path.to_str().unwrap()
        ])
        .status
        .success()
    );
    let b = f.ok(&["node", "show", "fixture", "4"]);
    f.ok(&[
        "node",
        "complete",
        "fixture",
        "4",
        "--expect",
        b["completion_token"].as_str().unwrap(),
        "--reviewed",
    ]);
    let a = f.ok(&["node", "show", "fixture", "3"]);
    let out = f.output(&[
        "deps",
        "set",
        "fixture",
        "3",
        "2000",
        "--expect",
        a["structural_token"].as_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let a = f.ok(&["node", "show", "fixture", "3"]);
    let b = f.ok(&["node", "show", "fixture", "4"]);
    assert!(
        f.save("4", b["token"].as_str().unwrap(), "prerequisite changed")
            .status
            .success()
    );
    assert!(
        !f.output(&[
            "node",
            "complete",
            "fixture",
            "3",
            "--expect",
            a["completion_token"].as_str().unwrap(),
            "--reviewed"
        ])
        .status
        .success()
    );
}
#[test]
fn completed_segment_tamper_and_read_only_plan_are_detected() {
    if skipped_without_fault_injection() {
        return;
    }
    let f = Fixture::new();
    let input = f.input(2);
    let before = fs::read(f.home.join("projects/fixture/project.json")).unwrap();
    f.ok(&[
        "import",
        "plan",
        "fixture",
        "--file",
        input.to_str().unwrap(),
    ]);
    assert_eq!(
        before,
        fs::read(f.home.join("projects/fixture/project.json")).unwrap()
    );
    let failed = f
        .command(&[
            "import",
            "start",
            "fixture",
            "--file",
            input.to_str().unwrap(),
        ])
        .env("SCOPEWORK_IMPORT_FAIL_AT", "segment_done")
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(99));
    let p = f.ok(&["project", "show", "fixture"]);
    let id = p["active_import"].as_str().unwrap();
    let segment = f.home.join(format!(
        "projects/fixture/tools/workspaces/import-jobs/{id}/segments/000000.json"
    ));
    let mut data: Value = serde_json::from_slice(&fs::read(&segment).unwrap()).unwrap();
    data["outputs"] = json!([]);
    fs::write(segment, serde_json::to_vec(&data).unwrap()).unwrap();
    assert!(
        !f.output(&["import", "resume", "fixture", id])
            .status
            .success()
    );
    assert_eq!(
        f.ok(&["project", "show", "fixture"])["nodes"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
}
#[test]
fn provider_failure_resumes_without_reallocating_or_overwriting() {
    if skipped_without_fault_injection() {
        return;
    }
    let f = Fixture::new();
    let input = f.input(4);
    assert!(
        !f.command(&[
            "import",
            "start",
            "fixture",
            "--file",
            input.to_str().unwrap()
        ])
        .env("SCOPEWORK_IMPORT_IO_ERROR", "1")
        .output()
        .unwrap()
        .status
        .success()
    );
    let p = f.ok(&["project", "show", "fixture"]);
    let id = p["active_import"].as_str().unwrap();
    assert_eq!(
        f.ok(&["import", "status", "fixture", id])["state"],
        "Failed"
    );
    assert_eq!(p["nodes"].as_array().unwrap().len(), 5);
    f.ok(&["import", "resume", "fixture", id]);
    let after = f.ok(&["project", "show", "fixture"]);
    assert_eq!(after["next_id"], p["next_id"]);
    assert_eq!(after["nodes"].as_array().unwrap().len(), 9);
}
#[test]
fn shared_reads_coexist_and_exclude_cross_process_writes() {
    let f = Fixture::new();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(f.home.join("projects/.project-locks/fixture.lock"))
        .unwrap();
    lock.try_lock_shared().unwrap();
    f.ok(&["node", "show", "fixture", "3"]);
    assert!(
        !f.output(&[
            "node",
            "create",
            "fixture",
            "--parent",
            "2",
            "--name",
            "Blocked writer",
            "--rva",
            "3000"
        ])
        .status
        .success()
    );
    drop(lock);
    f.ok(&[
        "node",
        "create",
        "fixture",
        "--parent",
        "2",
        "--name",
        "Released writer",
        "--rva",
        "3000",
    ]);
}
#[test]
fn metadata_journals_recover_dependencies_and_empty_pending_is_reusable() {
    if skipped_without_fault_injection() {
        return;
    }
    let f = Fixture::new();
    let a = f.ok(&["node", "show", "fixture", "3"]);
    let failed = f
        .command(&[
            "deps",
            "set",
            "fixture",
            "3",
            "2000",
            "--expect",
            a["structural_token"].as_str().unwrap(),
        ])
        .env("SCOPEWORK_METADATA_FAIL_AT", "journal")
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(99));
    let fresh = f.ok(&["node", "show", "fixture", "3"]);
    assert_eq!(fresh["requires"], json!([4]));
    let pending = f
        .home
        .join("projects/fixture/tools/workspaces/catalog-transactions-v4/pending");
    fs::create_dir_all(&pending).unwrap();
    assert!(
        f.save(
            "3",
            fresh["token"].as_str().unwrap(),
            "after interrupted journal admission"
        )
        .status
        .success()
    );
}
#[test]
fn single_creation_refuses_foreign_data_and_reserved_windows_names() {
    let f = Fixture::new();
    let folder = f
        .home
        .join("projects/fixture/architectures/000001/000002/00003000");
    fs::create_dir_all(folder.join("data")).unwrap();
    fs::write(folder.join("data/foreign.bin"), b"foreign").unwrap();
    assert!(
        !f.output(&[
            "node",
            "create",
            "fixture",
            "--parent",
            "2",
            "--name",
            "Collision",
            "--rva",
            "3000"
        ])
        .status
        .success()
    );
    assert_eq!(
        fs::read(folder.join("data/foreign.bin")).unwrap(),
        b"foreign"
    );
    for id in ["com2", "com9", "lpt3", "lpt9"] {
        assert!(
            !f.output(&["project", "create", id, "--name", "Invalid"])
                .status
                .success()
        );
    }
}

#[test]
fn branch_delete_recovers_after_each_publication_boundary() {
    if skipped_without_fault_injection() {
        return;
    }
    for point in ["journal", "moves", "reports", "catalog"] {
        let f = Fixture::new();
        let p = f.ok(&["project", "show", "fixture"]);
        let report = f.ok(&["node", "show", "fixture", "3"]);
        let source = PathBuf::from(report["report"].as_str().unwrap());
        fs::write(
            source.parent().unwrap().join("data/capture.bin"),
            b"retained capture",
        )
        .unwrap();
        let request = f.home.join("remove.json");
        fs::write(
            &request,
            serde_json::to_vec(
                &json!({"schema":1,"binary":p["binary"],"expect_revision":p["revision"],"ids":[2]}),
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
        let failed = f
            .command(&[
                "node",
                "delete",
                "fixture",
                "--file",
                request.to_str().unwrap(),
                "--expect",
                preview["token"].as_str().unwrap(),
            ])
            .env("SCOPEWORK_CATALOG_FAIL_AT", point)
            .output()
            .unwrap();
        assert_eq!(
            failed.status.code(),
            Some(99),
            "{point}: {}",
            String::from_utf8_lossy(&failed.stderr)
        );
        let after = f.ok(&["project", "show", "fixture"]);
        assert_eq!(after["nodes"].as_array().unwrap().len(), 2);
        let archive = preview["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["id"] == 3)
            .unwrap()["archive"]
            .as_str()
            .unwrap();
        let capture = f
            .home
            .join("projects/fixture")
            .join(archive)
            .parent()
            .unwrap()
            .join("data/capture.bin");
        assert_eq!(fs::read(capture).unwrap(), b"retained capture");
        assert_eq!(
            after["revision"],
            f.ok(&["project", "show", "fixture"])["revision"]
        );
    }
}

#[test]
fn journal_inspection_and_compaction_do_not_recover_or_migrate_pending_projects() {
    let f = Fixture::new();
    let root = f.home.join("projects/fixture");
    let path = root.join("project.json");
    let mut catalog: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    catalog["layout_version"] = json!(4);
    let original = serde_json::to_vec(&catalog).unwrap();
    fs::write(&path, &original).unwrap();
    let pending = root.join("tools/workspaces/catalog-transactions-v4/pending");
    fs::create_dir_all(&pending).unwrap();
    fs::write(pending.join("journal.json"), b"conflicted pending fixture").unwrap();
    for args in [
        vec!["journal", "fixture"],
        vec!["journal", "fixture", "--compact"],
    ] {
        let status = f.ok(&args);
        assert_eq!(status["pending_bytes"], b"conflicted pending fixture".len());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(
            fs::read(pending.join("journal.json")).unwrap(),
            b"conflicted pending fixture"
        );
    }
    assert!(!root.join("tools/workspaces/layout-migration-v5").exists());
}

#[test]
fn format_five_upgrade_preserves_reports_and_original_catalog() {
    let f = Fixture::new();
    let root = f.home.join("projects/fixture");
    let report = fs::read(root.join("app.md")).unwrap();
    let path = root.join("project.json");
    let mut catalog: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    catalog["layout_version"] = json!(4);
    fs::write(&path, serde_json::to_vec(&catalog).unwrap()).unwrap();
    let migrated = f.ok(&["project", "show", "fixture"]);
    assert_eq!(migrated["layout_version"], 5);
    assert_eq!(migrated["nodes"], catalog["nodes"]);
    assert_eq!(fs::read(root.join("app.md")).unwrap(), report);
    let backup: Value = serde_json::from_slice(
        &fs::read(root.join("tools/workspaces/layout-migration-v5/before.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(backup, catalog);
}
