//! The implementation tracker through the `research` executable: tree, placements, items,
//! counters, completion and history.
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
    /// A project with architecture 1, chunk 2 and functions 3, 4 and 5.
    fn new() -> Self {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-workspaces");
        let home = base.join(format!("tracker-{}", uuid::Uuid::new_v4()));
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
        for (name, rva) in [("Alloc", "1000"), ("Free", "2000"), ("Init", "3000")] {
            f.ok(&[
                "node", "create", "fixture", "--parent", "2", "--name", name, "--rva", rva,
            ]);
        }
        f
    }
    fn run(&self, args: &[&str]) -> Output {
        std::process::Command::new(env!("CARGO_BIN_EXE_research"))
            .env("SCOPEWORK_COORDINATOR", "off")
            .env("SCOPEWORK_SUBMITTER", "agent-a")
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
        assert!(!o.status.success(), "{args:?} should fail");
        String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
    }
    fn node(&self, view: &Value, id: u64) -> Value {
        view["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["id"] == id)
            .cloned()
            .unwrap()
    }
}

#[test]
fn a_tree_with_placements_and_items_counts_up_its_subtrees() {
    let f = Fixture::new();
    f.ok(&[
        "impl",
        "repo",
        "fixture",
        "--path",
        "C:/src/fixture-rs",
        "--remote",
        "https://example.invalid/fixture-rs.git",
    ]);
    let krate = f.ok(&["impl", "add", "fixture", "--name", "fixture_rs"])["node"]
        .as_u64()
        .unwrap();
    let k = krate.to_string();
    let heap = f.ok(&[
        "impl", "add", "fixture", "--name", "heap.rs", "--parent", &k,
    ])["node"]
        .as_u64()
        .unwrap();
    let init = f.ok(&[
        "impl", "add", "fixture", "--name", "init.rs", "--parent", &k,
    ])["node"]
        .as_u64()
        .unwrap();
    let (h, i) = (heap.to_string(), init.to_string());
    assert_eq!(
        f.ok(&["impl", "place", "fixture", &h, "3", "4"])["placed"],
        json!([3, 4])
    );
    f.ok(&["impl", "place", "fixture", &i, "5"]);
    let item = f.ok(&[
        "impl",
        "item-add",
        "fixture",
        &h,
        "--name",
        "Host heap shim",
    ])["item"]
        .as_u64()
        .unwrap();
    f.ok(&[
        "impl",
        "complete",
        "fixture",
        "--record",
        "3",
        "--item",
        &item.to_string(),
    ]);

    let view = f.ok(&["impl", "show", "fixture", "--node", &h]);
    assert_eq!(view["repository"], "C:/src/fixture-rs");
    assert_eq!(view["not_placed"], 0);
    assert_eq!(
        view["totals"],
        json!({"research": {"done": 1, "total": 3}, "implementation_only": {"done": 1, "total": 1}})
    );
    assert_eq!(
        f.node(&view, krate)["subtree"],
        json!({"research": {"done": 1, "total": 3}, "implementation_only": {"done": 1, "total": 1}})
    );
    assert_eq!(
        f.node(&view, krate)["own"]["research"],
        json!({"done": 0, "total": 0})
    );
    assert_eq!(f.node(&view, heap)["path"], "fixture_rs/heap.rs");
    let placements = view["node"]["placements"].as_array().unwrap();
    assert_eq!(placements[0]["name"], "Alloc");
    assert_eq!(placements[0]["rva"], "0x00001000");
    assert_eq!(placements[0]["completed"]["by"], "agent-a");
    assert!(placements[0]["completed"]["at_unix"].as_u64().unwrap() > 0);
    assert!(placements[1]["completed"].is_null());
    assert_eq!(view["node"]["items"][0]["name"], "Host heap shim");

    // Reopening clears the completion.
    f.ok(&["impl", "reopen", "fixture", "--record", "3"]);
    let view = f.ok(&["impl", "show", "fixture"]);
    assert_eq!(view["totals"]["research"], json!({"done": 0, "total": 3}));
}

#[test]
fn a_record_is_placed_once_and_found_where_it_is_implemented() {
    let f = Fixture::new();
    let a = f.ok(&["impl", "add", "fixture", "--name", "a"])["node"]
        .as_u64()
        .unwrap()
        .to_string();
    let b = f.ok(&["impl", "add", "fixture", "--name", "b"])["node"]
        .as_u64()
        .unwrap()
        .to_string();
    f.ok(&["impl", "place", "fixture", &a, "3"]);
    // Placing it again on the same node changes nothing; elsewhere is refused.
    assert_eq!(
        f.ok(&["impl", "place", "fixture", &a, "3"])["placed"],
        json!([])
    );
    assert!(
        f.fails(&["impl", "place", "fixture", &b, "3"])
            .contains("already placed in a")
    );
    assert!(
        f.fails(&["impl", "place", "fixture", &b, "2"])
            .contains("not a function")
    );
    assert!(
        f.fails(&["impl", "place", "fixture", &b, "99"])
            .contains("Unknown record")
    );
    assert_eq!(f.ok(&["impl", "where", "fixture", "3"])["path"], "a");
    assert!(f.ok(&["impl", "where", "fixture", "4"])["node"].is_null());
    let unplaced = f.ok(&["impl", "show", "fixture", "--unplaced"]);
    assert_eq!(unplaced["not_placed"], 2);
    let ids: Vec<u64> = unplaced["unplaced"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids, vec![4, 5]);
    // Unplace, then it may go elsewhere.
    assert_eq!(
        f.ok(&["impl", "unplace", "fixture", "3"])["unplaced"],
        json!([3])
    );
    f.ok(&["impl", "place", "fixture", &b, "3"]);
    assert_eq!(f.ok(&["impl", "where", "fixture", "3"])["path"], "b");
    assert!(
        f.fails(&["impl", "complete", "fixture", "--record", "4"])
            .contains("not placed")
    );
}

#[test]
fn nodes_move_rename_and_are_removed_only_when_empty() {
    let f = Fixture::new();
    let top = f.ok(&["impl", "add", "fixture", "--name", "top"])["node"]
        .as_u64()
        .unwrap()
        .to_string();
    let child = f.ok(&[
        "impl", "add", "fixture", "--name", "child", "--parent", &top,
    ])["node"]
        .as_u64()
        .unwrap()
        .to_string();
    assert!(
        f.fails(&["impl", "move", "fixture", &top, "--parent", &child])
            .contains("inside itself")
    );
    f.ok(&["impl", "rename", "fixture", &child, "--name", "renamed"]);
    f.ok(&["impl", "move", "fixture", &child]);
    let view = f.ok(&["impl", "show", "fixture"]);
    assert_eq!(f.node(&view, child.parse().unwrap())["path"], "renamed");
    f.ok(&["impl", "place", "fixture", &child, "4"]);
    assert!(
        f.fails(&["impl", "remove", "fixture", &child])
            .contains("still holds")
    );
    f.ok(&["impl", "move", "fixture", &child, "--parent", &top]);
    assert!(
        f.fails(&["impl", "remove", "fixture", &top])
            .contains("child nodes")
    );
    f.ok(&["impl", "unplace", "fixture", "4"]);
    f.ok(&["impl", "remove", "fixture", &child]);
    f.ok(&["impl", "remove", "fixture", &top]);
    assert_eq!(f.ok(&["impl", "show", "fixture"])["nodes"], json!([]));
    assert!(
        f.fails(&["impl", "add", "fixture", "--name", " "])
            .contains("single line")
    );
}

#[test]
fn tracker_changes_are_journaled_with_history() {
    let f = Fixture::new();
    let before = f.ok(&["project", "revision", "fixture"])["revision"]
        .as_u64()
        .unwrap();
    let node = f.ok(&["impl", "add", "fixture", "--name", "crate"])["node"]
        .as_u64()
        .unwrap()
        .to_string();
    f.ok(&["impl", "place", "fixture", &node, "3"]);
    let after = f.ok(&["project", "revision", "fixture"])["revision"]
        .as_u64()
        .unwrap();
    assert_eq!(after, before + 2, "each change is one commit");
    let history = f.ok(&[
        "history", "show", "fixture", "--only", "catalog", "--limit", "2",
    ]);
    let sidecars: Vec<String> = history["entries"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|e| e["sidecars"].as_array().unwrap().clone())
        .map(|s| s.as_str().unwrap().to_string())
        .collect();
    assert!(sidecars.contains(&"implementation/tree.json".to_string()));
    assert!(sidecars.contains(&format!("implementation/nodes/{node:0>6}.json")));
    // A node that never held anything has no file.
    f.ok(&["impl", "add", "fixture", "--name", "empty"]);
    let files = fs::read_dir(f.home.join("projects/fixture/implementation/nodes"))
        .unwrap()
        .count();
    assert_eq!(files, 1);
}
