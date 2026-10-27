//! Restricted research pools (docs/proposal-research-pools.md). A pooled record (a chunk
//! covers its functions) is open work only for the agents registered to its pool: every write
//! that changes it needs a member, the human's direction or the dashboard. Anyone may read it
//! and build on it. Its status is its own.
//!
//! Pools live in a journaled sidecar beside the catalog, like review flags, with a log of every
//! change. Pools replace quarantine: `quarantine.json` is migrated by `migrate_quarantine`, and
//! a project that still holds quarantine records refuses writes until it is.
use super::catalog_batch::{Journal, Sidecar};
use super::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};

pub const POOLS: &str = "pools.json";
/// The sidecar pools replace, read only to migrate it.
pub const QUARANTINE: &str = "quarantine.json";
/// Why a record enters a pool: an agent hit a safety stop on it, or the human put it there.
pub const ORIGINS: [&str; 2] = ["safety-stop", "human"];
/// The longest pool name, in characters.
const NAME_MAX: usize = 60;

static HUMAN: AtomicBool = AtomicBool::new(false);
/// Marks this process as acting for the human (the dashboard): pools never refuse it.
pub fn act_for_human() {
    HUMAN.store(true, Ordering::Release);
}
fn human() -> bool {
    HUMAN.load(Ordering::Acquire)
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Debug)]
pub struct Pool {
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub members: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub struct Pooled {
    pub pool: String,
    pub origin: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub by: String,
    pub at_revision: u64,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub struct LogEntry {
    pub event: String,
    pub pool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<u64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub agent: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub by: String,
    pub at_revision: u64,
}
#[derive(Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PoolFile {
    pub schema: u32,
    #[serde(default)]
    pub pools: BTreeMap<String, Pool>,
    #[serde(default)]
    pub records: BTreeMap<u64, Pooled>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub log: Vec<LogEntry>,
}
impl PoolFile {
    /// The pooled record covering `id` and the node that carries it (itself or its chunk).
    pub fn covering(&self, project: &Project, id: u64) -> Option<(u64, &Pooled)> {
        let parent = || project.nodes.iter().find(|n| n.id == id)?.parent;
        self.covering_with(id, parent)
    }
    /// `covering`, with the parent looked up by `parent` only when `id` itself is not pooled.
    fn covering_with(
        &self,
        id: u64,
        parent: impl FnOnce() -> Option<u64>,
    ) -> Option<(u64, &Pooled)> {
        if let Some(record) = self.records.get(&id) {
            return Some((id, record));
        }
        let parent = parent()?;
        self.records.get(&parent).map(|r| (parent, r))
    }
    /// Every pooled node with its pool: recorded ones and the functions of pooled chunks.
    pub fn effective(&self, project: &Project) -> HashMap<u64, String> {
        let mut out: HashMap<u64, String> = self
            .records
            .iter()
            .map(|(id, r)| (*id, r.pool.clone()))
            .collect();
        for n in &project.nodes {
            if let Some(parent) = n.parent
                && let Some(r) = self.records.get(&parent)
            {
                out.entry(n.id).or_insert_with(|| r.pool.clone());
            }
        }
        out
    }
    pub fn is_member(&self, pool: &str, agent: &str) -> bool {
        self.pools
            .get(pool)
            .is_some_and(|p| p.members.iter().any(|m| m == agent))
    }
    /// Pooled nodes `agent` may not change (none for the human).
    pub fn closed_to(&self, project: &Project, agent: &str) -> HashSet<u64> {
        if human() {
            return HashSet::new();
        }
        self.effective(project)
            .into_iter()
            .filter(|(_, pool)| !self.is_member(pool, agent))
            .map(|(id, _)| id)
            .collect()
    }
}

fn parse(text: &str) -> Result<PoolFile> {
    if text.trim().is_empty() {
        return Ok(PoolFile {
            schema: 1,
            ..Default::default()
        });
    }
    let file: PoolFile = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if file.schema != 1 {
        return Err("Unsupported pools file".into());
    }
    Ok(file)
}
fn render(file: &PoolFile) -> Result<String> {
    serde_json::to_string_pretty(file).map_err(|e| e.to_string())
}
/// The pools file without records of `removed` nodes (the log keeps them), or `None` when no
/// record changes.
pub(crate) fn without_removed(text: &str, removed: &HashSet<u64>) -> Result<Option<String>> {
    let mut file = parse(text)?;
    let count = file.records.len();
    file.records.retain(|id, _| !removed.contains(id));
    if file.records.len() == count {
        return Ok(None);
    }
    render(&file).map(Some)
}
/// Why a write is refused while the pools file cannot be read: a damaged file, or one written
/// by a newer version, must never open pooled records to everyone.
pub(super) fn unreadable(error: &str) -> String {
    format!(
        "pools_unreadable: {POOLS} cannot be read ({error}). Writes to this project are refused until it is repaired; reads treat it as holding no pools meanwhile."
    )
}
/// Quarantine records as 0.14 wrote them, for migration.
#[derive(Deserialize)]
struct LegacyQuarantine {
    origin: String,
    reason: String,
    #[serde(default)]
    by: String,
}
#[derive(Deserialize)]
struct LegacyFile {
    #[serde(default)]
    records: BTreeMap<u64, LegacyQuarantine>,
}
fn legacy_records(text: &str) -> Result<BTreeMap<u64, LegacyQuarantine>> {
    if text.trim().is_empty() {
        return Ok(BTreeMap::new());
    }
    serde_json::from_str::<LegacyFile>(text)
        .map(|f| f.records)
        .map_err(|e| e.to_string())
}
/// Each pooled node with a short description (inherited ones name their chunk).
pub(super) fn descriptions(project: &Project, file: &PoolFile) -> HashMap<u64, String> {
    let mut out: HashMap<u64, String> = file
        .records
        .iter()
        .map(|(id, r)| (*id, format!("pool {}, {}: {}", r.pool, r.origin, r.reason)))
        .collect();
    for n in &project.nodes {
        if let Some(parent) = n.parent
            && let Some(r) = file.records.get(&parent)
        {
            out.entry(n.id).or_insert_with(|| {
                format!(
                    "pool {} through chunk {parent}, {}: {}",
                    r.pool, r.origin, r.reason
                )
            });
        }
    }
    out
}
/// Pools read from a project folder (for snapshots built outside a library).
pub(super) fn read_file(dir: &Path) -> PoolFile {
    fs::read_to_string(dir.join(POOLS))
        .ok()
        .and_then(|text| parse(&text).ok())
        .unwrap_or_default()
}
fn valid_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty()
        || name.chars().count() > NAME_MAX
        || name
            .chars()
            .any(|c| c.is_control() || c == '/' || c == '\\')
    {
        return Err(format!(
            "A pool name is 1 to {NAME_MAX} characters on one line, without slashes."
        ));
    }
    Ok(name.into())
}

impl Library {
    /// The project's pools (none when the file is missing or cannot be read: reads keep working;
    /// writes check `pools_for_write` first).
    pub fn pools(&self, p: usize) -> PoolFile {
        self.pools_checked(p).unwrap_or_default()
    }
    fn pools_checked(&self, p: usize) -> Result<PoolFile> {
        match self.text_opt(p, POOLS)? {
            Some(text) => parse(&text),
            None => parse(""),
        }
    }
    /// The research home this library's projects live in.
    fn home(&self) -> PathBuf {
        self.root
            .parent()
            .map_or_else(|| self.root.clone(), Path::to_path_buf)
    }
    /// The pools for the quarantine migration's own commit: only an unreadable file refuses.
    pub(super) fn pools_checked_for_migration(&self, p: usize) -> Result<()> {
        self.pools_checked(p)
            .map(|_| ())
            .map_err(|e| unreadable(&e))
    }
    /// The pools for a write. An unreadable file refuses it, and so do quarantine records that
    /// were never migrated: nothing they protected may change before the human migrates them.
    pub(super) fn pools_for_write(&self, p: usize) -> Result<PoolFile> {
        if let Some(text) = self.text_opt(p, QUARANTINE)?
            && !legacy_records(&text).is_ok_and(|r| r.is_empty())
        {
            return Err(format!(
                "quarantine_unmigrated: this project still has quarantined records in {QUARANTINE}. Pools replace quarantine: run research pool migrate-quarantine {} first.",
                self.projects[p].id
            ));
        }
        self.pools_checked(p).map_err(|e| unreadable(&e))
    }
    /// The pooled record covering `id` (with the node that carries it and its pool's members),
    /// and whether the caller may change `id`: always, when it is not pooled. One read of the
    /// pools file answers both.
    pub fn pool_access(&self, p: usize, id: u64) -> (Option<(u64, Pooled, Vec<String>)>, bool) {
        let file = self.pools(p);
        match file.covering(&self.projects[p], id) {
            Some((carrier, record)) => {
                let may_write = human() || file.is_member(&record.pool, &caller());
                let members = file
                    .pools
                    .get(&record.pool)
                    .map(|pool| pool.members.clone())
                    .unwrap_or_default();
                (Some((carrier, record.clone(), members)), may_write)
            }
            None => (None, true),
        }
    }
    /// Pooled nodes the caller may not change.
    pub fn closed_to_caller(&self, p: usize) -> HashSet<u64> {
        self.pools(p).closed_to(&self.projects[p], &caller())
    }
    /// Refuses a change touching pooled records unless the caller is a member of their pool,
    /// the human directed it, or the dashboard asks.
    pub(super) fn check_pool(
        &self,
        p: usize,
        touched: impl IntoIterator<Item = u64>,
        directed_by: Option<&str>,
        what: &str,
    ) -> Result<()> {
        if directed_by.is_some_and(|d| !d.trim().is_empty()) || human() {
            return self.pools_for_write(p).map(|_| ());
        }
        let file = self.pools_for_write(p)?;
        if file.records.is_empty() {
            return Ok(());
        }
        let agent = caller();
        // One map for every record checked: a batch may touch the whole project.
        let parents: HashMap<u64, u64> = self.projects[p]
            .nodes
            .iter()
            .filter_map(|n| Some((n.id, n.parent?)))
            .collect();
        for id in touched {
            if let Some((carrier, record)) = file.covering_with(id, || parents.get(&id).copied())
                && !file.is_member(&record.pool, &agent)
            {
                let through = if carrier == id {
                    String::new()
                } else {
                    format!(" through chunk {carrier}")
                };
                return Err(format!(
                    "pooled: node {id} is in pool {}{through} ({}: {}). {what} it is for the pool's members; {agent} is not one. Read it and build on it freely; the human directs who joins (research pool join).",
                    record.pool, record.origin, record.reason
                ));
            }
        }
        Ok(())
    }
    /// Adds records to `pool` (created when new), or takes them out (`pool` None). Returns the
    /// records that changed. Pooling a record that is in no pool is open to everyone, so an
    /// agent that hits a safety stop can always pool it; taking a record out of its pool,
    /// moving it to another or changing its entry is a change to a pooled record.
    pub fn pool_records(
        &mut self,
        p: usize,
        pool: Option<&str>,
        ids: &[u64],
        origin: &str,
        reason: &str,
        directed_by: Option<&str>,
    ) -> Result<Vec<u64>> {
        if !self.writable(p) {
            let ids = ids.to_vec();
            return self.write_project(p, move |l, p| {
                l.pool_records(p, pool, &ids, origin, reason, directed_by)
            });
        }
        let reason = reason.trim();
        if reason.is_empty() {
            return Err("A pool change needs a reason.".into());
        }
        if ids.is_empty() {
            return Err("Name at least one record.".into());
        }
        let pool = pool.map(valid_name).transpose()?;
        if pool.is_some() && !ORIGINS.contains(&origin) {
            return Err("A pooled record's origin is safety-stop or human.".into());
        }
        let mut file = self.pools_for_write(p)?;
        // Records already in a pool change only through its members (or the human).
        self.check_pool(
            p,
            ids.iter()
                .copied()
                .filter(|id| file.records.contains_key(id)),
            directed_by,
            "Changing the pool of",
        )?;
        let revision = self.projects[p].revision + 1;
        let by = caller();
        let mut changed = vec![];
        for id in ids {
            let node = self.projects[p]
                .nodes
                .iter()
                .find(|n| n.id == *id)
                .ok_or_else(|| format!("Unknown node {id}"))?;
            if !matches!(node.kind, Kind::Chunk | Kind::Function) {
                return Err(format!(
                    "Node {id}: only chunks and functions can be pooled."
                ));
            }
            match &pool {
                Some(pool) => {
                    let log = |event: &str, pool: &str| LogEntry {
                        event: event.into(),
                        pool: pool.into(),
                        node: Some(*id),
                        agent: String::new(),
                        reason: reason.into(),
                        by: by.clone(),
                        at_revision: revision,
                    };
                    match file.records.get_mut(id) {
                        Some(r) if &r.pool == pool => {
                            if r.origin == origin && r.reason == reason {
                                continue;
                            }
                            // Same pool, new origin or reason: the entry is updated in place.
                            r.origin = origin.into();
                            r.reason = reason.into();
                            r.by = by.clone();
                            r.at_revision = revision;
                            file.log.push(log("update", pool));
                            changed.push(*id);
                            continue;
                        }
                        // Another pool: the move leaves it first, so history shows both.
                        Some(r) => {
                            let old = r.pool.clone();
                            file.log.push(log("remove", &old));
                        }
                        None => {}
                    }
                    file.pools.entry(pool.clone()).or_insert_with(|| Pool {
                        reason: reason.into(),
                        members: vec![],
                    });
                    file.records.insert(
                        *id,
                        Pooled {
                            pool: pool.clone(),
                            origin: origin.into(),
                            reason: reason.into(),
                            by: by.clone(),
                            at_revision: revision,
                        },
                    );
                    file.log.push(log("add", pool));
                }
                None => {
                    let Some(record) = file.records.remove(id) else {
                        if let Some((carrier, _)) = file.covering(&self.projects[p], *id) {
                            return Err(format!(
                                "Node {id} is pooled through chunk {carrier}; take the chunk out."
                            ));
                        }
                        continue;
                    };
                    file.log.push(LogEntry {
                        event: "remove".into(),
                        pool: record.pool,
                        node: Some(*id),
                        agent: String::new(),
                        reason: reason.into(),
                        by: by.clone(),
                        at_revision: revision,
                    });
                }
            }
            changed.push(*id);
        }
        if changed.is_empty() {
            return Ok(changed);
        }
        self.write_pools(p, &file)?;
        Ok(changed)
    }
    /// Adds `agent` to `pool`'s members (`join`) or removes it. Only registered agents join.
    pub fn pool_membership(
        &mut self,
        p: usize,
        pool: &str,
        agent: &str,
        join: bool,
    ) -> Result<bool> {
        if !self.writable(p) {
            return self.write_project(p, |l, p| l.pool_membership(p, pool, agent, join));
        }
        let pool = valid_name(pool)?;
        let agent = agent.trim();
        if join
            && !crate::agents::registered(&self.home())?
                .iter()
                .any(|a| a == agent)
        {
            return Err(format!(
                "agent_unregistered: {agent} is not a registered agent. Register it first: research agent register {agent}."
            ));
        }
        let mut file = self.pools_for_write(p)?;
        let Some(entry) = file.pools.get_mut(&pool) else {
            return Err(format!(
                "No pool named {pool} in this project (pool list shows them)."
            ));
        };
        let present = entry.members.iter().any(|m| m == agent);
        if present == join {
            return Ok(false);
        }
        if join {
            entry.members.push(agent.into());
            entry.members.sort();
        } else {
            entry.members.retain(|m| m != agent);
        }
        file.log.push(LogEntry {
            event: if join { "join" } else { "leave" }.into(),
            pool,
            node: None,
            agent: agent.into(),
            reason: String::new(),
            by: caller(),
            at_revision: self.projects[p].revision + 1,
        });
        self.write_pools(p, &file)?;
        Ok(true)
    }
    /// Moves every quarantine record into the pool `quarantine` and removes `quarantine.json`,
    /// in one transaction. Returns the records moved.
    pub fn migrate_quarantine(&mut self, p: usize) -> Result<Vec<u64>> {
        if !self.writable(p) {
            return self.write_project(p, |l, p| l.migrate_quarantine(p));
        }
        let Some(legacy_text) = self.text_opt(p, QUARANTINE)? else {
            return Ok(vec![]);
        };
        let legacy = legacy_records(&legacy_text).map_err(|e| {
            format!("{QUARANTINE} cannot be read ({e}); repair it before migrating.")
        })?;
        let mut file = self.pools_checked(p).map_err(|e| unreadable(&e))?;
        let revision = self.projects[p].revision + 1;
        let mut moved = vec![];
        for (id, q) in legacy {
            if !self.projects[p].nodes.iter().any(|n| n.id == id) {
                continue;
            }
            let origin = if q.origin == "cyber-stop" {
                "safety-stop"
            } else {
                "human"
            };
            file.pools
                .entry("quarantine".into())
                .or_insert_with(|| Pool {
                    reason: "Migrated from quarantine".into(),
                    members: vec![],
                });
            file.records.insert(
                id,
                Pooled {
                    pool: "quarantine".into(),
                    origin: origin.into(),
                    reason: q.reason.clone(),
                    by: q.by.clone(),
                    at_revision: revision,
                },
            );
            file.log.push(LogEntry {
                event: "migrate".into(),
                pool: "quarantine".into(),
                node: Some(id),
                agent: String::new(),
                reason: q.reason,
                by: caller(),
                at_revision: revision,
            });
            moved.push(id);
        }
        let pools_before = self.pools_before_text(p)?;
        let before = self.projects[p].clone();
        let mut after = before.clone();
        after.revision += 1;
        let journal = Journal {
            before,
            after,
            edits: vec![],
            moves: vec![],
            dependencies: vec![],
            sidecars: vec![
                Sidecar {
                    path: POOLS.into(),
                    before: pools_before,
                    after: render(&file)?,
                },
                // An empty quarantine file holds nothing; the next tidy leaves it alone.
                Sidecar {
                    path: QUARANTINE.into(),
                    before: legacy_text,
                    after: String::new(),
                },
            ],
            ledger: false,
            repin_ledger: false,
            directed_by: None,
            repin_ids: vec![],
        };
        self.commit_or_stage(p, journal)?;
        Ok(moved)
    }
    /// The pools file's current text, creating an empty one first so a transaction has a
    /// `before` to recover to.
    fn pools_before_text(&self, p: usize) -> Result<String> {
        Ok(match self.text_opt(p, POOLS)? {
            Some(text) => text,
            None => {
                let empty = render(&parse("")?)?;
                atomic(&self.dir(p).join(POOLS), empty.as_bytes())?;
                empty
            }
        })
    }
    fn write_pools(&mut self, p: usize, file: &PoolFile) -> Result<()> {
        let before_text = self.pools_before_text(p)?;
        let before = self.projects[p].clone();
        let mut after = before.clone();
        after.revision += 1;
        let journal = Journal {
            before,
            after,
            edits: vec![],
            moves: vec![],
            dependencies: vec![],
            sidecars: vec![Sidecar {
                path: POOLS.into(),
                before: before_text,
                after: render(file)?,
            }],
            ledger: false,
            repin_ledger: false,
            directed_by: None,
            repin_ids: vec![],
        };
        self.commit_or_stage(p, journal).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pooled_chunk_covers_its_functions_and_membership_opens_it() {
        let node = |id, parent, kind| Node {
            context_only: false,
            id,
            parent,
            kind,
            name: format!("N{id}"),
            status: Status::NotStarted,
            rva: None,
            revision: 0,
            accepted_hash: None,
            accepted_prerequisites: Default::default(),
        };
        let mut project: Project = serde_json::from_value(serde_json::json!({
            "id": "q", "name": "Q", "binary": "b", "next_id": 5, "revision": 1, "nodes": []
        }))
        .unwrap();
        project.nodes = vec![
            node(0, None, Kind::App),
            node(1, Some(0), Kind::Architecture),
            node(2, Some(1), Kind::Chunk),
            node(3, Some(2), Kind::Function),
            node(4, Some(1), Kind::Chunk),
        ];
        let mut file = parse("").unwrap();
        file.pools.insert(
            "voice".into(),
            Pool {
                reason: "Voice code".into(),
                members: vec!["astra".into()],
            },
        );
        file.records.insert(
            2,
            Pooled {
                pool: "voice".into(),
                origin: "human".into(),
                reason: "Voice code only".into(),
                by: String::new(),
                at_revision: 1,
            },
        );
        let pooled: HashSet<u64> = file.effective(&project).into_keys().collect();
        assert_eq!(pooled, [2, 3].into());
        assert_eq!(file.closed_to(&project, "sol"), [2, 3].into());
        assert!(file.closed_to(&project, "astra").is_empty());
        assert!(parse(r#"{"schema":2}"#).is_err());
        assert!(legacy_records("").unwrap().is_empty());
    }
}
