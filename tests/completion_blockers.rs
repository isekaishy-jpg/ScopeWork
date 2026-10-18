//! What the Research records window reports as blocking completion.
use research_dashboard::research::Library;
use std::{fs, path::PathBuf};

struct Home(PathBuf);
impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn blockers_name_flags_open_children_and_prerequisites() {
    let home = Home(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-workspaces")
            .join(format!("blockers-{}", uuid::Uuid::new_v4())),
    );
    let root = home.0.join("projects");
    fs::create_dir_all(&root).unwrap();
    let mut lib = Library::open_project(root.clone(), Some("fixture")).unwrap();
    lib.create("fixture", "Fixture", "synthetic").unwrap();
    let arch = lib.add(0, 0, "Architecture", "").unwrap();
    assert_eq!(
        lib.completion_blockers(0, arch).unwrap(),
        vec!["No required chunk records yet.".to_string()]
    );
    let chunk = lib.add(0, arch, "Chunk", "").unwrap();
    let first = lib.add(0, chunk, "First", "1000").unwrap();
    let second = lib.add(0, chunk, "Second", "2000").unwrap();
    lib.set_dependencies(0, second, "1000").unwrap();
    lib.set_review_flag(0, chunk, Some("Check offsets"))
        .unwrap();
    assert_eq!(
        lib.completion_blockers(0, chunk).unwrap(),
        vec![
            "Flagged for review: clear the flag first.".to_string(),
            "2 of 2 required function records not complete.".to_string()
        ]
    );
    assert_eq!(
        lib.completion_blockers(0, second).unwrap(),
        vec!["1 prerequisite function(s) not complete.".to_string()]
    );
    assert!(lib.completion_blockers(0, first).unwrap().is_empty());
}
