//! Annotation changes through Scopework once a project is promoted
//! (docs/design-storage-promotion.md §5). Before promotion the project tools' ledger is
//! authoritative and changes only through them; afterwards the store owns the annotations,
//! `annotations.json` is their export, and each change made here is recorded in the store as
//! it is written. A tool's own rewrite of the file is recorded at the next sync.
use super::*;
use serde_json::{Value, json};
use std::collections::HashMap;

/// A row to set: its key (kind and address) and its fields.
pub struct RowFields<'a> {
    pub kind: &'a str,
    pub rva: &'a str,
    pub value: &'a str,
    pub report: &'a str,
    pub reason: &'a str,
    pub classification: &'a str,
}

fn address(rva: &str) -> Result<String> {
    Ok(format!("0x{:08X}", parse_rva(rva)?))
}
fn same_address(row: &Value, rva: &str) -> bool {
    row["rva"].as_str().and_then(|r| parse_rva(r).ok()) == parse_rva(rva).ok()
}

/// Applies `edit` to the promoted project's ledger under the project's lock, writes it, and
/// records the change in the store, holding the store's lock from reading to recording so the
/// change is recorded with its author. A tool rewriting the ledger meanwhile wins: the edit is
/// applied again to its text.
fn change(
    root: &Path,
    id: &str,
    edit: impl Fn(&mut Value, &Project, &Path) -> Result<Value>,
) -> Result<Value> {
    let lib = Library::open_project(root.to_path_buf(), Some(id))?;
    let project = &lib.projects[0];
    if !promoted(project) {
        return Err(format!(
            "not_promoted: until project {id} is promoted (research store promote), the project tools' ledger is authoritative and annotations change through them"
        ));
    }
    let dir = root.join(id);
    let path = dir.join(ledger::LEDGER);
    let read = || match fs::read(&path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("Cannot read {}: {e}", ledger::LEDGER)),
    };
    let attempt = || -> Result<Value> {
        for _ in 0..5 {
            let bytes = read()?;
            let mut ledger: Value = match &bytes {
                Some(b) => serde_json::from_slice(b).map_err(|e| {
                    format!("The ledger does not parse ({e}); retry once its writer finishes")
                })?,
                None => json!({"schema": 1, "annotations": [], "types": []}),
            };
            let answer = edit(&mut ledger, project, &dir)?;
            if read()? != bytes {
                continue;
            }
            fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
            atomic(
                &path,
                &serde_json::to_vec_pretty(&ledger).map_err(|e| e.to_string())?,
            )?;
            return Ok(answer);
        }
        Err("busy: the ledger kept changing while it was edited; retry".into())
    };
    let (answer, recorded) = store::ledger_change(root, id, &caller(), attempt)?;
    Ok(json!({"project": id, "change": answer, "store": recorded}))
}

/// Sets the row of `fields.kind` at `fields.rva`, pinned to its report's current acceptance
/// hash.
pub fn set(root: &Path, id: &str, fields: &RowFields) -> Result<Value> {
    let rva = address(fields.rva)?;
    change(root, id, |ledger, project, dir| {
        let index = catalog_index::Index::new(project);
        if !index.paths.values().any(|p| p == fields.report) {
            return Err(format!(
                "{} is not a record's report in this project.",
                fields.report
            ));
        }
        let text = fs::read_to_string(dir.join(fields.report)).map_err(|e| e.to_string())?;
        let row = json!({
            "kind": fields.kind,
            "rva": rva,
            "value": fields.value,
            "report": fields.report,
            "report_sha256": acceptance_hash(project, &text),
            "reason": fields.reason,
            "classification": fields.classification,
            "reviewed": true,
        });
        let rows = ledger["annotations"]
            .as_array_mut()
            .ok_or("The ledger has no annotations list")?;
        let at = rows
            .iter()
            .position(|r| r["kind"] == fields.kind && same_address(r, &rva));
        let replaced = at.map(|i| std::mem::replace(&mut rows[i], row.clone()));
        if at.is_none() {
            rows.push(row.clone());
        }
        Ok(json!({"set": row, "replaced": replaced}))
    })
}

/// Removes the row of `kind` at `rva`.
pub fn remove(root: &Path, id: &str, kind: &str, rva: &str) -> Result<Value> {
    let rva = address(rva)?;
    change(root, id, |ledger, _, _| {
        let rows = ledger["annotations"]
            .as_array_mut()
            .ok_or("The ledger has no annotations list")?;
        let at = rows
            .iter()
            .position(|r| r["kind"] == kind && same_address(r, &rva))
            .ok_or_else(|| format!("No {kind} row at {rva}."))?;
        Ok(json!({"removed": rows.remove(at)}))
    })
}

/// Rows and types whose report pin no longer matches their report's acceptance hash (or whose
/// report is gone), in this project's acceptance scope.
pub fn stale(root: &Path, id: &str) -> Result<Value> {
    let lib = Library::read_project(root.to_path_buf(), id)?;
    let project = &lib.projects[0];
    let dir = root.join(id);
    let bytes = fs::read(dir.join(ledger::LEDGER)).map_err(|_| {
        format!(
            "Project {id} has no annotation ledger ({}).",
            ledger::LEDGER
        )
    })?;
    let ledger: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    // Only records' reports are read: a ledger row is the tools' text, and may name any path.
    let index = catalog_index::Index::new(project);
    let reports: std::collections::HashSet<&str> =
        index.paths.values().map(String::as_str).collect();
    let mut hashes: HashMap<String, Option<String>> = HashMap::new();
    let mut found = vec![];
    let mut checked = 0usize;
    for (list, key) in [("annotations", "rva"), ("types", "name")] {
        for row in ledger[list].as_array().into_iter().flatten() {
            let (Some(report), Some(pin)) = (row["report"].as_str(), row["report_sha256"].as_str())
            else {
                continue;
            };
            checked += 1;
            if !reports.contains(report) {
                found.push(json!({
                    "list": list,
                    "kind": row["kind"],
                    key: row[key],
                    "report": report,
                    "pinned": pin,
                    "current": null,
                    "note": "not a record's report",
                }));
                continue;
            }
            let current = hashes
                .entry(report.to_string())
                .or_insert_with(|| {
                    fs::read_to_string(dir.join(report))
                        .ok()
                        .map(|t| acceptance_hash(project, &t))
                })
                .clone();
            if current.as_deref() != Some(pin) {
                found.push(json!({
                    "list": list,
                    "kind": row["kind"],
                    key: row[key],
                    "report": report,
                    "pinned": pin,
                    "current": current,
                }));
            }
        }
    }
    const LISTED: usize = 1000;
    Ok(json!({
        "project": id,
        "acceptance": if promoted(project) { "body" } else { "whole_file" },
        "checked": checked,
        "stale": found.len(),
        "listed": found.iter().take(LISTED).collect::<Vec<_>>(),
        "truncated": found.len() > LISTED,
    }))
}
