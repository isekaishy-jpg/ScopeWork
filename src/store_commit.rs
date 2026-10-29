//! Commits of promoted projects (docs/design-storage-promotion.md §4): the store holds their
//! history, and no receipts are written.
//!
//! The pending journal stays the roll-forward record, as for every project. Applying it takes the
//! store first ([`begin_commit`]), checks its head before any file changes, appends the commit's
//! entry before `project.json` is written, and records the commit's own ledger changes. An entry
//! already appended by an interrupted attempt is recognised by its after-fingerprint, so applying
//! again is harmless. Once the files are applied the pending folder is removed.
use super::*;

/// How long a commit waits for another process writing the store (a sync, the ledger
/// follower) before it reports the store busy.
const WAIT: std::time::Duration = std::time::Duration::from_secs(30);

/// A promoted commit's hold on its project's store: taken before any of the commit's files
/// change and kept until its catalog is written. Holding it keeps the store's head where the
/// commit found it, and keeps the coordinator's ledger follower (which only tries the lock) from
/// recording the commit's ledger changes first, without their author.
pub(crate) struct StoreCommit {
    w: Writer,
    store: PathBuf,
    last: Segment,
}
/// Takes the store of the project in `dir` for commit `delta` (a pending catalog delta, texts
/// included). Refuses with `store_mismatch` when the store's head is neither the catalog the
/// commit starts from nor the one it makes (then nothing of the commit has been applied).
pub(crate) fn begin_commit(dir: &Path, delta: &Value) -> Result<StoreCommit> {
    let store = dir.join(FOLDER);
    let w = Writer::open_waiting(&store, WAIT)?;
    let last = w.active()?;
    let (head_revision, head_sha) = last.head()?;
    if delta["after_hash"] != head_sha.as_str() && delta["before_hash"] != head_sha.as_str() {
        return Err(format!(
            "store_mismatch: the store's head (revision {head_revision}) is neither the catalog this commit starts from nor the one it makes; run research store verify"
        ));
    }
    Ok(StoreCommit { w, store, last })
}
impl StoreCommit {
    /// Appends `delta`, unless an interrupted attempt already did (the head is its after-state).
    /// `after_bytes` is the catalog after it, which checkpoints a new segment when the log is
    /// long.
    pub(crate) fn append(&mut self, delta: &Value, after_bytes: &[u8]) -> Result<()> {
        let (_, head_sha) = self.last.head()?;
        if delta["after_hash"] == head_sha.as_str() {
            return Ok(());
        }
        let revision = delta["after"]["revision"]
            .as_u64()
            .ok_or("Commit without revision")?;
        let entry = commit_entry(delta, revision, Some(unix_now()), &mut self.w)?;
        self.w.flush_texts()?;
        let log = &self.last.log;
        append(
            log,
            &frame(&serde_json::to_vec(&entry).map_err(|e| e.to_string())?)?,
        )?;
        if fs::metadata(log).map_err(|e| e.to_string())?.len() as usize >= segment_bytes() {
            let (next, checkpoint) = segment_files(&self.store, self.last.seq + 1);
            atomic(&checkpoint, &deflate(after_bytes)?)?;
            let start = json!({
                "type": "start",
                "revision": revision,
                "catalog_sha256": delta["after_hash"],
                "at_unix": unix_now(),
                "follows": "commit",
            });
            append(
                &next,
                &frame(&serde_json::to_vec(&start).map_err(|e| e.to_string())?)?,
            )?;
        }
        Ok(())
    }
    /// Records the ledger change this commit made, naming `by`. Problems are answered, never
    /// raised: the commit stands, and the next sync records the change without its author.
    pub(crate) fn record_ledger(&mut self, dir: &Path, revision: u64, by: &str) -> Value {
        ledger::follow_inner(&mut self.w, dir, &self.store, revision, Some(by))
            .unwrap_or_else(|e| json!({"error": e}))
    }
}
/// [`begin_commit`] and [`StoreCommit::append`] together, for a writer that changes no ledger.
pub(crate) fn append_commit(dir: &Path, delta: &Value, after_bytes: &[u8]) -> Result<()> {
    begin_commit(dir, delta)?.append(delta, after_bytes)
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
