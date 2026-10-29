//! Promoting a project to the store (docs/design-storage-promotion.md §5): the manual step,
//! run by the user, after which the store is authoritative and the files are exports.
use super::*;
use serde_json::{Value, json};
use std::collections::BTreeMap;

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
        // The ledger must be the one the store recorded last, or the promotion's pin conversion
        // would be recorded together with a tool's change nobody has seen.
        let ledger = dir.join(ledger::LEDGER);
        if ledger.is_file() {
            let sha = sha256_hex(&fs::read(&ledger).map_err(|e| e.to_string())?);
            if store::ledger_recorded(&dir)?.as_deref() != Some(sha.as_str()) {
                return Err(
                    "ledger_changed: the annotation ledger changed since the store last recorded it; run research store sync and promote again"
                        .into(),
                );
            }
        }
        // The files as they were, kept before anything changes.
        let kept = dir.join(KEPT);
        fs::create_dir_all(&kept).map_err(|e| e.to_string())?;
        atomic(&kept.join("project.json"), &bytes)?;
        if ledger.is_file() {
            atomic(
                &kept.join("annotations.json"),
                &fs::read(&ledger).map_err(|e| e.to_string())?,
            )?;
        }
        // Acceptance moves to the research body and evidence. Each report's whole-file hash
        // now maps to its body hash; an acceptance, prerequisite stamp or ledger pin converts
        // only when it equals a current whole-file hash, so anything stale stays stale.
        let before = self.projects[p].clone();
        let index = catalog_index::Index::new(&before);
        let mut mapping: BTreeMap<String, String> = BTreeMap::new();
        let mut reports = vec![];
        for (id, path) in &index.paths {
            if let Ok(text) = fs::read_to_string(dir.join(path)) {
                let (whole, body) = (hash(&text), hash(acceptance_text(&text)));
                reports.push(
                    json!({"id": id, "path": path, "whole_sha256": whole, "body_sha256": body}),
                );
                mapping.insert(whole, body);
            }
        }
        drop(index);
        let mut after = before.clone();
        let (mut converted, mut stale) = (0usize, 0usize);
        for n in &mut after.nodes {
            if let Some(accepted) = &n.accepted_hash {
                match mapping.get(accepted) {
                    Some(body) => {
                        n.accepted_hash = Some(body.clone());
                        converted += 1;
                    }
                    None => stale += 1,
                }
            }
            for stamp in n.accepted_prerequisites.values_mut() {
                if let Some((revision, sha)) = stamp.split_once(':')
                    && let Some(body) = mapping.get(sha)
                {
                    *stamp = format!("{revision}:{body}");
                }
            }
        }
        after.layout_version = STORE_FORMAT;
        after.revision += 1;
        // Every old and new hash, kept beside the copies.
        reports.sort_by_key(|r| r["id"].as_u64());
        atomic(
            &kept.join("acceptance.json"),
            &serde_json::to_vec_pretty(&json!({"revision": after.revision, "reports": reports}))
                .map_err(|e| e.to_string())?,
        )?;
        let repin_pairs = mapping.into_iter().filter(|(a, b)| a != b).collect();
        self.commit_metadata_with(
            p,
            before,
            after,
            vec![],
            catalog_batch::MetadataCommit {
                repin_pairs,
                ..Default::default()
            },
        )?;
        let project = &self.projects[p];
        Ok(json!({
            "project": project.id,
            "promoted": true,
            "revision": project.revision,
            "layout_version": project.layout_version,
            "acceptances_converted": converted,
            "acceptances_stale": stale,
            "kept": kept.display().to_string(),
        }))
    }
}
