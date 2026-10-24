//! history show summarises catalog receipts and tooling entries; history append writes the tooling log.
use serde_json::Value;
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
        let home = base.join(format!("history-{}", uuid::Uuid::new_v4()));
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
            "node", "create", "fixture", "--parent", "1", "--name", "Chunk",
        ]);
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
}

#[test]
fn catalog_receipts_and_tooling_entries_read_as_one_history() {
    let f = Fixture::new();
    f.ok(&[
        "node",
        "flag",
        "fixture",
        "2",
        "--reason",
        "check membership",
    ]);
    let flagged = f.ok(&[
        "history", "show", "fixture", "--only", "catalog", "--limit", "1",
    ]);
    let newest = &flagged["entries"][0];
    assert_eq!(newest["kind"], "catalog");
    assert_eq!(newest["sidecars"][0], "review-flags.json");

    let diff = f.home.join("diff.json");
    fs::write(&diff, r#"{"rows_added":2,"rows_removed":0}"#).unwrap();
    let entry = f.ok(&[
        "history",
        "append",
        "fixture",
        "--kind",
        "ledger-change",
        "--summary",
        "Queued two signatures",
        "--node",
        "2",
        "--ref",
        "tools/profiles/annotations.json",
        "--diff-file",
        diff.to_str().unwrap(),
    ]);
    assert_eq!(entry["diff"]["rows_added"], 2);

    let tooling = f.ok(&["history", "show", "fixture", "--only", "tooling"]);
    assert_eq!(tooling["entries"].as_array().unwrap().len(), 1);
    assert_eq!(tooling["entries"][0]["entry_kind"], "ledger-change");

    let chunk = f.ok(&["history", "show", "fixture", "--node", "2"]);
    let kinds: Vec<&str> = chunk["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"tooling") && kinds.contains(&"catalog"));
    let created = chunk["entries"].as_array().unwrap().iter().find(|e| {
        e["kind"] == "catalog"
            && e["changes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["created"] == true)
    });
    assert!(
        created.is_some(),
        "the chunk's creation is in its history: {chunk}"
    );

    for bad in [["--kind", "catalog"], ["--kind", "Bad Kind"]] {
        let out = f.run(&[
            "history",
            "append",
            "fixture",
            bad[0],
            bad[1],
            "--summary",
            "x",
        ]);
        assert!(!out.status.success());
    }
}

fn today() -> String {
    research_dashboard::crash_log::timestamp(std::time::SystemTime::now())[..10].to_string()
}
fn receipt(f: &Fixture, revision: u64) -> PathBuf {
    f.home.join(format!(
        "projects/fixture/tools/workspaces/catalog-transactions-v4/revision-{revision}/journal.json"
    ))
}
fn set_time(path: &PathBuf, unix: u64) {
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(unix))
        .unwrap();
}
fn revisions(v: &Value) -> Vec<u64> {
    v["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["revision"].as_u64().unwrap())
        .collect()
}

#[test]
fn history_filters_by_rva_and_by_day() {
    let f = Fixture::new();
    f.ok(&[
        "node", "create", "fixture", "--parent", "2", "--name", "F", "--rva", "1000",
    ]);
    f.ok(&["node", "flag", "fixture", "3", "--reason", "check"]);
    let by_rva = f.ok(&["history", "show", "fixture", "--rva", "0x1000"]);
    let entries = by_rva["entries"].as_array().unwrap();
    assert!(!entries.is_empty());
    for e in entries {
        assert!(
            e["changes"]
                .as_array()
                .unwrap()
                .iter()
                .all(|c| c["id"] == 3),
            "{e}"
        );
    }
    assert_eq!(
        revisions(&by_rva),
        revisions(&f.ok(&["history", "show", "fixture", "--node", "3"]))
    );
    let unknown = f.run(&["history", "show", "fixture", "--rva", "2000"]);
    assert!(!unknown.status.success());
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("No record has RVA 2000"));

    // New receipts carry their commit time; today's entries are found by day.
    let all = f.ok(&["history", "show", "fixture", "--only", "catalog"]);
    assert!(
        all["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["at_unix"].as_u64().is_some())
    );
    let day = today();
    let today_entries = f.ok(&["history", "show", "fixture", "--from", &day, "--to", &day]);
    assert_eq!(
        revisions(&today_entries).len(),
        all["entries"].as_array().unwrap().len()
    );
    let none = f.ok(&["history", "show", "fixture", "--to", "2000-01-01"]);
    assert!(none["entries"].as_array().unwrap().is_empty());
    assert!(
        !f.run(&["history", "show", "fixture", "--from", "2026-13-01"])
            .status
            .success()
    );

    // An older receipt without a recorded time is dated by its journal's modification time.
    let old = receipt(&f, 2);
    let mut value: Value = serde_json::from_slice(&fs::read(&old).unwrap()).unwrap();
    value.as_object_mut().unwrap().remove("at_unix");
    fs::write(&old, serde_json::to_vec(&value).unwrap()).unwrap();
    set_time(&old, 978_307_200 + 3_600); // 2001-01-01 01:00 UTC
    let dated = f.ok(&[
        "history",
        "show",
        "fixture",
        "--from",
        "2001-01-01",
        "--to",
        "2001-01-01",
    ]);
    assert_eq!(revisions(&dated), vec![2]);
    assert_eq!(dated["entries"][0]["at_unix"], 978_307_200 + 3_600);
}

#[test]
fn compaction_keeps_a_receipts_time() {
    let f = Fixture::new();
    // A receipt left as a pending delta is rewritten by compaction.
    let path = receipt(&f, 2);
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value.as_object_mut().unwrap().remove("at_unix");
    value["kind"] = "pending-catalog-delta".into();
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    set_time(&path, 978_307_200);
    f.ok(&["journal", "fixture", "--compact"]);
    let after: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(after["kind"], "completed-catalog-delta");
    assert_eq!(after["at_unix"], 978_307_200);
    let modified = fs::metadata(&path)
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert_eq!(modified, 978_307_200);
}

#[test]
fn known_kinds_name_what_they_must() {
    let f = Fixture::new();
    let kinds = f.ok(&["history", "kinds"]);
    let reviewed = kinds["kinds"]
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["kind"] == "reviewed-type")
        .unwrap();
    assert_eq!(reviewed["needs_ref"], true);
    for (args, ok) in [
        (vec!["--kind", "typed-signature"], false),
        (vec!["--kind", "typed-signature", "--node", "2"], true),
        (vec!["--kind", "reviewed-type", "--node", "2"], false),
        (vec!["--kind", "reviewed-type", "--ref", "SpellCast"], true),
        (
            vec!["--kind", "behaviour-artifact-review", "--node", "2"],
            false,
        ),
        (
            vec![
                "--kind",
                "dependency-list",
                "--ref",
                "tools/profiles/dependencies.json",
            ],
            true,
        ),
        (vec!["--kind", "custom-note"], true),
    ] {
        let mut argv = vec!["history", "append", "fixture", "--summary", "x"];
        argv.extend(args.iter().copied());
        let out = f.run(&argv);
        assert_eq!(
            out.status.success(),
            ok,
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let day = today();
    let typed = f.ok(&[
        "history",
        "show",
        "fixture",
        "--kind",
        "typed-signature",
        "--from",
        &day,
    ]);
    assert_eq!(typed["entries"].as_array().unwrap().len(), 1);
}

/// Revisions that have a catalog receipt, ascending.
fn receipted(f: &Fixture) -> Vec<u64> {
    let dir = f
        .home
        .join("projects/fixture/tools/workspaces/catalog-transactions-v4");
    let mut found: Vec<u64> = fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| {
            let name = e.unwrap().file_name().to_string_lossy().into_owned();
            name.strip_prefix("revision-")?.parse().ok()
        })
        .collect();
    found.sort_unstable();
    found
}

#[test]
fn scope_changes_and_import_reservations_leave_receipts() {
    let f = Fixture::new();
    let p = f.ok(&["project", "show", "fixture"]);
    let revision = p["revision"].as_u64().unwrap().to_string();
    f.ok(&[
        "project",
        "set-scope",
        "fixture",
        "--scope",
        "Rendering",
        "--expect-revision",
        &revision,
    ]);
    let latest = f.ok(&[
        "history", "show", "fixture", "--only", "catalog", "--limit", "1",
    ]);
    let entry = &latest["entries"][0];
    assert_eq!(entry["project"]["research_scope"]["after"], "Rendering");
    assert!(
        research_dashboard::research::history::entry_line(entry).contains("scope -> Rendering"),
        "{entry}"
    );

    // An import: its reservation is an ordinary receipt; its publication has its own.
    let p = f.ok(&["project", "show", "fixture"]);
    let request = serde_json::json!({"schema": 1, "binary": p["binary"], "expect_revision": p["revision"],
        "create": [{"parent": 2, "name": "F", "rva": 0x1000}]});
    let path = f.home.join("import.json");
    fs::write(&path, serde_json::to_vec(&request).unwrap()).unwrap();
    let job = f.ok(&[
        "import",
        "start",
        "fixture",
        "--file",
        path.to_str().unwrap(),
    ]);
    let published = job["revision"].as_u64().unwrap();
    let current = f.ok(&["project", "show", "fixture"])["revision"]
        .as_u64()
        .unwrap();
    assert_eq!(current, published);
    // Every revision since creation has a receipt, except the publication.
    let expected: Vec<u64> = (1..=current).filter(|r| *r != published).collect();
    assert_eq!(receipted(&f), expected);
    let reserved: Value =
        serde_json::from_slice(&fs::read(receipt(&f, published - 1)).unwrap()).unwrap();
    assert_eq!(reserved["after"]["active_import"], job["id"]);
}

#[test]
fn a_cancelled_imports_release_leaves_a_receipt() {
    if !cfg!(debug_assertions) {
        eprintln!("skipped: requires debug-build fault injection (run without --release)");
        return;
    }
    let f = Fixture::new();
    let p = f.ok(&["project", "show", "fixture"]);
    let request = serde_json::json!({"schema": 1, "binary": p["binary"], "expect_revision": p["revision"],
        "create": [{"parent": 2, "name": "F", "rva": 0x1000}]});
    let path = f.home.join("import.json");
    fs::write(&path, serde_json::to_vec(&request).unwrap()).unwrap();
    let failed = std::process::Command::new(env!("CARGO_BIN_EXE_research"))
        .env("SCOPEWORK_COORDINATOR", "off")
        .env("SCOPEWORK_IMPORT_IO_ERROR", "1")
        .arg("--home")
        .arg(&f.home)
        .args([
            "import",
            "start",
            "fixture",
            "--file",
            path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!failed.status.success());
    let id = f.ok(&["project", "show", "fixture"])["active_import"]
        .as_str()
        .unwrap()
        .to_string();
    f.ok(&["import", "cancel", "fixture", &id]);
    let current = f.ok(&["project", "show", "fixture"]);
    assert!(current["active_import"].is_null());
    let current = current["revision"].as_u64().unwrap();
    assert_eq!(receipted(&f), (1..=current).collect::<Vec<_>>());
    let released: Value = serde_json::from_slice(&fs::read(receipt(&f, current)).unwrap()).unwrap();
    assert_eq!(released["before"]["active_import"], id.as_str());
    assert!(released["after"]["active_import"].is_null());
}
