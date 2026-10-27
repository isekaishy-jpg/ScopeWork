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
    /// Only entries about the function with this RVA (resolved to its node).
    pub rva: Option<u32>,
    /// Only entries at or after this Unix time.
    pub from: Option<u64>,
    /// Only entries before this Unix time.
    pub to: Option<u64>,
}

/// A tooling kind the app knows: what it records and what an entry must name.
pub struct KnownKind {
    pub name: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub needs_node: bool,
    pub needs_ref: bool,
}
const fn kind(
    name: &'static str,
    label: &'static str,
    description: &'static str,
    needs_node: bool,
    needs_ref: bool,
) -> KnownKind {
    KnownKind {
        name,
        label,
        description,
        needs_node,
        needs_ref,
    }
}
/// The first-class tooling kinds. Other kinds may still be appended.
pub const KINDS: &[KnownKind] = &[
    kind(
        "reviewed-type",
        "Reviewed type",
        "A struct or enum definition reviewed into the annotation ledger; refs name the type.",
        false,
        true,
    ),
    kind(
        "typed-signature",
        "Typed signature",
        "A function signature typed from research; nodes name the functions.",
        true,
        false,
    ),
    kind(
        "behaviour-artifact-review",
        "Behaviour artifact review",
        "A behaviour artifact (1:1 Rust or Lua reconstruction) reviewed against its records; nodes name them, refs the artifact.",
        true,
        true,
    ),
    kind(
        "dependency-list",
        "Dependency list",
        "The project's dependency list changed; refs name the list file.",
        false,
        true,
    ),
    kind(
        "ledger-change",
        "Ledger change",
        "Annotation ledger rows changed.",
        false,
        false,
    ),
    kind(
        "ghidra-apply",
        "Ghidra apply",
        "The annotation ledger was applied to the analysis project.",
        false,
        false,
    ),
    kind(
        "generated-artifact",
        "Generated artifact",
        "A generated file was written or replaced.",
        false,
        false,
    ),
    kind(
        "tooling-upgrade",
        "Tooling upgrade",
        "Project tooling adopted a new version or behaviour.",
        false,
        false,
    ),
    kind(
        "coordinator-commits",
        "Coordinator commits",
        "Daily commit figures of the coordinator (written by the app).",
        false,
        false,
    ),
    kind(
        "coordinator-reads",
        "Coordinator reads",
        "Daily read figures of the coordinator (written by the app).",
        false,
        false,
    ),
    kind(
        "drift",
        "Drift",
        "A coordinated file changed outside the coordinator (written by the app).",
        false,
        false,
    ),
    kind(
        "coordinator-leases",
        "Coordinator leases",
        "Daily tool-lease figures of the coordinator (written by the app).",
        false,
        false,
    ),
];

/// The known kinds as the CLI lists them.
pub fn kinds_value() -> Value {
    json!({"kinds": KINDS.iter().map(|k| json!({
        "kind": k.name,
        "label": k.label,
        "description": k.description,
        "needs_node": k.needs_node,
        "needs_ref": k.needs_ref,
    })).collect::<Vec<_>>()})
}

/// Unix time of a UTC calendar day `YYYY-MM-DD` at midnight.
pub fn day_start(text: &str) -> Result<u64> {
    let bad = || format!("Dates are YYYY-MM-DD (UTC): {text}");
    let mut parts = text.trim().splitn(3, '-');
    let mut next = || -> Result<i64> {
        parts
            .next()
            .and_then(|p| p.parse::<i64>().ok())
            .ok_or_else(bad)
    };
    let (y, m, d) = (next()?, next()?, next()?);
    let days_in = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        _ => return Err(bad()),
    };
    if !(1970..=9999).contains(&y) || !(1..=days_in).contains(&d) {
        return Err(bad());
    }
    // Days since 1970-01-01 from a civil date (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Ok(((era * 146_097 + doe - 719_468) * 86_400) as u64)
}

/// The Unix times a `--from`/`--to` day filter covers: from the start of `from` up to the end
/// of `to` (the next day's start).
pub fn day_range(from: Option<&str>, to: Option<&str>) -> Result<(Option<u64>, Option<u64>)> {
    let from = from.map(day_start).transpose()?;
    let to = to.map(|d| day_start(d).map(|t| t + 86_400)).transpose()?;
    Ok((from, to))
}
/// One line for an entry: revision, UTC time, kind and what changed.
pub fn entry_line(entry: &Value) -> String {
    let at = entry["at_unix"]
        .as_u64()
        .map(|t| {
            crate::crash_log::timestamp(std::time::UNIX_EPOCH + std::time::Duration::from_secs(t))
                .chars()
                .take(16)
                .collect::<String>()
                .replace('T', " ")
        })
        .unwrap_or_else(|| "-".repeat(16));
    let revision = entry["revision"].as_u64().unwrap_or(0);
    if entry["kind"] == "catalog" {
        let changes = entry["changes"].as_array().map_or(&[][..], Vec::as_slice);
        let count = |f: &str| changes.iter().filter(|c| !c[f].is_null()).count();
        let records = entry["records_changed"].as_u64().unwrap_or(0);
        let mut parts = vec![];
        if records > 0 {
            parts.push(format!(
                "{records} record{}",
                if records == 1 { "" } else { "s" }
            ));
        }
        for (field, label) in [
            ("created", "created"),
            ("removed", "removed"),
            ("status", "status"),
            ("name", "renamed"),
            ("parent", "moved"),
            ("reopened", "reopened"),
        ] {
            let n = count(field);
            if n > 0 {
                parts.push(format!("{n} {label}"));
            }
        }
        if let [single] = changes {
            let id = single["id"].as_u64().unwrap_or(0);
            if let Some(status) = single["status"]["after"].as_str() {
                parts.push(format!("#{id} -> {status}"));
            } else if let Some(name) = single["name"]["after"].as_str() {
                parts.push(format!("#{id} -> {name}"));
            } else {
                parts.push(format!("#{id}"));
            }
        }
        let project = &entry["project"];
        if let Some(scope) = project["research_scope"]["after"].as_str() {
            parts.push(format!("scope -> {scope}"));
        }
        if let Some(name) = project["name"]["after"].as_str() {
            parts.push(format!("name -> {name}"));
        }
        if let Some(job) = entry["import"].as_str() {
            parts.push(format!("import {job} published"));
        }
        if project["binary"].is_object() {
            parts.push("binary identity amended".into());
        }
        match (
            project["active_import"]["before"].as_str(),
            project["active_import"]["after"].as_str(),
        ) {
            (_, Some(job)) => parts.push(format!("import {job} reserved")),
            (Some(job), None) => parts.push(format!("import {job} released")),
            (None, None) => {}
        }
        // Sidecar-only changes (flags, claims, pools, proofs, the review queue) name their files.
        let sidecars: Vec<&str> = entry["sidecars"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        if !sidecars.is_empty() {
            parts.push(sidecars.join(", "));
        }
        if parts.is_empty() {
            parts.push("no record changes".into());
        }
        let directed = if entry["directed_by"].is_string() {
            " [directed]"
        } else {
            ""
        };
        format!("r{revision}  {at}  catalog: {}{directed}", parts.join(", "))
    } else {
        let kind = entry["entry_kind"].as_str().unwrap_or("tooling");
        let summary: String = entry["summary"]
            .as_str()
            .unwrap_or_default()
            .lines()
            .next()
            .unwrap_or_default()
            .chars()
            .take(160)
            .collect();
        format!("r{revision}  {at}  {kind}: {summary}")
    }
}

/// Committed import publications as history entries. A publication changes the catalog under
/// its own journal in its job folder, so the catalog receipts never list it; its job receipt
/// names the revision it published and the records it created.
fn publications(root: &Path) -> Vec<Value> {
    let mut entries = vec![];
    let jobs = root.join("tools/workspaces/import-jobs");
    for entry in fs::read_dir(&jobs).into_iter().flatten().flatten() {
        let path = entry.path().join("receipt.json");
        let Ok(bytes) = fs::read(&path) else { continue };
        let Ok(job) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        let (Some(revision), Some(first), Some(end)) = (
            job["revision"].as_u64(),
            job["first_id"].as_u64(),
            job["end_id"].as_u64(),
        ) else {
            continue;
        };
        if job["state"] != "Committed" {
            continue;
        }
        let at = fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        entries.push(json!({
            "kind": "catalog",
            "revision": revision,
            "import": job["id"],
            "records_changed": end.saturating_sub(first),
            "created": {"first": first, "end": end},
            "changes": [],
            "at_unix": at,
        }));
    }
    entries
}

/// Receipts newest first, with the journal's modification time (the commit time; compaction keeps it).
fn receipts(root: &Path) -> Vec<(u64, PathBuf, Option<u64>)> {
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
                if let Ok(meta) = fs::metadata(&journal)
                    && meta.is_file()
                {
                    let at = meta
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs());
                    all.push((rev, journal, at));
                }
            }
        }
    }
    all.sort_by_key(|(rev, _, _)| std::cmp::Reverse(*rev));
    all.dedup_by_key(|(rev, _, _)| *rev);
    all
}

/// The parts of a receipt a summary reads. Unlisted fields, such as report and sidecar texts
/// that can reach many megabytes, are skipped by the parser instead of being built as values.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Receipt {
    /// The catalog's own fields before and after (no record arrays).
    before: Value,
    after: Value,
    changes: Value,
    edits: Vec<EditRef>,
    moves: Vec<Value>,
    sidecars: Vec<PathRef>,
    dependencies: Vec<Value>,
    directed_by: Option<String>,
    at_unix: Option<u64>,
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
            "before": self.before,
            "after": self.after,
            "changes": self.changes,
            "edits": self.edits.into_iter().map(|e| json!({"id": e.id, "old": e.old, "new": e.new})).collect::<Vec<_>>(),
            "moves": self.moves,
            "sidecars": self.sidecars.into_iter().map(|s| json!({"path": s.path})).collect::<Vec<_>>(),
            "dependencies": self.dependencies,
            "directed_by": self.directed_by,
            "at_unix": self.at_unix,
        })
    }
}

/// Catalog fields that history reports when a receipt changes them.
const PROJECT_FIELDS: [&str; 5] = [
    "research_scope",
    "name",
    "binary",
    "active_import",
    "last_import",
];

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
    // Changes to the catalog's own fields: scope, name, binary identity, import reservations.
    let mut project = serde_json::Map::new();
    if filter.node.is_none() {
        for f in PROJECT_FIELDS {
            let (a, b) = (&receipt["before"][f], &receipt["after"][f]);
            if receipt["before"].is_object() && receipt["after"].is_object() && a != b {
                project.insert(f.to_string(), json!({"before": a, "after": b}));
            }
        }
    }
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
    if !project.is_empty() {
        entry["project"] = Value::Object(project);
    }
    // A change to pooled records may carry the human's instruction that allowed it.
    if let Some(directed) = receipt["directed_by"].as_str() {
        entry["directed_by"] = directed.into();
    }
    if let Some(at) = receipt["at_unix"].as_u64() {
        entry["at_unix"] = at.into();
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
        let in_time = |at: Option<u64>| {
            (filter.from.is_none() && filter.to.is_none())
                || at.is_some_and(|t| {
                    filter.from.is_none_or(|f| t >= f) && filter.to.is_none_or(|u| t < u)
                })
        };
        // An RVA names exactly one record; an unknown one is an error, not an empty answer.
        let mut node = filter.node;
        if let Some(rva) = filter.rva {
            let id = self.projects[p]
                .nodes
                .iter()
                .find(|n| n.rva == Some(rva))
                .map(|n| n.id)
                .ok_or_else(|| format!("No record has RVA {rva:X}"))?;
            if node.is_some_and(|n| n != id) {
                return Err(format!("RVA {rva:X} is node {id}, not the given node"));
            }
            node = Some(id);
        }
        let narrowed = HistoryFilter {
            node,
            kind: filter.kind.clone(),
            ..*filter
        };
        let filter = &narrowed;
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
                if in_range(rev) && in_time(entry["at_unix"].as_u64()) && node_ok && kind_ok {
                    entries.push(entry);
                }
            }
        }
        if filter.catalog && filter.kind.as_deref().is_none_or(|k| k == "catalog") {
            let mut found = 0usize;
            for (rev, path, modified) in receipts(&root) {
                if !in_range(rev) {
                    continue;
                }
                // A receipt's file is written after its commit, so it is never older than the
                // time it records: one last written well before `--from` cannot match.
                if let (Some(from), Some(written)) = (filter.from, modified)
                    && written.saturating_add(2) < from
                {
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
                let mut receipt = receipt.summary_value();
                // The recorded commit time; receipts older than 0.13 have none and are dated by
                // their file.
                if receipt["at_unix"].is_null()
                    && let Some(at) = modified
                {
                    receipt["at_unix"] = at.into();
                }
                if !in_time(receipt["at_unix"].as_u64()) {
                    continue;
                }
                if let Some(summary) = summarize(rev, &receipt, filter) {
                    entries.push(summary);
                    found += 1;
                }
            }
            entries.extend(publications(&root).into_iter().filter(|entry| {
                let created = &entry["created"];
                in_range(entry["revision"].as_u64().unwrap_or(0))
                    && in_time(entry["at_unix"].as_u64())
                    && filter.node.is_none_or(|n| {
                        created["first"].as_u64().is_some_and(|first| n >= first)
                            && created["end"].as_u64().is_some_and(|end| n < end)
                    })
            }));
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
        if !crate::coordinator::manifest::valid_name(entry_kind) {
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
        if let Some(known) = KINDS.iter().find(|k| k.name == entry_kind) {
            if known.needs_node && nodes.is_empty() {
                return Err(format!(
                    "A {} entry names the records it concerns (--node).",
                    known.name
                ));
            }
            if known.needs_ref && refs.iter().all(|r| r.trim().is_empty()) {
                return Err(format!(
                    "A {} entry names what it refers to (--ref).",
                    known.name
                ));
            }
        }
        let known: std::collections::HashSet<u64> =
            self.projects[p].nodes.iter().map(|n| n.id).collect();
        if let Some(bad) = nodes.iter().find(|id| !known.contains(id)) {
            return Err(format!("Unknown node {bad} in history entry"));
        }
        let at = unix_now();
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
