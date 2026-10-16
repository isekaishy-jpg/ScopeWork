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
            for n in &before.nodes {
                let path = &old.paths[&n.id];
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
        let mut queue: Vec<_> = changed.iter().copied().collect();
        let mut invalid = HashSet::new();
        let mut dependents: HashMap<u64, Vec<u64>> = HashMap::new();
        for e in &before.dependencies {
            dependents
                .entry(e.prerequisite)
                .or_default()
                .push(e.dependent);
        }
        while let Some(id) = queue.pop() {
            if !invalid.insert(id) {
                continue;
            }
            if let Some(parent) = old.nodes[&id].parent {
                queue.push(parent);
            }
            if let Some(consumers) = dependents.get(&id) {
                queue.extend(consumers);
            }
        }
        drop(new);
        for n in &mut after.nodes {
            if invalid.contains(&n.id) {
                n.revision += 1;
                n.accepted_hash = None;
                n.accepted_prerequisites.clear();
                if n.status == Status::Complete {
                    n.status = Status::InProgress;
                }
            }
        }
        let mut dependencies: Vec<_> = removed_dependencies
            .iter()
            .flat_map(|e| [e.dependent, e.prerequisite])
            .filter(|id| !removed.contains(id))
            .collect();
        dependencies.sort_unstable();
        dependencies.dedup();
        let annotation_path = root.join("tools/profiles/annotations.json");
        let mut sidecars = vec![];
        if !removed.is_empty() && annotation_path.is_file() {
            contained(&root, &annotation_path)?;
            let original = fs::read_to_string(&annotation_path).map_err(|e| e.to_string())?;
            let mut ledger: Value = serde_json::from_str(&original).map_err(|e| e.to_string())?;
            if ledger["schema"] != 1 {
                return Err(
                    "Unknown annotation schema; removal cannot safely update provenance.".into(),
                );
            }
            let mut modified = false;
            let archived_reports: HashMap<_, _> = removed
                .iter()
                .map(|id| (old.paths[id].as_str(), *id))
                .collect();
            if let Some(rows) = ledger["annotations"].as_array_mut() {
                for row in rows {
                    if let Some(id) = row["report"]
                        .as_str()
                        .and_then(|path| archived_reports.get(path).copied())
                    {
                        row["report"] = format!("{archive}/{id}/research.md").into();
                        row["catalog_move"] = "Source report archived after explicit node removal; original review hash retained. Re-review before applying.".into();
                        modified = true;
                    }
                }
            }
            if modified {
                sidecars.push(Sidecar {
                    path: "tools/profiles/annotations.json".into(),
                    before: original,
                    after: serde_json::to_string_pretty(&ledger).map_err(|e| e.to_string())?,
                });
            }
        }
        if !removed.is_empty() {
            after.revision += 1;
            after.structural_epoch += 1;
        }
        let j = Journal {
            before,
            after,
            edits,
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
