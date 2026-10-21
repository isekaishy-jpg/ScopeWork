//! One readable project history: catalog transaction receipts plus an append-only tooling log.
//!
//! Every catalog change already leaves a change receipt (`revision-N/journal.json`, a delta of
//! changed records, report edits, moves and sidecars). `history` summarises those receipts
//! newest first. Project tooling records its own changes (annotation applies, generated
//! artifacts, superseded sections) as diff entries in `tools/workspaces/history/tooling.jsonl`
//! through `append_history`, instead of copying whole files per operation. Restore stays manual.
use super::*;
use serde_json::{Value, json};

pub const TOOLING_LOG: &str = "tools/workspaces/history/tooling.jsonl";
const RECEIPTS: [&str; 2] = [
    "tools/workspaces/catalog-transactions-v4",
    "tools/workspaces/catalog-transactions",
];

#[derive(Default)]
pub struct HistoryFilter {
    pub node: Option<u64>,
    pub since: Option<u64>,
    pub until: Option<u64>,
    pub limit: usize,
    pub catalog: bool,
    pub tooling: bool,
    /// Include the changed records themselves, not only a summary.
    pub detail: bool,
    pub kind: Option<String>,
}

fn receipts(root: &Path) -> Vec<(u64, PathBuf)> {
    let mut all = vec![];
    for folder in RECEIPTS {
        let Ok(entries) = fs::read_dir(root.join(folder)) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(rev) = name
                .strip_prefix("revision-")
                .and_then(|r| r.parse::<u64>().ok())
            {
                let journal = entry.path().join("journal.json");
                if journal.is_file() {
                    all.push((rev, journal));
                }
            }
        }
    }
    all.sort_by_key(|(rev, _)| std::cmp::Reverse(*rev));
    all.dedup_by_key(|(rev, _)| *rev);
    all
}

/// The parts of a receipt a summary reads. Unlisted fields, such as report and sidecar texts
/// that can reach many megabytes, are skipped by the parser instead of being built as values.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Receipt {
    changes: Value,
    edits: Vec<EditRef>,
    moves: Vec<Value>,
    sidecars: Vec<PathRef>,
    dependencies: Vec<Value>,
    directed_by: Option<String>,
}
#[derive(Deserialize)]
struct EditRef {
    id: u64,
    old: Option<String>,
    new: String,
}
#[derive(Deserialize)]
struct PathRef {
    path: String,
}
impl Receipt {
    fn summary_value(self) -> Value {
        json!({
            "changes": self.changes,
            "edits": self.edits.into_iter().map(|e| json!({"id": e.id, "old": e.old, "new": e.new})).collect::<Vec<_>>(),
            "moves": self.moves,
            "sidecars": self.sidecars.into_iter().map(|s| json!({"path": s.path})).collect::<Vec<_>>(),
            "dependencies": self.dependencies,
            "directed_by": self.directed_by,
        })
    }
}

/// Summary of one receipt: which records changed and how, without report text.
fn summarize(revision: u64, receipt: &Value, filter: &HistoryFilter) -> Option<Value> {
    let empty = vec![];
    let before = receipt["changes"]["nodes"]["before"]
        .as_array()
        .unwrap_or(&empty);
    let after = receipt["changes"]["nodes"]["after"]
        .as_array()
        .unwrap_or(&empty);
    let mut nodes: std::collections::BTreeMap<u64, (Option<&Value>, Option<&Value>)> =
        Default::default();
    for r in before {
        if let Some(id) = r["record"]["id"].as_u64() {
            nodes.entry(id).or_default().0 = Some(&r["record"]);
        }
    }
    for r in after {
        if let Some(id) = r["record"]["id"].as_u64() {
            nodes.entry(id).or_default().1 = Some(&r["record"]);
        }
    }
    let edits = receipt["edits"].as_array().unwrap_or(&empty);
    let moves = receipt["moves"].as_array().unwrap_or(&empty);
    if let Some(node) = filter.node {
        let touched = nodes.contains_key(&node)
            || edits.iter().any(|e| e["id"].as_u64() == Some(node))
            || moves.iter().any(|m| m["id"].as_u64() == Some(node));
        if !touched {
            return None;
        }
    }
    let mut changes = vec![];
    for (id, (old, new)) in &nodes {
        if filter.node.is_some_and(|n| n != *id) {
            continue;
        }
        let field = |r: Option<&Value>, f: &str| r.map(|r| r[f].clone()).unwrap_or(Value::Null);
        let mut change = json!({"id": id});
        match (old, new) {
            (None, Some(_)) => change["created"] = true.into(),
            (Some(_), None) => change["removed"] = true.into(),
            _ => {}
        }
        for f in ["status", "parent", "name", "context_only"] {
            let (a, b) = (field(*old, f), field(*new, f));
            if old.is_some() && new.is_some() && a != b {
                change[f] = json!({"before": a, "after": b});
            }
        }
        if old.is_some_and(|r| !r["accepted_hash"].is_null())
            && new.is_some_and(|r| r["accepted_hash"].is_null())
        {
            change["reopened"] = true.into();
        }
        if filter.detail {
            change["before"] = old.cloned().unwrap_or(Value::Null);
            change["after"] = new.cloned().unwrap_or(Value::Null);
        }
        changes.push(change);
    }
    let edited: Vec<Value> = edits
        .iter()
        .filter(|e| filter.node.is_none_or(|n| e["id"].as_u64() == Some(n)))
        .map(|e| json!({"id": e["id"], "report": e["new"], "moved_from": if e["old"] != e["new"] { e["old"].clone() } else { Value::Null }}))
        .collect();
    let moved: Vec<Value> = moves
        .iter()
        .filter(|m| filter.node.is_none_or(|n| m["id"].as_u64() == Some(n)))
        .map(|m| json!({"id": m["id"], "from": m["old"], "to": m["new"]}))
        .collect();
    let sidecars: Vec<Value> = receipt["sidecars"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .map(|s| s["path"].clone())
        .collect();
    let mut entry = json!({
        "kind": "catalog",
        "revision": revision,
        "records_changed": nodes.len(),
        "changes": changes,
        "reports_edited": edited,
        "moves": moved,
        "sidecars": sidecars,
        "dependencies_resynced": receipt["dependencies"].as_array().map_or(0, Vec::len),
    });
    // A change to quarantined records carries the human's instruction that allowed it.
    if let Some(directed) = receipt["directed_by"].as_str() {
        entry["directed_by"] = directed.into();
    }
    Some(entry)
}

impl Library {
    /// History entries newest first, from catalog receipts and the tooling log.
    pub fn history(&self, p: usize, filter: &HistoryFilter) -> Result<Value> {
        let root = self.dir(p);
        let limit = if filter.limit == 0 { 50 } else { filter.limit };
        let in_range = |rev: u64| {
            filter.since.is_none_or(|s| rev >= s) && filter.until.is_none_or(|u| rev <= u)
        };
        let mut entries: Vec<Value> = vec![];
        if filter.tooling
            && let Ok(text) = fs::read_to_string(root.join(TOOLING_LOG))
        {
            for line in text.lines().rev() {
                let Ok(entry) = serde_json::from_str::<Value>(line) else {
                    continue;
                };
                let rev = entry["revision"].as_u64().unwrap_or(0);
                let node_ok = filter.node.is_none_or(|n| {
                    entry["nodes"]
                        .as_array()
                        .is_some_and(|a| a.iter().any(|v| v.as_u64() == Some(n)))
                });
                let kind_ok = filter
                    .kind
                    .as_deref()
                    .is_none_or(|k| entry["entry_kind"].as_str() == Some(k));
                if in_range(rev) && node_ok && kind_ok {
                    entries.push(entry);
                }
            }
        }
        if filter.catalog && filter.kind.as_deref().is_none_or(|k| k == "catalog") {
            let mut found = 0usize;
            for (rev, path) in receipts(&root) {
                if !in_range(rev) {
                    continue;
                }
                if found >= limit {
                    break;
                }
                let Ok(bytes) = fs::read(&path) else { continue };
                // Report and sidecar texts are skipped while parsing; summaries never show them.
                let Ok(receipt) = serde_json::from_slice::<Receipt>(&bytes) else {
                    continue;
                };
                let receipt = receipt.summary_value();
                if let Some(summary) = summarize(rev, &receipt, filter) {
                    entries.push(summary);
                    found += 1;
                }
            }
        }
        // Newest first; tooling entries record the catalog revision current when they were made.
        entries.sort_by_key(|e| {
            std::cmp::Reverse((
                e["revision"].as_u64().unwrap_or(0),
                e["at_unix"].as_u64().unwrap_or(u64::MAX),
            ))
        });
        entries.truncate(limit);
        Ok(
            json!({"project": self.projects[p].id, "revision": self.projects[p].revision, "entries": entries}),
        )
    }

    /// Appends one tooling entry. `diff` carries what changed (for example ledger rows before and
    /// after); whole-file copies are what this log replaces. Appends are serialised by the
    /// project's exclusive lock.
    pub fn append_history(
        &mut self,
        p: usize,
        entry_kind: &str,
        summary: &str,
        nodes: &[u64],
        refs: &[String],
        diff: Option<Value>,
    ) -> Result<Value> {
        if !self.writable(p) {
            return self.write_project(p, |l, p| {
                l.append_history(p, entry_kind, summary, nodes, refs, diff.clone())
            });
        }
        let entry_kind = entry_kind.trim();
        if entry_kind.is_empty()
            || !entry_kind
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(
                "History kinds are lowercase words joined by hyphens, such as ledger-change."
                    .into(),
            );
        }
        if entry_kind == "catalog" {
            return Err("The catalog kind is reserved for catalog transaction receipts.".into());
        }
        if summary.trim().is_empty() {
            return Err("A history entry needs a summary.".into());
        }
        let known: std::collections::HashSet<u64> =
            self.projects[p].nodes.iter().map(|n| n.id).collect();
        if let Some(bad) = nodes.iter().find(|id| !known.contains(id)) {
            return Err(format!("Unknown node {bad} in history entry"));
        }
        let at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let entry = json!({
            "kind": "tooling",
            "entry_kind": entry_kind,
            "revision": self.projects[p].revision,
            "at_unix": at,
            "summary": summary.trim(),
            "nodes": nodes,
            "refs": refs,
            "diff": diff.unwrap_or(Value::Null),
        });
        let path = self.dir(p).join(TOOLING_LOG);
        fs::create_dir_all(path.parent().ok_or("Invalid history path")?)
            .map_err(|e| e.to_string())?;
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| e.to_string())?;
        let mut line = serde_json::to_vec(&entry).map_err(|e| e.to_string())?;
        line.push(b'\n');
        file.write_all(&line).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        Ok(entry)
    }
}
