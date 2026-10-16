//! Completed history is a change receipt, never an input to pending recovery.
use super::*;
use serde_json::{Value, json};

fn delta(j: &Journal) -> Result<Value> {
    let mut before = serde_json::to_value(&j.before).map_err(|e| e.to_string())?;
    let mut after = serde_json::to_value(&j.after).map_err(|e| e.to_string())?;
    let mut changes = serde_json::Map::new();
    for (field, identity) in [("nodes", "id"), ("dependencies", "")] {
        let a = before.as_object_mut().unwrap().remove(field).unwrap();
        let b = after.as_object_mut().unwrap().remove(field).unwrap();
        let key = |v: &Value| {
            if identity.is_empty() {
                v.to_string()
            } else {
                v[identity].to_string()
            }
        };
        let aa: HashMap<_, _> = a
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(i, v)| (key(v), (i, v)))
            .collect();
        let bb: HashMap<_, _> = b
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(i, v)| (key(v), (i, v)))
            .collect();
        let common_before: Vec<_> = a
            .as_array()
            .unwrap()
            .iter()
            .map(&key)
            .filter(|k| bb.contains_key(k))
            .collect();
        let common_after: Vec<_> = b
            .as_array()
            .unwrap()
            .iter()
            .map(&key)
            .filter(|k| aa.contains_key(k))
            .collect();
        let reordered = common_before != common_after;
        let changed = |own: &(usize, &Value), other: Option<&(usize, &Value)>| {
            other.is_none_or(|other| own.1 != other.1 || (reordered && own.0 != other.0))
        };
        let removed: Vec<_> = a
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .filter(|(i, v)| changed(&(*i, *v), bb.get(&key(v))))
            .map(|(i, v)| json!({"index":i,"record":v}))
            .collect();
        let added: Vec<_> = b
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .filter(|(i, v)| changed(&(*i, *v), aa.get(&key(v))))
            .map(|(i, v)| json!({"index":i,"record":v}))
            .collect();
        changes.insert(field.into(), json!({"before":removed,"after":added}));
    }
    Ok(
        json!({"schema":1,"kind":"completed-catalog-delta","before":before,"after":after,"changes":changes,
        "before_hash":hash(&serde_json::to_string(&j.before).map_err(|e|e.to_string())?),
        "after_hash":hash(&serde_json::to_string(&j.after).map_err(|e|e.to_string())?),
        "edits":j.edits,"moves":j.moves,"dependencies":j.dependencies,"sidecars":j.sidecars}),
    )
}

pub(super) fn pending_bytes(j: &Journal) -> Result<Vec<u8>> {
    let mut value = delta(j)?;
    value["kind"] = "pending-catalog-delta".into();
    serde_json::to_vec(&value).map_err(|e| e.to_string())
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
    let base = serde_json::to_value(current).map_err(|e| e.to_string())?;
    let mut other = value[target].clone();
    for field in ["nodes", "dependencies"] {
        let mut records = base[field]
            .as_array()
            .ok_or("Invalid catalog array")?
            .clone();
        let changes = &value["changes"][field];
        let mut remove: Vec<_> = changes[source]
            .as_array()
            .ok_or("Invalid journal delta")?
            .iter()
            .collect();
        remove.sort_by_key(|r| std::cmp::Reverse(r["index"].as_u64().unwrap_or(u64::MAX)));
        for row in remove {
            let index = row["index"].as_u64().ok_or("Invalid delta index")? as usize;
            if records.get(index) != Some(&row["record"]) {
                return Err("Journal delta input mismatch".into());
            }
            records.remove(index);
        }
        let mut insert: Vec<_> = changes[target]
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
            records.insert(index, row["record"].clone());
        }
        other[field] = records.into();
    }
    let other: Project = serde_json::from_value(other).map_err(|e| e.to_string())?;
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
        before,
        after,
        edits: serde_json::from_value(value["edits"].clone()).map_err(|e| e.to_string())?,
        moves: serde_json::from_value(value["moves"].clone()).map_err(|e| e.to_string())?,
        dependencies: serde_json::from_value(value["dependencies"].clone())
            .map_err(|e| e.to_string())?,
        sidecars: serde_json::from_value(value["sidecars"].clone()).map_err(|e| e.to_string())?,
    })
}

pub(super) fn compact(path: &Path, journal: &Journal) -> Result<()> {
    let mut value: Value = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if value["kind"] == "pending-catalog-delta" && value["schema"] == 1 {
        value["kind"] = "completed-catalog-delta".into();
    } else {
        value = delta(journal)?;
    }
    let bytes = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
    atomic(path, &bytes)
}

impl Library {
    /// History maintenance deliberately bypasses project migration and pending recovery.
    pub fn inspect_journals(root: PathBuf, id: &str, apply: bool) -> Result<Value> {
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
        gate.try_lock_shared().map_err(|_| {
            "Research library is busy with another operation. Retry shortly.".to_owned()
        })?;
        let lock = project_lock(&root, id, apply)?;
        let path = dir.join("project.json");
        contained(&root, &path)?;
        let project: Project = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        if project.id != id || project.layout_version > 5 {
            return Err("Unsupported catalog or project identity mismatch".into());
        }
        let mut library = Library {
            root,
            projects: vec![project],
            busy_projects: vec![],
            _lock: Some(OperationLocks {
                project: Some(id.into()),
                writable: apply,
                _project: Some(lock),
                _gate: gate,
            }),
        };
        library.journal_history(0, apply)
    }

    /// Inspect/compact only completed revision directories. Pending is never modified here.
    pub fn journal_history(&mut self, p: usize, apply: bool) -> Result<Value> {
        if apply && !self.writable(p) {
            return self.write_project(p, |l, p| l.journal_history(p, true));
        }
        let root = self.dir(p);
        let mut rows = vec![];
        let mut total = 0u64;
        let mut reclaimed = 0u64;
        let mut pending_bytes = 0u64;
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
                let mut retained = size;
                let format = if (value["kind"] == "completed-catalog-delta"
                    || value["kind"] == "pending-catalog-delta")
                    && value["schema"] == 1
                {
                    if value["after"]["revision"] != revision
                        || value["after"]["id"] != self.projects[p].id
                    {
                        return Err(format!("History identity mismatch: {}", path.display()));
                    }
                    if apply && value["kind"] == "pending-catalog-delta" {
                        let mut completed = value;
                        completed["kind"] = "completed-catalog-delta".into();
                        atomic(
                            &path,
                            &serde_json::to_vec(&completed).map_err(|e| e.to_string())?,
                        )?;
                    }
                    "delta"
                } else {
                    let j: Journal = serde_json::from_value(value)
                        .map_err(|e| format!("{}: {e}", path.display()))?;
                    if j.after.revision != revision
                        || j.after.id != self.projects[p].id
                        || j.before.id != j.after.id
                    {
                        return Err(format!("History identity mismatch: {}", path.display()));
                    }
                    let compacted = serde_json::to_vec(&delta(&j)?).map_err(|e| e.to_string())?;
                    retained = compacted.len() as u64;
                    if apply {
                        atomic(&path, &compacted)?;
                    }
                    "full"
                };
                total += size;
                reclaimed += size.saturating_sub(retained);
                rows.push(
                    json!({"path":path,"format":format,"bytes":size,"compacted_bytes":retained}),
                );
            }
        }
        rows.sort_by_key(|r| r["path"].as_str().unwrap_or_default().to_owned());
        Ok(
            json!({"project":self.projects[p].id,"applied":apply,"completed_bytes":total,"reclaimable_bytes":reclaimed,"pending_bytes":pending_bytes,"journals":rows}),
        )
    }
}
