//! Explicit branch removal and conservative, previewed organizational pruning.
use super::*;
use serde_json::{Value, json};

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
        let mut removed = HashSet::new();
        if prune {
            loop {
                let previous = removed.len();
                for n in before.nodes.iter().rev() {
                    if !matches!(n.kind, Kind::Architecture | Kind::Chunk)
                        || removed.contains(&n.id)
                        || request.keep.contains(&n.id)
                        || (!request.ids.is_empty() && !request.ids.contains(&n.id))
                        || (request.context_only && !n.context_only)
                        || request
                            .name_prefix
                            .as_ref()
                            .is_some_and(|prefix| !n.name.starts_with(prefix))
                    {
                        continue;
                    }
                    let kids = old.children.get(&n.id).cloned().unwrap_or_default();
                    let empty = if request.recursive {
                        kids.iter().all(|id| removed.contains(id))
                    } else {
                        kids.is_empty()
                    };
                    if !empty {
                        continue;
                    }
                    let path = root.join(&old.paths[&n.id]);
                    contained(&root, &path)?;
                    let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
                    let (body, evidence) = parts(&text)?;
                    if !request.include_authored {
                        if !body.trim().is_empty() || !evidence.trim().is_empty() {
                            continue;
                        }
                        let child_folders: HashSet<_> = kids
                            .iter()
                            .map(|id| root.join(&old.paths[id]).parent().unwrap().to_path_buf())
                            .collect();
                        let extra = fs::read_dir(path.parent().unwrap())
                            .map_err(|e| e.to_string())?
                            .map(|entry| entry.map(|entry| entry.path()).map_err(|e| e.to_string()))
                            .collect::<Result<Vec<_>>>()?
                            .iter()
                            .any(|entry| entry != &path && !child_folders.contains(entry));
                        if extra {
                            continue;
                        }
                    }
                    removed.insert(n.id);
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
        }
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
        let reached = reopen::reopen_affected(&mut after, &changed, &old_parents, &consumers);
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
                let folder = prepare_pending(&root)?;
                atomic(&folder.join("journal.json"), &bytes)?;
                resume(&root, &mut self.projects[p])?;
            }
        }
        Ok(
            json!({"project":self.projects[p].id,"binary":self.projects[p].binary,"revision":self.projects[p].revision,"applied":expect.is_some(),"token":token,"candidates":candidates,"removed_dependencies":removed_dependencies,"requires_review":reopened,"archive":archive}),
        )
    }
}
