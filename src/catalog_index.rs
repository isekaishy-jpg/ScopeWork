use super::*;
use std::collections::{HashMap, HashSet};

/// A per-operation index. The serialized vector remains the explicit sibling order.
pub(super) struct Index<'a> {
    pub nodes: HashMap<u64, &'a Node>,
    pub children: HashMap<u64, Vec<u64>>,
    pub paths: HashMap<u64, String>,
    pub requires: HashMap<u64, Vec<u64>>,
    pub required: HashSet<u64>,
}
impl<'a> Index<'a> {
    pub fn new(project: &'a Project) -> Self {
        let nodes: HashMap<_, _> = project.nodes.iter().map(|n| (n.id, n)).collect();
        let mut children: HashMap<u64, Vec<u64>> = HashMap::new();
        let mut requires: HashMap<u64, Vec<u64>> = HashMap::new();
        for n in &project.nodes {
            if let Some(parent) = n.parent {
                children.entry(parent).or_default().push(n.id);
            }
        }
        for e in &project.dependencies {
            requires
                .entry(e.dependent)
                .or_default()
                .push(e.prerequisite);
        }
        fn path(id: u64, nodes: &HashMap<u64, &Node>, paths: &mut HashMap<u64, String>) -> String {
            if let Some(value) = paths.get(&id) {
                return value.clone();
            }
            let n = nodes[&id];
            let value = match n.kind {
                Kind::App => "app.md".to_string(),
                Kind::Architecture => format!("architectures/{:06}/research.md", n.id),
                _ => {
                    let parent = path(n.parent.unwrap(), nodes, paths);
                    let folder = if n.kind == Kind::Function {
                        format!("{:08X}", n.rva.unwrap())
                    } else {
                        format!("{:06}", n.id)
                    };
                    format!(
                        "{}/{folder}/research.md",
                        parent.trim_end_matches("/research.md")
                    )
                }
            };
            paths.insert(id, value.clone());
            value
        }
        let mut paths = HashMap::new();
        for n in &project.nodes {
            path(n.id, &nodes, &mut paths);
        }
        let mut required = HashSet::new();
        let mut queue = vec![0];
        while let Some(id) = queue.pop() {
            if !required.insert(id) {
                continue;
            }
            if let Some(kids) = children.get(&id) {
                queue.extend(kids.iter().copied().filter(|id| !nodes[id].context_only));
            }
            if let Some(deps) = requires.get(&id) {
                queue.extend(deps);
            }
        }
        Self {
            nodes,
            children,
            paths,
            requires,
            required,
        }
    }
    pub fn render(&self, project: &Project, id: u64, body: &str, evidence: &str) -> String {
        let n = self.nodes[&id];
        let mut md = format!(
            "# {}\n\nProject: {} (`{}`)\n\nBinary: {}\n\nKind: {} | ID: {}\n",
            n.name,
            project.name,
            project.id,
            project.binary,
            n.kind.label(),
            id
        );
        if let Some(rva) = n.rva {
            md += &format!("\nRVA: `0x{rva:08X}`\n");
        }
        md += "\n## Source reports\n";
        if let Some(kids) = self.children.get(&id) {
            for child in kids {
                let c = self.nodes[child];
                md += &format!(
                    "- [{}]({}){}\n",
                    c.name.replace('[', "(").replace(']', ")"),
                    layout::link(&self.paths[&id], &self.paths[child]),
                    if c.context_only {
                        " (context inventory)"
                    } else {
                        ""
                    }
                );
            }
        }
        md += "\n## Research and understanding\n<!-- research-content -->\n";
        md += body;
        md += "\n<!-- research-evidence -->\n";
        md += evidence;
        md
    }
    /// Hashes of the reports an evaluation of `roots` may compare: every completed node in the
    /// closure over children and prerequisites, plus those nodes' prerequisites.
    fn report_hashes(
        &self,
        dir: &Path,
        roots: impl IntoIterator<Item = u64>,
        overrides: &dyn Fn() -> HashMap<String, Option<String>>,
    ) -> HashMap<u64, Option<String>> {
        let mut seen = HashSet::new();
        let mut queue: Vec<u64> = roots.into_iter().collect();
        let mut wanted = HashSet::new();
        while let Some(id) = queue.pop() {
            if !seen.insert(id) || !self.nodes.contains_key(&id) {
                continue;
            }
            let deps = self.requires.get(&id).map(Vec::as_slice).unwrap_or(&[]);
            if self.nodes[&id].status == Status::Complete {
                wanted.insert(id);
                wanted.extend(deps.iter().copied());
            }
            queue.extend(deps.iter().copied());
            if let Some(kids) = self.children.get(&id) {
                queue.extend(kids.iter().copied());
            }
        }
        let ids: Vec<u64> = wanted.into_iter().collect();
        let rels: Vec<&str> = ids.iter().map(|id| self.paths[id].as_str()).collect();
        let hashes = report_hashes::hashes(dir, &rels);
        // Read after hashing: a text replaced meanwhile is already in the caller's overrides.
        let overrides = overrides();
        ids.into_iter()
            .map(|id| {
                let path = &self.paths[&id];
                let hash = match overrides.get(path) {
                    Some(overridden) => overridden.clone(),
                    None => hashes.get(path).cloned().flatten(),
                };
                (id, hash)
            })
            .collect()
    }
    fn state(
        &self,
        hashes: &HashMap<u64, Option<String>>,
        id: u64,
        cache: &mut HashMap<u64, Status>,
        active: &mut HashSet<u64>,
    ) -> Status {
        if let Some(state) = cache.get(&id) {
            return *state;
        }
        if !active.insert(id) {
            return Status::InProgress;
        }
        let n = self.nodes[&id];
        let kids: Vec<_> = self
            .children
            .get(&id)
            .into_iter()
            .flatten()
            .copied()
            .filter(|id| !self.nodes[id].context_only)
            .collect();
        let mut state = n.status;
        if state == Status::NotStarted
            && kids
                .iter()
                .any(|id| self.state(hashes, *id, cache, active) != Status::NotStarted)
        {
            state = Status::InProgress;
        }
        if state == Status::Complete {
            let valid = hashes.get(&id).cloned().flatten().is_some()
                && hashes.get(&id).cloned().flatten() == n.accepted_hash;
            let deps = self.requires.get(&id).cloned().unwrap_or_default();
            let stamps: std::collections::BTreeMap<_, _> = deps
                .iter()
                .map(|id| {
                    // A missing or unreadable prerequisite report stamps as the empty text, as before.
                    let digest = hashes
                        .get(id)
                        .cloned()
                        .flatten()
                        .unwrap_or_else(|| hash(""));
                    (*id, format!("{}:{}", self.nodes[id].revision, digest))
                })
                .collect();
            if !valid
                || n.accepted_prerequisites != stamps
                || deps
                    .iter()
                    .any(|id| self.state(hashes, *id, cache, active) != Status::Complete)
                || (n.kind != Kind::Function
                    && (kids.is_empty()
                        || kids
                            .iter()
                            .any(|id| self.state(hashes, *id, cache, active) != Status::Complete)))
            {
                state = Status::InProgress;
            }
        }
        active.remove(&id);
        cache.insert(id, state);
        state
    }
    /// Effective status of `ids`, with report hashes that differ from the files on disk for the caller's view
    /// (texts staged in a merged commit, or replaced since a snapshot was taken).
    pub fn states_with(
        &self,
        dir: &Path,
        ids: impl IntoIterator<Item = u64>,
        overrides: &dyn Fn() -> HashMap<String, Option<String>>,
    ) -> HashMap<u64, Status> {
        let ids: Vec<u64> = ids.into_iter().collect();
        let hashes = self.report_hashes(dir, ids.iter().copied(), overrides);
        let mut cache = HashMap::new();
        let mut active = HashSet::new();
        for id in ids {
            self.state(&hashes, id, &mut cache, &mut active);
        }
        cache
    }
    pub fn states(&self, dir: &Path) -> HashMap<u64, Status> {
        self.states_all_with(dir, &HashMap::new)
    }
    /// `states` with hash overrides, as in `states_with`.
    pub fn states_all_with(
        &self,
        dir: &Path,
        overrides: &dyn Fn() -> HashMap<String, Option<String>>,
    ) -> HashMap<u64, Status> {
        let hashes = self.report_hashes(dir, self.nodes.keys().copied(), overrides);
        let mut cache = HashMap::new();
        let mut active = HashSet::new();
        for id in self.nodes.keys() {
            self.state(&hashes, *id, &mut cache, &mut active);
        }
        cache
    }
}
