//! A change reopens only the complete records it reaches; incomplete ancestors keep their
//! revision, and every stale token is still refused.
use research_dashboard::research::{Library, Status};
use std::{fs, path::PathBuf};

struct Home(PathBuf);
impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn revision(lib: &Library, id: u64) -> u64 {
    lib.projects[0]
        .nodes
        .iter()
        .find(|n| n.id == id)
        .unwrap()
        .revision
}
fn status(lib: &Library, id: u64) -> Status {
    lib.projects[0]
        .nodes
        .iter()
        .find(|n| n.id == id)
        .unwrap()
        .status
}
fn save(lib: &mut Library, id: u64, body: &str) {
    let token = lib.report_token(0, id).unwrap();
    lib.save_report(0, id, body, "Synthetic evidence", false, &token)
        .unwrap();
}
fn complete(lib: &mut Library, id: u64) {
    let token = lib.completion_token(0, id).unwrap();
    let (body, evidence) = lib.read(0, id).unwrap();
    lib.save_report(0, id, &body, &evidence, true, &token)
        .unwrap();
}

/// App 0 → architecture 1 → chunk 2 → functions 3, 4 and 5; 5 tags 4 as a prerequisite.
fn fixture() -> (Home, Library) {
    let home = Home(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-workspaces")
            .join(format!("narrow-{}", uuid::Uuid::new_v4())),
    );
    let root = home.0.join("projects");
    fs::create_dir_all(&root).unwrap();
    let mut lib = Library::open_project(root, Some("fixture")).unwrap();
    lib.create("fixture", "Fixture", "synthetic").unwrap();
    let a = lib.add(0, 0, "Architecture", "").unwrap();
    let c = lib.add(0, a, "Chunk", "").unwrap();
    for rva in ["1000", "2000", "3000"] {
        lib.add(0, c, &format!("F{rva}"), rva).unwrap();
    }
    lib.set_dependencies(0, 5, "2000").unwrap();
    (home, lib)
}

#[test]
fn work_below_incomplete_ancestors_leaves_their_revisions_alone() {
    let (_home, mut lib) = fixture();
    let before: Vec<u64> = [0, 1, 2].iter().map(|id| revision(&lib, *id)).collect();
    save(&mut lib, 3, "Research");
    complete(&mut lib, 3);
    let after: Vec<u64> = [0, 1, 2].iter().map(|id| revision(&lib, *id)).collect();
    assert_eq!(
        before, after,
        "App, architecture and chunk were not complete"
    );
}

#[test]
fn complete_ancestors_and_consumers_reopen_and_stale_tokens_fail() {
    let (_home, mut lib) = fixture();
    for id in [3, 4, 5] {
        save(&mut lib, id, "Research");
        complete(&mut lib, id);
    }
    save(&mut lib, 2, "Chunk synthesis");
    complete(&mut lib, 2);
    assert_eq!(status(&lib, 2), Status::Complete);
    let stale_draft = lib.report_token(0, 2).unwrap();
    let stale_completion = lib.completion_token(0, 2).unwrap();
    let architecture = revision(&lib, 1);
    // Reworking prerequisite 4 reopens it, its consumer 5 and the complete chunk 2.
    save(&mut lib, 4, "Revised research");
    assert_eq!(status(&lib, 5), Status::InProgress, "consumer reopened");
    assert_eq!(
        status(&lib, 2),
        Status::InProgress,
        "complete ancestor reopened"
    );
    assert_eq!(status(&lib, 3), Status::Complete, "sibling untouched");
    assert_eq!(
        revision(&lib, 1),
        architecture,
        "incomplete architecture untouched"
    );
    // The chunk's text did not change, so its draft token still holds; completion does not.
    assert_eq!(lib.report_token(0, 2).unwrap(), stale_draft);
    let (body, evidence) = lib.read(0, 2).unwrap();
    assert!(
        lib.save_report(0, 2, &body, &evidence, true, &stale_completion)
            .is_err(),
        "a stale completion token is refused"
    );
    let stale_structural = lib.structural_token(0, 5).unwrap();
    lib.set_dependencies(0, 5, "").unwrap();
    assert!(
        lib.check_edit_token(0, 5, &stale_structural, "structural")
            .is_err(),
        "a stale structural token is refused"
    );
}
