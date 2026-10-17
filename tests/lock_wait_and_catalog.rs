//! CLI lock waiting, holder reporting, orphaned catalog temporaries and compact catalog writes.
use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
    process::Output,
    time::{Duration, Instant},
};

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
        let home = base.join(format!("locks-{}", uuid::Uuid::new_v4()));
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
        f
    }
    fn run(&self, args: &[&str]) -> Output {
        std::process::Command::new(env!("CARGO_BIN_EXE_research"))
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
    fn projects(&self) -> PathBuf {
        self.home.join("projects")
    }
    /// Holds the project's exclusive lock as another writer would, recording a holder.
    fn hold(&self) -> fs::File {
        let folder = self.projects().join(".project-locks");
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(folder.join("fixture.lock"))
            .unwrap();
        file.lock().unwrap();
        fs::write(
            folder.join("fixture.holder"),
            r#"{"pid":4242,"command":"node complete fixture","since":"unix 0"}"#,
        )
        .unwrap();
        file
    }
}

#[test]
fn reads_wait_for_a_writer_instead_of_failing_busy() {
    let f = Fixture::new();
    let held = f.hold();
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        drop(held);
    });
    let start = Instant::now();
    let shown = f.ok(&["--wait", "10", "node", "show", "fixture", "0"]);
    assert!(start.elapsed() >= Duration::from_millis(350));
    assert_eq!(shown["node"]["id"], 0);
    release.join().unwrap();
}

#[test]
fn zero_wait_reports_busy_with_the_recorded_holder() {
    let f = Fixture::new();
    let _held = f.hold();
    let out = f.run(&["--wait", "0", "node", "show", "fixture", "0"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("busy"), "{err}");
    assert!(
        err.contains("pid 4242") && err.contains("node complete fixture"),
        "{err}"
    );
}

#[test]
fn writes_remove_orphaned_temporaries_and_store_compact_catalogs() {
    let f = Fixture::new();
    let dir = f.projects().join("fixture");
    let orphan = dir.join("project.0b6f2f6e-5d4f-4a3e-9c1a-2f5e3d8a9b10.pending");
    fs::write(&orphan, b"interrupted").unwrap();
    f.ok(&[
        "node",
        "create",
        "fixture",
        "--parent",
        "0",
        "--name",
        "Architecture",
    ]);
    assert!(
        !orphan.exists(),
        "orphaned catalog temporary must be removed"
    );
    let catalog = fs::read_to_string(dir.join("project.json")).unwrap();
    assert!(!catalog.contains("\n  "), "catalog should be compact JSON");
    let parsed: Value = serde_json::from_str(&catalog).unwrap();
    assert_eq!(parsed["nodes"].as_array().unwrap().len(), 2);
}
