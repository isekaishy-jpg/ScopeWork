//! Commits of promoted projects (docs/design-storage-promotion.md §4): the store holds their
//! history, and no receipts are written.
//!
//! The pending journal stays the roll-forward record, as for every project. Applying it appends
//! the commit's entry to the store before `project.json` is written, and an entry already
//! appended by an interrupted attempt is recognised by its after-fingerprint, so applying again
//! is harmless. Once the files are applied the pending folder is removed.
use super::*;

/// How long a commit waits for another process writing the store (a sync, the ledger
/// follower) before it reports the store busy.
const WAIT: std::time::Duration = std::time::Duration::from_secs(30);

/// Appends commit `delta` (a pending catalog delta, texts included) to the store of the project
/// in `dir`, unless the store's head already is its after-state. `after_bytes` is the catalog
/// after it, which checkpoints a new segment when the log is long.
pub(crate) fn append_commit(dir: &Path, delta: &Value, after_bytes: &[u8]) -> Result<()> {
    let store = dir.join(FOLDER);
    let mut w = Writer::open_waiting(&store, WAIT)?;
    w.repair()?;
    let (segments, _) = segments(&store)?;
    let last = segments.last().ok_or(
        "store_mismatch: the promoted project's store has no segment; run research store verify",
    )?;
    let (head_revision, head_sha) = last.head()?;
    if delta["after_hash"] == head_sha.as_str() {
        return Ok(());
    }
    if delta["before_hash"] != head_sha.as_str() {
        return Err(format!(
            "store_mismatch: the store's head (revision {head_revision}) is not the catalog this commit starts from; run research store verify"
        ));
    }
    let revision = delta["after"]["revision"]
        .as_u64()
        .ok_or("Commit without revision")?;
    let entry = commit_entry(delta, revision, Some(unix_now()), &mut w)?;
    w.flush_texts()?;
    append(
        &last.log,
        &frame(&serde_json::to_vec(&entry).map_err(|e| e.to_string())?)?,
    )?;
    if fs::metadata(&last.log).map_err(|e| e.to_string())?.len() as usize >= segment_bytes() {
        let (log, checkpoint) = segment_files(&store, last.seq + 1);
        atomic(&checkpoint, &deflate(after_bytes)?)?;
        let start = json!({
            "type": "start",
            "revision": revision,
            "catalog_sha256": delta["after_hash"],
            "at_unix": unix_now(),
            "follows": "commit",
        });
        append(
            &log,
            &frame(&serde_json::to_vec(&start).map_err(|e| e.to_string())?)?,
        )?;
    }
    Ok(())
}

/// Whether the store of the project in `dir` ends at `revision` with catalog fingerprint `sha`.
pub(crate) fn holds(dir: &Path, revision: u64, sha: &str) -> Result<bool> {
    let (segments, _) = segments(&dir.join(FOLDER))?;
    Ok(match segments.last() {
        Some(last) => last.head()? == (revision, sha.to_string()),
        None => false,
    })
}
/// The store's commit entries newer than `after`, newest first, in the shape history reads
/// from receipts (texts left out): `(revision, receipt summary)`.
pub(crate) fn receipts_after(dir: &Path, after: u64) -> Result<Vec<(u64, Value)>> {
    let (segments, _) = segments(&dir.join(FOLDER))?;
    let mut out = vec![];
    for segment in &segments {
        for f in 1..segment.frames.len() {
            let entry = segment.entry(f)?;
            let Some(revision) = entry["revision"].as_u64().filter(|r| *r > after) else {
                continue;
            };
            let edits: Vec<Value> = entry["reports"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|r| json!({"id": r["id"], "old": r["old_path"], "new": r["new_path"]}))
                .collect();
            let sidecars: Vec<Value> = entry["sidecars"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|s| json!({"path": s["path"]}))
                .collect();
            out.push((
                revision,
                json!({
                    "before": entry["before"],
                    "after": entry["after"],
                    "changes": {"nodes": entry["nodes"], "dependencies": entry["dependencies"]},
                    "edits": edits,
                    "moves": entry["moves"],
                    "sidecars": sidecars,
                    "dependencies": entry["dependency_edits"],
                    "directed_by": entry["directed_by"],
                    "at_unix": entry["at_unix"],
                }),
            ));
        }
    }
    out.reverse();
    Ok(out)
}
