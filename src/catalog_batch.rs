//! Revision-checked catalog changes with a durable, resumable journal.
use super::*;
use std::collections::{HashMap, HashSet};

#[path = "catalog_remove.rs"]
mod catalog_remove;
#[path = "journal_history.rs"]
mod journal_history;
pub use catalog_remove::Removal;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Batch {
    pub schema: u32,
    pub binary: String,
    /// Required (and exact) for schema 1; optional for schema 2, which checks each updated
    /// record's `expect_node_revision` instead.
    #[serde(default)]
    pub expect_revision: Option<u64>,
    #[serde(default)]
    pub repair_legacy_links: bool,
    #[serde(default)]
    pub create: Vec<Create>,
    #[serde(default)]
    pub update: Vec<Update>,
    #[serde(default)]
    pub reorder: Vec<Order>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Create {
    pub parent: u64,
    pub name: String,
    pub rva: Option<u32>,
    #[serde(default)]
    pub context_only: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    pub id: u64,
    pub parent: Option<u64>,
    pub name: Option<String>,
    pub context_only: Option<bool>,
    /// The record's revision as last read; required by schema 2.
    #[serde(default)]
    pub expect_node_revision: Option<u64>,
}
/// What a catalog batch changed.
#[derive(Serialize)]
pub struct BatchOutcome {
    pub created: Vec<u64>,
    /// Report paths of moved records: (node, old path, new path).
    pub moved: Vec<(u64, String, String)>,
    /// Annotation-ledger report paths that now follow their records.
    pub ledger_rows_repathed: usize,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Order {
    pub parent: u64,
    pub children: Vec<u64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportBatch {
    pub schema: u32,
    pub binary: String,
    pub expect_revision: Option<u64>,
    pub reports: Vec<ReportSave>,
    /// Attach generated evidence without starting research: Not Started stays Not Started.
    /// A completed report still reopens, because its accepted text changes.
    #[serde(default)]
    pub evidence_only: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportSave {
    pub id: u64,
    pub expect: String,
    pub body: String,
    pub evidence: String,
}
#[derive(Serialize, Deserialize)]
pub(super) struct Edit {
    pub(super) id: u64,
    pub(super) old: Option<String>,
    pub(super) new: String,
    pub(super) before: Option<String>,
    pub(super) after: String,
}
#[derive(Serialize, Deserialize)]
pub(super) struct Move {
    id: u64,
    old: String,
    new: String,
}
#[derive(Serialize, Deserialize)]
pub(super) struct Sidecar {
    pub(super) path: String,
    pub(super) before: String,
    pub(super) after: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Journal {
    pub(super) before: Project,
    pub(super) after: Project,
    pub(super) edits: Vec<Edit>,
    pub(super) moves: Vec<Move>,
    pub(super) dependencies: Vec<u64>,
    pub(super) sidecars: Vec<Sidecar>,
    /// Repath the annotation ledger when the catalog commits: this transaction's folder moves
    /// (which include archived nodes), then canonical resolution against `after`. See `ledger.rs`.
    #[serde(default)]
    pub(super) ledger: bool,
}

fn parts(text: &str) -> Result<(&str, &str)> {
    text.split_once("\n<!-- research-content -->\n")
        .ok_or("Missing report content marker")?
        .1
        .split_once("\n<!-- research-evidence -->\n")
        .ok_or_else(|| "Missing evidence marker".into())
}

fn same(a: &Project, b: &Project) -> bool {
    a == b
}
/// An I/O error naming the file it concerns, so a locked or missing file can be found.
fn at(path: &Path, e: std::io::Error) -> String {
    format!("{}: {e}", path.display())
}
/// Recovery guidance appended to every conflict that stops a pending transaction.
fn blocked(root: &Path, what: String) -> String {
    format!(
        "{what}. The pending catalog transaction in {} was not applied, and project writes stay blocked until it can resume. Reads still answer from the committed catalog. Restore the named file to the text the journal records as `before` or `after` (or remove the outside edit), then run any project command to resume; keep the journal.",
        pending(root).display()
    )
}
fn pending(root: &Path) -> PathBuf {
    let legacy = root.join("tools/workspaces/catalog-transactions/pending");
    if legacy.join("journal.json").is_file() {
        legacy
    } else {
        root.join("tools/workspaces/catalog-transactions-v4/pending")
    }
}
fn staged(root: &Path, id: u64) -> PathBuf {
    pending(root).join("moves").join(id.to_string())
}
fn contained(root: &Path, path: &Path) -> Result<()> {
    layout::inside(root, path)
}

fn fault(_point: &str) {
    #[cfg(debug_assertions)]
    if std::env::var("SCOPEWORK_CATALOG_FAIL_AT").as_deref() == Ok(_point) {
        std::process::exit(99);
    }
}

/// Completes the pending catalog transaction, if any. Returns how many annotation-ledger
/// report paths it repathed.
pub(super) fn resume(root: &Path, project: &mut Project) -> Result<usize> {
    let folder = pending(root);
    let path = folder.join("journal.json");
    if !path.exists() {
        return Ok(0);
    }
    contained(root, &path)?;
    let j = journal_history::read_pending(&fs::read(&path).map_err(|e| at(&path, e))?, project)
        .map_err(|e| blocked(root, e))?;
    fault("journal");
    if !same(project, &j.before) && !same(project, &j.after) {
        return Err(blocked(
            root,
            "Catalog changed during batch transaction".into(),
        ));
    }
    finish(root, project, &j, None)
}

/// Commits a transaction this process has just built: writes its pending journal, then
/// applies it. Unlike recovery, the journal is not read back and re-verified, and the
/// after-catalog is serialised once for its fingerprint and the catalog write. Returns how
/// many annotation-ledger report paths it repathed.
pub(super) fn commit(root: &Path, project: &mut Project, j: &Journal) -> Result<usize> {
    let started = std::time::Instant::now();
    let (delta, after_bytes) = journal_history::pending_delta(j)?;
    operations::record_commit_time(operations::DELTA, started.elapsed());
    let started = std::time::Instant::now();
    let folder = prepare_pending(root)?;
    atomic(
        &folder.join("journal.json"),
        &serde_json::to_vec(&delta).map_err(|e| e.to_string())?,
    )?;
    operations::record_commit_time(operations::JOURNAL, started.elapsed());
    #[cfg(debug_assertions)]
    if std::env::var("SCOPEWORK_METADATA_FAIL_AT").as_deref() == Ok("journal") {
        std::process::exit(99);
    }
    fault("journal");
    let mut previous: Vec<(String, Option<String>)> = j
        .edits
        .iter()
        .flat_map(|e| {
            let mut files = vec![];
            if let Some(old) = &e.old {
                files.push((old.clone(), e.before.clone()));
            }
            if e.old.as_deref() != Some(e.new.as_str()) {
                files.push((e.new.clone(), None));
            }
            files
        })
        .collect();
    previous.extend(
        j.sidecars
            .iter()
            .map(|s| (s.path.clone(), Some(s.before.clone()))),
    );
    operations::before_commit(&previous);
    let after_hash = delta["after_hash"].as_str().unwrap_or_default().to_string();
    let started = std::time::Instant::now();
    let repathed = finish(root, project, j, Some((delta, after_bytes)))?;
    operations::record_commit_time(operations::APPLY, started.elapsed());
    journal_history::committed(&j.after, &after_hash);
    Ok(repathed)
}

/// Applies a pending journal's reports, moves, sidecars and catalog, then turns the pending
/// folder into the completed receipt. `prepared` carries the delta and after-catalog bytes
/// when the caller built them in memory; recovery passes `None` and re-derives both.
fn finish(
    root: &Path,
    project: &mut Project,
    j: &Journal,
    prepared: Option<(serde_json::Value, Vec<u8>)>,
) -> Result<usize> {
    let folder = pending(root);
    // All existing reports must still match a recorded input or output before resuming.
    for edit in &j.edits {
        let candidates = [
            edit.old.as_ref().map(|s| root.join(s)),
            Some(staged(root, edit.id).join("research.md")),
            Some(root.join(&edit.new)),
        ];
        for candidate in candidates.into_iter().flatten() {
            contained(root, &candidate)?;
            if candidate.is_file() {
                let text = fs::read_to_string(&candidate).map_err(|e| at(&candidate, e))?;
                if Some(&text) != edit.before.as_ref() && text != edit.after {
                    return Err(blocked(
                        root,
                        format!("Report changed during batch: {}", candidate.display()),
                    ));
                }
            }
        }
        if edit.before.is_some()
            && !edit.old.as_ref().is_some_and(|s| root.join(s).is_file())
            && !staged(root, edit.id).join("research.md").is_file()
            && !root.join(&edit.new).is_file()
        {
            return Err(
                "Batch source report is missing; restore it from the journal before continuing."
                    .into(),
            );
        }
    }
    // Journals from 0.8.0 and earlier carry whole annotation-ledger copies. A tool's later write
    // no longer blocks them: the ledger is repathed in place instead, like newer journals.
    let mut repath_ledger = j.ledger;
    for file in &j.sidecars {
        let path = root.join(&file.path);
        contained(root, &path)?;
        if file.path == ledger::LEDGER {
            repath_ledger = true;
            continue;
        }
        let current = fs::read_to_string(&path).map_err(|e| at(&path, e))?;
        if current != file.before && current != file.after {
            return Err(blocked(
                root,
                format!("Sidecar changed during batch: {}", file.path),
            ));
        }
    }
    // Extract deepest folders first so parents and children can move independently.
    let mut moves: Vec<_> = j.moves.iter().collect();
    moves.sort_by_key(|m| std::cmp::Reverse(m.old.matches('/').count()));
    for m in &moves {
        let from = root.join(&m.old);
        let stage = staged(root, m.id);
        let to = root.join(&m.new);
        contained(root, &from)?;
        contained(root, &stage)?;
        contained(root, &to)?;
        if from.exists() {
            if stage.exists() || to.exists() {
                return Err(format!(
                    "Batch move destination already exists for node {}",
                    m.id
                ));
            }
            fs::create_dir_all(stage.parent().unwrap()).map_err(|e| e.to_string())?;
            rename_retrying(&from, &stage).map_err(|e| at(&from, e))?;
        }
    }
    moves.sort_by_key(|m| m.new.matches('/').count());
    for m in moves {
        let stage = staged(root, m.id);
        let to = root.join(&m.new);
        if stage.exists() {
            if to.exists() {
                return Err(format!("Refusing to overwrite {}", to.display()));
            }
            fs::create_dir_all(to.parent().unwrap()).map_err(|e| e.to_string())?;
            rename_retrying(&stage, &to).map_err(|e| at(&stage, e))?;
        }
    }
    fault("moves");
    let functions: HashSet<_> = j
        .after
        .nodes
        .iter()
        .filter(|n| n.kind == Kind::Function)
        .map(|n| n.id)
        .collect();
    for edit in &j.edits {
        let path = root.join(&edit.new);
        fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        if functions.contains(&edit.id) {
            fs::create_dir_all(path.parent().unwrap().join("data")).map_err(|e| e.to_string())?;
        }
        if fs::read_to_string(&path).ok().as_ref() != Some(&edit.after) {
            atomic(&path, edit.after.as_bytes())?;
        }
    }
    for file in j.sidecars.iter().filter(|f| f.path != ledger::LEDGER) {
        atomic(&root.join(&file.path), file.after.as_bytes())?;
    }
    let ledger_rows = if repath_ledger {
        let moved: Vec<(String, String)> = j
            .moves
            .iter()
            .map(|m| (m.old.clone(), m.new.clone()))
            .collect();
        ledger::apply(root, &j.after, &moved)?
    } else {
        0
    };
    fault("reports");
    let temporary = Library {
        root: root.parent().ok_or("Missing library directory")?.into(),
        projects: vec![j.after.clone()],
        busy_projects: vec![],
        _lock: None,
        recovery_blocked: None,
        overlay: None,
        staging: None,
    };
    if !j.dependencies.is_empty() {
        temporary.sync_dependency_subset(0, Some(&j.dependencies.iter().copied().collect()))?;
    }
    let (delta, after_bytes) = match prepared {
        Some((delta, bytes)) => (Some(delta), bytes),
        None => (None, catalog_bytes(&j.after)?),
    };
    atomic(&root.join("project.json"), &after_bytes)?;
    *project = j.after.clone();
    fault("catalog");
    let receipt = folder.with_file_name(format!("revision-{}", project.revision));
    if receipt.exists() {
        return Err("Batch receipt already exists; preserve the pending journal.".into());
    }
    rename_retrying(&folder, &receipt).map_err(|e| at(&folder, e))?;
    match delta {
        Some(mut delta) => {
            delta["kind"] = "completed-catalog-delta".into();
            atomic(
                &receipt.join("journal.json"),
                &serde_json::to_vec(&delta).map_err(|e| e.to_string())?,
            )?;
        }
        None => journal_history::compact(&receipt.join("journal.json"), j)?,
    }
    Ok(ledger_rows)
}

/// Review flags live in a journaled sidecar beside the catalog, keyed by node ID, so older
/// Scopework versions leave them intact instead of dropping an unknown catalog field.
pub const REVIEW_FLAGS: &str = "review-flags.json";
#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub struct ReviewFlag {
    pub reason: String,
    pub flagged_at_revision: u64,
}
pub(super) fn read_flags(root: &Path) -> Result<std::collections::BTreeMap<u64, ReviewFlag>> {
    let path = root.join(REVIEW_FLAGS);
    if !path.is_file() {
        return Ok(Default::default());
    }
    parse_flags(&fs::read_to_string(&path).map_err(|e| e.to_string())?)
}
fn parse_flags(text: &str) -> Result<std::collections::BTreeMap<u64, ReviewFlag>> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if value["schema"] != 1 {
        return Err("Unsupported review flag file".into());
    }
    serde_json::from_value(value["flags"].clone()).map_err(|e| e.to_string())
}
fn flags_text(flags: &std::collections::BTreeMap<u64, ReviewFlag>) -> Result<String> {
    serde_json::to_string_pretty(&serde_json::json!({"schema":1,"flags":flags}))
        .map_err(|e| e.to_string())
}
impl Library {
    pub fn review_flags(&self, p: usize) -> Result<std::collections::BTreeMap<u64, ReviewFlag>> {
        match self.text_opt(p, REVIEW_FLAGS)? {
            Some(text) => parse_flags(&text),
            None => Ok(Default::default()),
        }
    }
    /// Flag (Some reason) or clear (None) a record for review. Flags never change status; they block completion.
    pub fn set_review_flag(&mut self, p: usize, id: u64, reason: Option<&str>) -> Result<()> {
        if !self.writable(p) {
            return self.write_project(p, |l, p| l.set_review_flag(p, id, reason));
        }
        if !self.projects[p].nodes.iter().any(|n| n.id == id) {
            return Err("Unknown node".into());
        }
        let before_text = match self.text_opt(p, REVIEW_FLAGS)? {
            Some(text) => text,
            None => {
                // An empty file first, so the transaction has a `before` to recover to.
                let empty = flags_text(&Default::default())?;
                atomic(&self.dir(p).join(REVIEW_FLAGS), empty.as_bytes())?;
                empty
            }
        };
        let mut flags = parse_flags(&before_text)?;
        match reason.map(str::trim) {
            Some("") => return Err("A review flag needs a short reason.".into()),
            Some(reason) => {
                flags.insert(
                    id,
                    ReviewFlag {
                        reason: reason.into(),
                        flagged_at_revision: self.projects[p].revision + 1,
                    },
                );
            }
            None => {
                if flags.remove(&id).is_none() {
                    return Ok(());
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
            sidecars: vec![Sidecar {
                path: REVIEW_FLAGS.into(),
                before: before_text,
                after: flags_text(&flags)?,
            }],
            ledger: false,
        };
        self.commit_or_stage(p, journal).map(|_| ())
    }
}
impl Library {
    /// Replaces a journaled sidecar beside the catalog (claims, review queue) in one catalog
    /// transaction, so it is recoverable and appears in history like a catalog change.
    pub(super) fn write_sidecar(&mut self, p: usize, name: &str, after_text: String) -> Result<()> {
        let root = self.dir(p);
        let field = if name == coordination::CLAIMS {
            "claims"
        } else {
            "entries"
        };
        let before_text = match self.text_opt(p, name)? {
            Some(text) => text,
            None => coordination::ensure(&root, name, field)?,
        };
        if before_text == after_text {
            return Ok(());
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
            sidecars: vec![Sidecar {
                path: name.into(),
                before: before_text,
                after: after_text,
            }],
            ledger: false,
        };
        self.commit_or_stage(p, journal).map(|_| ())
    }
}
/// Return started-but-incomplete records to Not Started (for example after generated evidence was attached).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusReset {
    pub schema: u32,
    pub binary: String,
    pub expect_revision: u64,
    #[serde(default)]
    pub ids: Vec<u64>,
    /// Apply to every In Progress record instead of listing IDs.
    #[serde(default)]
    pub all_in_progress: bool,
}
impl Library {
    pub fn reset_status(&mut self, p: usize, request: &StatusReset) -> Result<Vec<u64>> {
        if !self.writable(p) {
            return self.write_project(p, |l, p| l.reset_status(p, request));
        }
        let before = self.projects[p].clone();
        if request.schema != 1
            || request.binary != before.binary
            || request.expect_revision != before.revision
            || request.all_in_progress == !request.ids.is_empty()
        {
            return Err("Status reset requires schema 1, the exact binary and revision, and either ids or all_in_progress.".into());
        }
        let wanted: HashSet<u64> = request.ids.iter().copied().collect();
        if wanted
            .iter()
            .any(|id| !before.nodes.iter().any(|n| n.id == *id))
        {
            return Err("Unknown node in status reset".into());
        }
        let mut after = before.clone();
        let mut reset = vec![];
        for n in &mut after.nodes {
            if n.status == Status::InProgress && (request.all_in_progress || wanted.contains(&n.id))
            {
                n.status = Status::NotStarted;
                n.revision += 1;
                reset.push(n.id);
            }
        }
        if reset.is_empty() {
            return Ok(reset);
        }
        after.revision += 1;
        self.commit_metadata(p, before, after, vec![], vec![])?;
        Ok(reset)
    }
}
impl Library {
    pub(super) fn commit_report(
        &mut self,
        p: usize,
        before: Project,
        after: Project,
        id: u64,
        original: String,
        output: String,
    ) -> Result<()> {
        self.commit_metadata(p, before, after, vec![(id, original, output)], vec![])
    }
    pub(super) fn commit_metadata(
        &mut self,
        p: usize,
        before: Project,
        after: Project,
        reports: Vec<(u64, String, String)>,
        dependencies: Vec<u64>,
    ) -> Result<()> {
        let index = catalog_index::Index::new(&before);
        let edits = reports
            .into_iter()
            .map(|(id, original, output)| {
                let path = index.paths[&id].clone();
                Edit {
                    id,
                    old: Some(path.clone()),
                    new: path,
                    before: Some(original),
                    after: output,
                }
            })
            .collect();
        let journal = Journal {
            before,
            after,
            edits,
            moves: vec![],
            dependencies,
            sidecars: vec![],
            ledger: false,
        };
        self.commit_or_stage(p, journal).map(|_| ())
    }
    pub fn read_reports(&self, p: usize, ids: &[u64]) -> Result<serde_json::Value> {
        let project = &self.projects[p];
        let index = catalog_index::Index::new(project);
        let mut reports = Vec::with_capacity(ids.len());
        let mut seen = HashSet::new();
        for id in ids {
            let node = index.nodes.get(id).ok_or("Unknown report")?;
            if !seen.insert(id) {
                return Err("Duplicate report ID".into());
            }
            let path = self.dir(p).join(&index.paths[id]);
            let text = self.text(p, &index.paths[id])?;
            let (body, evidence) = parts(&text)?;
            reports.push(serde_json::json!({"id":id,"report":path,"body":body,"evidence":evidence,
                "token":edit_tokens::draft(project,&index,*id,&text)?,"criteria":node.kind.completion_criteria()}));
        }
        Ok(
            serde_json::json!({"project":project.id,"binary":project.binary,"revision":project.revision,"reports":reports}),
        )
    }

    /// Independently validate all reports against one snapshot before any writes.
    /// Reuse the catalog transaction journal; saving can never attest completion.
    pub fn save_reports(&mut self, p: usize, batch: &ReportBatch) -> Result<Vec<u64>> {
        if !self.writable(p) {
            return self.write_project(p, |l, p| l.save_reports(p, batch));
        }
        let before = self.projects[p].clone();
        if !matches!(batch.schema, 1 | 2)
            || batch.binary != before.binary
            || (batch.schema == 1 && batch.expect_revision.is_none())
            || batch.expect_revision.is_some_and(|r| r != before.revision)
        {
            return Err("Report batch schema, binary identity or revision does not match. Reload and review.".into());
        }
        if batch.reports.is_empty() {
            return Err("Report batch is empty".into());
        }
        let root = self.dir(p);
        let index = catalog_index::Index::new(&before);
        let mut seen = HashSet::new();
        let mut edits = vec![];
        let mut statuses = HashMap::new();
        for input in &batch.reports {
            let node = index.nodes.get(&input.id).ok_or("Unknown report")?;
            if !seen.insert(input.id) {
                return Err("Duplicate report ID".into());
            }
            if input.body.contains("<!-- research-") || input.evidence.contains("<!-- research-") {
                return Err("Reserved report markers cannot appear in report content.".into());
            }
            let path = &index.paths[&input.id];
            contained(&root, &root.join(path))?;
            let original = self.text(p, path)?;
            let current = if input.expect.starts_with("d4:") {
                edit_tokens::draft(&before, &index, input.id, &original)?
            } else {
                hash(&format!("{}\n{original}", before.revision))
            };
            if current != input.expect {
                return Err(format!(
                    "Report {} changed. Reload and review the batch; no reports were saved.",
                    input.id
                ));
            }
            parts(&original)?;
            let output = index.render(&before, input.id, &input.body, &input.evidence);
            if original == output && node.status != Status::Complete {
                continue;
            }
            if !batch.evidence_only {
                statuses.insert(
                    input.id,
                    if input.body.trim().is_empty() && input.evidence.trim().is_empty() {
                        Status::NotStarted
                    } else {
                        Status::InProgress
                    },
                );
            }
            edits.push(Edit {
                id: input.id,
                old: Some(path.clone()),
                new: path.clone(),
                before: Some(original),
                after: output,
            });
        }
        let changed: Vec<_> = edits.iter().map(|e| e.id).collect();
        if changed.is_empty() {
            return Ok(changed);
        }
        let mut after = before.clone();
        let edited: HashSet<u64> = changed.iter().copied().collect();
        for node in &mut after.nodes {
            if edited.contains(&node.id) {
                reopen::mark_changed(node, statuses.get(&node.id).copied());
            }
        }
        let consumers = reopen::consumers_of(&before.dependencies);
        reopen::reopen_affected(&mut after, &edited, &HashMap::new(), &consumers);
        after.revision += 1;
        let journal = Journal {
            before,
            after,
            edits,
            moves: vec![],
            dependencies: vec![],
            sidecars: vec![],
            ledger: false,
        };
        self.commit_or_stage(p, journal)?;
        Ok(changed)
    }

    pub fn batch(&mut self, p: usize, batch: &Batch) -> Result<Vec<u64>> {
        self.apply_batch(p, batch).map(|outcome| outcome.created)
    }
    /// Applies a catalog batch and reports what it created and moved.
    pub fn apply_batch(&mut self, p: usize, batch: &Batch) -> Result<BatchOutcome> {
        if !self.writable(p) {
            return self.write_project(p, |l, p| l.apply_batch(p, batch));
        }
        self.ensure_structural(p)?;
        let before = self.projects[p].clone();
        let preconditions = match batch.schema {
            1 => batch.expect_revision == Some(before.revision),
            // Schema 2: record-level preconditions, so unrelated commits do not conflict.
            2 => batch.expect_revision.is_none_or(|r| r == before.revision),
            _ => false,
        };
        if !preconditions || batch.binary != before.binary {
            return Err("Batch schema, exact binary identity or project revision does not match. Reload and review the batch.".into());
        }
        if batch.schema == 2 {
            for u in &batch.update {
                let current = before
                    .nodes
                    .iter()
                    .find(|n| n.id == u.id)
                    .map(|n| n.revision);
                match (u.expect_node_revision, current) {
                    (None, _) => {
                        return Err(format!(
                            "Schema-2 batch updates need expect_node_revision (node {} has none).",
                            u.id
                        ));
                    }
                    (Some(expected), Some(actual)) if expected != actual => {
                        return Err(format!(
                            "record_conflict: node {} changed (revision {actual}, expected {expected}); reload and review it.",
                            u.id
                        ));
                    }
                    _ => {}
                }
            }
        }
        let mut after = before.clone();
        let mut positions: HashMap<_, _> = after
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id, i))
            .collect();
        let mut rvas: HashSet<_> = after.nodes.iter().filter_map(|n| n.rva).collect();
        let mut changed = HashSet::new();
        let mut created = vec![];
        for c in &batch.create {
            let parent = *positions
                .get(&c.parent)
                .ok_or("Unknown batch parent; create parents before children")?;
            let kind = after.nodes[parent]
                .kind
                .child()
                .ok_or("Functions cannot own children")?;
            if c.name.trim().is_empty() || (kind == Kind::Function) != c.rva.is_some() {
                return Err("Batch node requires a name and an RVA only for functions".into());
            }
            if let Some(rva) = c.rva
                && (before.binary.trim().is_empty() || !rvas.insert(rva))
            {
                return Err("Empty binary identity or duplicate function RVA in batch".into());
            }
            let id = after.next_id;
            after.next_id = id.checked_add(1).ok_or("Node ID exhausted")?;
            positions.insert(id, after.nodes.len());
            after.nodes.push(Node {
                id,
                parent: Some(c.parent),
                kind,
                name: c.name.trim().into(),
                rva: c.rva,
                context_only: c.context_only,
                status: Status::NotStarted,
                revision: 0,
                accepted_hash: None,
                accepted_prerequisites: Default::default(),
            });
            created.push(id);
            changed.insert(id);
            changed.insert(c.parent);
        }
        let mut updates = HashSet::new();
        for u in &batch.update {
            if !updates.insert(u.id) {
                return Err("Duplicate node update in batch".into());
            }
            let i = *positions.get(&u.id).ok_or("Unknown update node")?;
            if let Some(parent) = u.parent {
                let p = *positions.get(&parent).ok_or("Unknown new parent")?;
                if after.nodes[p].kind.child() != Some(after.nodes[i].kind) {
                    return Err(
                        "Move must preserve App / Architecture / Chunk / Function hierarchy".into(),
                    );
                }
                if let Some(old) = after.nodes[i].parent {
                    changed.insert(old);
                }
                after.nodes[i].parent = Some(parent);
                changed.insert(parent);
            }
            if let Some(name) = &u.name {
                if name.trim().is_empty() {
                    return Err("Node name cannot be empty".into());
                }
                after.nodes[i].name = name.trim().into();
            }
            if let Some(context) = u.context_only {
                if u.id == 0 && context {
                    return Err("App cannot be context-only".into());
                }
                after.nodes[i].context_only = context;
            }
            changed.insert(u.id);
            if let Some(parent) = after.nodes[i].parent {
                changed.insert(parent);
            }
        }
        let mut ordered = HashSet::new();
        for order in &batch.reorder {
            if !positions.contains_key(&order.parent) || !ordered.insert(order.parent) {
                return Err("Unknown or duplicate reorder parent".into());
            }
            let slots: Vec<_> = after
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, n)| n.parent == Some(order.parent))
                .map(|(i, _)| i)
                .collect();
            let expected: HashSet<_> = slots.iter().map(|i| after.nodes[*i].id).collect();
            if order.children.len() != expected.len()
                || order.children.iter().copied().collect::<HashSet<_>>() != expected
            {
                return Err("Reorder must list every child exactly once".into());
            }
            let nodes: Vec<_> = order
                .children
                .iter()
                .map(|id| after.nodes[positions[id]].clone())
                .collect();
            for (slot, node) in slots.into_iter().zip(nodes) {
                after.nodes[slot] = node;
            }
            positions = after
                .nodes
                .iter()
                .enumerate()
                .map(|(i, n)| (n.id, i))
                .collect();
            changed.insert(order.parent);
        }
        if changed.is_empty() && !batch.repair_legacy_links {
            return Ok(BatchOutcome {
                created,
                moved: vec![],
                ledger_rows_repathed: 0,
            });
        }
        let old = catalog_index::Index::new(&before);
        let new = catalog_index::Index::new(&after);
        let root = self.dir(p);
        let mut moves = vec![];
        let mut rewrites = vec![];
        for n in &before.nodes {
            if old.paths[&n.id] != new.paths[&n.id] {
                let from = old.paths[&n.id]
                    .trim_end_matches("/research.md")
                    .to_string();
                let to = new.paths[&n.id]
                    .trim_end_matches("/research.md")
                    .to_string();
                if root.join(&to).exists() {
                    return Err(format!("Move destination exists: {to}"));
                }
                moves.push(Move {
                    id: n.id,
                    old: from.clone(),
                    new: to.clone(),
                });
                rewrites.push((from, to));
                changed.insert(n.id);
            }
        }
        if batch.repair_legacy_links {
            for n in &after.nodes {
                let from = match n.kind {
                    Kind::Chunk => Some(format!("chunks/{:06}", n.id)),
                    Kind::Function => Some(format!("functions/{:08X}", n.rva.unwrap())),
                    _ => None,
                };
                if let Some(from) = from {
                    rewrites.push((
                        from,
                        new.paths[&n.id].trim_end_matches("/research.md").into(),
                    ));
                }
            }
        }
        let rewriter = layout::Rewriter::new(&rewrites, &root);
        rewrites.sort_by_key(|(from, _)| std::cmp::Reverse(from.len()));
        // Only reports that mention a moved folder can hold a reference that must change;
        // legacy link repair still reads every report.
        let mentioning = if rewrites.is_empty() || batch.repair_legacy_links {
            HashSet::new()
        } else {
            let folders: HashSet<String> = moves
                .iter()
                .filter_map(|m| m.old.rsplit('/').next())
                .map(str::to_uppercase)
                .collect();
            let reports: Vec<&str> = before
                .nodes
                .iter()
                .filter(|n| !changed.contains(&n.id))
                .map(|n| old.paths[&n.id].as_str())
                .collect();
            report_hashes::mentioning(&root, &reports, &folders)
        };
        let mut edits = vec![];
        for n in &after.nodes {
            let original = if let Some(path) = old.paths.get(&n.id) {
                if changed.contains(&n.id) || batch.repair_legacy_links || mentioning.contains(path)
                {
                    Some(fs::read_to_string(root.join(path)).map_err(|e| e.to_string())?)
                } else {
                    continue;
                }
            } else {
                if root
                    .join(&new.paths[&n.id])
                    .parent()
                    .is_some_and(|p| p.exists())
                {
                    return Err(
                        "New canonical folder already exists; refusing to adopt unrelated files"
                            .into(),
                    );
                }
                None
            };
            let (body, evidence) = original
                .as_deref()
                .map(parts)
                .transpose()?
                .unwrap_or(("", ""));
            let old_path = old.paths.get(&n.id).unwrap_or(&new.paths[&n.id]);
            let body = if rewrites.is_empty() {
                body.into()
            } else {
                rewriter.rewrite(body, old_path, &new.paths[&n.id])
            };
            let evidence = if rewrites.is_empty() {
                evidence.into()
            } else {
                rewriter.rewrite(evidence, old_path, &new.paths[&n.id])
            };
            let output = new.render(&after, n.id, &body, &evidence);
            if original.as_ref() != Some(&output) || old_path != &new.paths[&n.id] {
                changed.insert(n.id);
                edits.push(Edit {
                    id: n.id,
                    old: old.paths.get(&n.id).cloned(),
                    new: new.paths[&n.id].clone(),
                    before: original,
                    after: output,
                });
            }
        }
        // Changed ownership, membership, ordering or linked evidence requires review of the changed
        // records and of the complete records they reach, up both old and new trees.
        let old_parents: HashMap<u64, u64> = old
            .nodes
            .iter()
            .filter_map(|(id, n)| {
                let parent = n.parent?;
                (new.nodes.get(id).and_then(|m| m.parent) != Some(parent)).then_some((*id, parent))
            })
            .collect();
        let mut dependencies: HashSet<u64> = created
            .iter()
            .copied()
            .filter(|id| new.nodes[id].kind == Kind::Function)
            .collect();
        for m in &moves {
            if new.nodes[&m.id].kind == Kind::Function {
                dependencies.insert(m.id);
            }
        }
        for e in &after.dependencies {
            if dependencies.contains(&e.dependent) || dependencies.contains(&e.prerequisite) {
                dependencies.insert(e.dependent);
                dependencies.insert(e.prerequisite);
            }
        }
        drop(new);
        let existing: HashSet<u64> = changed
            .iter()
            .copied()
            .filter(|id| old.nodes.contains_key(id))
            .collect();
        for n in &mut after.nodes {
            if existing.contains(&n.id) {
                reopen::mark_changed(n, None);
            }
        }
        let consumers = reopen::consumers_of(&after.dependencies);
        let reached = reopen::reopen_affected(&mut after, &existing, &old_parents, &consumers);
        after.revision += 1;
        after.layout_version = before.layout_version.max(3);
        after.structural_epoch += 1;
        let mut sidecars = vec![];
        // Completed records reopened by this change need their owner's review: queue them.
        let reopened: Vec<u64> = before
            .nodes
            .iter()
            .filter(|n| n.status == Status::Complete && existing.contains(&n.id))
            .map(|n| n.id)
            .chain(reached)
            .collect();
        if !reopened.is_empty() {
            let moved: Vec<u64> = batch
                .update
                .iter()
                .filter(|u| u.parent.is_some())
                .map(|u| u.id)
                .collect();
            let original = coordination::ensure(&root, coordination::REVIEW_QUEUE, "entries")?;
            let updated = coordination::queue_reopened(
                &original,
                &after,
                &reopened,
                &coordination::batch_reason(&moved),
            )?;
            sidecars.push(Sidecar {
                path: coordination::REVIEW_QUEUE.into(),
                before: original,
                after: updated,
            });
        }
        let moved: Vec<(u64, String, String)> = edits
            .iter()
            .filter_map(|e| Some((e.id, e.old.clone()?, e.new.clone())))
            .filter(|(_, old, new)| old != new)
            .collect();
        let j = Journal {
            before,
            after,
            edits,
            ledger: !moves.is_empty(),
            moves,
            dependencies: dependencies.into_iter().collect(),
            sidecars,
        };
        let ledger_rows_repathed = commit(&root, &mut self.projects[p], &j)?;
        Ok(BatchOutcome {
            created,
            moved,
            ledger_rows_repathed,
        })
    }
}

fn prepare_pending(root: &Path) -> Result<PathBuf> {
    let folder = pending(root);
    contained(root, &folder)?;
    if folder.exists() {
        for entry in fs::read_dir(&folder).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let temp = name
                .strip_prefix("journal.")
                .and_then(|s| s.strip_suffix(".pending"))
                .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok());
            if !temp || !entry.file_type().map_err(|e| e.to_string())?.is_file() {
                return Err(
                    "Pending transaction exists; reopen to recover or inspect its journal".into(),
                );
            }
        }
    }
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    Ok(folder)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Workspace(PathBuf);
    impl Workspace {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target/test-workspaces")
                .join(format!(
                    "{}-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }
        fn populate(&self, id: &str) -> Library {
            let mut lib = Library::open_project(self.0.clone(), Some(id)).unwrap();
            lib.create(id, id, "synthetic binary, image base 0x400000")
                .unwrap();
            let a = lib.add(0, 0, "Architecture", "").unwrap();
            let c = lib.add(0, a, "Chunk", "").unwrap();
            lib.add(0, c, "Producer", "1000").unwrap();
            lib.add(0, c, "Consumer", "2000").unwrap();
            lib
        }
    }
    impl Drop for Workspace {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    fn inputs(lib: &Library, ids: &[u64]) -> ReportBatch {
        let view = lib.read_reports(0, ids).unwrap();
        ReportBatch {
            schema: 1,
            binary: lib.projects[0].binary.clone(),
            expect_revision: Some(lib.projects[0].revision),
            reports: view["reports"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| ReportSave {
                    id: r["id"].as_u64().unwrap(),
                    expect: r["token"].as_str().unwrap().into(),
                    body: format!("Research for {}", r["id"]),
                    evidence: "Synthetic fixture evidence".into(),
                })
                .collect(),
            evidence_only: false,
        }
    }
    #[test]
    fn project_locks_isolate_writes_and_preserve_gui_snapshots() {
        let workspace = Workspace::new();
        drop(workspace.populate("alpha"));
        drop(workspace.populate("beta"));
        let mut gui = Library::open(workspace.0.clone()).unwrap();
        let mut alpha = Library::open_project(workspace.0.clone(), Some("alpha")).unwrap();
        assert!(Library::open_project(workspace.0.clone(), Some("alpha")).is_err());
        let mut beta = Library::open_project(workspace.0.clone(), Some("beta")).unwrap();
        beta.save_reports(0, &inputs(&beta, &[3, 4])).unwrap();
        drop(beta);
        gui.refresh().unwrap();
        assert_eq!(gui.projects.len(), 2);
        assert_eq!(gui.busy_projects, vec!["alpha"]);
        let b = gui.projects.iter().position(|p| p.id == "beta").unwrap();
        gui.set_scope(b, "Assets", gui.projects[b].revision)
            .unwrap();
        assert_eq!(gui.projects.len(), 2);
        alpha
            .set_scope(0, "Rendering", alpha.projects[0].revision)
            .unwrap();
        drop(alpha);
        gui.refresh().unwrap();
        assert!(gui.busy_projects.is_empty());
        assert_eq!(
            gui.projects
                .iter()
                .find(|p| p.id == "alpha")
                .unwrap()
                .research_scope,
            "Rendering"
        );
    }
    #[test]
    fn legacy_exclusive_gate_and_new_shared_gate_exclude_each_other() {
        let workspace = Workspace::new();
        drop(workspace.populate("alpha"));
        let gate = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(workspace.0.join(".library.lock"))
            .unwrap();
        gate.try_lock().unwrap();
        assert!(Library::open_project(workspace.0.clone(), Some("alpha")).is_err());
        gate.unlock().unwrap();
        let alpha = Library::open_project(workspace.0.clone(), Some("alpha")).unwrap();
        assert!(gate.try_lock().is_err());
        drop(alpha);
        gate.try_lock().unwrap();
    }
    #[test]
    fn stale_later_token_prevents_every_write() {
        let workspace = Workspace::new();
        let mut lib = workspace.populate("alpha");
        let mut batch = inputs(&lib, &[3, 4]);
        let original = fs::read(lib.path(0, 3).unwrap()).unwrap();
        let revision = lib.projects[0].revision;
        batch.reports[1].expect = "stale".into();
        assert!(lib.save_reports(0, &batch).is_err());
        assert_eq!(fs::read(lib.path(0, 3).unwrap()).unwrap(), original);
        assert_eq!(lib.projects[0].revision, revision);
        assert!(!pending(&lib.dir(0)).exists());
        let mut external = inputs(&lib, &[3, 4]);
        fs::write(lib.path(0, 4).unwrap(), "external edit").unwrap();
        assert!(lib.save_reports(0, &external).is_err());
        external.expect_revision = external.expect_revision.map(|r| r + 1);
        assert!(lib.save_reports(0, &external).is_err());
        assert_eq!(fs::read(lib.path(0, 3).unwrap()).unwrap(), original);
    }
    #[test]
    fn malformed_batches_are_rejected_before_publication() {
        let workspace = Workspace::new();
        let mut lib = workspace.populate("alpha");
        let mut batch = inputs(&lib, &[3, 4]);
        batch.reports[1].id = 3;
        assert!(lib.save_reports(0, &batch).is_err());
        batch = inputs(&lib, &[3, 4]);
        batch.reports[1].body = "<!-- research-content -->".into();
        assert!(lib.save_reports(0, &batch).is_err());
        batch = inputs(&lib, &[3, 4]);
        batch.binary = "wrong build".into();
        assert!(lib.save_reports(0, &batch).is_err());
        batch = inputs(&lib, &[3, 4]);
        batch.reports[1].id = 9999;
        assert!(lib.save_reports(0, &batch).is_err());
        assert!(serde_json::from_value::<ReportBatch>(serde_json::json!({"schema":1,"binary":"b","expect_revision":1,"reports":[],"complete":true})).is_err());
        assert!(!pending(&lib.dir(0)).exists());
    }
    #[test]
    fn batch_reopens_consumers_and_ancestors_preserving_evidence_files() {
        let workspace = Workspace::new();
        let mut lib = workspace.populate("alpha");
        lib.set_dependencies(0, 4, "1000").unwrap();
        for id in [3, 4, 2, 1, 0] {
            let token = lib.completion_token(0, id).unwrap();
            lib.save_report(0, id, "Reviewed fixture", "Fixture evidence", true, &token)
                .unwrap();
        }
        assert_eq!(lib.effective(0, 0), Status::Complete);
        let data = lib
            .path(0, 3)
            .unwrap()
            .parent()
            .unwrap()
            .join("data/capture.bin");
        fs::write(&data, [1, 2, 3, 4]).unwrap();
        let batch = inputs(&lib, &[3]);
        let revision = lib.projects[0].revision;
        assert_eq!(lib.save_reports(0, &batch).unwrap(), vec![3]);
        assert_eq!(lib.projects[0].revision, revision + 1);
        assert!(
            lib.projects[0]
                .nodes
                .iter()
                .all(|n| n.status == Status::InProgress && n.accepted_hash.is_none())
        );
        assert_eq!(fs::read(data).unwrap(), vec![1, 2, 3, 4]);
        assert!(
            lib.dir(0)
                .join(format!(
                    "tools/workspaces/catalog-transactions-v4/revision-{}",
                    revision + 1
                ))
                .is_dir()
        );
    }
    #[test]
    fn interrupted_report_transaction_resumes_from_partial_publication() {
        let workspace = Workspace::new();
        let mut lib = workspace.populate("alpha");
        let batch = inputs(&lib, &[3, 4]);
        let before = lib.projects[0].clone();
        lib.save_reports(0, &batch).unwrap();
        let root = lib.dir(0);
        let receipt = root.join(format!(
            "tools/workspaces/catalog-transactions-v4/revision-{}",
            lib.projects[0].revision
        ));
        let delta: serde_json::Value =
            serde_json::from_slice(&fs::read(receipt.join("journal.json")).unwrap()).unwrap();
        assert_eq!(delta["kind"], "completed-catalog-delta");
        let journal = Journal {
            before,
            after: lib.projects[0].clone(),
            edits: serde_json::from_value(delta["edits"].clone()).unwrap(),
            moves: vec![],
            dependencies: vec![],
            sidecars: vec![],
            ledger: false,
        };
        atomic(
            &receipt.join("journal.json"),
            &serde_json::to_vec(&journal).unwrap(),
        )
        .unwrap();
        // Simulate interruption after the first report, before catalog commit.
        atomic(
            &root.join("project.json"),
            &serde_json::to_vec(&journal.before).unwrap(),
        )
        .unwrap();
        atomic(
            &root.join(&journal.edits[1].new),
            journal.edits[1].before.as_ref().unwrap().as_bytes(),
        )
        .unwrap();
        fs::rename(&receipt, pending(&root)).unwrap();
        drop(lib);
        let recovered = Library::open_project(workspace.0.clone(), Some("alpha")).unwrap();
        assert_eq!(recovered.projects[0].revision, journal.after.revision);
        for edit in &journal.edits {
            assert_eq!(
                fs::read_to_string(root.join(&edit.new)).unwrap(),
                edit.after
            );
        }
        assert!(!pending(&root).exists());
        assert!(receipt.exists());
    }

    #[test]
    fn delta_journal_round_trips_both_states_and_rejects_unrelated_changes() {
        let workspace = Workspace::new();
        let lib = workspace.populate("alpha");
        let mut before = lib.projects[0].clone();
        // Scale metadata in memory only: no per-function files are generated.
        for id in 5..1005 {
            let mut node = before.nodes[3].clone();
            node.id = id;
            node.rva = Some(id as u32 * 16);
            before.nodes.push(node);
        }
        before.next_id = 1005;
        let mut after = before.clone();
        after.nodes[3].name = "Changed".into();
        after.revision += 1;
        let j = Journal {
            before,
            after,
            edits: vec![],
            moves: vec![],
            dependencies: vec![],
            sidecars: vec![],
            ledger: false,
        };
        let bytes = journal_history::pending_bytes(&j).unwrap();
        assert!(
            bytes.len() < 4000,
            "a single edit must not duplicate the inventory"
        );
        for current in [&j.before, &j.after] {
            let recovered = journal_history::read_pending(&bytes, current).unwrap();
            assert!(same(&recovered.before, &j.before));
            assert!(same(&recovered.after, &j.after));
        }
        let mut conflict = j.before.clone();
        conflict.nodes[4].name = "Unrelated external edit".into();
        assert!(journal_history::read_pending(&bytes, &conflict).is_err());
        let mut deleted = j.after.clone();
        deleted.nodes.remove(3);
        let deletion = Journal {
            before: j.after.clone(),
            after: deleted,
            edits: vec![],
            moves: vec![],
            dependencies: vec![],
            sidecars: vec![],
            ledger: false,
        };
        let bytes = journal_history::pending_bytes(&deletion).unwrap();
        assert!(
            bytes.len() < 4000,
            "deletion must not copy records merely because their array indices shifted"
        );
        for current in [&deletion.before, &deletion.after] {
            let recovered = journal_history::read_pending(&bytes, current).unwrap();
            assert!(same(&recovered.before, &deletion.before));
            assert!(same(&recovered.after, &deletion.after));
        }
        let mut reordered = j.after.clone();
        reordered.nodes.swap(3, 4);
        reordered.nodes.remove(5);
        reordered.dependencies.push(Dependency {
            dependent: 4,
            prerequisite: 3,
        });
        let rearranged = Journal {
            before: j.after,
            after: reordered,
            edits: vec![],
            moves: vec![],
            dependencies: vec![],
            sidecars: vec![],
            ledger: false,
        };
        let bytes = journal_history::pending_bytes(&rearranged).unwrap();
        for current in [&rearranged.before, &rearranged.after] {
            let recovered = journal_history::read_pending(&bytes, current).unwrap();
            assert!(same(&recovered.before, &rearranged.before));
            assert!(same(&recovered.after, &rearranged.after));
        }
    }

    #[test]
    fn single_serialisation_gives_the_fingerprints_recovery_checks() {
        let workspace = Workspace::new();
        let lib = workspace.populate("alpha");
        let before = lib.projects[0].clone();
        let mut after = before.clone();
        after.nodes[3].name = "Renamed".into();
        after.revision += 1;
        let j = Journal {
            before: before.clone(),
            after: after.clone(),
            edits: vec![],
            moves: vec![],
            dependencies: vec![],
            sidecars: vec![],
            ledger: false,
        };
        let (delta, bytes) = journal_history::pending_delta(&j).unwrap();
        assert_eq!(bytes, catalog_bytes(&after).unwrap());
        assert_eq!(
            serde_json::to_vec(&delta).unwrap(),
            journal_history::pending_bytes(&j).unwrap()
        );
        let pending = serde_json::to_vec(&delta).unwrap();
        for current in [&before, &after] {
            let recovered = journal_history::read_pending(&pending, current).unwrap();
            assert!(same(&recovered.before, &before) && same(&recovered.after, &after));
        }
    }

    #[test]
    fn remembered_fingerprints_equal_the_catalog_hash() {
        use sha2::{Digest, Sha256};
        let hash = |p: &Project| format!("{:x}", Sha256::digest(catalog_bytes(p).unwrap()));
        let workspace = Workspace::new();
        let lib = workspace.populate("alpha");
        let first = lib.projects[0].clone();
        let mut second = first.clone();
        second.nodes[3].name = "Renamed".into();
        second.revision += 1;
        let mut third = second.clone();
        third.nodes[3].name = "Renamed again".into();
        third.revision += 1;
        let journal = |before: &Project, after: &Project| Journal {
            before: before.clone(),
            after: after.clone(),
            edits: vec![],
            moves: vec![],
            dependencies: vec![],
            sidecars: vec![],
            ledger: false,
        };
        let (a, _) = journal_history::pending_delta(&journal(&first, &second)).unwrap();
        journal_history::committed(&second, a["after_hash"].as_str().unwrap());
        // The next commit starts where the last ended: its before fingerprint is reused.
        let (b, _) = journal_history::pending_delta(&journal(&second, &third)).unwrap();
        assert_eq!(b["before_hash"], a["after_hash"]);
        assert_eq!(b["before_hash"], hash(&second));
        // A different before-catalog is hashed, never matched to the remembered one.
        let (c, _) = journal_history::pending_delta(&journal(&first, &third)).unwrap();
        assert_eq!(c["before_hash"], hash(&first));
    }

    #[test]
    fn history_compaction_preserves_pending_and_report_history() {
        let workspace = Workspace::new();
        let mut lib = workspace.populate("alpha");
        let mut before = lib.projects[0].clone();
        before.revision -= 1;
        let j = Journal {
            before,
            after: lib.projects[0].clone(),
            edits: vec![],
            moves: vec![],
            dependencies: vec![],
            sidecars: vec![],
            ledger: false,
        };
        let receipt = lib.dir(0).join(format!(
            "tools/workspaces/catalog-transactions-v4/revision-{}/journal.json",
            j.after.revision
        ));
        atomic(&receipt, &serde_json::to_vec(&j).unwrap()).unwrap();
        let folder = prepare_pending(&lib.dir(0)).unwrap();
        fs::write(folder.join("journal.json"), b"pending sentinel").unwrap();
        let status = lib.journal_history(0, false).unwrap();
        assert!(status["reclaimable_bytes"].as_u64().unwrap() > 0);
        lib.journal_history(0, true).unwrap();
        assert_eq!(
            fs::read(folder.join("journal.json")).unwrap(),
            b"pending sentinel"
        );
        let receipt: serde_json::Value =
            serde_json::from_slice(&fs::read(receipt).unwrap()).unwrap();
        assert_eq!(receipt["kind"], "completed-catalog-delta");
        assert!(receipt["before"].get("nodes").is_none());
        assert_eq!(
            lib.journal_history(0, false).unwrap()["reclaimable_bytes"],
            0
        );
    }

    #[test]
    fn branch_removal_archives_evidence_rewrites_links_and_reopens_consumers() {
        let workspace = Workspace::new();
        let mut lib = workspace.populate("alpha");
        lib.set_dependencies(0, 4, "1000").unwrap();
        let source = lib.path(0, 3).unwrap();
        fs::write(
            source.parent().unwrap().join("data/capture.bin"),
            b"evidence",
        )
        .unwrap();
        let token = lib.report_token(0, 4).unwrap();
        lib.save_report(
            0,
            4,
            "[source](../00001000/research.md)",
            "evidence",
            false,
            &token,
        )
        .unwrap();
        let request: Removal = serde_json::from_value(serde_json::json!({"schema":1,"binary":lib.projects[0].binary,"expect_revision":lib.projects[0].revision,"ids":[3]})).unwrap();
        let preview = lib.remove_nodes(0, &request, false, None).unwrap();
        assert!(source.exists());
        assert_eq!(preview["candidates"].as_array().unwrap().len(), 1);
        assert!(lib.remove_nodes(0, &request, false, Some("stale")).is_err());
        let result = lib
            .remove_nodes(0, &request, false, preview["token"].as_str())
            .unwrap();
        assert!(!source.exists());
        let archived = lib
            .dir(0)
            .join(result["candidates"][0]["archive"].as_str().unwrap());
        assert_eq!(
            fs::read(archived.parent().unwrap().join("data/capture.bin")).unwrap(),
            b"evidence"
        );
        assert!(lib.projects[0].dependencies.is_empty());
        assert!(
            fs::read_to_string(lib.path(0, 4).unwrap())
                .unwrap()
                .contains("deleted-nodes")
        );
        assert!(
            !fs::read_to_string(lib.path(0, 2).unwrap())
                .unwrap()
                .contains("[Producer]")
        );
        assert!(
            lib.remove_nodes(0, &request, false, preview["token"].as_str())
                .is_err()
        );
    }

    #[test]
    fn prune_preserves_authored_scopes_functions_and_explicit_keep() {
        let workspace = Workspace::new();
        let mut lib = workspace.populate("alpha");
        let empty = lib.add(0, 1, "Retired empty", "").unwrap();
        let authored = lib.add(0, 1, "Open discovery", "").unwrap();
        let keep = lib.add(0, 1, "Retained empty", "").unwrap();
        let token = lib.report_token(0, authored).unwrap();
        lib.save_report(0, authored, "Original source evidence", "", false, &token)
            .unwrap();
        let request: Removal = serde_json::from_value(serde_json::json!({"schema":1,"binary":lib.projects[0].binary,"expect_revision":lib.projects[0].revision,"recursive":true,"keep":[keep]})).unwrap();
        let preview = lib.remove_nodes(0, &request, true, None).unwrap();
        assert_eq!(preview["candidates"].as_array().unwrap().len(), 1);
        assert_eq!(preview["candidates"][0]["id"], empty);
        lib.remove_nodes(0, &request, true, preview["token"].as_str())
            .unwrap();
        assert!(lib.projects[0].nodes.iter().any(|n| n.id == authored));
        assert!(lib.projects[0].nodes.iter().any(|n| n.id == keep));
        assert!(lib.projects[0].nodes.iter().any(|n| n.id == 3));
    }
}
