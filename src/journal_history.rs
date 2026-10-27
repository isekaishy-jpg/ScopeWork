//! Completed history is a change receipt, never an input to pending recovery.
use super::*;
use serde_json::{Value, json};

/// Catalog fields other than the record arrays, serialized as the receipt's `before`/`after`.
fn header(p: &Project) -> Result<Value> {
    let mut v = serde_json::to_value(Project {
        nodes: vec![],
        dependencies: vec![],
        layout_version: p.layout_version,
        id: p.id.clone(),
        name: p.name.clone(),
        binary: p.binary.clone(),
        research_scope: p.research_scope.clone(),
        next_id: p.next_id,
        revision: p.revision,
        structural_epoch: p.structural_epoch,
        active_import: p.active_import.clone(),
        last_import: p.last_import.clone(),
    })
    .map_err(|e| e.to_string())?;
    let object = v.as_object_mut().ok_or("Invalid catalog")?;
    object.remove("nodes");
    object.remove("dependencies");
    Ok(v)
}
/// Changed records between two arrays keyed by identity, comparing typed records so only the
/// changes are serialized. A record counts as changed when it is new, gone, different, or moved
/// while the surviving records were reordered.
fn record_changes<T: PartialEq + Serialize, K: Eq + std::hash::Hash>(
    a: &[T],
    b: &[T],
    key: impl Fn(&T) -> K,
) -> Result<Value> {
    let aa: HashMap<K, (usize, &T)> = a
        .iter()
        .enumerate()
        .map(|(i, v)| (key(v), (i, v)))
        .collect();
    let bb: HashMap<K, (usize, &T)> = b
        .iter()
        .enumerate()
        .map(|(i, v)| (key(v), (i, v)))
        .collect();
    let common_before: Vec<K> = a.iter().map(&key).filter(|k| bb.contains_key(k)).collect();
    let common_after: Vec<K> = b.iter().map(&key).filter(|k| aa.contains_key(k)).collect();
    let reordered = common_before != common_after;
    let changed = |own: (usize, &T), other: Option<&(usize, &T)>| {
        other.is_none_or(|other| own.1 != other.1 || (reordered && own.0 != other.0))
    };
    let mut removed = vec![];
    for (i, v) in a.iter().enumerate() {
        if changed((i, v), bb.get(&key(v))) {
            removed.push(
                json!({"index": i, "record": serde_json::to_value(v).map_err(|e| e.to_string())?}),
            );
        }
    }
    let mut added = vec![];
    for (i, v) in b.iter().enumerate() {
        if changed((i, v), aa.get(&key(v))) {
            added.push(
                json!({"index": i, "record": serde_json::to_value(v).map_err(|e| e.to_string())?}),
            );
        }
    }
    Ok(json!({"before": removed, "after": added}))
}
fn delta(j: &Journal) -> Result<Value> {
    Ok(delta_with_after(j)?.0)
}
/// The last catalog this process fingerprinted, with its fingerprint. A commit's after-catalog
/// is usually the next commit's before-catalog, so its fingerprint is reused instead of
/// serialising and hashing the catalog again. Reuse requires equality: catalogs serialise
/// deterministically (no unordered maps), so equal catalogs have equal fingerprints.
static LAST_FINGERPRINT: std::sync::Mutex<Option<(Project, String)>> = std::sync::Mutex::new(None);
fn before_fingerprint(before: &Project) -> Result<String> {
    if let Ok(last) = LAST_FINGERPRINT.lock()
        && let Some((catalog, hash)) = last.as_ref()
        && catalog == before
    {
        return Ok(hash.clone());
    }
    Ok(sha256_hex(&catalog_bytes(before)?))
}
fn remember_fingerprint(catalog: &Project, hash: &str) {
    if let Ok(mut last) = LAST_FINGERPRINT.lock() {
        *last = Some((catalog.clone(), hash.to_string()));
    }
}
/// The delta receipt plus the after-catalog's bytes (`catalog_bytes`), serialised once and
/// used both for the after fingerprint and, by `commit`, for writing `project.json`.
fn delta_with_after(j: &Journal) -> Result<(Value, Vec<u8>)> {
    let after_bytes = catalog_bytes(&j.after)?;
    let before_hash = before_fingerprint(&j.before)?;
    let after_hash = sha256_hex(&after_bytes);
    let mut changes = serde_json::Map::new();
    changes.insert(
        "nodes".into(),
        record_changes(&j.before.nodes, &j.after.nodes, |n| n.id)?,
    );
    changes.insert(
        "dependencies".into(),
        record_changes(&j.before.dependencies, &j.after.dependencies, |d| {
            (d.dependent, d.prerequisite)
        })?,
    );
    Ok((
        json!({"schema":1,"kind":"completed-catalog-delta","before":header(&j.before)?,"after":header(&j.after)?,"changes":changes,
        "before_hash":before_hash,
        "after_hash":after_hash,
        "edits":j.edits,"moves":j.moves,"dependencies":j.dependencies,"sidecars":j.sidecars,"ledger":j.ledger,"repin_ledger":j.repin_ledger,"directed_by":j.directed_by,"repin_ids":j.repin_ids}),
        after_bytes,
    ))
}

pub(super) fn pending_bytes(j: &Journal) -> Result<Vec<u8>> {
    serde_json::to_vec(&pending_delta(j)?.0).map_err(|e| e.to_string())
}
/// The pending journal value and the after-catalog bytes, for an in-process commit.
pub(super) fn pending_delta(j: &Journal) -> Result<(Value, Vec<u8>)> {
    let (mut value, after) = delta_with_after(j)?;
    value["kind"] = "pending-catalog-delta".into();
    Ok((value, after))
}
/// Records that `catalog` (a commit's after-catalog) has fingerprint `hash`, once that commit
/// is written.
pub(super) fn committed(catalog: &Project, hash: &str) {
    remember_fingerprint(catalog, hash);
}

pub(super) fn read_pending(bytes: &[u8], current: &Project) -> Result<Journal> {
    let value: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if value.get("kind").is_none() {
        return serde_json::from_value(value).map_err(|e| e.to_string());
    }
    if value["kind"] != "pending-catalog-delta" || value["schema"] != 1 {
        return Err(
            "Unrecognized pending journal; completed receipts cannot be replayed as pending."
                .into(),
        );
    }
    let fingerprint = hash(&serde_json::to_string(current).map_err(|e| e.to_string())?);
    let (source, target) = if value["before_hash"] == fingerprint {
        ("before", "after")
    } else if value["after_hash"] == fingerprint {
        ("after", "before")
    } else {
        return Err(
            "Catalog changed during batch transaction; preserve the pending journal.".into(),
        );
    };
    // Apply the delta to typed records; only changed records are converted from JSON.
    fn apply<T: PartialEq + Clone + serde::de::DeserializeOwned>(
        records: &[T],
        changes: &Value,
        source: &str,
        target: &str,
    ) -> Result<Vec<T>> {
        let mut records = records.to_vec();
        let mut remove: Vec<&Value> = changes[source]
            .as_array()
            .ok_or("Invalid journal delta")?
            .iter()
            .collect();
        remove.sort_by_key(|r| std::cmp::Reverse(r["index"].as_u64().unwrap_or(u64::MAX)));
        for row in remove {
            let index = row["index"].as_u64().ok_or("Invalid delta index")? as usize;
            let record: T =
                serde_json::from_value(row["record"].clone()).map_err(|e| e.to_string())?;
            if records.get(index) != Some(&record) {
                return Err("Journal delta input mismatch".into());
            }
            records.remove(index);
        }
        let mut insert: Vec<&Value> = changes[target]
            .as_array()
            .ok_or("Invalid journal delta")?
            .iter()
            .collect();
        insert.sort_by_key(|r| r["index"].as_u64().unwrap_or(u64::MAX));
        for row in insert {
            let index = row["index"].as_u64().ok_or("Invalid delta index")? as usize;
            if index > records.len() {
                return Err("Journal delta index outside catalog".into());
            }
            records.insert(
                index,
                serde_json::from_value(row["record"].clone()).map_err(|e| e.to_string())?,
            );
        }
        Ok(records)
    }
    let mut other = value[target].clone();
    let object = other.as_object_mut().ok_or("Invalid journal header")?;
    object.insert("nodes".into(), Value::Array(vec![]));
    object.insert("dependencies".into(), Value::Array(vec![]));
    let mut other: Project = serde_json::from_value(other).map_err(|e| e.to_string())?;
    other.nodes = apply(&current.nodes, &value["changes"]["nodes"], source, target)?;
    other.dependencies = apply(
        &current.dependencies,
        &value["changes"]["dependencies"],
        source,
        target,
    )?;
    if value[format!("{target}_hash")]
        != hash(&serde_json::to_string(&other).map_err(|e| e.to_string())?)
    {
        return Err("Journal delta output mismatch".into());
    }
    let (before, after) = if source == "before" {
        (current.clone(), other)
    } else {
        (other, current.clone())
    };
    Ok(Journal {
        changes: vec![],
        before,
        after,
        edits: serde_json::from_value(value["edits"].clone()).map_err(|e| e.to_string())?,
        moves: serde_json::from_value(value["moves"].clone()).map_err(|e| e.to_string())?,
        dependencies: serde_json::from_value(value["dependencies"].clone())
            .map_err(|e| e.to_string())?,
        sidecars: serde_json::from_value(value["sidecars"].clone()).map_err(|e| e.to_string())?,
        ledger: value["ledger"].as_bool().unwrap_or(false),
        repin_ledger: value["repin_ledger"].as_bool().unwrap_or(false),
        directed_by: value["directed_by"].as_str().map(str::to_string),
        repin_ids: serde_json::from_value(value["repin_ids"].clone()).unwrap_or_default(),
    })
}

pub(super) fn compact(path: &Path, journal: &Journal, at_unix: u64) -> Result<()> {
    let mut value: Value = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if value["kind"] == "pending-catalog-delta" && value["schema"] == 1 {
        value["kind"] = "completed-catalog-delta".into();
    } else {
        value = delta(journal)?;
    }
    if value["at_unix"].is_null() {
        value["at_unix"] = at_unix.into();
    }
    let bytes = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
    atomic(path, &bytes)
}

/// Sidecar texts at least this large are reported, and replaced by `--compact-sidecars`.
const LARGE_SIDECAR: usize = 1 << 20;
/// Changed values between two JSON documents, each with its JSON-pointer path. Arrays of
/// different lengths are recorded whole; a missing side is omitted from the entry.
fn json_changes(path: &mut String, a: Option<&Value>, b: Option<&Value>, out: &mut Vec<Value>) {
    match (a, b) {
        (Some(Value::Object(x)), Some(Value::Object(y))) => {
            let keys: std::collections::BTreeSet<&String> = x.keys().chain(y.keys()).collect();
            for key in keys {
                let len = path.len();
                path.push('/');
                path.push_str(&key.replace('~', "~0").replace('/', "~1"));
                json_changes(path, x.get(key), y.get(key), out);
                path.truncate(len);
            }
        }
        (Some(Value::Array(x)), Some(Value::Array(y))) if x.len() == y.len() => {
            for (i, (u, v)) in x.iter().zip(y).enumerate() {
                let len = path.len();
                path.push_str(&format!("/{i}"));
                json_changes(path, Some(u), Some(v), out);
                path.truncate(len);
            }
        }
        _ if a == b => {}
        _ => {
            let mut entry = json!({"path": path.clone()});
            if let Some(a) = a {
                entry["before"] = a.clone();
            }
            if let Some(b) = b {
                entry["after"] = b.clone();
            }
            out.push(entry);
        }
    }
}
/// A compact record of a large sidecar change: hashes, sizes and the changed JSON values.
/// `None` when either text is not JSON (it is then kept as it is).
fn compact_sidecar(sidecar: &Value) -> Option<Value> {
    let before = sidecar["before"].as_str()?;
    let after = sidecar["after"].as_str()?;
    let a: Value = serde_json::from_str(before).ok()?;
    let b: Value = serde_json::from_str(after).ok()?;
    let mut changes = vec![];
    json_changes(&mut String::new(), Some(&a), Some(&b), &mut changes);
    Some(json!({
        "path": sidecar["path"],
        "compacted": "Whole texts replaced by their changed JSON values (research journal --compact-sidecars)",
        "before_sha256": hash(before),
        "after_sha256": hash(after),
        "before_bytes": before.len(),
        "after_bytes": after.len(),
        "changes": changes,
    }))
}
fn large_sidecar(sidecar: &Value) -> Option<usize> {
    let size = sidecar["before"].as_str().map_or(0, str::len)
        + sidecar["after"].as_str().map_or(0, str::len);
    (size >= LARGE_SIDECAR).then_some(size)
}

impl Library {
    /// History maintenance deliberately bypasses project migration and pending recovery.
    pub fn inspect_journals(root: PathBuf, id: &str, apply: bool) -> Result<Value> {
        Self::maintain_journals(root, id, apply, false)
    }
    /// Like [`Self::inspect_journals`]; with `sidecars`, also replaces large sidecar texts in
    /// completed receipts by their changed values. That drops the exact historical bytes.
    pub fn maintain_journals(
        root: PathBuf,
        id: &str,
        apply: bool,
        sidecars: bool,
    ) -> Result<Value> {
        if !valid_id(id) {
            return Err("Invalid project identity".into());
        }
        let dir = root.join(id);
        contained(&root, &dir)?;
        let gate = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join(".library.lock"))
            .map_err(|e| e.to_string())?;
        wait_lock(&gate, false).map_err(|_| {
            "Research library is busy with another operation. Retry shortly.".to_owned()
        })?;
        let path = dir.join("project.json");
        contained(&root, &path)?;
        if !path.is_file() {
            return Err(format!("Unknown project: {id}"));
        }
        // Held before the catalog is read, so a catalog that cannot be read releases the lock
        // and removes its holder record like any other failure.
        let locks = OperationLocks {
            project: Some(id.into()),
            writable: apply,
            _project: Some(project_lock(&root, id, apply)?),
            _gate: gate,
            holder: holder_path(&root, id, apply),
        };
        let project: Project = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        if project.id != id || project.layout_version > 5 {
            return Err("Unsupported catalog or project identity mismatch".into());
        }
        let mut library = Library {
            root,
            projects: vec![project],
            busy_projects: vec![],
            unreadable: vec![],
            recovery_blocked: None,
            overlay: None,
            staging: None,
            _lock: Some(locks),
        };
        library.journal_maintenance(0, apply, sidecars)
    }

    /// Inspect/compact only completed revision directories. Pending is never modified here.
    pub fn journal_history(&mut self, p: usize, apply: bool) -> Result<Value> {
        self.journal_maintenance(p, apply, false)
    }
    /// [`Self::journal_history`], optionally also replacing large sidecar texts (see
    /// [`Self::maintain_journals`]).
    pub fn journal_maintenance(&mut self, p: usize, apply: bool, sidecars: bool) -> Result<Value> {
        if apply && !self.writable(p) {
            return self.write_project(p, |l, p| l.journal_maintenance(p, true, sidecars));
        }
        let root = self.dir(p);
        let mut rows = vec![];
        let mut total = 0u64;
        let mut reclaimed = 0u64;
        let mut pending_bytes = 0u64;
        let mut large_sidecars = 0u64;
        for directory in ["catalog-transactions", "catalog-transactions-v4"] {
            let folder = root.join("tools/workspaces").join(directory);
            if !folder.exists() {
                continue;
            }
            contained(&root, &folder)?;
            for entry in fs::read_dir(&folder).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                if !entry.file_type().map_err(|e| e.to_string())?.is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                let path = entry.path().join("journal.json");
                if !path.is_file() {
                    continue;
                }
                contained(&root, &path)?;
                let size = fs::metadata(&path).map_err(|e| e.to_string())?.len();
                if name == "pending" {
                    pending_bytes += size;
                    continue;
                }
                let Some(revision) = name
                    .strip_prefix("revision-")
                    .and_then(|s| s.parse::<u64>().ok())
                else {
                    continue;
                };
                let bytes = fs::read(&path).map_err(|e| e.to_string())?;
                let value: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                // What this receipt would be written as, if anything changes.
                let (format, mut output) = if (value["kind"] == "completed-catalog-delta"
                    || value["kind"] == "pending-catalog-delta")
                    && value["schema"] == 1
                {
                    if value["after"]["revision"] != revision
                        || value["after"]["id"] != self.projects[p].id
                    {
                        return Err(format!("History identity mismatch: {}", path.display()));
                    }
                    let mut completed = value;
                    let renamed = completed["kind"] == "pending-catalog-delta";
                    completed["kind"] = "completed-catalog-delta".into();
                    ("delta", (renamed, completed))
                } else {
                    let j: Journal = serde_json::from_value(value)
                        .map_err(|e| format!("{}: {e}", path.display()))?;
                    if j.after.revision != revision
                        || j.after.id != self.projects[p].id
                        || j.before.id != j.after.id
                    {
                        return Err(format!("History identity mismatch: {}", path.display()));
                    }
                    let mut compacted = delta(&j)?;
                    compacted["kind"] = "completed-catalog-delta".into();
                    ("full", (true, compacted))
                };
                let large: Vec<(usize, usize)> = output.1["sidecars"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .enumerate()
                    .filter_map(|(i, s)| Some((i, large_sidecar(s)?)))
                    .collect();
                let sidecar_size: usize = large.iter().map(|(_, size)| size).sum();
                if sidecars {
                    for (i, _) in &large {
                        if let Some(compact) = compact_sidecar(&output.1["sidecars"][*i]) {
                            output.1["sidecars"][*i] = compact;
                            output.0 = true;
                        }
                    }
                }
                let mut retained = size;
                if output.0 {
                    // The receipt's time is its journal's modification time until now; keep it
                    // in the receipt and on the rewritten file, so history dates survive.
                    let modified = fs::metadata(&path).and_then(|m| m.modified()).ok();
                    if output.1["at_unix"].is_null()
                        && let Some(at) =
                            modified.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    {
                        output.1["at_unix"] = at.as_secs().into();
                    }
                    let bytes = serde_json::to_vec(&output.1).map_err(|e| e.to_string())?;
                    retained = bytes.len() as u64;
                    if apply {
                        atomic(&path, &bytes)?;
                        if let Some(modified) = modified {
                            fs::File::options()
                                .write(true)
                                .open(&path)
                                .and_then(|f| f.set_modified(modified))
                                .map_err(|e| at(&path, e))?;
                        }
                    }
                }
                total += size;
                reclaimed += size.saturating_sub(retained);
                large_sidecars += sidecar_size as u64;
                rows.push(
                    json!({"path":path,"format":format,"bytes":size,"compacted_bytes":retained,"large_sidecar_bytes":sidecar_size}),
                );
            }
        }
        rows.sort_by_key(|r| r["path"].as_str().unwrap_or_default().to_owned());
        Ok(
            json!({"project":self.projects[p].id,"applied":apply,"compact_sidecars":sidecars,"completed_bytes":total,"reclaimable_bytes":reclaimed,"large_sidecar_bytes":large_sidecars,"pending_bytes":pending_bytes,"journals":rows}),
        )
    }
}
