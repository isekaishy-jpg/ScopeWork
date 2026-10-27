//! Explicit branch removal and conservative, previewed organizational pruning.
use super::*;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Removal {
    pub schema: u32,
    pub binary: String,
    pub expect_revision: u64,
    #[serde(default)]
    pub ids: Vec<u64>,
    #[serde(default)]
    pub keep: Vec<u64>,
    pub name_prefix: Option<String>,
    #[serde(default)]
    pub context_only: bool,
    #[serde(default)]
    pub include_authored: bool,
    #[serde(default)]
    pub recursive: bool,
    /// The human's instruction, required to remove quarantined records.
    #[serde(default)]
    pub directed_by: Option<String>,
}

impl Library {
    pub fn remove_nodes(
        &mut self,
        p: usize,
        request: &Removal,
        prune: bool,
        expect: Option<&str>,
    ) -> Result<Value> {
        if expect.is_some() && !self.writable(p) {
            return self.write_project(p, |l, p| l.remove_nodes(p, request, prune, expect));
        }
        self.ensure_structural(p)?;
        let before = self.projects[p].clone();
        if request.schema != 1
            || request.binary != before.binary
            || request.expect_revision != before.revision
        {
            return Err(
                "Removal schema, exact binary or revision mismatch; reload and preview again."
                    .into(),
            );
        }
        if !prune
            && (request.ids.is_empty()
                || !request.keep.is_empty()
                || request.name_prefix.is_some()
                || request.context_only
                || request.include_authored
                || request.recursive)
        {
            return Err(
                "Explicit deletion requires ids; prune filters are only valid with node prune."
                    .into(),
            );
        }
        let root = self.dir(p);
        let old = catalog_index::Index::new(&before);
        for id in request.ids.iter().chain(&request.keep) {
            if *id == 0 || !old.nodes.contains_key(id) {
                return Err("Unknown node or App selected; App cannot be removed.".into());
            }
        }
        // Quarantined records are sealed: removing one, or a function out of a quarantined chunk,
        // needs the human's direction. Prune passes over them instead of failing.
        let directed_by = request
            .directed_by
            .as_deref()
            .map(str::trim)
            .filter(|d| !d.is_empty());
        let sealed = if directed_by.is_some() {
            HashSet::new()
        } else {
            self.quarantined(p)
        };
        let mut removed = HashSet::new();
        // Why each requested node (or, without `ids`, up to 50 scopes) was not selected, so an empty preview
        // explains itself. The last reason seen wins; a node removed in a later recursive pass is dropped.
        let mut excluded: BTreeMap<u64, Value> = BTreeMap::new();
        let report_exclusion = |excluded: &mut BTreeMap<u64, Value>, n: &Node, reason: Value| {
            if request.ids.contains(&n.id)
                || (request.ids.is_empty()
                    && matches!(n.kind, Kind::Architecture | Kind::Chunk)
                    && (excluded.len() < 50 || excluded.contains_key(&n.id)))
            {
                excluded.insert(n.id, reason);
            }
        };
        if prune {
            for id in &request.ids {
                if let Some(n) = before.nodes.iter().find(|n| n.id == *id)
                    && !matches!(n.kind, Kind::Architecture | Kind::Chunk)
                {
                    excluded.insert(n.id, json!({"reason":"kind","detail":"only architectures and chunks can be pruned"}));
                }
            }
            loop {
                let previous = removed.len();
                for n in before.nodes.iter().rev() {
                    if !matches!(n.kind, Kind::Architecture | Kind::Chunk)
                        || removed.contains(&n.id)
                        || (!request.ids.is_empty() && !request.ids.contains(&n.id))
                    {
                        continue;
                    }
                    if request.keep.contains(&n.id) {
                        report_exclusion(&mut excluded, n, json!({"reason":"keep"}));
                        continue;
                    }
                    if sealed.contains(&n.id) {
                        report_exclusion(
                            &mut excluded,
                            n,
                            json!({"reason":"quarantined","detail":"removing a quarantined record needs --directed-by"}),
                        );
                        continue;
                    }
                    if request.context_only && !n.context_only {
                        report_exclusion(
                            &mut excluded,
                            n,
                            json!({"reason":"context-filter","detail":"not a context-only branch"}),
                        );
                        continue;
                    }
                    if let Some(prefix) = request.name_prefix.as_ref()
                        && !n.name.starts_with(prefix)
                    {
                        report_exclusion(
                            &mut excluded,
                            n,
                            json!({"reason":"name-prefix","detail":format!("name does not start with {prefix:?}")}),
                        );
                        continue;
                    }
                    let kids = old.children.get(&n.id).cloned().unwrap_or_default();
                    let remaining: Vec<u64> = if request.recursive {
                        kids.iter()
                            .copied()
                            .filter(|id| !removed.contains(id))
                            .collect()
                    } else {
                        kids.clone()
                    };
                    if !remaining.is_empty() {
                        report_exclusion(
                            &mut excluded,
                            n,
                            json!({"reason":"has-children","children":remaining}),
                        );
                        continue;
                    }
                    let path = root.join(&old.paths[&n.id]);
                    contained(&root, &path)?;
                    let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
                    let (body, evidence) = parts(&text)?;
                    if !request.include_authored {
                        if !body.trim().is_empty() || !evidence.trim().is_empty() {
                            let which = match (body.trim().is_empty(), evidence.trim().is_empty()) {
                                (false, false) => "body and evidence",
                                (false, true) => "body",
                                _ => "evidence",
                            };
                            report_exclusion(
                                &mut excluded,
                                n,
                                json!({"reason":"authored-content","detail":which}),
                            );
                            continue;
                        }
                        let child_folders: HashSet<_> = kids
                            .iter()
                            .map(|id| root.join(&old.paths[id]).parent().unwrap().to_path_buf())
                            .collect();
                        let extra: Vec<String> = fs::read_dir(path.parent().unwrap())
                            .map_err(|e| e.to_string())?
                            .map(|entry| entry.map(|entry| entry.path()).map_err(|e| e.to_string()))
                            .collect::<Result<Vec<_>>>()?
                            .into_iter()
                            .filter(|entry| entry != &path && !child_folders.contains(entry))
                            .map(|entry| {
                                entry
                                    .file_name()
                                    .map(|f| f.to_string_lossy().into_owned())
                                    .unwrap_or_default()
                            })
                            .collect();
                        if !extra.is_empty() {
                            let shown: Vec<_> = extra.iter().take(5).collect();
                            report_exclusion(
                                &mut excluded,
                                n,
                                json!({"reason":"extra-files","files":shown,"count":extra.len()}),
                            );
                            continue;
                        }
                    }
                    removed.insert(n.id);
                    excluded.remove(&n.id);
                }
                if !request.recursive || previous == removed.len() {
                    break;
                }
            }
        } else {
            let mut queue = request.ids.clone();
            while let Some(id) = queue.pop() {
                if removed.insert(id)
                    && let Some(kids) = old.children.get(&id)
                {
                    queue.extend(kids);
                }
            }
            let mut ordered: Vec<u64> = removed.iter().copied().collect();
            ordered.sort_unstable();
            self.check_seal(p, ordered, directed_by, "Removing")?;
        }
        // A surviving record that loses a prerequisite has its own prerequisites changed, which
        // the seal covers like any other change to them.
        let losing: std::collections::BTreeSet<u64> = before
            .dependencies
            .iter()
            .filter(|e| removed.contains(&e.prerequisite) && !removed.contains(&e.dependent))
            .map(|e| e.dependent)
            .collect();
        self.check_seal(p, losing, directed_by, "Removing a prerequisite of")?;
        let archive = format!(
            "tools/workspaces/deleted-nodes/revision-{}",
            before.revision + 1
        );
        let mut after = before.clone();
        after.nodes.retain(|n| !removed.contains(&n.id));
        let removed_dependencies: Vec<_> = before
            .dependencies
            .iter()
            .filter(|e| removed.contains(&e.dependent) || removed.contains(&e.prerequisite))
            .cloned()
            .collect();
        after
            .dependencies
            .retain(|e| !removed.contains(&e.dependent) && !removed.contains(&e.prerequisite));
        let new = catalog_index::Index::new(&after);
        let mut moves = vec![];
        let mut rewrites = vec![];
        let mut candidates = vec![];
        for n in &before.nodes {
            if !removed.contains(&n.id) {
                continue;
            }
            let from = old.paths[&n.id].trim_end_matches("/research.md").to_owned();
            let to = format!("{archive}/{}", n.id);
            contained(&root, &root.join(&from))?;
            contained(&root, &root.join(&to))?;
            if root.join(&to).exists() {
                return Err("Removal archive destination exists; refusing overwrite.".into());
            }
            candidates.push(json!({"id":n.id,"name":n.name,"kind":n.kind,"report":old.paths[&n.id],"archive":format!("{to}/research.md")}));
            rewrites.push((from.clone(), to.clone()));
            moves.push(Move {
                id: n.id,
                old: from,
                new: to,
            });
        }
        let rewriter = layout::Rewriter::new(&rewrites, &root);
        let mut edits = vec![];
        let mut changed = removed.clone();
        if !removed.is_empty() {
            // Removed reports move to the archive; other reports change only if they mention a
            // removed folder or are a removed record's parent (its source list changes).
            let folders: HashSet<String> = moves
                .iter()
                .filter_map(|m| m.old.rsplit('/').next())
                .map(str::to_uppercase)
                .collect();
            let parents: HashSet<u64> = removed
                .iter()
                .filter_map(|id| old.nodes[id].parent)
                .collect();
            let others: Vec<&str> = before
                .nodes
                .iter()
                .filter(|n| !removed.contains(&n.id) && !parents.contains(&n.id))
                .map(|n| old.paths[&n.id].as_str())
                .collect();
            let mentioning = report_hashes::mentioning(&root, &others, &folders);
            for n in &before.nodes {
                let path = &old.paths[&n.id];
                if !removed.contains(&n.id)
                    && !parents.contains(&n.id)
                    && !mentioning.contains(path)
                {
                    continue;
                }
                contained(&root, &root.join(path))?;
                let original = fs::read_to_string(root.join(path)).map_err(|e| e.to_string())?;
                let destination = if removed.contains(&n.id) {
                    format!("{archive}/{}/research.md", n.id)
                } else {
                    path.clone()
                };
                let output = if removed.contains(&n.id) {
                    rewriter.rewrite(&original, path, &destination)
                } else {
                    let (body, evidence) = parts(&original)?;
                    new.render(
                        &after,
                        n.id,
                        &rewriter.rewrite(body, path, path),
                        &rewriter.rewrite(evidence, path, path),
                    )
                };
                if output != original || destination != *path {
                    changed.insert(n.id);
                    edits.push(Edit {
                        id: n.id,
                        old: Some(path.clone()),
                        new: destination,
                        before: Some(original),
                        after: output,
                    });
                }
            }
        }
        drop(new);
        // Changed (edited) surviving records need review, and so do the complete records the
        // change reaches, including the old parents of removed records.
        let edited: HashSet<u64> = changed
            .iter()
            .copied()
            .filter(|id| !removed.contains(id))
            .collect();
        for n in &mut after.nodes {
            if edited.contains(&n.id) {
                reopen::mark_changed(n, None);
            }
        }
        let old_parents: HashMap<u64, u64> = removed
            .iter()
            .filter_map(|id| Some((*id, old.nodes[id].parent?)))
            .collect();
        let consumers = reopen::consumers_of(&before.dependencies);
        let quarantined =
            crate::research::quarantine::effective(&after, &self.quarantines_for_write(p)?);
        let reached =
            reopen::reopen_affected(&mut after, &changed, &old_parents, &consumers, &quarantined);
        let invalid: HashSet<u64> = edited.iter().copied().chain(reached).collect();
        let mut dependencies: Vec<_> = removed_dependencies
            .iter()
            .flat_map(|e| [e.dependent, e.prerequisite])
            .filter(|id| !removed.contains(id))
            .collect();
        dependencies.sort_unstable();
        dependencies.dedup();
        let mut sidecars = vec![];
        let flag_path = root.join(super::REVIEW_FLAGS);
        if !removed.is_empty() && flag_path.is_file() {
            let original = fs::read_to_string(&flag_path).map_err(|e| e.to_string())?;
            let mut flags = super::read_flags(&root)?;
            let count = flags.len();
            flags.retain(|id, _| !removed.contains(id));
            if flags.len() != count {
                sidecars.push(Sidecar {
                    path: super::REVIEW_FLAGS.into(),
                    before: original,
                    after: serde_json::to_string_pretty(&json!({"schema":1,"flags":flags}))
                        .map_err(|e| e.to_string())?,
                });
            }
        }
        let proofs_path = root.join(crate::research::proofs::PROOFS);
        if !removed.is_empty() && proofs_path.is_file() {
            let original = fs::read_to_string(&proofs_path).map_err(|e| e.to_string())?;
            if let Some(updated) = crate::research::proofs::without_removed(&original, &removed)? {
                sidecars.push(Sidecar {
                    path: crate::research::proofs::PROOFS.into(),
                    before: original,
                    after: updated,
                });
            }
        }
        let quarantine_path = root.join(crate::research::quarantine::QUARANTINE);
        if !removed.is_empty() && quarantine_path.is_file() {
            let original = fs::read_to_string(&quarantine_path).map_err(|e| e.to_string())?;
            if let Some(updated) =
                crate::research::quarantine::without_removed(&original, &removed)?
            {
                sidecars.push(Sidecar {
                    path: crate::research::quarantine::QUARANTINE.into(),
                    before: original,
                    after: updated,
                });
            }
        }
        if !removed.is_empty() {
            after.revision += 1;
            after.structural_epoch += 1;
            // Completed records this removal reopens wait for review like those a move reopens;
            // claims and queue entries of removed records go with them.
            let reopened: Vec<u64> = before
                .nodes
                .iter()
                .filter(|n| {
                    n.status == Status::Complete
                        && invalid.contains(&n.id)
                        && !removed.contains(&n.id)
                })
                .map(|n| n.id)
                .collect();
            let queue_path = root.join(coordination::REVIEW_QUEUE);
            if !reopened.is_empty() || queue_path.is_file() {
                let original =
                    coordination::current_or_empty(&root, coordination::REVIEW_QUEUE, "entries")?;
                let updated = coordination::queue_reopened(
                    &original,
                    &after,
                    &reopened,
                    "Reopened by removing records it summarised or depended on",
                )?;
                if updated != original {
                    sidecars.push(Sidecar {
                        path: coordination::REVIEW_QUEUE.into(),
                        before: original,
                        after: updated,
                    });
                }
            }
            if root.join(coordination::CLAIMS).is_file() {
                let original = fs::read_to_string(root.join(coordination::CLAIMS))
                    .map_err(|e| e.to_string())?;
                let updated = coordination::without_nodes(&original, "claims", &removed)?;
                if updated != original {
                    sidecars.push(Sidecar {
                        path: coordination::CLAIMS.into(),
                        before: original,
                        after: updated,
                    });
                }
            }
        }
        let j = Journal {
            before,
            after,
            edits,
            ledger: !removed.is_empty(),
            repin_ledger: false,
            directed_by: directed_by.map(str::to_string),
            repin_ids: vec![],
            moves,
            dependencies,
            sidecars,
        };
        let bytes = journal_history::pending_bytes(&j)?;
        let token = hash(std::str::from_utf8(&bytes).map_err(|e| e.to_string())?);
        let reopened: Vec<_> = j
            .after
            .nodes
            .iter()
            .filter(|n| invalid.contains(&n.id))
            .map(|n| n.id)
            .collect();
        if let Some(expect) = expect {
            if expect != token {
                return Err(
                    "Removal preview changed; review a new preview before applying.".into(),
                );
            }
            if !removed.is_empty() {
                if j.sidecars
                    .iter()
                    .any(|s| s.path == coordination::REVIEW_QUEUE)
                {
                    coordination::ensure(&root, coordination::REVIEW_QUEUE, "entries")?;
                }
                commit(&root, &mut self.projects[p], &j)?;
            }
        }
        Ok(
            json!({"project":self.projects[p].id,"binary":self.projects[p].binary,"revision":self.projects[p].revision,"applied":expect.is_some(),"token":token,"candidates":candidates,"excluded":excluded.iter().map(|(id, why)| { let mut v = why.clone(); v["id"] = json!(id); v }).collect::<Vec<_>>(),"removed_dependencies":removed_dependencies,"requires_review":reopened,"archive":archive}),
        )
    }
}
