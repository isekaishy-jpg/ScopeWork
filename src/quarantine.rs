//! Quarantined records (docs/proposal-quarantine.md). A chunk or function the human has taken
//! out of research counts as complete everywhere, never appears as open work, and is sealed
//! against structural changes unless a request carries the human's direction. A quarantined
//! chunk covers its functions. Report text stays editable and never reopens the record.
//!
//! The records live in a journaled sidecar beside the catalog, like review flags, together with
//! a log of lifted quarantines, so older Scopework versions leave them intact.
use super::catalog_batch::{Journal, REVIEW_FLAGS, Sidecar};
use super::*;
use std::collections::{BTreeMap, HashMap, HashSet};

pub const QUARANTINE: &str = "quarantine.json";
/// Why a record can be quarantined: the human asked for it, or Claude issued a cyber
/// (safety) stop while researching it.
pub const ORIGINS: [&str; 2] = ["human", "cyber-stop"];

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub struct Quarantine {
    pub origin: String,
    pub reason: String,
    pub at_revision: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub by: String,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub struct Lifted {
    pub node: u64,
    pub record: Quarantine,
    pub reason: String,
    pub at_revision: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub by: String,
}
#[derive(Default, Serialize, Deserialize)]
struct QuarantineFile {
    schema: u32,
    #[serde(default)]
    records: BTreeMap<u64, Quarantine>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    lifted: Vec<Lifted>,
}

fn parse(text: &str) -> Result<QuarantineFile> {
    if text.trim().is_empty() {
        return Ok(QuarantineFile {
            schema: 1,
            ..Default::default()
        });
    }
    let file: QuarantineFile = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if file.schema != 1 {
        return Err("Unsupported quarantine file".into());
    }
    Ok(file)
}
fn render(file: &QuarantineFile) -> Result<String> {
    serde_json::to_string_pretty(file).map_err(|e| e.to_string())
}

/// The quarantine file without records of `removed` nodes (their lifts stay in the log), or
/// `None` when no record changes.
pub(crate) fn without_removed(text: &str, removed: &HashSet<u64>) -> Result<Option<String>> {
    let mut file = parse(text)?;
    let count = file.records.len();
    file.records.retain(|id, _| !removed.contains(id));
    if file.records.len() == count {
        return Ok(None);
    }
    render(&file).map(Some)
}
/// Recorded quarantines read from a project folder (for snapshots built outside a library).
pub(super) fn read_records(dir: &Path) -> BTreeMap<u64, Quarantine> {
    fs::read_to_string(dir.join(QUARANTINE))
        .ok()
        .and_then(|text| parse(&text).ok())
        .map(|file| file.records)
        .unwrap_or_default()
}
/// Each quarantined node with a short description (inherited ones name their chunk).
pub(super) fn descriptions(
    project: &Project,
    records: &BTreeMap<u64, Quarantine>,
) -> HashMap<u64, String> {
    let mut out: HashMap<u64, String> = records
        .iter()
        .map(|(id, q)| (*id, format!("{}: {}", q.origin, q.reason)))
        .collect();
    for n in &project.nodes {
        if let Some(parent) = n.parent
            && let Some(q) = records.get(&parent)
        {
            out.entry(n.id)
                .or_insert_with(|| format!("through chunk {parent} ({}: {})", q.origin, q.reason));
        }
    }
    out
}
/// Every quarantined node: the recorded ones and every child of a recorded chunk.
pub(super) fn effective(project: &Project, records: &BTreeMap<u64, Quarantine>) -> HashSet<u64> {
    let mut out: HashSet<u64> = records.keys().copied().collect();
    for n in &project.nodes {
        if let Some(parent) = n.parent
            && records.contains_key(&parent)
        {
            out.insert(n.id);
        }
    }
    out
}

impl Library {
    /// Recorded quarantines by node.
    pub fn quarantines(&self, p: usize) -> Result<BTreeMap<u64, Quarantine>> {
        Ok(match self.text_opt(p, QUARANTINE)? {
            Some(text) => parse(&text)?.records,
            None => BTreeMap::new(),
        })
    }
    /// Quarantined node IDs, recorded and inherited from a quarantined chunk. An unreadable
    /// file counts as no quarantine, so reads keep working.
    pub fn quarantined(&self, p: usize) -> HashSet<u64> {
        effective(&self.projects[p], &self.quarantines(p).unwrap_or_default())
    }
    /// The quarantine covering `id` and the node that carries it (itself or its chunk).
    pub fn quarantine_of(&self, p: usize, id: u64) -> Option<(u64, Quarantine)> {
        let records = self.quarantines(p).ok()?;
        if let Some(q) = records.get(&id) {
            return Some((id, q.clone()));
        }
        let parent = self.projects[p].nodes.iter().find(|n| n.id == id)?.parent?;
        records.get(&parent).map(|q| (parent, q.clone()))
    }
    /// Refuses a change touching quarantined records unless the human directed it.
    pub(super) fn check_seal(
        &self,
        p: usize,
        touched: impl IntoIterator<Item = u64>,
        directed_by: Option<&str>,
        what: &str,
    ) -> Result<()> {
        if directed_by.is_some_and(|d| !d.trim().is_empty()) {
            return Ok(());
        }
        // Read once for every record checked (an unreadable file counts as no quarantine).
        let records = self.quarantines(p).unwrap_or_default();
        if records.is_empty() {
            return Ok(());
        }
        let parents: HashMap<u64, Option<u64>> = self.projects[p]
            .nodes
            .iter()
            .map(|n| (n.id, n.parent))
            .collect();
        for id in touched {
            let carrier = if records.contains_key(&id) {
                Some(id)
            } else {
                parents
                    .get(&id)
                    .copied()
                    .flatten()
                    .filter(|parent| records.contains_key(parent))
            };
            if let Some((carrier, q)) = carrier.map(|c| (c, &records[&c])) {
                let through = if carrier == id {
                    String::new()
                } else {
                    format!(" through chunk {carrier}")
                };
                return Err(format!(
                    "quarantined: node {id} is quarantined{through} ({}: {}). {what} it needs --directed-by with the human's instruction.",
                    q.origin, q.reason
                ));
            }
        }
        Ok(())
    }
    /// Refuses an operation that makes no sense on a quarantined record (completion, reopening,
    /// flags, claims), whoever asks.
    pub(super) fn refuse_quarantined(&self, p: usize, id: u64, what: &str) -> Result<()> {
        match self.quarantine_of(p, id) {
            Some((carrier, q)) => Err(format!(
                "quarantined: node {id} is quarantined{} ({}: {}) and already counts as complete; {what} does not apply.",
                if carrier == id {
                    String::new()
                } else {
                    format!(" through chunk {carrier}")
                },
                q.origin,
                q.reason
            )),
            None => Ok(()),
        }
    }
    /// Quarantines `id` (with `origin`) or lifts its quarantine (`origin` None). Setting clears
    /// the record's review flag, claim and review-queue entries in the same transaction; lifting
    /// is logged with its reason. Returns false when nothing changed.
    pub fn set_quarantine(
        &mut self,
        p: usize,
        id: u64,
        origin: Option<&str>,
        reason: &str,
        by: &str,
    ) -> Result<bool> {
        if !self.writable(p) {
            return self.write_project(p, |l, p| l.set_quarantine(p, id, origin, reason, by));
        }
        let node = self.projects[p]
            .nodes
            .iter()
            .find(|n| n.id == id)
            .ok_or("Unknown node")?;
        let kind = node.kind;
        if !matches!(kind, Kind::Chunk | Kind::Function) {
            return Err("Only chunks and functions can be quarantined.".into());
        }
        let reason = reason.trim();
        if reason.is_empty() {
            return Err("A quarantine change needs a reason.".into());
        }
        let before_text = match self.text_opt(p, QUARANTINE)? {
            Some(text) => text,
            None => {
                // An empty file first, so the transaction has a `before` to recover to.
                let empty = render(&parse("")?)?;
                atomic(&self.dir(p).join(QUARANTINE), empty.as_bytes())?;
                empty
            }
        };
        let mut file = parse(&before_text)?;
        let revision = self.projects[p].revision + 1;
        match origin {
            Some(origin) => {
                if !ORIGINS.contains(&origin) {
                    return Err("A quarantine's origin is human or cyber-stop.".into());
                }
                let record = Quarantine {
                    origin: origin.into(),
                    reason: reason.into(),
                    at_revision: revision,
                    by: by.into(),
                };
                if file
                    .records
                    .get(&id)
                    .is_some_and(|q| q.origin == record.origin && q.reason == record.reason)
                {
                    return Ok(false);
                }
                file.records.insert(id, record);
            }
            None => {
                let Some(record) = file.records.remove(&id) else {
                    return Err(match self.quarantine_of(p, id) {
                        Some((carrier, _)) => format!(
                            "Node {id} is quarantined through chunk {carrier}; lift the chunk's quarantine."
                        ),
                        None => format!("Node {id} is not quarantined."),
                    });
                };
                file.lifted.push(Lifted {
                    node: id,
                    record,
                    reason: reason.into(),
                    at_revision: revision,
                    by: by.into(),
                });
            }
        }
        let mut sidecars = vec![Sidecar {
            path: QUARANTINE.into(),
            before: before_text,
            after: render(&file)?,
        }];
        if origin.is_some() {
            // A quarantined record's flag, claim and queue entries no longer mean anything.
            let mut covered: HashSet<u64> = [id].into();
            if kind == Kind::Chunk {
                covered.extend(
                    self.projects[p]
                        .nodes
                        .iter()
                        .filter(|n| n.parent == Some(id))
                        .map(|n| n.id),
                );
            }
            for (name, field) in [
                (REVIEW_FLAGS, "flags"),
                (coordination::CLAIMS, "claims"),
                (coordination::REVIEW_QUEUE, "entries"),
            ] {
                if let Some(text) = self.text_opt(p, name)? {
                    let after = coordination::without_nodes(&text, field, &covered)?;
                    if after != text {
                        sidecars.push(Sidecar {
                            path: name.into(),
                            before: text,
                            after,
                        });
                    }
                }
            }
        }
        let before = self.projects[p].clone();
        let mut after = before.clone();
        after.revision += 1;
        let journal = Journal {
            before,
            after,
            edits: vec![],
            moves: vec![],
            dependencies: vec![],
            sidecars,
            ledger: false,
            repin_ledger: false,
            directed_by: None,
            repin_ids: vec![],
        };
        self.commit_or_stage(p, journal).map(|_| true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quarantined_chunk_covers_its_functions() {
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
        let records: BTreeMap<u64, Quarantine> = [(
            2,
            Quarantine {
                origin: "human".into(),
                reason: "Voice code only".into(),
                at_revision: 1,
                by: String::new(),
            },
        )]
        .into();
        let set = effective(&project, &records);
        assert_eq!(set, [2, 3].into());
        assert!(parse("").unwrap().records.is_empty());
        assert!(parse(r#"{"schema":2}"#).is_err());
    }
}
