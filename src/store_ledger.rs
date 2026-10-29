//! The annotation ledger followed into the store (docs/design-storage-promotion.md §1).
//!
//! The project tools' ledger (`tools/profiles/annotations.json`) stays authoritative. Each sync
//! that finds it changed appends one entry to `ledger.log`: the rows and types added, removed
//! and changed since the last entry, keyed by `(rva, kind)` and by type name. The first entry,
//! and any change those keys cannot describe (a duplicate key), stores the whole ledger as a
//! text instead. Fingerprints are of the canonical form (keys sorted, rows ordered by key), so
//! a replay reproduces them exactly; `ledger-head.zlib` keeps the latest canonical ledger so a
//! sync need not replay.
use super::*;
use std::collections::BTreeMap;

pub(super) const LOG: &str = "ledger.log";
const HEAD: &str = "ledger-head.zlib";
use super::super::ledger::LEDGER as FILE;
/// Row kinds the ledger holds today; any other is reported by `store conflicts`.
const KNOWN: [&str; 8] = [
    "function-name",
    "function-signature",
    "function-noreturn",
    "plate-comment",
    "label",
    "data-definition",
    "data-type",
    "data-retire",
];

/// The ledger file's size and modification time, which change with every write.
pub(crate) fn stamp(dir: &Path) -> Option<(u64, std::time::SystemTime)> {
    let meta = fs::metadata(dir.join(FILE)).ok()?;
    Some((meta.len(), meta.modified().ok()?))
}

type Key = fn(&Value) -> Option<String>;
fn row_key(row: &Value) -> Option<String> {
    Some(format!(
        "{}|{}",
        row["rva"].as_str()?,
        row["kind"].as_str()?
    ))
}
fn type_key(t: &Value) -> Option<String> {
    t["name"].as_str().map(str::to_string)
}
/// The ledger's lists, each with its key and the name of its changes in an entry.
const LISTS: [(&str, &str, Key); 2] = [
    ("annotations", "rows", row_key),
    ("types", "types", type_key),
];

/// A ledger as read: its file's hash, its canonical value, bytes and fingerprint.
struct Ledger {
    file_sha: String,
    value: Value,
    text: String,
    sha: String,
}
impl Ledger {
    fn new(file_sha: String, mut value: Value) -> Result<Self> {
        for (field, _, key) in LISTS {
            if let Some(list) = value.get_mut(field).and_then(Value::as_array_mut) {
                list.sort_by_cached_key(|r| key(r).unwrap_or_default());
            }
        }
        let text = serde_json::to_string(&value).map_err(|e| e.to_string())?;
        let sha = hash(&text);
        Ok(Self {
            file_sha,
            value,
            text,
            sha,
        })
    }
    fn count(&self, field: &str) -> usize {
        self.value[field].as_array().map_or(0, Vec::len)
    }
}
/// The ledger file's bytes and their hash; none when it does not exist.
fn read_bytes(dir: &Path) -> Option<(Vec<u8>, String)> {
    let bytes = fs::read(dir.join(FILE)).ok()?;
    let sha = sha256_hex(&bytes);
    Some((bytes, sha))
}
/// The ledger in `bytes`, or a note when it does not parse (a write in progress outside the
/// lease; the next sync retries).
fn parse(bytes: &[u8], file_sha: String) -> std::result::Result<Ledger, String> {
    serde_json::from_slice(bytes)
        .map_err(|e| format!("the ledger does not parse ({e}); recorded at a later sync"))
        .and_then(|value| Ledger::new(file_sha, value))
}
/// The ledger's fields other than its lists.
fn header(value: &Value) -> Value {
    let mut out = value.clone();
    if let Some(map) = out.as_object_mut() {
        for (field, _, _) in LISTS {
            map.remove(field);
        }
    }
    out
}
/// `list` by key; none when a row has no key or two rows share one.
fn keyed(list: &Value, key: Key) -> Option<BTreeMap<String, &Value>> {
    let mut out = BTreeMap::new();
    for row in list.as_array().into_iter().flatten() {
        if out.insert(key(row)?, row).is_some() {
            return None;
        }
    }
    Some(out)
}
/// The rows removed, added and changed from `before` to `after`.
fn diff(before: &Value, after: &Value, key: Key) -> Option<Value> {
    let (b, a) = (keyed(before, key)?, keyed(after, key)?);
    let removed: Vec<&Value> = b
        .iter()
        .filter(|(k, _)| !a.contains_key(*k))
        .map(|(_, v)| *v)
        .collect();
    let added: Vec<&Value> = a
        .iter()
        .filter(|(k, _)| !b.contains_key(*k))
        .map(|(_, v)| *v)
        .collect();
    let changed: Vec<Value> = a
        .iter()
        .filter_map(|(k, after)| {
            let before = b.get(k)?;
            (before != after).then(|| json!({"before": before, "after": after}))
        })
        .collect();
    Some(json!({"removed": removed, "added": added, "changed": changed}))
}
/// Applies one diff entry to `state`. Every row removed or changed must be there as recorded.
fn apply(state: &mut Value, entry: &Value) -> Result<()> {
    let map = state
        .as_object_mut()
        .ok_or("Stored ledger is not an object")?;
    if let Some(change) = entry.get("header") {
        let before = change["before"].as_object().ok_or("Bad ledger header")?;
        let current: serde_json::Map<String, Value> = map
            .iter()
            .filter(|(k, _)| LISTS.iter().all(|(field, _, _)| *field != k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if &current != before {
            return Err("ledger header differs from the one recorded".into());
        }
        map.retain(|k, _| LISTS.iter().any(|(field, _, _)| *field == k.as_str()));
        for (k, v) in change["after"].as_object().ok_or("Bad ledger header")? {
            map.insert(k.clone(), v.clone());
        }
    }
    for (field, changes, key) in LISTS {
        let changes = &entry[changes];
        let present = map.contains_key(field);
        let mut rows: BTreeMap<String, Value> = BTreeMap::new();
        if let Some(Value::Array(list)) = map.remove(field) {
            for row in list {
                rows.insert(key(&row).ok_or("Stored ledger row without key")?, row);
            }
        }
        for row in changes["removed"].as_array().into_iter().flatten() {
            let k = key(row).ok_or("Ledger entry row without key")?;
            if rows.remove(&k).as_ref() != Some(row) {
                return Err(format!("ledger row {k} is not as recorded"));
            }
        }
        for change in changes["changed"].as_array().into_iter().flatten() {
            let k = key(&change["before"]).ok_or("Ledger entry row without key")?;
            match rows.get_mut(&k) {
                Some(row) if *row == change["before"] => *row = change["after"].clone(),
                _ => return Err(format!("ledger row {k} is not as recorded")),
            }
        }
        for row in changes["added"].as_array().into_iter().flatten() {
            let k = key(row).ok_or("Ledger entry row without key")?;
            if rows.insert(k.clone(), row.clone()).is_some() {
                return Err(format!("ledger row {k} added twice"));
            }
        }
        if present || !rows.is_empty() {
            map.insert(field.into(), Value::Array(rows.into_values().collect()));
        }
    }
    Ok(())
}

/// The entries of `ledger.log` (none before the first), up to the last complete frame.
fn entries(store: &Path) -> Result<Vec<Value>> {
    let bytes = match fs::read(store.join(LOG)) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.to_string()),
    };
    let (frames, end) = frames(&bytes);
    if let End::Damaged(at) = end {
        return Err(format!(
            "store_damaged: {LOG} has a damaged frame at byte {at}; run research store verify"
        ));
    }
    frames
        .into_iter()
        .map(|r| serde_json::from_slice(&bytes[r]).map_err(|e| e.to_string()))
        .collect()
}
/// The ledger after every entry, rebuilt from the stored texts and diffs.
fn replay(store: &Path, entries: &[Value]) -> Result<Value> {
    let texts = text_index(store)?;
    let mut state = Value::Null;
    for entry in entries {
        if let Some(sha) = entry["full"].as_str() {
            let text = texts
                .read(sha)?
                .ok_or_else(|| format!("stored ledger text {sha} is missing"))?;
            state = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        } else {
            apply(&mut state, entry)
                .map_err(|e| format!("ledger entry at revision {}: {e}", entry["revision"]))?;
        }
    }
    Ok(state)
}
/// The ledger as of the last entry: the head copy when it matches, otherwise a replay.
fn last_state(store: &Path, entries: &[Value]) -> Result<Value> {
    let expect = entries.last().and_then(|e| e["after_sha256"].as_str());
    if let Ok(bytes) = fs::read(store.join(HEAD)).map_err(|e| e.to_string())
        && let Ok(text) = inflate(&bytes)
        && expect == Some(sha256_hex(&text).as_str())
    {
        return serde_json::from_slice(&text).map_err(|e| e.to_string());
    }
    replay(store, entries)
}

/// Records the ledger's change since the last entry, if any. Called by `sync` after the
/// catalog, with the writer's texts flushed. Problems are answered, never raised: the catalog's
/// sync has already succeeded.
pub(super) fn follow(w: &mut Writer, dir: &Path, store: &Path, revision: u64) -> Value {
    match follow_inner(w, dir, store, revision, None) {
        Ok(v) => v,
        Err(e) => json!({"error": e}),
    }
}
/// [`follow`], answering problems as errors; `by` names who made the change when Scopework
/// made it (`research annotate`).
pub(super) fn follow_inner(
    w: &mut Writer,
    dir: &Path,
    store: &Path,
    revision: u64,
    by: Option<&str>,
) -> Result<Value> {
    let Some((bytes, file_sha)) = read_bytes(dir) else {
        return Ok(Value::Null);
    };
    let entries = entries(store)?;
    let last = entries.last();
    // Unchanged bytes need no parse.
    if last.is_some_and(|e| e["file_sha256"] == file_sha.as_str()) {
        return Ok(json!({"up_to_date": true}));
    }
    let now = match parse(&bytes, file_sha) {
        Ok(l) => l,
        Err(note) => return Ok(json!({"skipped": note})),
    };
    drop(bytes);
    let mut entry = json!({
        "type": "ledger",
        "revision": revision,
        "at_unix": unix_now(),
        "file_sha256": now.file_sha,
        "before_sha256": last.map(|e| e["after_sha256"].clone()),
        "after_sha256": now.sha,
        "rows_total": now.count("annotations"),
        "types_total": now.count("types"),
    });
    if let Some(by) = by {
        entry["by"] = by.into();
    }
    let mut summary =
        json!({"rows_total": now.count("annotations"), "types_total": now.count("types")});
    let previous = match last {
        Some(e) if e["after_sha256"] == now.sha.as_str() => None, // reordered only
        Some(_) => Some(last_state(store, &entries)?),
        None => None,
    };
    // A change that rewrites most of a large ledger (a pin conversion) is smaller stored whole.
    let diffs = previous.as_ref().and_then(|p| {
        let rows = diff(&p["annotations"], &now.value["annotations"], row_key)?;
        let types = diff(&p["types"], &now.value["types"], type_key)?;
        let size = rows.to_string().len() + types.to_string().len();
        (now.text.len() < 1 << 20 || size <= now.text.len() / 2).then_some((p, rows, types))
    });

    match diffs {
        Some((p, rows, types)) => {
            for (name, changes) in [("rows", &rows), ("types", &types)] {
                summary[name] = json!({
                    "removed": changes["removed"].as_array().map_or(0, Vec::len),
                    "added": changes["added"].as_array().map_or(0, Vec::len),
                    "changed": changes["changed"].as_array().map_or(0, Vec::len),
                });
            }
            let (before, after) = (header(p), header(&now.value));
            if before != after {
                entry["header"] = json!({"before": before, "after": after});
            }
            entry["rows"] = rows;
            entry["types"] = types;
        }
        None if last.is_some_and(|e| e["after_sha256"] == now.sha.as_str()) => {
            summary["reordered"] = true.into();
        }
        None => {
            entry["full"] = w.text(&now.text)?.into();
            summary["full"] = true.into();
        }
    }
    w.flush_texts()?;
    append(
        &store.join(LOG),
        &frame(&serde_json::to_vec(&entry).map_err(|e| e.to_string())?)?,
    )?;
    // Compressed for speed: the head copy is rewritten on every ledger change.
    atomic(
        &store.join(HEAD),
        &deflate_at(now.text.as_bytes(), flate2::Compression::fast())?,
    )?;
    Ok(summary)
}

/// The ledger file hash of the last entry, if there is one.
pub(super) fn last_file_sha(store: &Path) -> Result<Option<String>> {
    Ok(entries(store)?
        .last()
        .and_then(|e| e["file_sha256"].as_str().map(str::to_string)))
}
/// The ledger facts for `store status`.
pub(super) fn status(store: &Path) -> Result<Value> {
    let entries = entries(store)?;
    Ok(entries.last().map_or(Value::Null, |last| {
        json!({
            "entries": entries.len(),
            "file_sha256": last["file_sha256"],
            "rows": last["rows_total"],
            "types": last["types_total"],
            "revision": last["revision"],
            "at_unix": last["at_unix"],
        })
    }))
}
/// Checks the ledger chain for `store verify`: every entry applies, the replay ends with the
/// last recorded fingerprint, and the stored texts it needs are named in `referenced`.
pub(super) fn verify(store: &Path, problems: &mut Vec<String>, referenced: &mut HashSet<String>) {
    let entries = match entries(store) {
        Ok(e) => e,
        Err(e) => return problems.push(e),
    };
    if entries.is_empty() {
        return;
    }
    let mut chain: Option<&str> = None;
    for entry in &entries {
        if let Some(sha) = entry["full"].as_str() {
            referenced.insert(sha.into());
        } else if entry["before_sha256"].as_str() != chain {
            problems.push(format!(
                "{LOG}: the entry at revision {} does not follow the one before it",
                entry["revision"]
            ));
        }
        chain = entry["after_sha256"].as_str();
    }
    match replay(store, &entries) {
        Ok(state) => {
            let sha = serde_json::to_string(&state)
                .map(|t| hash(&t))
                .unwrap_or_default();
            if Some(sha.as_str()) != chain {
                problems.push(format!(
                    "{LOG}: replay ends with another ledger than recorded"
                ));
            }
        }
        Err(e) => problems.push(format!("{LOG}: {e}")),
    }
}

/// An address as `parse_rva` reads it (hexadecimal, `0x` optional).
fn parse_address(text: &str) -> Option<u64> {
    parse_rva(text).ok().map(u64::from)
}
/// The ledger rows at an address through the store's history, newest first.
pub fn annotations(root: &Path, project: &str, address: &str, kind: Option<&str>) -> Result<Value> {
    let (_, store) = open_store(root, project)?;
    let rva =
        parse_address(address).ok_or("The address must be hexadecimal, such as 0x0041A2B0.")?;
    let wanted = |row: &Value| {
        row["rva"].as_str().and_then(parse_address) == Some(rva)
            && kind.is_none_or(|k| row["kind"] == k)
    };
    let entries = entries(&store)?;
    let texts = text_index(&store)?;
    let mut current: BTreeMap<String, Value> = BTreeMap::new();
    let mut changes = vec![];
    for entry in &entries {
        let mut next = current.clone();
        if let Some(sha) = entry["full"].as_str() {
            let text = texts
                .read(sha)?
                .ok_or_else(|| format!("stored ledger text {sha} is missing"))?;
            let state: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            next = state["annotations"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|r| wanted(r))
                .filter_map(|r| Some((r["kind"].as_str()?.to_string(), r.clone())))
                .collect();
        } else {
            let rows = &entry["rows"];
            for row in rows["removed"].as_array().into_iter().flatten() {
                if wanted(row) {
                    next.remove(row["kind"].as_str().unwrap_or_default());
                }
            }
            let changed = rows["changed"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|c| &c["after"]);
            for row in rows["added"]
                .as_array()
                .into_iter()
                .flatten()
                .chain(changed)
            {
                if wanted(row) {
                    next.insert(row["kind"].as_str().unwrap_or_default().into(), row.clone());
                }
            }
        }
        let kinds: std::collections::BTreeSet<&String> =
            current.keys().chain(next.keys()).collect();
        for k in kinds {
            let (before, after) = (current.get(k), next.get(k));
            if before != after {
                changes.push(json!({
                    "revision": entry["revision"],
                    "at_unix": entry["at_unix"],
                    "by": entry["by"],
                    "kind": k,
                    "before": before,
                    "after": after,
                }));
            }
        }
        current = next;
    }
    changes.reverse();
    Ok(json!({
        "backend": "store",
        "project": project,
        "rva": format!("0x{rva:08X}"),
        "current": current.into_values().collect::<Vec<_>>(),
        "changes": changes,
        "history_from": entries.first().map(|e| &e["revision"]),
    }))
}

/// Disagreements between the catalog and the ledger as they are now (the user's rule: these
/// are resolved before the backend is adopted; the backend only reports them).
pub fn conflicts(root: &Path, project: &str) -> Result<Value> {
    let dir = project_dir(root, project)?;
    let bytes = fs::read(dir.join("project.json")).map_err(|e| e.to_string())?;
    let catalog: Project = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let (bytes, file_sha) = read_bytes(&dir)
        .ok_or_else(|| format!("Project {project} has no annotation ledger ({FILE})."))?;
    let ledger = parse(&bytes, file_sha)?;
    let index = catalog_index::Index::new(&catalog);
    let leaves: HashMap<u64, &Node> = catalog
        .nodes
        .iter()
        // Ledger rows name addresses in the default space.
        .filter(|n| n.kind.is_leaf() && n.space.is_none())
        .filter_map(|n| Some((u64::from(n.rva?), n)))
        .collect();
    let mut found = vec![];
    let mut unplaced = 0usize;
    for row in ledger.value["annotations"].as_array().into_iter().flatten() {
        let kind = row["kind"].as_str().unwrap_or_default();
        let rva = row["rva"].as_str().and_then(parse_address);
        let mut conflict = |what: &str, detail: Value| {
            found
                .push(json!({"conflict": what, "kind": kind, "rva": row["rva"], "detail": detail}));
        };
        if !KNOWN.contains(&kind) {
            conflict(
                "unknown",
                json!({"note": "a row kind this build does not know"}),
            );
            continue;
        }
        let Some(node) = rva.and_then(|rva| leaves.get(&rva)) else {
            unplaced += 1;
            continue;
        };
        let is_function = node.kind == Kind::Function;
        let named = match (kind, is_function) {
            ("function-name", true) | ("label", false) => Some(&row["value"]),
            _ => None,
        };
        if let Some(value) = named
            && value.as_str() != Some(node.name.as_str())
        {
            conflict(
                "name",
                json!({"record": node.id, "catalog": node.name, "ledger": value}),
            );
        }
        let wrong_kind = if is_function {
            matches!(kind, "data-definition" | "data-type")
        } else {
            kind.starts_with("function-")
        };
        if wrong_kind {
            conflict("kind", json!({"record": node.id, "record_kind": node.kind}));
        }
        if let (Some(report), Some(path)) = (row["report"].as_str(), index.paths.get(&node.id))
            && report != path
        {
            conflict(
                "report",
                json!({"record": node.id, "ledger": report, "catalog": path}),
            );
        }
    }
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for c in &found {
        *counts
            .entry(c["conflict"].as_str().unwrap_or_default().into())
            .or_insert(0) += 1;
    }
    const LISTED: usize = 1000;
    Ok(json!({
        "project": project,
        "revision": catalog.revision,
        "ledger_sha256": ledger.file_sha,
        "rows": ledger.count("annotations"),
        "conflicts": found.len(),
        "by_kind": counts,
        "unplaced_rows": unplaced,
        "listed": found.iter().take(LISTED).collect::<Vec<_>>(),
        "truncated": found.len() > LISTED,
    }))
}
