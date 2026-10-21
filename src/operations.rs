//! One code path for record operations, whether a command commits them one at a time or the
//! coordinator's writer merges several into one commit.
//!
//! A `Library` normally commits each operation as its own transaction. In staging mode it
//! instead applies each operation to its working catalog and records the report and sidecar
//! texts it would write in its overlay, so later operations validate against everything staged
//! before them, exactly as if those had been committed. `commit_staged` then writes everything
//! as one transaction. Every read an operation makes goes through the overlay (`Library::text`),
//! so validation, tokens and effective status see the same state either way.
use super::*;
use std::collections::{BTreeMap, HashMap};

/// What a merged commit has collected so far.
pub(crate) struct Staged {
    /// The catalog before the first staged operation.
    before: Project,
    /// Report edits by path: node, text on disk before the first operation, latest text.
    reports: BTreeMap<String, (u64, String, String)>,
    /// Sidecars by path: text on disk before the first operation, latest text.
    sidecars: BTreeMap<String, (String, String)>,
    /// How many operations were staged.
    pub operations: usize,
}

/// The previous text of each file a commit changes (`None` for a file it creates).
type Previous = [(String, Option<String>)];
type Observer = Box<dyn Fn(&Previous)>;
thread_local! {
    /// Called by a commit, before it changes any file, with the previous text of every file it
    /// will change (`None` for a file it creates). The coordinator uses this to keep older
    /// snapshots answering with the texts they were taken with.
    static COMMIT_OBSERVER: std::cell::RefCell<Option<Observer>> =
        const { std::cell::RefCell::new(None) };
}
thread_local! {
    /// Time this thread's commits spent building the delta and fingerprints, writing the
    /// pending journal, and applying it (files, catalog write and fsync), until taken.
    static COMMIT_TIMES: std::cell::Cell<[std::time::Duration; 3]> =
        const { std::cell::Cell::new([std::time::Duration::ZERO; 3]) };
}
/// Commit stages timed by `record_commit_time`.
pub(crate) const DELTA: usize = 0;
pub(crate) const JOURNAL: usize = 1;
pub(crate) const APPLY: usize = 2;
pub(super) fn record_commit_time(stage: usize, time: std::time::Duration) {
    COMMIT_TIMES.with(|t| {
        let mut times = t.get();
        times[stage] += time;
        t.set(times);
    });
}
/// This thread's commit stage times since the last call, then zero.
pub(crate) fn take_commit_times() -> [std::time::Duration; 3] {
    COMMIT_TIMES.with(|t| t.replace([std::time::Duration::ZERO; 3]))
}
/// Runs `work` with `observer` notified before each commit on this thread changes files.
pub(crate) fn with_commit_observer<T>(
    observer: impl Fn(&[(String, Option<String>)]) + 'static,
    work: impl FnOnce() -> T,
) -> T {
    let previous = COMMIT_OBSERVER.with(|o| o.replace(Some(Box::new(observer))));
    let result = work();
    COMMIT_OBSERVER.with(|o| *o.borrow_mut() = previous);
    result
}
/// Notifies the observer, if any, of the files a commit is about to change.
pub(super) fn before_commit(previous: &[(String, Option<String>)]) {
    COMMIT_OBSERVER.with(|o| {
        if let Some(observer) = o.borrow().as_ref() {
            observer(previous);
        }
    });
}

impl Library {
    /// A report or sidecar as this view sees it: the overlay's text, or the file on disk.
    ///
    /// The file is read first and the overlay checked afterwards. A coordinator records a
    /// file's previous text in every live snapshot's overlay before it changes the file, so if
    /// the read saw a newer text, the overlay already holds the one this view must use. Readers
    /// therefore never hold a lock while reading, and a commit never waits for them.
    pub(crate) fn text_opt(&self, p: usize, rel: &str) -> Result<Option<String>> {
        let entry = |overlay: &Overlay| -> Result<Option<Option<String>>> {
            let guard = overlay.read().map_err(|_| "Overlay poisoned".to_string())?;
            Ok(guard.get(rel).cloned())
        };
        if let Some(overlay) = &self.overlay
            && let Some(text) = entry(overlay)?
        {
            return Ok(text);
        }
        let disk = match fs::read_to_string(self.dir(p).join(rel)) {
            Ok(text) => Some(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("{rel}: {e}")),
        };
        if let Some(overlay) = &self.overlay
            && let Some(text) = entry(overlay)?
        {
            return Ok(text);
        }
        Ok(disk)
    }
    /// Like `text_opt` for a file that must exist.
    pub(crate) fn text(&self, p: usize, rel: &str) -> Result<String> {
        self.text_opt(p, rel)?
            .ok_or_else(|| format!("{rel}: file not found"))
    }
    /// Report hashes that differ from the files on disk for this view. Taken after the disk
    /// hashes, for the same reason as in `text_opt` (see `Index::states_with`).
    pub(crate) fn hash_overrides(&self) -> HashMap<String, Option<String>> {
        let Some(overlay) = &self.overlay else {
            return HashMap::new();
        };
        let guard = match overlay.read() {
            Ok(guard) => guard,
            Err(_) => return HashMap::new(),
        };
        guard
            .iter()
            .filter(|(rel, _)| rel.ends_with(".md"))
            .map(|(rel, text)| (rel.clone(), text.as_deref().map(hash)))
            .collect()
    }
    /// Starts collecting operations on project `p` into one commit.
    pub(crate) fn begin_staging(&mut self, p: usize) {
        self.staging = Some(Staged {
            before: self.projects[p].clone(),
            reports: BTreeMap::new(),
            sidecars: BTreeMap::new(),
            operations: 0,
        });
        self.overlay = Some(Default::default());
    }
    /// Commits a journal now, or, while staging, merges it into the pending commit. Staged
    /// operations may not move reports or change dependency views; those commit alone.
    pub(super) fn commit_or_stage(&mut self, p: usize, j: catalog_batch::Journal) -> Result<usize> {
        let Some(staged) = self.staging.as_mut() else {
            let root = self.dir(p);
            return catalog_batch::commit(&root, &mut self.projects[p], &j);
        };
        if !j.moves.is_empty()
            || !j.dependencies.is_empty()
            || j.ledger
            || j.repin_ledger
            || j.edits
                .iter()
                .any(|e| e.old.as_deref() != Some(e.new.as_str()))
        {
            return Err("Structural changes cannot be merged with other operations.".into());
        }
        let overlay = self.overlay.get_or_insert_with(Default::default).clone();
        let mut texts = overlay
            .write()
            .map_err(|_| "Overlay poisoned".to_string())?;
        for edit in j.edits {
            texts.insert(edit.new.clone(), Some(edit.after.clone()));
            let before = edit.before.unwrap_or_default();
            staged
                .reports
                .entry(edit.new)
                .and_modify(|entry| entry.2 = edit.after.clone())
                .or_insert((edit.id, before, edit.after));
        }
        for sidecar in j.sidecars {
            texts.insert(sidecar.path.clone(), Some(sidecar.after.clone()));
            staged
                .sidecars
                .entry(sidecar.path)
                .and_modify(|entry| entry.1 = sidecar.after.clone())
                .or_insert((sidecar.before, sidecar.after));
        }
        staged.operations += 1;
        self.projects[p] = j.after;
        Ok(0)
    }
    /// Commits everything staged as one transaction and leaves staging mode. Returns how many
    /// operations it contained.
    pub(crate) fn commit_staged(&mut self, p: usize) -> Result<usize> {
        self.overlay = None;
        let Some(staged) = self.staging.take() else {
            return Ok(0);
        };
        if staged.operations == 0 {
            return Ok(0);
        }
        let root = self.dir(p);
        let j = catalog_batch::Journal {
            before: staged.before,
            after: self.projects[p].clone(),
            edits: staged
                .reports
                .into_iter()
                .map(|(path, (id, before, after))| catalog_batch::Edit {
                    id,
                    old: Some(path.clone()),
                    new: path,
                    before: Some(before),
                    after,
                })
                .collect(),
            moves: vec![],
            dependencies: vec![],
            sidecars: staged
                .sidecars
                .into_iter()
                .map(|(path, (before, after))| catalog_batch::Sidecar {
                    path,
                    before,
                    after,
                })
                .collect(),
            ledger: false,
            repin_ledger: false,
            directed_by: None,
        };
        if let Err(e) = catalog_batch::commit(&root, &mut self.projects[p], &j) {
            // Nothing staged may outlive a failed commit in memory.
            self.projects[p] = j.before;
            return Err(e);
        }
        Ok(staged.operations)
    }
    /// Runs one operation while staging. If it fails without staging anything, its in-memory
    /// changes are undone, as they would vanish with its own process; if it staged and then
    /// failed, the staged part stays, as its commit would have.
    pub(crate) fn stage_one<R>(
        &mut self,
        p: usize,
        work: impl FnOnce(&mut Self) -> R,
        failed: impl Fn(&R) -> bool,
    ) -> R {
        let checkpoint = self.projects[p].clone();
        let staged = self.staging.as_ref().map_or(0, |s| s.operations);
        let result = work(self);
        if failed(&result) && self.staging.as_ref().map_or(0, |s| s.operations) == staged {
            self.projects[p] = checkpoint;
        }
        result
    }
    /// Abandons staged operations: restores the catalog from before the first one.
    pub(crate) fn discard_staged(&mut self, p: usize) {
        self.overlay = None;
        if let Some(staged) = self.staging.take() {
            self.projects[p] = staged.before;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Home(PathBuf);
    impl Drop for Home {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    /// App 0 → architecture 1 → chunk 2 → functions 3 (0x1000) and 4 (0x2000).
    fn fixture() -> (Home, Library) {
        let home = Home(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target/test-workspaces")
                .join(format!("staging-{}", uuid::Uuid::new_v4())),
        );
        fs::create_dir_all(&home.0).unwrap();
        let mut lib = Library::open_project(home.0.clone(), Some("fixture")).unwrap();
        lib.create("fixture", "Fixture", "synthetic").unwrap();
        let a = lib.add(0, 0, "Architecture", "").unwrap();
        let c = lib.add(0, a, "Chunk", "").unwrap();
        lib.add(0, c, "First", "1000").unwrap();
        lib.add(0, c, "Second", "2000").unwrap();
        (home, lib)
    }
    /// Operations whose tokens were all taken before any of them ran, as concurrent agents
    /// would submit them.
    fn run(lib: &mut Library, tokens: &[String]) -> Vec<bool> {
        vec![
            lib.save_report(0, 3, "First research", "Evidence", false, &tokens[0])
                .is_ok(),
            lib.save_report(0, 4, "Second research", "Evidence", false, &tokens[1])
                .is_ok(),
            // Stale: the first save changed this record's text.
            lib.save_report(0, 3, "Conflicting edit", "Evidence", false, &tokens[0])
                .is_ok(),
            lib.set_review_flag(0, 4, Some("Check offsets")).is_ok(),
            // Stale: the completion token predates the save and the flag.
            lib.save_report(0, 4, "Second research", "Evidence", true, &tokens[2])
                .is_ok(),
            lib.claim(0, 2, "agent-a", "", None, false).is_ok(),
            // A second claim must keep the first; a conflicting one is refused.
            lib.claim(0, 1, "agent-b", "", None, false).is_ok(),
            lib.claim(0, 2, "agent-c", "", None, false).is_ok(),
        ]
    }
    fn tokens(lib: &Library) -> Vec<String> {
        vec![
            lib.report_token(0, 3).unwrap(),
            lib.report_token(0, 4).unwrap(),
            lib.completion_token(0, 4).unwrap(),
        ]
    }
    fn reports(lib: &Library) -> Vec<String> {
        (0..5)
            .map(|id| fs::read_to_string(lib.path(0, id).unwrap()).unwrap())
            .collect()
    }

    #[test]
    fn merged_operations_match_serial_application() {
        let (_serial_home, mut serial) = fixture();
        let serial_tokens = tokens(&serial);
        let serial_results = run(&mut serial, &serial_tokens);

        let (_merged_home, mut merged) = fixture();
        let merged_tokens = tokens(&merged);
        let start = merged.projects[0].revision;
        merged.begin_staging(0);
        let merged_results = run(&mut merged, &merged_tokens);
        assert_eq!(
            merged.commit_staged(0).unwrap(),
            5,
            "five operations accepted"
        );

        assert_eq!(merged_results, serial_results);
        assert_eq!(
            merged_results,
            vec![true, true, false, true, false, true, true, false]
        );
        assert!(merged.projects[0] == serial.projects[0]);
        assert_eq!(reports(&merged), reports(&serial));
        assert_eq!(
            merged.review_flags(0).unwrap(),
            serial.review_flags(0).unwrap()
        );
        assert_eq!(merged.claims(0).unwrap(), serial.claims(0).unwrap());
        assert_eq!(merged.claims(0).unwrap().len(), 2);
        // One receipt for the whole merged commit.
        let receipts = fs::read_dir(
            merged
                .dir(0)
                .join("tools/workspaces/catalog-transactions-v4"),
        )
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .strip_prefix("revision-")
                .and_then(|r| r.parse::<u64>().ok())
                .is_some_and(|r| r > start)
        })
        .count();
        assert_eq!(receipts, 1);
    }

    #[test]
    fn structural_changes_are_not_merged() {
        let (_home, mut lib) = fixture();
        lib.begin_staging(0);
        assert!(lib.set_dependencies(0, 4, "1000").is_err());
        lib.discard_staged(0);
        assert!(lib.set_dependencies(0, 4, "1000").is_ok());
    }

    #[test]
    fn a_failed_merged_commit_leaves_no_staged_state_in_memory() {
        let (_home, mut lib) = fixture();
        let before = lib.projects[0].clone();
        let token = lib.report_token(0, 3).unwrap();
        lib.begin_staging(0);
        lib.save_report(0, 3, "Staged research", "Evidence", false, &token)
            .unwrap();
        assert!(lib.projects[0] != before, "the save is staged in memory");
        // An unexpected file in the pending folder makes the commit fail before it writes.
        let pending = lib
            .dir(0)
            .join("tools/workspaces/catalog-transactions-v4/pending");
        fs::create_dir_all(&pending).unwrap();
        fs::write(pending.join("stray.txt"), "x").unwrap();
        assert!(lib.commit_staged(0).is_err());
        assert!(
            lib.projects[0] == before,
            "the failed commit's state was undone"
        );
    }

    #[test]
    fn a_refused_operation_leaves_nothing_in_the_merged_commit() {
        let (_home, mut lib) = fixture();
        let before = lib.projects[0].clone();
        lib.begin_staging(0);
        let refused = lib.stage_one(0, |l| l.set_dependencies(0, 4, "1000"), Result::is_err);
        assert!(refused.is_err());
        // An operation that changes the catalog and then fails before staging.
        let failed = lib.stage_one(
            0,
            |l| {
                l.projects[0].nodes[3].name = "Half-applied".into();
                Err::<(), String>("failed after changing the catalog".into())
            },
            Result::is_err,
        );
        assert!(failed.is_err());
        assert!(lib.projects[0] == before, "the refused change was undone");
        lib.commit_staged(0).unwrap();
        assert!(lib.projects[0] == before);
    }
}
