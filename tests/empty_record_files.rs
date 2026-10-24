//! Record folders hold only what is used: no generated prerequisite views, no import markers
//! after a commit, no empty `data/` folders; `project tidy` cleans folders made by older builds.
use serde_json::{Value, json};
use std::{fs, path::PathBuf, process::Command};

struct Fixture {
    home: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-workspaces");
        let home = base.join(format!("empty-files-{}", uuid::Uuid::new_v4()));
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
    fn command(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_research"));
        c.env("SCOPEWORK_COORDINATOR", "off");
        c.arg("--home").arg(&self.home).args(args);
        c
    }
    fn ok(&self, args: &[&str]) -> Value {
        let o = self.command(args).output().unwrap();
        assert!(
            o.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        serde_json::from_slice(&o.stdout).unwrap()
    }
    /// The folder of record `id`, from `node show`.
    fn folder(&self, id: u64) -> PathBuf {
        let shown = self.ok(&["node", "show", "fixture", &id.to_string()]);
        PathBuf::from(shown["report"].as_str().unwrap())
            .parent()
            .unwrap()
            .to_path_buf()
    }
    fn entries(&self, id: u64) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.folder(id))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n != "research.md" && !n.chars().all(|c| c.is_ascii_hexdigit()))
            .collect();
        names.sort();
        names
    }
    fn import(&self, count: usize) -> Value {
        assert!(count <= 20, "small fixtures only");
        let p = self.ok(&["project", "show", "fixture"]);
        let request = json!({"schema": 1, "binary": p["binary"], "expect_revision": p["revision"],
            "create": (0..count).map(|i| json!({"parent": 2, "name": format!("F{i}"), "rva": 0x1000 + i * 16})).collect::<Vec<_>>()});
        let path = self.home.join("import.json");
        fs::write(&path, serde_json::to_vec(&request).unwrap()).unwrap();
        self.ok(&[
            "import",
            "start",
            "fixture",
            "--file",
            path.to_str().unwrap(),
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

#[test]
fn an_import_leaves_only_reports() {
    let f = Fixture::new();
    f.import(3);
    let p = f.ok(&["project", "show", "fixture"]);
    for node in p["nodes"].as_array().unwrap() {
        let id = node["id"].as_u64().unwrap();
        if id == 0 {
            continue;
        }
        assert_eq!(f.entries(id), Vec::<String>::new(), "record {id}");
    }
}

#[test]
fn saves_and_prerequisites_write_no_views_or_data_folders() {
    let f = Fixture::new();
    f.ok(&[
        "node", "create", "fixture", "--parent", "2", "--name", "A", "--rva", "1000",
    ]);
    f.ok(&[
        "node", "create", "fixture", "--parent", "2", "--name", "B", "--rva", "2000",
    ]);
    let b = f.ok(&["node", "show", "fixture", "4"]);
    f.ok(&[
        "deps",
        "set",
        "fixture",
        "4",
        "1000",
        "--expect",
        b["structural_token"].as_str().unwrap(),
    ]);
    let shown = f.ok(&["node", "show", "fixture", "3"]);
    let body = f.home.join("body.md");
    let evidence = f.home.join("evidence.md");
    fs::write(&body, "Draft").unwrap();
    fs::write(&evidence, "Evidence").unwrap();
    f.ok(&[
        "node",
        "save",
        "fixture",
        "3",
        "--expect",
        shown["token"].as_str().unwrap(),
        "--body-file",
        body.to_str().unwrap(),
        "--evidence-file",
        evidence.to_str().unwrap(),
    ]);
    assert_eq!(f.entries(3), Vec::<String>::new());
    assert_eq!(f.entries(4), Vec::<String>::new());
    // The prerequisite is still answered from the catalog.
    assert_eq!(
        f.ok(&["deps", "get", "fixture", "4"])["requires"],
        json!([3])
    );
    // The data path is still reported, for tools to create when they store something.
    assert!(shown["data"].as_str().unwrap().ends_with("data"));
}

#[test]
fn tidy_removes_exactly_what_holds_nothing() {
    let f = Fixture::new();
    for (name, rva) in [("A", "1000"), ("B", "2000"), ("C", "3000")] {
        f.ok(&[
            "node", "create", "fixture", "--parent", "2", "--name", name, "--rva", rva,
        ]);
    }
    let header = "# Managed by Scopework. Edit through the app or research deps set.\nschema = 1\n";
    let (a, b, c) = (f.folder(3), f.folder(4), f.folder(5));
    // Record A: everything an older build left behind.
    fs::write(a.join("dep.toml"), header).unwrap();
    fs::write(a.join("prereq.toml"), header).unwrap();
    fs::write(a.join(".scopework-import-old-job"), "old-job").unwrap();
    fs::create_dir(a.join("data")).unwrap();
    // Record B: a hand-written file and a used data folder stay.
    fs::write(b.join("dep.toml"), "hand written\n").unwrap();
    fs::create_dir(b.join("data")).unwrap();
    fs::write(b.join("data/capture.bin"), [1u8, 2, 3]).unwrap();
    // Record C: an unrelated file stays.
    fs::write(c.join("notes.txt"), "kept").unwrap();

    let preview = f.ok(&["project", "tidy", "fixture"]);
    assert_eq!(preview["applied"], false);
    assert_eq!(preview["views"]["remove"], 2);
    assert_eq!(preview["views"]["kept"], 1);
    assert_eq!(preview["import_markers"]["remove"], 1);
    assert_eq!(preview["data_folders"]["remove"], 1);
    assert_eq!(preview["data_folders"]["kept"], 1);
    assert!(a.join("dep.toml").is_file(), "a preview removes nothing");

    let applied = f.ok(&["project", "tidy", "fixture", "--apply"]);
    assert_eq!(
        applied["removed"],
        json!({"views": 2, "import_markers": 1, "data_folders": 1})
    );
    assert_eq!(applied["failed"], json!([]));
    assert_eq!(f.entries(3), Vec::<String>::new());
    assert_eq!(
        f.entries(4),
        vec!["data".to_string(), "dep.toml".to_string()]
    );
    assert_eq!(f.entries(5), vec!["notes.txt".to_string()]);
    let history = f.ok(&["history", "show", "fixture", "--kind", "project-tidy"]);
    assert_eq!(history["entries"].as_array().unwrap().len(), 1);

    let again = f.ok(&["project", "tidy", "fixture", "--apply"]);
    assert_eq!(
        again["removed"],
        json!({"views": 0, "import_markers": 0, "data_folders": 0})
    );
}

#[test]
fn tidy_keeps_the_active_imports_markers() {
    if !cfg!(debug_assertions) {
        eprintln!("skipped: requires debug-build fault injection (run without --release)");
        return;
    }
    let f = Fixture::new();
    let p = f.ok(&["project", "show", "fixture"]);
    let request = json!({"schema": 1, "binary": p["binary"], "expect_revision": p["revision"],
        "create": [{"parent": 2, "name": "F", "rva": 0x1000}]});
    let path = f.home.join("import.json");
    fs::write(&path, serde_json::to_vec(&request).unwrap()).unwrap();
    let failed = f
        .command(&[
            "import",
            "start",
            "fixture",
            "--file",
            path.to_str().unwrap(),
        ])
        .env("SCOPEWORK_IMPORT_IO_ERROR", "1")
        .output()
        .unwrap();
    assert!(!failed.status.success());
    let active = f.ok(&["project", "show", "fixture"])["active_import"]
        .as_str()
        .unwrap()
        .to_string();
    let chunk = f.folder(2);
    fs::write(chunk.join(format!(".scopework-import-{active}")), &active).unwrap();
    fs::write(chunk.join(".scopework-import-committed-job"), "x").unwrap();
    let applied = f.ok(&["project", "tidy", "fixture", "--apply"]);
    assert_eq!(applied["removed"]["import_markers"], 1);
    assert_eq!(applied["import_markers"]["kept"], 1);
    assert!(chunk.join(format!(".scopework-import-{active}")).is_file());
    assert!(!chunk.join(".scopework-import-committed-job").exists());
}

#[test]
fn prune_selects_an_empty_chunk_once_its_marker_is_gone() {
    let f = Fixture::new();
    f.ok(&[
        "node", "create", "fixture", "--parent", "1", "--name", "Empty",
    ]);
    fs::write(f.folder(3).join(".scopework-import-old-job"), "old-job").unwrap();
    let preview = || {
        let p = f.ok(&["project", "show", "fixture"]);
        let request = json!({"schema": 1, "binary": p["binary"], "expect_revision": p["revision"], "ids": [3]});
        let path = f.home.join("prune.json");
        fs::write(&path, serde_json::to_vec(&request).unwrap()).unwrap();
        f.ok(&["node", "prune", "fixture", "--file", path.to_str().unwrap()])
    };
    let blocked = preview();
    assert_eq!(blocked["candidates"], json!([]));
    assert_eq!(blocked["excluded"][0]["reason"], "extra-files");
    f.ok(&["project", "tidy", "fixture", "--apply"]);
    let candidates = preview()["candidates"].clone();
    assert_eq!(candidates.as_array().unwrap().len(), 1);
    assert_eq!(candidates[0]["id"], 3);
}
