//! Artifacts and address spaces: a project's input files, and addresses counted in another
//! file or processor than the default image.
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

const DISK: &str = "9a27558e61c5d35f4640710c5cb9f2f99e2f87f3432045a2e3614475ee98ff2d";
const FILE: &str = "6f2e35e9efb90adb4a78addaeea9ab6ef216e4475833e1379de098301e83f008";

struct Fixture {
    home: PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.home);
    }
}
impl Fixture {
    /// A project with an architecture (1), a chunk (2) and a function at 0x1000 (3).
    fn new() -> Self {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-workspaces");
        let home = base.join(format!("spaces-{}", uuid::Uuid::new_v4()));
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
        f.ok(&[
            "node", "create", "fixture", "--parent", "2", "--name", "F", "--rva", "1000",
        ]);
        f
    }
    fn run(&self, args: &[&str]) -> std::process::Output {
        std::process::Command::new(env!("CARGO_BIN_EXE_research"))
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
    fn fails(&self, args: &[&str]) -> String {
        let o = self.run(args);
        assert!(!o.status.success(), "{args:?} succeeded");
        String::from_utf8_lossy(&o.stderr).into_owned()
    }
    fn project(&self) -> PathBuf {
        self.home.join("projects/fixture")
    }
    fn catalog(&self) -> Value {
        serde_json::from_slice(&fs::read(self.project().join("project.json")).unwrap()).unwrap()
    }
    fn revision(&self) -> String {
        self.catalog()["revision"].to_string()
    }
}

#[test]
fn artifacts_and_spaces_are_journaled_catalog_fields_that_raise_the_format() {
    let f = Fixture::new();
    let before = fs::read(f.project().join("project.json")).unwrap();
    assert_eq!(f.catalog()["layout_version"], 5);
    assert!(!String::from_utf8_lossy(&before).contains("artifacts"));

    let r = f.revision();
    let bad = f.fails(&[
        "project",
        "artifact",
        "add",
        "fixture",
        "--name",
        "Disk",
        "--sha256",
        "abc",
        "--expect-revision",
        &r,
    ]);
    assert!(bad.contains("64 lowercase hexadecimal"), "{bad}");
    assert_eq!(f.catalog()["layout_version"], 5);

    let added = f.ok(&[
        "project",
        "artifact",
        "add",
        "fixture",
        "--name",
        "Disk",
        "--sha256",
        DISK,
        "--size",
        "901120",
        "--format",
        "ADF",
        "--expect-revision",
        &r,
    ]);
    assert_eq!(added["artifact"], 1);
    assert_eq!(added["layout_version"], 7);
    let r = f.revision();
    // The extracted file names the disk it is on, and the path there.
    let inside = f.ok(&[
        "project",
        "artifact",
        "add",
        "fixture",
        "--name",
        "DAD",
        "--sha256",
        FILE,
        "--inside",
        "1",
        "--path",
        "c/DAD",
        "--expect-revision",
        &r,
    ]);
    assert_eq!(inside["artifacts"][1]["inside"], 1);
    let r = f.revision();
    assert!(
        f.fails(&[
            "project",
            "artifact",
            "add",
            "fixture",
            "--name",
            "Disk",
            "--sha256",
            FILE,
            "--expect-revision",
            &r,
        ])
        .contains("repeats")
    );
    assert!(
        f.fails(&[
            "project",
            "space",
            "add",
            "fixture",
            "--name",
            "Land",
            "--kind",
            "file",
            "--expect-revision",
            &r,
        ])
        .contains("needs the artifact")
    );
    let space = f.ok(&[
        "project",
        "space",
        "add",
        "fixture",
        "--name",
        "Land",
        "--kind",
        "file",
        "--artifact",
        "2",
        "--description",
        "Offsets in DAD",
        "--expect-revision",
        &r,
    ]);
    assert_eq!(space["space"], 1);
    // A stale revision is refused.
    assert!(
        f.fails(&[
            "project",
            "space",
            "add",
            "fixture",
            "--name",
            "Sound",
            "--kind",
            "processor",
            "--expect-revision",
            &r,
        ])
        .contains("Project changed")
    );

    // Every change left a receipt, so history has it.
    let receipts = f.project().join("tools/workspaces/catalog-transactions-v4");
    let revision: u64 = f.revision().parse().unwrap();
    assert!(receipts.join(format!("revision-{revision}")).is_dir());
    let shown = f.ok(&["project", "show", "fixture"]);
    assert_eq!(shown["spaces"][0]["name"], "Land", "{shown}");
    assert_eq!(shown["artifacts"].as_array().unwrap().len(), 2);

    // In use: neither the artifact the space names nor the one the file is inside can go.
    let r = f.revision();
    assert!(
        f.fails(&[
            "project",
            "artifact",
            "remove",
            "fixture",
            "2",
            "--expect-revision",
            &r
        ])
        .contains("in use")
    );
    assert!(
        f.fails(&[
            "project",
            "artifact",
            "remove",
            "fixture",
            "1",
            "--expect-revision",
            &r
        ])
        .contains("in use")
    );
    let updated = f.ok(&[
        "project",
        "space",
        "update",
        "fixture",
        "1",
        "--description",
        "",
        "--expect-revision",
        &r,
    ]);
    assert_eq!(updated["spaces"][0].get("description"), None);
}

#[test]
fn an_address_is_unique_per_space_and_spaced_records_get_their_own_folders() {
    let f = Fixture::new();
    let r = f.revision();
    f.ok(&[
        "project",
        "artifact",
        "add",
        "fixture",
        "--name",
        "Disk",
        "--sha256",
        DISK,
        "--expect-revision",
        &r,
    ]);
    let r = f.revision();
    f.ok(&[
        "project",
        "space",
        "add",
        "fixture",
        "--name",
        "Land",
        "--kind",
        "file",
        "--artifact",
        "1",
        "--expect-revision",
        &r,
    ]);
    // The same number as function 3, in the file space.
    let table = f.ok(&[
        "node", "create", "fixture", "--parent", "2", "--name", "Table", "--rva", "1000",
        "--length", "64", "--space", "1",
    ]);
    let table_id = table["node"]["id"].as_u64().unwrap().to_string();
    assert_eq!(table["node"]["space"], 1);
    let report = f
        .project()
        .join("architectures/000001/000002/S1-D00001000/research.md");
    assert!(report.is_file(), "{table}");
    let text = fs::read_to_string(&report).unwrap();
    assert!(text.contains("Space: Land (S1)"), "{text}");
    let code = f.ok(&[
        "node", "create", "fixture", "--parent", "2", "--name", "Code", "--rva", "2000", "--space",
        "1",
    ]);
    assert!(
        f.project()
            .join("architectures/000001/000002/S1-00002000/research.md")
            .is_file(),
        "{code}"
    );
    assert!(
        f.fails(&[
            "node", "create", "fixture", "--parent", "2", "--name", "Again", "--rva", "1000",
            "--space", "1",
        ])
        .contains("already has")
    );
    assert!(
        f.fails(&[
            "node", "create", "fixture", "--parent", "2", "--name", "Nowhere", "--rva", "3000",
            "--space", "9",
        ])
        .contains("space")
    );
    // A batch creation checks the same identity.
    let r = f.revision();
    let batch = f.home.join("batch.json");
    fs::write(
        &batch,
        serde_json::to_vec(&json!({
            "schema": 1, "binary": "synthetic", "expect_revision": r.parse::<u64>().unwrap(),
            "create": [{"parent": 2, "name": "Dup", "rva": 4096, "space": 1}],
        }))
        .unwrap(),
    )
    .unwrap();
    assert!(
        f.fails(&[
            "node",
            "batch",
            "fixture",
            "--file",
            batch.to_str().unwrap()
        ])
        .to_lowercase()
        .contains("duplicate")
    );

    // A prerequisite in another space is named `S<n>:<RVA>`.
    let token = f.ok(&["node", "show", "fixture", "3"])["structural_token"]
        .as_str()
        .unwrap()
        .to_string();
    let deps = f.ok(&["deps", "set", "fixture", "3", "--expect", &token, "S1:1000"]);
    assert!(deps.to_string().contains(&table_id), "{deps}");
    // Ledger rows name default-space addresses: the function at 0x1000, not the table there.
    let ledger = f.project().join("tools/profiles/annotations.json");
    fs::create_dir_all(ledger.parent().unwrap()).unwrap();
    fs::write(
        &ledger,
        json!({"schema": 1, "types": [], "annotations": [
            {"kind": "function-name", "rva": "0x00001000", "value": "F", "reason": "r"},
        ]})
        .to_string(),
    )
    .unwrap();
    let conflicts = f.ok(&["store", "conflicts", "fixture"]);
    assert_eq!(conflicts["conflicts"], 0, "{conflicts}");
    // A space with records cannot go.
    let r = f.revision();
    assert!(
        f.fails(&[
            "project",
            "space",
            "remove",
            "fixture",
            "1",
            "--expect-revision",
            &r
        ])
        .contains("still holds")
    );
}

#[test]
fn the_store_follows_artifact_and_space_changes() {
    let f = Fixture::new();
    f.ok(&["store", "init", "fixture"]);
    let r = f.revision();
    f.ok(&[
        "project",
        "artifact",
        "add",
        "fixture",
        "--name",
        "Disk",
        "--sha256",
        DISK,
        "--expect-revision",
        &r,
    ]);
    let sync = f.ok(&["store", "sync", "fixture"]);
    assert_eq!(sync["commits_added"], 1, "{sync}");
    let verify = f.ok(&["store", "verify", "fixture"]);
    assert_eq!(verify["ok"], true, "{verify}");
    let revision: u64 = f.revision().parse().unwrap();
    let out = f.home.join("catalog.json");
    f.ok(&[
        "store",
        "catalog",
        "fixture",
        "--revision",
        &revision.to_string(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert_eq!(
        fs::read(&out).unwrap(),
        fs::read(f.project().join("project.json")).unwrap()
    );
}
