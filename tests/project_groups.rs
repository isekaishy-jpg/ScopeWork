//! `research group` keeps dashboard groups in the research home's settings, never in projects.
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
    fn new() -> Self {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-workspaces");
        let home = base.join(format!("groups-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&home).unwrap();
        let f = Self { home };
        for (id, name) in [("alpha", "Alpha"), ("wow-a", "WoW A"), ("wow-b", "WoW B")] {
            f.ok(&["project", "create", id, "--name", name]);
        }
        f
    }
    fn run(&self, args: &[&str]) -> Output {
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
}

#[test]
fn groups_are_set_cleared_and_listed_without_touching_projects() {
    let f = Fixture::new();
    let catalogs = || {
        ["alpha", "wow-a", "wow-b"]
            .map(|id| fs::read(f.home.join(format!("projects/{id}/project.json"))).unwrap())
    };
    let before = catalogs();
    assert_eq!(
        f.ok(&["group", "list"]),
        json!({"groups": [], "ungrouped": ["alpha", "wow-a", "wow-b"]})
    );
    f.ok(&["group", "set", "wow-b", "WoW"]);
    let listed = f.ok(&["group", "set", "wow-a", " WoW "]);
    assert_eq!(
        listed,
        json!({"groups": [{"name": "WoW", "projects": ["wow-a", "wow-b"]}], "ungrouped": ["alpha"]})
    );
    // Moving a project to another group; clearing the last member removes the group.
    f.ok(&["group", "set", "wow-b", "Other"]);
    f.ok(&["group", "clear", "wow-a"]);
    assert_eq!(
        f.ok(&["group", "list"]),
        json!({"groups": [{"name": "Other", "projects": ["wow-b"]}], "ungrouped": ["alpha", "wow-a"]})
    );
    assert!(!f.run(&["group", "set", "missing", "WoW"]).status.success());
    assert!(!f.run(&["group", "set", "alpha", "  "]).status.success());
    assert!(
        !f.run(&["group", "set", "alpha", &"x".repeat(41)])
            .status
            .success()
    );
    assert_eq!(catalogs(), before, "groups never change a project");
    let stored: Value =
        serde_json::from_slice(&fs::read(f.home.join("settings.json")).unwrap()).unwrap();
    assert_eq!(stored["schema"], 1);
}
