//! The implementation tracker: the recreation's own organisation as a free-form tree, the
//! research functions placed where they are implemented, implementation-only items, and their
//! completion.
//!
//! Stored as journaled sidecars, sharded so a change copies only what it touches into its
//! receipt: `implementation/tree.json` (repository and nodes) and one
//! `implementation/nodes/<id>.json` per node (its placements and items). Every read goes through
//! `Library::text_opt`, so operations merged into one coordinator commit see each other.
use super::*;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};

pub(super) const TREE: &str = "implementation/tree.json";
/// The longest node or item name, in characters.
const NAME_MAX: usize = 120;

fn shard_path(id: u64) -> String {
    format!("implementation/nodes/{id:06}.json")
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
struct Tree {
    schema: u32,
    #[serde(default)]
    repository: String,
    #[serde(default)]
    remote: String,
    /// The next ID for a node or an item; IDs are never reused.
    next_id: u64,
    #[serde(default)]
    nodes: Vec<TreeNode>,
}
impl Default for Tree {
    fn default() -> Self {
        Self {
            schema: 1,
            repository: String::new(),
            remote: String::new(),
            next_id: 1,
            nodes: vec![],
        }
    }
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
struct TreeNode {
    id: u64,
    parent: Option<u64>,
    name: String,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
struct Shard {
    schema: u32,
    #[serde(default)]
    placements: Vec<Placement>,
    #[serde(default)]
    items: Vec<Item>,
}
impl Default for Shard {
    fn default() -> Self {
        Self {
            schema: 1,
            placements: vec![],
            items: vec![],
        }
    }
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
struct Completion {
    by: String,
    at_unix: u64,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
struct Placement {
    record: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    completed: Option<Completion>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
struct Item {
    id: u64,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    completed: Option<Completion>,
}

fn text<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string_pretty(value)
        .map(|mut s| {
            s.push('\n');
            s
        })
        .map_err(|e| e.to_string())
}
fn valid_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().any(char::is_control) || name.chars().count() > NAME_MAX {
        return Err(format!(
            "A name is a single line of 1 to {NAME_MAX} characters."
        ));
    }
    Ok(name.to_string())
}
/// Who completes something: the submitter the coordinator also reports, else the user name.
fn completion() -> Completion {
    let by = caller();
    let at_unix = unix_now();
    Completion { by, at_unix }
}

/// A tracker change in progress: the tree and the shards read so far, and what changed.
struct Change {
    tree: Tree,
    shards: BTreeMap<u64, Shard>,
    tree_changed: bool,
    changed: HashSet<u64>,
}

impl Library {
    fn read_tree(&self, p: usize) -> Result<Tree> {
        match self.text_opt(p, TREE)? {
            Some(text) => serde_json::from_str(&text).map_err(|e| format!("{TREE}: {e}")),
            None => Ok(Tree::default()),
        }
    }
    fn read_shard(&self, p: usize, id: u64) -> Result<Shard> {
        let path = shard_path(id);
        match self.text_opt(p, &path)? {
            Some(text) => serde_json::from_str(&text).map_err(|e| format!("{path}: {e}")),
            None => Ok(Shard::default()),
        }
    }
    fn begin_change(&self, p: usize) -> Result<Change> {
        Ok(Change {
            tree: self.read_tree(p)?,
            shards: BTreeMap::new(),
            tree_changed: false,
            changed: HashSet::new(),
        })
    }
    /// Every node's shard, keyed by node.
    fn all_shards(&self, p: usize, tree: &Tree) -> Result<BTreeMap<u64, Shard>> {
        tree.nodes
            .iter()
            .map(|n| Ok((n.id, self.read_shard(p, n.id)?)))
            .collect()
    }
    /// Commits the changed tree and shards as one journaled transaction.
    fn commit_change(&mut self, p: usize, change: Change) -> Result<()> {
        let mut files: Vec<(String, String)> = vec![];
        if change.tree_changed {
            files.push((TREE.to_string(), text(&change.tree)?));
        }
        for id in &change.changed {
            if let Some(shard) = change.shards.get(id) {
                files.push((shard_path(*id), text(shard)?));
            }
        }
        if files.is_empty() {
            return Ok(());
        }
        let root = self.dir(p);
        let mut sidecars = vec![];
        for (path, after) in files {
            let before = match self.text_opt(p, &path)? {
                Some(text) => text,
                None => {
                    // A sidecar exists before its first transaction, like the other sidecars.
                    let empty = if path == TREE {
                        text(&Tree::default())?
                    } else {
                        text(&Shard::default())?
                    };
                    let file = root.join(&path);
                    fs::create_dir_all(file.parent().ok_or("Invalid tracker path")?)
                        .map_err(|e| e.to_string())?;
                    atomic(&file, empty.as_bytes())?;
                    empty
                }
            };
            if before != after {
                sidecars.push(catalog_batch::Sidecar {
                    path,
                    before,
                    after,
                });
            }
        }
        if sidecars.is_empty() {
            return Ok(());
        }
        let before = self.projects[p].clone();
        let mut after = before.clone();
        after.revision += 1;
        let journal = catalog_batch::Journal {
            before,
            after,
            sidecars,
            ..Default::default()
        };
        self.commit_or_stage(p, journal).map(|_| ())
    }
    fn node_exists(tree: &Tree, id: u64) -> Result<()> {
        if tree.nodes.iter().any(|n| n.id == id) {
            Ok(())
        } else {
            Err(format!("No implementation node {id}"))
        }
    }
    /// A node's path from the top, names joined by `/`.
    fn node_path(tree: &Tree, id: u64) -> String {
        let mut names = vec![];
        let mut seen = HashSet::new();
        let mut current = Some(id);
        // `seen` stops at a cycle a hand-edited tree.json may hold.
        while let Some(id) = current.filter(|id| seen.insert(*id)) {
            let Some(node) = tree.nodes.iter().find(|n| n.id == id) else {
                break;
            };
            names.push(node.name.as_str());
            current = node.parent;
        }
        names.reverse();
        names.join("/")
    }
    /// Runs `apply` on a fresh change and commits it, re-opening for writing when needed.
    fn implementation_write<T>(
        &mut self,
        p: usize,
        apply: impl FnOnce(&Self, &mut Change) -> Result<T>,
    ) -> Result<T> {
        if !self.writable(p) {
            return self.write_project(p, |l, p| l.implementation_write(p, apply));
        }
        let mut change = self.begin_change(p)?;
        let result = apply(self, &mut change)?;
        self.commit_change(p, change)?;
        Ok(result)
    }

    pub fn impl_set_repository(&mut self, p: usize, path: &str, remote: &str) -> Result<()> {
        let (path, remote) = (path.trim().to_string(), remote.trim().to_string());
        if path.chars().any(char::is_control) || remote.chars().any(char::is_control) {
            return Err("The repository path and remote are single lines.".into());
        }
        self.implementation_write(p, |_, c| {
            c.tree.repository = path;
            c.tree.remote = remote;
            c.tree_changed = true;
            Ok(())
        })
    }
    pub fn impl_add(&mut self, p: usize, parent: Option<u64>, name: &str) -> Result<u64> {
        let name = valid_name(name)?;
        self.implementation_write(p, |_, c| {
            if let Some(parent) = parent {
                Self::node_exists(&c.tree, parent)?;
            }
            let id = c.tree.next_id;
            c.tree.next_id += 1;
            c.tree.nodes.push(TreeNode { id, parent, name });
            c.tree_changed = true;
            Ok(id)
        })
    }
    pub fn impl_rename(&mut self, p: usize, node: u64, name: &str) -> Result<()> {
        let name = valid_name(name)?;
        self.implementation_write(p, |_, c| {
            Self::node_exists(&c.tree, node)?;
            if let Some(n) = c.tree.nodes.iter_mut().find(|n| n.id == node) {
                n.name = name;
            }
            c.tree_changed = true;
            Ok(())
        })
    }
    pub fn impl_move(&mut self, p: usize, node: u64, parent: Option<u64>) -> Result<()> {
        self.implementation_write(p, |_, c| {
            Self::node_exists(&c.tree, node)?;
            if let Some(parent) = parent {
                Self::node_exists(&c.tree, parent)?;
                // The new parent may not be the node or below it. `seen` stops at a cycle a
                // hand-edited tree.json may hold.
                let mut seen = HashSet::new();
                let mut current = Some(parent);
                while let Some(id) = current {
                    if id == node {
                        return Err("A node cannot move inside itself.".into());
                    }
                    if !seen.insert(id) {
                        return Err(format!(
                            "The implementation tree has a cycle through node {id}; repair {TREE}."
                        ));
                    }
                    current = c
                        .tree
                        .nodes
                        .iter()
                        .find(|n| n.id == id)
                        .and_then(|n| n.parent);
                }
            }
            if let Some(n) = c.tree.nodes.iter_mut().find(|n| n.id == node) {
                n.parent = parent;
            }
            c.tree_changed = true;
            Ok(())
        })
    }
    pub fn impl_remove(&mut self, p: usize, node: u64) -> Result<()> {
        self.implementation_write(p, |lib, c| {
            Self::node_exists(&c.tree, node)?;
            if c.tree.nodes.iter().any(|n| n.parent == Some(node)) {
                return Err(format!(
                    "Node {node} has child nodes; move or remove them first."
                ));
            }
            let shard = lib.read_shard(p, node)?;
            if !shard.placements.is_empty() || !shard.items.is_empty() {
                return Err(format!(
                    "Node {node} still holds placements or items; unplace or remove them first."
                ));
            }
            c.tree.nodes.retain(|n| n.id != node);
            c.tree_changed = true;
            Ok(())
        })
    }
    pub fn impl_place(&mut self, p: usize, node: u64, records: &[u64]) -> Result<Value> {
        let mut unique = HashSet::new();
        let records: Vec<u64> = records
            .iter()
            .copied()
            .filter(|r| unique.insert(*r))
            .collect();
        let records = records.as_slice();
        let catalog: HashMap<u64, Kind> = self.projects[p]
            .nodes
            .iter()
            .map(|n| (n.id, n.kind))
            .collect();
        for record in records {
            match catalog.get(record) {
                Some(Kind::Function) => {}
                Some(_) => return Err(format!("Record {record} is not a function.")),
                None => return Err(format!("Unknown record {record}")),
            }
        }
        self.implementation_write(p, |lib, c| {
            Self::node_exists(&c.tree, node)?;
            let shards = lib.all_shards(p, &c.tree)?;
            let mut placed = vec![];
            for record in records {
                if let Some((owner, _)) = shards
                    .iter()
                    .find(|(_, s)| s.placements.iter().any(|pl| pl.record == *record))
                {
                    if *owner == node {
                        continue;
                    }
                    return Err(format!(
                        "Record {record} is already placed in {} (node {owner}); unplace it first.",
                        Self::node_path(&c.tree, *owner)
                    ));
                }
                placed.push(*record);
            }
            let shard = c
                .shards
                .entry(node)
                .or_insert_with(|| shards[&node].clone());
            for record in &placed {
                if !shard.placements.iter().any(|pl| pl.record == *record) {
                    shard.placements.push(Placement {
                        record: *record,
                        completed: None,
                    });
                }
            }
            c.changed.insert(node);
            Ok(json!({"node": node, "placed": placed}))
        })
    }
    pub fn impl_unplace(&mut self, p: usize, records: &[u64]) -> Result<Value> {
        self.implementation_write(p, |lib, c| {
            let shards = lib.all_shards(p, &c.tree)?;
            let wanted: HashSet<u64> = records.iter().copied().collect();
            let mut removed = vec![];
            for (id, shard) in shards {
                if shard
                    .placements
                    .iter()
                    .any(|pl| wanted.contains(&pl.record))
                {
                    let mut shard = shard;
                    removed.extend(
                        shard
                            .placements
                            .iter()
                            .filter(|pl| wanted.contains(&pl.record))
                            .map(|pl| pl.record),
                    );
                    shard.placements.retain(|pl| !wanted.contains(&pl.record));
                    c.shards.insert(id, shard);
                    c.changed.insert(id);
                }
            }
            removed.sort_unstable();
            Ok(json!({"unplaced": removed}))
        })
    }
    pub fn impl_add_item(&mut self, p: usize, node: u64, name: &str) -> Result<u64> {
        let name = valid_name(name)?;
        self.implementation_write(p, |lib, c| {
            Self::node_exists(&c.tree, node)?;
            let id = c.tree.next_id;
            c.tree.next_id += 1;
            c.tree_changed = true;
            let shard = lib.read_shard(p, node)?;
            let shard = c.shards.entry(node).or_insert(shard);
            shard.items.push(Item {
                id,
                name,
                completed: None,
            });
            c.changed.insert(node);
            Ok(id)
        })
    }
    pub fn impl_remove_item(&mut self, p: usize, item: u64) -> Result<()> {
        self.implementation_write(p, |lib, c| {
            for (id, mut shard) in lib.all_shards(p, &c.tree)? {
                if shard.items.iter().any(|i| i.id == item) {
                    shard.items.retain(|i| i.id != item);
                    c.shards.insert(id, shard);
                    c.changed.insert(id);
                    return Ok(());
                }
            }
            Err(format!("No implementation-only item {item}"))
        })
    }
    /// Marks placed records and items complete (or not). Every one must exist.
    pub fn impl_set_complete(
        &mut self,
        p: usize,
        records: &[u64],
        items: &[u64],
        complete: bool,
    ) -> Result<Value> {
        let stamp = completion();
        self.implementation_write(p, |lib, c| {
            let mut records: HashSet<u64> = records.iter().copied().collect();
            let mut items: HashSet<u64> = items.iter().copied().collect();
            let mut changed = 0usize;
            for (id, mut shard) in lib.all_shards(p, &c.tree)? {
                let mut touched = false;
                for pl in &mut shard.placements {
                    if records.remove(&pl.record) && pl.completed.is_some() != complete {
                        pl.completed = complete.then(|| stamp.clone());
                        touched = true;
                        changed += 1;
                    }
                }
                for item in &mut shard.items {
                    if items.remove(&item.id) && item.completed.is_some() != complete {
                        item.completed = complete.then(|| stamp.clone());
                        touched = true;
                        changed += 1;
                    }
                }
                if touched {
                    c.shards.insert(id, shard);
                    c.changed.insert(id);
                }
            }
            if let Some(record) = records.iter().min() {
                return Err(format!(
                    "Record {record} is not placed in the implementation."
                ));
            }
            if let Some(item) = items.iter().min() {
                return Err(format!("No implementation-only item {item}"));
            }
            Ok(json!({"complete": complete, "changed": changed}))
        })
    }
    /// Where every placed record is implemented: record to its node's path.
    pub fn impl_placements(&self, p: usize) -> Result<HashMap<u64, String>> {
        let tree = self.read_tree(p)?;
        Ok(self
            .all_shards(p, &tree)?
            .into_iter()
            .flat_map(|(id, shard)| {
                let path = Self::node_path(&tree, id);
                shard
                    .placements
                    .into_iter()
                    .map(move |pl| (pl.record, path.clone()))
            })
            .collect())
    }
    /// Where `record` is implemented: its node and that node's path, or null.
    pub fn impl_where(&self, p: usize, record: u64) -> Result<Value> {
        let tree = self.read_tree(p)?;
        for (id, shard) in self.all_shards(p, &tree)? {
            if let Some(pl) = shard.placements.iter().find(|pl| pl.record == record) {
                return Ok(
                    json!({"record": record, "node": id, "path": Self::node_path(&tree, id),
                    "complete": pl.completed.is_some()}),
                );
            }
        }
        Ok(json!({"record": record, "node": null, "path": null, "complete": false}))
    }
    /// The tracker: repository, the tree with counters per node (own and rolled up), totals,
    /// and on request one node's placements and items or the functions not yet placed.
    pub fn impl_view(&self, p: usize, detail: Option<u64>, unplaced: bool) -> Result<Value> {
        let tree = self.read_tree(p)?;
        let shards = self.all_shards(p, &tree)?;
        let project = &self.projects[p];
        let records: HashMap<u64, &Node> = project.nodes.iter().map(|n| (n.id, n)).collect();
        // Own counters: [research done, research total, items done, items total].
        let own: HashMap<u64, [usize; 4]> = shards
            .iter()
            .map(|(id, s)| {
                (
                    *id,
                    [
                        s.placements
                            .iter()
                            .filter(|pl| pl.completed.is_some())
                            .count(),
                        s.placements.len(),
                        s.items.iter().filter(|i| i.completed.is_some()).count(),
                        s.items.len(),
                    ],
                )
            })
            .collect();
        let mut children: HashMap<Option<u64>, Vec<u64>> = HashMap::new();
        for n in &tree.nodes {
            children.entry(n.parent).or_default().push(n.id);
        }
        fn rolled(
            id: u64,
            own: &HashMap<u64, [usize; 4]>,
            children: &HashMap<Option<u64>, Vec<u64>>,
            out: &mut HashMap<u64, [usize; 4]>,
        ) -> [usize; 4] {
            let mut total = own.get(&id).copied().unwrap_or_default();
            out.insert(id, total);
            for child in children.get(&Some(id)).into_iter().flatten() {
                if out.contains_key(child) {
                    // Already counted: a hand-edited tree.json may hold a cycle.
                    continue;
                }
                let sub = rolled(*child, own, children, out);
                for k in 0..4 {
                    total[k] += sub[k];
                }
            }
            out.insert(id, total);
            total
        }
        let mut subtree = HashMap::new();
        for top in children.get(&None).into_iter().flatten() {
            rolled(*top, &own, &children, &mut subtree);
        }
        let counters = |c: [usize; 4]| {
            json!({"research": {"done": c[0], "total": c[1]},
                "implementation_only": {"done": c[2], "total": c[3]}})
        };
        let nodes: Vec<Value> = tree
            .nodes
            .iter()
            .map(|n| {
                json!({"id": n.id, "parent": n.parent, "name": n.name,
                    "path": Self::node_path(&tree, n.id),
                    "own": counters(own.get(&n.id).copied().unwrap_or_default()),
                    "subtree": counters(subtree.get(&n.id).copied().unwrap_or_default())})
            })
            .collect();
        let totals = own.values().fold([0usize; 4], |mut acc, c| {
            for k in 0..4 {
                acc[k] += c[k];
            }
            acc
        });
        let placed: HashSet<u64> = shards
            .values()
            .flat_map(|s| s.placements.iter().map(|pl| pl.record))
            .collect();
        let not_placed: Vec<&Node> = project
            .nodes
            .iter()
            .filter(|n| n.kind == Kind::Function && !placed.contains(&n.id))
            .collect();
        // A placed record deleted from the research since stays placed until unplaced.
        let describe = |id: u64| {
            records.get(&id).map_or(
                json!({"id": id, "missing": true}),
                |n| json!({"id": id, "name": n.name, "rva": n.rva.map(|r| format!("0x{r:08X}")), "space": n.space}),
            )
        };
        let mut view = json!({
            "project": project.id,
            "repository": tree.repository,
            "remote": tree.remote,
            "nodes": nodes,
            "totals": counters(totals),
            "not_placed": not_placed.len(),
        });
        if let Some(id) = detail {
            Self::node_exists(&tree, id)?;
            let shard = &shards[&id];
            view["node"] = json!({
                "id": id,
                "path": Self::node_path(&tree, id),
                "placements": shard.placements.iter().map(|pl| {
                    let mut v = describe(pl.record);
                    v["completed"] = serde_json::to_value(&pl.completed).unwrap_or(Value::Null);
                    v
                }).collect::<Vec<_>>(),
                "items": shard.items,
            });
        }
        if unplaced {
            view["unplaced"] = not_placed.iter().map(|n| describe(n.id)).collect();
        }
        Ok(view)
    }
}
