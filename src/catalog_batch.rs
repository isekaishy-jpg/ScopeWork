//! Revision-checked catalog changes with a durable, resumable journal.
use super::*;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Batch {
    pub schema: u32,
    pub binary: String,
    pub expect_revision: u64,
    #[serde(default)]
    pub repair_legacy_links: bool,
    #[serde(default)]
    pub create: Vec<Create>,
    #[serde(default)]
    pub update: Vec<Update>,
    #[serde(default)]
    pub reorder: Vec<Order>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Create {
    pub parent: u64,
    pub name: String,
    pub rva: Option<u32>,
    #[serde(default)]
    pub context_only: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    pub id: u64,
    pub parent: Option<u64>,
    pub name: Option<String>,
    pub context_only: Option<bool>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Order {
    pub parent: u64,
    pub children: Vec<u64>,
}
#[derive(Serialize, Deserialize)]
struct Edit {
    id: u64,
    old: Option<String>,
    new: String,
    before: Option<String>,
    after: String,
}
#[derive(Serialize, Deserialize)]
struct Move {
    id: u64,
    old: String,
    new: String,
}
#[derive(Serialize, Deserialize)]
struct Sidecar {
    path: String,
    before: String,
    after: String,
}
#[derive(Serialize, Deserialize)]
struct Journal {
    before: Project,
    after: Project,
    edits: Vec<Edit>,
    moves: Vec<Move>,
    dependencies: Vec<u64>,
    sidecars: Vec<Sidecar>,
}

fn parts(text: &str) -> Result<(&str, &str)> {
    text.split_once("\n<!-- research-content -->\n")
        .ok_or("Missing report content marker")?
        .1
        .split_once("\n<!-- research-evidence -->\n")
        .ok_or_else(|| "Missing evidence marker".into())
}
fn same(a: &Project, b: &Project) -> bool {
    serde_json::to_value(a).unwrap() == serde_json::to_value(b).unwrap()
}
fn pending(root: &Path) -> PathBuf {
    root.join("tools/workspaces/catalog-transactions/pending")
}
fn staged(root: &Path, id: u64) -> PathBuf {
    pending(root).join("moves").join(id.to_string())
}
fn contained(root: &Path, path: &Path) -> Result<()> {
    layout::inside(root, path)
}

pub(super) fn resume(root: &Path, project: &mut Project) -> Result<()> {
    let folder = pending(root);
    let path = folder.join("journal.json");
    if !path.exists() {
        return Ok(());
    }
    contained(root, &path)?;
    let j: Journal = serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if !same(project, &j.before) && !same(project, &j.after) {
        return Err("Catalog changed during batch transaction. Preserve its journal and resolve the conflict.".into());
    }
    // All existing reports must still match a recorded input or output before resuming.
    for edit in &j.edits {
        let candidates = [
            edit.old.as_ref().map(|s| root.join(s)),
            Some(staged(root, edit.id).join("research.md")),
            Some(root.join(&edit.new)),
        ];
        for candidate in candidates.into_iter().flatten() {
            contained(root, &candidate)?;
            if candidate.is_file() {
                let text = fs::read_to_string(&candidate).map_err(|e| e.to_string())?;
                if Some(&text) != edit.before.as_ref() && text != edit.after {
                    return Err(format!(
                        "Report changed during batch: {}",
                        candidate.display()
                    ));
                }
            }
        }
        if edit.before.is_some()
            && !edit.old.as_ref().is_some_and(|s| root.join(s).is_file())
            && !staged(root, edit.id).join("research.md").is_file()
            && !root.join(&edit.new).is_file()
        {
            return Err(
                "Batch source report is missing; restore it from the journal before continuing."
                    .into(),
            );
        }
    }
    for file in &j.sidecars {
        let path = root.join(&file.path);
        contained(root, &path)?;
        let current = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        if current != file.before && current != file.after {
            return Err(format!("Sidecar changed during batch: {}", file.path));
        }
    }
    // Extract deepest folders first so parents and children can move independently.
    let mut moves: Vec<_> = j.moves.iter().collect();
    moves.sort_by_key(|m| std::cmp::Reverse(m.old.matches('/').count()));
    for m in &moves {
        let from = root.join(&m.old);
        let stage = staged(root, m.id);
        let to = root.join(&m.new);
        contained(root, &from)?;
        contained(root, &stage)?;
        contained(root, &to)?;
        if from.exists() {
            if stage.exists() || to.exists() {
                return Err(format!(
                    "Batch move destination already exists for node {}",
                    m.id
                ));
            }
            fs::create_dir_all(stage.parent().unwrap()).map_err(|e| e.to_string())?;
            fs::rename(from, stage).map_err(|e| e.to_string())?;
        }
    }
    moves.sort_by_key(|m| m.new.matches('/').count());
    for m in moves {
        let stage = staged(root, m.id);
        let to = root.join(&m.new);
        if stage.exists() {
            if to.exists() {
                return Err(format!("Refusing to overwrite {}", to.display()));
            }
            fs::create_dir_all(to.parent().unwrap()).map_err(|e| e.to_string())?;
            fs::rename(stage, to).map_err(|e| e.to_string())?;
        }
    }
    let functions: HashSet<_> = j
        .after
        .nodes
        .iter()
        .filter(|n| n.kind == Kind::Function)
        .map(|n| n.id)
        .collect();
    for edit in &j.edits {
        let path = root.join(&edit.new);
        fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        if functions.contains(&edit.id) {
            fs::create_dir_all(path.parent().unwrap().join("data")).map_err(|e| e.to_string())?;
        }
        if fs::read_to_string(&path).ok().as_ref() != Some(&edit.after) {
            atomic(&path, edit.after.as_bytes())?;
        }
    }
    for file in &j.sidecars {
        atomic(&root.join(&file.path), file.after.as_bytes())?;
    }
    let temporary = Library {
        root: root.parent().ok_or("Missing library directory")?.into(),
        projects: vec![j.after.clone()],
        _lock: None,
    };
    temporary.sync_dependency_subset(0, Some(&j.dependencies.iter().copied().collect()))?;
    atomic(
        &root.join("project.json"),
        &serde_json::to_vec_pretty(&j.after).map_err(|e| e.to_string())?,
    )?;
    *project = j.after;
    let receipt = folder.with_file_name(format!("revision-{}", project.revision));
    if receipt.exists() {
        return Err("Batch receipt already exists; preserve the pending journal.".into());
    }
    fs::rename(folder, receipt).map_err(|e| e.to_string())?;
    Ok(())
}

impl Library {
    pub fn batch(&mut self, p: usize, batch: &Batch) -> Result<Vec<u64>> {
        if self._lock.is_none() {
            return self.write_project(p, |l, p| l.batch(p, batch));
        }
        let before = self.projects[p].clone();
        if batch.schema != 1
            || batch.expect_revision != before.revision
            || batch.binary != before.binary
        {
            return Err("Batch schema, exact binary identity or project revision does not match. Reload and review the batch.".into());
        }
        let mut after = before.clone();
        let mut positions: HashMap<_, _> = after
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id, i))
            .collect();
        let mut rvas: HashSet<_> = after.nodes.iter().filter_map(|n| n.rva).collect();
        let mut changed = HashSet::new();
        let mut created = vec![];
        for c in &batch.create {
            let parent = *positions
                .get(&c.parent)
                .ok_or("Unknown batch parent; create parents before children")?;
            let kind = after.nodes[parent]
                .kind
                .child()
                .ok_or("Functions cannot own children")?;
            if c.name.trim().is_empty() || (kind == Kind::Function) != c.rva.is_some() {
                return Err("Batch node requires a name and an RVA only for functions".into());
            }
            if let Some(rva) = c.rva {
                if before.binary.trim().is_empty() || !rvas.insert(rva) {
                    return Err("Empty binary identity or duplicate function RVA in batch".into());
                }
            }
            let id = after.next_id;
            after.next_id = id.checked_add(1).ok_or("Node ID exhausted")?;
            positions.insert(id, after.nodes.len());
            after.nodes.push(Node {
                id,
                parent: Some(c.parent),
                kind,
                name: c.name.trim().into(),
                rva: c.rva,
                context_only: c.context_only,
                status: Status::NotStarted,
                revision: 0,
                accepted_hash: None,
                accepted_prerequisites: Default::default(),
            });
            created.push(id);
            changed.insert(id);
            changed.insert(c.parent);
        }
        let mut updates = HashSet::new();
        for u in &batch.update {
            if !updates.insert(u.id) {
                return Err("Duplicate node update in batch".into());
            }
            let i = *positions.get(&u.id).ok_or("Unknown update node")?;
            if let Some(parent) = u.parent {
                let p = *positions.get(&parent).ok_or("Unknown new parent")?;
                if after.nodes[p].kind.child() != Some(after.nodes[i].kind) {
                    return Err(
                        "Move must preserve App / Architecture / Chunk / Function hierarchy".into(),
                    );
                }
                if let Some(old) = after.nodes[i].parent {
                    changed.insert(old);
                }
                after.nodes[i].parent = Some(parent);
                changed.insert(parent);
            }
            if let Some(name) = &u.name {
                if name.trim().is_empty() {
                    return Err("Node name cannot be empty".into());
                }
                after.nodes[i].name = name.trim().into();
            }
            if let Some(context) = u.context_only {
                if u.id == 0 && context {
                    return Err("App cannot be context-only".into());
                }
                after.nodes[i].context_only = context;
            }
            changed.insert(u.id);
            if let Some(parent) = after.nodes[i].parent {
                changed.insert(parent);
            }
        }
        let mut ordered = HashSet::new();
        for order in &batch.reorder {
            if !positions.contains_key(&order.parent) || !ordered.insert(order.parent) {
                return Err("Unknown or duplicate reorder parent".into());
            }
            let slots: Vec<_> = after
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, n)| n.parent == Some(order.parent))
                .map(|(i, _)| i)
                .collect();
            let expected: HashSet<_> = slots.iter().map(|i| after.nodes[*i].id).collect();
            if order.children.len() != expected.len()
                || order.children.iter().copied().collect::<HashSet<_>>() != expected
            {
                return Err("Reorder must list every child exactly once".into());
            }
            let nodes: Vec<_> = order
                .children
                .iter()
                .map(|id| after.nodes[positions[id]].clone())
                .collect();
            for (slot, node) in slots.into_iter().zip(nodes) {
                after.nodes[slot] = node;
            }
            positions = after
                .nodes
                .iter()
                .enumerate()
                .map(|(i, n)| (n.id, i))
                .collect();
            changed.insert(order.parent);
        }
        if changed.is_empty() && !batch.repair_legacy_links {
            return Ok(created);
        }
        let old = catalog_index::Index::new(&before);
        let new = catalog_index::Index::new(&after);
        let root = self.dir(p);
        let mut moves = vec![];
        let mut rewrites = vec![];
        for n in &before.nodes {
            if old.paths[&n.id] != new.paths[&n.id] {
                let from = old.paths[&n.id]
                    .trim_end_matches("/research.md")
                    .to_string();
                let to = new.paths[&n.id]
                    .trim_end_matches("/research.md")
                    .to_string();
                if root.join(&to).exists() {
                    return Err(format!("Move destination exists: {to}"));
                }
                moves.push(Move {
                    id: n.id,
                    old: from.clone(),
                    new: to.clone(),
                });
                rewrites.push((from, to));
                changed.insert(n.id);
            }
        }
        if batch.repair_legacy_links {
            for n in &after.nodes {
                let from = match n.kind {
                    Kind::Chunk => Some(format!("chunks/{:06}", n.id)),
                    Kind::Function => Some(format!("functions/{:08X}", n.rva.unwrap())),
                    _ => None,
                };
                if let Some(from) = from {
                    rewrites.push((
                        from,
                        new.paths[&n.id].trim_end_matches("/research.md").into(),
                    ));
                }
            }
        }
        let rewriter = layout::Rewriter::new(&rewrites, &root);
        rewrites.sort_by_key(|(from, _)| std::cmp::Reverse(from.len()));
        let mut edits = vec![];
        for n in &after.nodes {
            let original = if let Some(path) = old.paths.get(&n.id) {
                if changed.contains(&n.id) || !rewrites.is_empty() {
                    Some(fs::read_to_string(root.join(path)).map_err(|e| e.to_string())?)
                } else {
                    continue;
                }
            } else {
                if root.join(&new.paths[&n.id]).exists() {
                    return Err("New canonical report already exists".into());
                }
                None
            };
            let (body, evidence) = original
                .as_deref()
                .map(parts)
                .transpose()?
                .unwrap_or(("", ""));
            let old_path = old.paths.get(&n.id).unwrap_or(&new.paths[&n.id]);
            let body = if rewrites.is_empty() {
                body.into()
            } else {
                rewriter.rewrite(body, old_path, &new.paths[&n.id])
            };
            let evidence = if rewrites.is_empty() {
                evidence.into()
            } else {
                rewriter.rewrite(evidence, old_path, &new.paths[&n.id])
            };
            let output = new.render(&after, n.id, &body, &evidence);
            if original.as_ref() != Some(&output) || old_path != &new.paths[&n.id] {
                changed.insert(n.id);
                edits.push(Edit {
                    id: n.id,
                    old: old.paths.get(&n.id).cloned(),
                    new: new.paths[&n.id].clone(),
                    before: original,
                    after: output,
                });
            }
        }
        // Changed ownership, membership, ordering or linked evidence requires review up both old and new trees.
        let mut queue: Vec<_> = changed.iter().copied().collect();
        let mut invalid = HashSet::new();
        let mut dependents: HashMap<u64, Vec<u64>> = HashMap::new();
        for e in &after.dependencies {
            dependents
                .entry(e.prerequisite)
                .or_default()
                .push(e.dependent);
        }
        while let Some(id) = queue.pop() {
            if !invalid.insert(id) {
                continue;
            }
            if let Some(parent) = old.nodes.get(&id).and_then(|n| n.parent) {
                queue.push(parent);
            }
            if let Some(parent) = new.nodes.get(&id).and_then(|n| n.parent) {
                queue.push(parent);
            }
            if let Some(targets) = dependents.get(&id) {
                queue.extend(targets);
            }
        }
        let mut dependencies: HashSet<u64> = created
            .iter()
            .copied()
            .filter(|id| new.nodes[id].kind == Kind::Function)
            .collect();
        for m in &moves {
            if new.nodes[&m.id].kind == Kind::Function {
                dependencies.insert(m.id);
            }
        }
        for e in &after.dependencies {
            if dependencies.contains(&e.dependent) || dependencies.contains(&e.prerequisite) {
                dependencies.insert(e.dependent);
                dependencies.insert(e.prerequisite);
            }
        }
        drop(new);
        for n in &mut after.nodes {
            if invalid.contains(&n.id) && old.nodes.contains_key(&n.id) {
                n.revision += 1;
                n.accepted_hash = None;
                n.accepted_prerequisites.clear();
                if n.status == Status::Complete {
                    n.status = Status::InProgress;
                }
            }
        }
        after.revision += 1;
        after.layout_version = 3;
        let mut sidecars = vec![];
        let annotation_path = root.join("tools/profiles/annotations.json");
        if !moves.is_empty() && annotation_path.is_file() {
            let original = fs::read_to_string(&annotation_path).map_err(|e| e.to_string())?;
            let mut ledger: serde_json::Value =
                serde_json::from_str(&original).map_err(|e| e.to_string())?;
            let paths: HashMap<_, _> = edits
                .iter()
                .filter_map(|e| e.old.as_ref().map(|old| (old.as_str(), e.new.as_str())))
                .collect();
            let mut modified = false;
            if ledger["schema"] == 1 {
                if let Some(rows) = ledger["annotations"].as_array_mut() {
                    for row in rows {
                        if let Some(path) = row["report"]
                            .as_str()
                            .and_then(|path| paths.get(path))
                            .copied()
                        {
                            row["report"] = path.into();
                            row["catalog_move"] = "Canonical report path updated; original review hash retained. Re-review any changed report before applying annotations.".into();
                            modified = true;
                        }
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
        let j = Journal {
            before,
            after,
            edits,
            moves,
            dependencies: dependencies.into_iter().collect(),
            sidecars,
        };
        let folder = pending(&root);
        contained(&root, &folder)?;
        if folder.exists() {
            return Err("Pending catalog transaction exists; reopen to resume it".into());
        }
        fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        atomic(
            &folder.join("journal.json"),
            &serde_json::to_vec(&j).map_err(|e| e.to_string())?,
        )?;
        resume(&root, &mut self.projects[p])?;
        Ok(created)
    }
}
