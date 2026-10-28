//! Promoting a project to the store (docs/design-storage-promotion.md §5): the manual step,
//! run by the user, after which the store is authoritative and the files are exports.
use super::*;
use serde_json::{Value, json};

/// Where promotion keeps the catalog and ledger as they were, for a return to files by hand.
const KEPT: &str = "store/promotion";

/// Promotes project `id` once its store holds the catalog exactly, verifies end to end and
/// reports no conflicts with the ledger.
pub fn promote(root: &Path, id: &str, expected_revision: u64) -> Result<Value> {
    // The store catches up first, then is checked whole.
    store::sync(root, id, false)?;
    let verified = store::verify(root, id)?;
    if verified["ok"] != true {
        return Err(format!(
            "store_unverified: research store verify reports problems; promotion needs a store that checks out: {}",
            verified["problems"]
        ));
    }
    let dir = root.join(id);
    if dir.join(ledger::LEDGER).is_file() {
        let found = store::conflicts(root, id)?;
        if found["conflicts"] != 0 {
            return Err(format!(
                "conflicts: the catalog and the annotation ledger disagree in {} places ({}); resolve them first (research store conflicts {id})",
                found["conflicts"], found["by_kind"]
            ));
        }
    }
    let mut lib = Library::open_project(root.to_path_buf(), Some(id))?;
    lib.promote(0, expected_revision)
}

impl Library {
    fn promote(&mut self, p: usize, expected_revision: u64) -> Result<Value> {
        let project = &self.projects[p];
        if project.revision != expected_revision {
            return Err("Project changed. Reload and review it before promoting.".into());
        }
        if promoted(project) {
            return Err("This project is already promoted to the store.".into());
        }
        if project.active_import.is_some() {
            return Err(
                "An import is in progress; promote once it is published or cancelled.".into(),
            );
        }
        let dir = self.dir(p);
        // Under this library's exclusive lock: the store's head must be this very catalog.
        let bytes = catalog_bytes(project)?;
        if fs::read(dir.join("project.json")).map_err(|e| e.to_string())? != bytes
            || !store::holds(&dir, project.revision, &sha256_hex(&bytes))?
        {
            return Err(
                "store_behind: the store does not hold the current catalog; run research store sync and promote again"
                    .into(),
            );
        }
        // The files as they were, kept before anything changes.
        let kept = dir.join(KEPT);
        fs::create_dir_all(&kept).map_err(|e| e.to_string())?;
        atomic(&kept.join("project.json"), &bytes)?;
        let ledger = dir.join(ledger::LEDGER);
        if ledger.is_file() {
            atomic(
                &kept.join("annotations.json"),
                &fs::read(&ledger).map_err(|e| e.to_string())?,
            )?;
        }
        let before = self.projects[p].clone();
        let after = &mut self.projects[p];
        after.layout_version = STORE_FORMAT;
        after.revision += 1;
        self.commit_catalog(p, before)?;
        let project = &self.projects[p];
        Ok(json!({
            "project": project.id,
            "promoted": true,
            "revision": project.revision,
            "layout_version": project.layout_version,
            "kept": kept.display().to_string(),
        }))
    }
}
