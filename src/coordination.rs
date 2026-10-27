//! Work claims and the review queue: coordination state for several agents on one project.
//!
//! Both live in journaled sidecars beside the catalog (like review flags), so older Scopework
//! versions leave them intact. A claim names who is working on a branch (an architecture or
//! chunk) and may carry a lease. The review queue records completed records that a structural
//! change reopened; an entry is open until its record is complete again, so completion needs
//! no extra bookkeeping.
use super::*;
use std::collections::BTreeMap;

pub const CLAIMS: &str = "claims.json";
pub const REVIEW_QUEUE: &str = "review-queue.json";

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub struct Claim {
    pub owner: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    pub claimed_at_revision: u64,
    /// Unix seconds after which the claim no longer counts; `None` keeps it until released.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_unix: Option<u64>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub struct QueueEntry {
    pub reason: String,
    pub revision: u64,
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
fn read_map<T: serde::de::DeserializeOwned>(text: &str, field: &str) -> Result<BTreeMap<u64, T>> {
    if text.trim().is_empty() {
        return Ok(BTreeMap::new());
    }
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if value["schema"] != 1 {
        return Err(format!("Unsupported {field} file"));
    }
    serde_json::from_value(value[field].clone()).map_err(|e| e.to_string())
}
fn write_map<T: Serialize>(field: &str, map: &BTreeMap<u64, T>) -> Result<String> {
    serde_json::to_string_pretty(&serde_json::json!({"schema":1, field: map}))
        .map_err(|e| e.to_string())
}
/// Current text of a sidecar, or the text `ensure` would create for it; never writes, so a
/// preview stays free of side effects. Call `ensure` before journaling a change to it.
pub(super) fn current_or_empty(root: &Path, name: &str, field: &str) -> Result<String> {
    match fs::read_to_string(root.join(name)) {
        Ok(text) => Ok(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            write_map::<serde_json::Value>(field, &BTreeMap::new())
        }
        Err(e) => Err(e.to_string()),
    }
}
/// Current text of a sidecar, creating an empty one first so its transaction has a `before`.
pub(super) fn ensure(root: &Path, name: &str, field: &str) -> Result<String> {
    let path = root.join(name);
    if !path.is_file() {
        atomic(
            &path,
            write_map::<serde_json::Value>(field, &BTreeMap::new())?.as_bytes(),
        )?;
    }
    fs::read_to_string(&path).map_err(|e| e.to_string())
}
/// Why a batch reopened records: the moved nodes it names, or a structural change.
pub(super) fn batch_reason(moved: &[u64]) -> String {
    let shown: Vec<String> = moved.iter().take(12).map(u64::to_string).collect();
    if moved.is_empty() {
        "Reopened by a membership, order or name change".to_string()
    } else {
        format!(
            "Reopened by moving node(s) {}{}",
            shown.join(", "),
            if moved.len() > shown.len() {
                format!(" and {} more", moved.len() - shown.len())
            } else {
                String::new()
            }
        )
    }
}
/// Adds review-queue entries for `reopened` records, and drops entries whose record is
/// complete or no longer exists.
pub(super) fn queue_reopened(
    original: &str,
    after: &Project,
    reopened: &[u64],
    reason: &str,
) -> Result<String> {
    let mut queue: BTreeMap<u64, Vec<QueueEntry>> = read_map(original, "entries")?;
    let open: std::collections::HashSet<u64> = after
        .nodes
        .iter()
        .filter(|n| n.status != Status::Complete)
        .map(|n| n.id)
        .collect();
    queue.retain(|id, _| open.contains(id));
    for id in reopened {
        queue.entry(*id).or_default().push(QueueEntry {
            reason: reason.into(),
            revision: after.revision,
        });
    }
    write_map("entries", &queue)
}
/// The sidecar text without entries for `removed` nodes.
pub(super) fn without_nodes(
    original: &str,
    field: &str,
    removed: &std::collections::HashSet<u64>,
) -> Result<String> {
    let mut map: BTreeMap<u64, serde_json::Value> = read_map(original, field)?;
    let count = map.len();
    map.retain(|id, _| !removed.contains(id));
    if map.len() == count {
        return Ok(original.into());
    }
    write_map(field, &map)
}

/// Queue entries whose record is not complete (by the catalog's recorded status), latest reason each.
pub(super) fn open_queue(dir: &Path, project: &Project) -> std::collections::HashMap<u64, String> {
    let text = fs::read_to_string(dir.join(REVIEW_QUEUE)).unwrap_or_default();
    let complete: std::collections::HashSet<u64> = project
        .nodes
        .iter()
        .filter(|n| n.status == Status::Complete)
        .map(|n| n.id)
        .collect();
    read_map::<Vec<QueueEntry>>(&text, "entries")
        .unwrap_or_default()
        .into_iter()
        .filter(|(id, _)| !complete.contains(id))
        .filter_map(|(id, entries)| entries.last().map(|e| (id, e.reason.clone())))
        .collect()
}
impl Library {
    /// Claims that are currently in force (unexpired), by node.
    pub fn claims(&self, p: usize) -> Result<BTreeMap<u64, Claim>> {
        let text = self.text_opt(p, CLAIMS)?.unwrap_or_default();
        let now = now_unix();
        Ok(read_map::<Claim>(&text, "claims")?
            .into_iter()
            .filter(|(_, c)| c.expires_unix.is_none_or(|t| t > now))
            .collect())
    }
    /// Claim a branch for `owner`, replacing that owner's previous claim on it. Another
    /// owner's unexpired claim on the same node is refused unless `force` is set.
    pub fn claim(
        &mut self,
        p: usize,
        id: u64,
        owner: &str,
        note: &str,
        lease_hours: Option<u64>,
        force: bool,
    ) -> Result<Claim> {
        if !self.writable(p) {
            return self.write_project(p, |l, p| l.claim(p, id, owner, note, lease_hours, force));
        }
        let node = self.projects[p]
            .nodes
            .iter()
            .find(|n| n.id == id)
            .ok_or("Unknown node")?;
        if !matches!(node.kind, Kind::Architecture | Kind::Chunk) {
            return Err("Claims apply to architectures and chunks.".into());
        }
        self.check_pool(p, [id], None, "Claiming")?;
        let owner = owner.trim();
        if owner.is_empty() {
            return Err("A claim needs an owner.".into());
        }
        let mut all: BTreeMap<u64, Claim> = read_map(&self.claims_text(p)?, "claims")?;
        let now = now_unix();
        if let Some(existing) = all.get(&id)
            && existing.owner != owner
            && existing.expires_unix.is_none_or(|t| t > now)
            && !force
        {
            return Err(format!(
                "claim_conflict: node {id} is claimed by {}{}",
                existing.owner,
                existing
                    .expires_unix
                    .map(|t| format!(" until unix {t}"))
                    .unwrap_or_default()
            ));
        }
        let expires_unix = match lease_hours {
            None => None,
            Some(hours) => Some(
                hours
                    .checked_mul(3600)
                    .and_then(|seconds| now.checked_add(seconds))
                    .ok_or("A claim lease of that many hours cannot be represented.")?,
            ),
        };
        let claim = Claim {
            owner: owner.into(),
            note: note.trim().into(),
            claimed_at_revision: self.projects[p].revision + 1,
            expires_unix,
        };
        all.insert(id, claim.clone());
        self.write_sidecar(p, CLAIMS, write_map("claims", &all)?)?;
        Ok(claim)
    }
    /// Release `owner`'s claim on a node (any owner with `force`).
    pub fn release_claim(&mut self, p: usize, id: u64, owner: &str, force: bool) -> Result<bool> {
        if !self.writable(p) {
            return self.write_project(p, |l, p| l.release_claim(p, id, owner, force));
        }
        let mut all: BTreeMap<u64, Claim> = read_map(&self.claims_text(p)?, "claims")?;
        match all.get(&id) {
            None => return Ok(false),
            Some(c) if c.owner != owner.trim() && !force => {
                return Err(format!(
                    "claim_conflict: node {id} is claimed by {}",
                    c.owner
                ));
            }
            _ => {}
        }
        all.remove(&id);
        self.write_sidecar(p, CLAIMS, write_map("claims", &all)?)?;
        Ok(true)
    }
    /// The claims sidecar as this view sees it (including claims staged earlier in a merged
    /// commit), created empty first when it does not exist yet.
    fn claims_text(&self, p: usize) -> Result<String> {
        match self.text_opt(p, CLAIMS)? {
            Some(text) => Ok(text),
            None => ensure(&self.dir(p), CLAIMS, "claims"),
        }
    }
    /// Open review-queue entries: records reopened by structural changes and not complete again.
    pub fn review_queue(
        &self,
        p: usize,
        states: &std::collections::HashMap<u64, Status>,
    ) -> Result<BTreeMap<u64, Vec<QueueEntry>>> {
        let text = self.text_opt(p, REVIEW_QUEUE)?.unwrap_or_default();
        Ok(read_map::<Vec<QueueEntry>>(&text, "entries")?
            .into_iter()
            .filter(|(id, _)| states.get(id).is_some_and(|s| *s != Status::Complete))
            .collect())
    }
    /// Every node covered by a claim (its own or its nearest claimed ancestor's), computed once.
    pub fn claim_index(
        &self,
        p: usize,
        claims: &BTreeMap<u64, Claim>,
    ) -> std::collections::HashMap<u64, (u64, String)> {
        let mut covered = std::collections::HashMap::new();
        if claims.is_empty() {
            return covered;
        }
        let mut children: std::collections::HashMap<u64, Vec<u64>> = Default::default();
        for n in &self.projects[p].nodes {
            if let Some(parent) = n.parent {
                children.entry(parent).or_default().push(n.id);
            }
        }
        // Visit claimed nodes from the root down so a nearer claim overrides an outer one.
        let mut stack: Vec<(u64, Option<(u64, String)>)> = vec![(0, None)];
        while let Some((id, inherited)) = stack.pop() {
            let current = claims.get(&id).map(|c| (id, c.owner.clone())).or(inherited);
            if let Some(claim) = &current {
                covered.insert(id, claim.clone());
            }
            for kid in children.get(&id).into_iter().flatten() {
                stack.push((*kid, current.clone()));
            }
        }
        covered
    }
}
