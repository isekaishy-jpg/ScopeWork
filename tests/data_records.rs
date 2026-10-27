//! Data items: a leaf kind beside functions in a chunk, with an address and a length.
use serde_json::{Value, json};
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
    /// A project with an architecture (1), a chunk (2), a second chunk (3) and a function at
    /// 0x1000 (4).
    fn new() -> Self {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-workspaces");
        let home = base.join(format!("data-records-{}", uuid::Uuid::new_v4()));
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
            "node", "create", "fixture", "--parent", "1", "--name", "Other",
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
    fn data(&self, name: &str, rva: &str, length: &str) -> Value {
        self.ok(&[
            "node", "create", "fixture", "--parent", "2", "--name", name, "--rva", rva, "--length",
            length,
        ])
    }
    fn file(&self, name: &str, text: &str) -> String {
        let path = self.home.join(name);
        fs::write(&path, text).unwrap();
        path.to_string_lossy().into_owned()
    }
    fn write_report(&self, id: &str) {
        let body = self.file(&format!("body-{id}.md"), &format!("Report for {id}."));
        let evidence = self.file(&format!("evidence-{id}.md"), "Fixture evidence.");
        let token = self.ok(&["node", "show", "fixture", id])["token"]
            .as_str()
            .unwrap()
            .to_string();
        self.ok(&[
            "node",
            "save",
            "fixture",
            id,
            "--body-file",
            &body,
            "--evidence-file",
            &evidence,
            "--expect",
            &token,
        ]);
    }
    fn complete(&self, id: &str) {
        let token = self.ok(&["node", "show", "fixture", id])["completion_token"]
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
    fn batch(&self, request: Value) -> std::process::Output {
        let mut request = request;
        request["binary"] = "synthetic".into();
        request["expect_revision"] = self.catalog()["revision"].clone();
        let file = self.file("batch.json", &request.to_string());
        self.run(&["node", "batch", "fixture", "--file", &file])
    }
}

#[test]
fn a_data_item_lives_beside_functions_and_raises_the_format() {
    let f = Fixture::new();
    // Without data the catalog keeps format 5 and writes no length.
    let before = fs::read_to_string(f.project().join("project.json")).unwrap();
    assert_eq!(f.catalog()["layout_version"], 5);
    assert!(!before.contains("length"));

    let item = f.data("Table", "2000", "12");
    assert_eq!(item["node"]["kind"], "Data");
    assert_eq!(item["node"]["length"], 12);
    assert_eq!(f.catalog()["layout_version"], 6);
    let report = f
        .project()
        .join("architectures/000001/000002/D00002000/research.md");
    let text = fs::read_to_string(&report).unwrap();
    assert!(text.contains("Kind: Data"), "{text}");
    assert!(text.contains("RVA: `0x00002000`"), "{text}");
    assert!(text.contains("Length: 12 bytes"), "{text}");
    assert!(
        item["criteria"]
            .as_str()
            .unwrap()
            .starts_with("Explain this data item's")
    );

    // The catalog opens again (validation accepts format 6 with data).
    let listed = f.ok(&["node", "list", "fixture", "--kind", "data"]);
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert!(
        listed[0]["report"]
            .as_str()
            .unwrap()
            .ends_with("architectures/000001/000002/D00002000/research.md")
    );
}

#[test]
fn an_address_names_one_record_of_either_kind() {
    let f = Fixture::new();
    // A function's address is not free for data, nor a data item's for a function.
    assert!(
        f.fails(&[
            "node", "create", "fixture", "--parent", "2", "--name", "Clash", "--rva", "1000",
            "--length", "4",
        ])
        .contains("already has a function or data item")
    );
    f.data("Table", "2000", "12");
    assert!(
        f.fails(&[
            "node", "create", "fixture", "--parent", "2", "--name", "Clash", "--rva", "2000",
        ])
        .contains("already has a function or data item")
    );
    // A data item inside another one's extent is allowed: containment follows the extents.
    f.data("Field", "2004", "4");
    // Data belongs to a chunk, has a length of at least one byte, and holds no children.
    assert!(
        f.fails(&[
            "node", "create", "fixture", "--parent", "1", "--name", "X", "--length", "4",
        ])
        .contains("belongs to a chunk")
    );
    assert!(
        f.fails(&[
            "node", "create", "fixture", "--parent", "2", "--name", "Z", "--rva", "3000",
            "--length", "0"
        ])
        .contains("at least one byte")
    );
    assert!(
        f.fails(&[
            "node", "create", "fixture", "--parent", "5", "--name", "Child"
        ])
        .contains("cannot have children")
    );
}

#[test]
fn prerequisites_connect_functions_and_data_and_completion_needs_evidence() {
    let f = Fixture::new();
    f.data("Table", "2000", "12");
    let token = |id: &str| {
        f.ok(&["node", "show", "fixture", id])["structural_token"]
            .as_str()
            .unwrap()
            .to_string()
    };
    // Function to data, and data to function.
    f.ok(&[
        "deps",
        "set",
        "fixture",
        "4",
        "--expect",
        &token("4"),
        "2000",
    ]);
    let shown = f.ok(&["deps", "set", "fixture", "5", "--expect", &token("5")]);
    assert_eq!(shown["node"]["id"], 5);
    let deps = f.catalog()["dependencies"].clone();
    assert_eq!(deps, json!([{"dependent":4,"prerequisite":5}]));

    // The function waits for its data prerequisite, and the data item needs evidence.
    f.write_report("4");
    let blocked = f.run(&[
        "node",
        "complete",
        "fixture",
        "4",
        "--expect",
        f.ok(&["node", "show", "fixture", "4"])["completion_token"]
            .as_str()
            .unwrap(),
        "--reviewed",
    ]);
    assert!(!blocked.status.success());
    f.write_report("5");
    f.complete("5");
    f.complete("4");

    // The chunk synthesizes both kinds of children.
    f.write_report("2");
    f.complete("2");
    let summary = f.ok(&["status", "fixture", "--summary"]);
    assert_eq!(
        summary["data_items"],
        json!({"total":1,"required":1,"complete":1})
    );
    assert_eq!(summary["inventory_functions"], 1);
    assert_eq!(summary["counts"]["Data"]["Complete"], 1);
}

#[test]
fn batches_create_move_and_resize_data_items() {
    let f = Fixture::new();
    let created = f.batch(json!({"schema":1,"create":[
        {"parent":2,"name":"Table","rva":8192,"length":16},
        {"parent":2,"name":"Func","rva":12288}
    ],"update":[],"reorder":[]}));
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let nodes = f.catalog()["nodes"].clone();
    let table = nodes
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["name"] == "Table")
        .unwrap();
    assert_eq!(table["kind"], "Data");
    assert_eq!(table["length"], 16);
    let id = table["id"].as_u64().unwrap();
    let revision = table["revision"].as_u64().unwrap();
    let func = nodes
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["name"] == "Func")
        .unwrap();
    assert_eq!(func["kind"], "Function");
    assert!(func.get("length").is_none());

    // Resize and move to the other chunk; the report follows to its new folder.
    let moved = f.batch(json!({"schema":2,"create":[],"update":[
        {"id":id,"parent":3,"length":24,"expect_node_revision":revision}
    ],"reorder":[]}));
    assert!(
        moved.status.success(),
        "{}",
        String::from_utf8_lossy(&moved.stderr)
    );
    let report = f
        .project()
        .join("architectures/000001/000003/D00002000/research.md");
    assert!(
        fs::read_to_string(report)
            .unwrap()
            .contains("Length: 24 bytes")
    );

    // A length on a function, and a data item under an architecture, are refused.
    let func_id = func["id"].as_u64().unwrap();
    let wrong =
        f.batch(json!({"schema":1,"create":[],"update":[{"id":func_id,"length":4}],"reorder":[]}));
    assert!(String::from_utf8_lossy(&wrong.stderr).contains("Only a data item has a length"));
    let wrong =
        f.batch(json!({"schema":1,"create":[],"update":[{"id":id,"parent":1}],"reorder":[]}));
    assert!(String::from_utf8_lossy(&wrong.stderr).contains("hierarchy"));
}
