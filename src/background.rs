//! Background preparation and immutable GUI publication.
use super::*;
use solworker::{
    SWExecutionClass, SWOwnedLimits, SWRuntime, SWRuntimeConfig, SWSpawnOptions, SWTask,
    SWWorkerConfig,
};
/// The process's one Solworker runtime, created on first use. Low runs bulk work (report
/// hashing, import rendering), Mid prepares GUI snapshots, High answers interactive browser
/// queries. Worker counts follow the machine; each class can hand all its workers owned work.
pub(super) fn shared() -> Result<&'static SWRuntime> {
    static RUNTIME: std::sync::OnceLock<std::result::Result<SWRuntime, String>> =
        std::sync::OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            let low = std::thread::available_parallelism()
                .map_or(2, |n| n.get().saturating_sub(1))
                .clamp(2, 8);
            let (mid, high) = (2, 2);
            let config = SWRuntimeConfig::new(
                low + mid + high,
                [
                    SWWorkerConfig::new(low),
                    SWWorkerConfig::new(mid),
                    SWWorkerConfig::new(high),
                ],
            )
            .map_err(|e| format!("Worker configuration: {e:?}"))?;
            SWRuntime::builder(config)
                .with_owned_limits(
                    SWOwnedLimits::new(64, 64, [32; 3], [low, mid, high])
                        .map_err(|e| format!("Worker capacity: {e:?}"))?,
                )
                .build()
                .map_err(|e| format!("Worker startup: {e:?}"))
        })
        .as_ref()
        .map_err(Clone::clone)
}
/// Runs `work` as an owned job on the shared runtime's `class` workers.
pub(super) fn spawn<T: Send + 'static>(
    class: SWExecutionClass,
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<SWTask<T>> {
    shared()?
        .lane(class)
        .try_spawn(SWSpawnOptions::default(), work)
        .map(|(task, _)| task)
        .map_err(|e| format!("Worker admission: {:?}", e.reason))
}
pub(super) fn join<T: Send + 'static>(mut task: SWTask<T>) -> Result<T> {
    task.completion()
        .wait()
        .map_err(|e| format!("Worker wait: {e:?}"))?;
    match task.try_take() {
        Some(solworker::SWOutcome::Success(value)) => Ok(value),
        _ => Err("Background preparation failed or was cancelled".into()),
    }
}

use std::{collections::HashMap, sync::Arc};
/// A project's sidecar state as the refresh reads it.
struct Sidecars {
    flags: std::collections::BTreeMap<u64, ReviewFlag>,
    queued: HashMap<u64, String>,
    quarantine: std::collections::BTreeMap<u64, quarantine::Quarantine>,
}
impl Sidecars {
    fn read(dir: &Path, project: &Project) -> Self {
        Self {
            flags: super::catalog_batch::read_flags(dir).unwrap_or_default(),
            queued: super::coordination::open_queue(dir, project),
            quarantine: super::quarantine::read_records(dir),
        }
    }
}
pub(super) struct Snapshot {
    pub root: Option<PathBuf>,
    pub projects: Vec<Arc<Project>>,
    pub browser: Vec<Arc<catalog_browser::BrowserData>>,
    pub views: Vec<ProjectView>,
    pub jobs: HashMap<String, imports::Job>,
    pub fingerprint: String,
    /// Per project read successfully, its part of the fingerprint (see `signature`).
    pub signatures: HashMap<String, String>,
    /// Projects that could not be read, with why; they keep what was shown before.
    pub unreadable: Vec<(String, String)>,
}
pub(super) struct Loaded {
    pub project: String,
    pub id: u64,
    pub body: String,
    pub evidence: String,
    pub draft: String,
    pub completion: String,
    pub complete: bool,
    pub binary: String,
    pub scope: String,
    pub revision: u64,
    pub dependencies: String,
    pub status: Status,
    pub blockers: Vec<String>,
    /// Where the record is implemented, from the implementation tracker; an error when the
    /// tracker cannot be read, which never stops the record itself from loading.
    pub implemented_in: std::result::Result<Option<String>, String>,
    /// The function's proof, "Unproven" when the project requires one, else empty.
    pub proof: String,
}
pub(super) fn load(project: &str, id: u64) -> Result<Loaded> {
    let root = crate::paths::home(None)?.join("projects");
    let lib = Library::read_project(root, project)?;
    if lib.projects.is_empty() {
        return Err("Project was removed".into());
    }
    let record = lib.record(0, id)?;
    Ok(Loaded {
        project: project.into(),
        id,
        body: record.body,
        evidence: record.evidence,
        draft: record.token,
        completion: record.completion_token,
        complete: record.effective_status == Status::Complete,
        binary: record.binary,
        scope: record.research_scope,
        revision: record.revision,
        dependencies: record.dependency_rvas,
        status: record.effective_status,
        blockers: lib.completion_blockers(0, id)?,
        implemented_in: implemented_in(&lib, id),
        proof: match (&record.proof, record.unproven) {
            (Some(p), _) => format!("Proof: {} (tests: {})", p.recreation, p.tests.join(", ")),
            (None, true) => "Unproven: this project requires a proof before completion.".into(),
            (None, false) => String::new(),
        },
    })
}

/// Where record `id` of `lib`'s project is implemented. The placement map is read from every
/// tracker shard once per project revision and kept for the next record (tracker changes raise
/// the revision, so it never answers from an older tracker).
fn implemented_in(lib: &Library, id: u64) -> std::result::Result<Option<String>, String> {
    type Placements = (PathBuf, u64, Arc<HashMap<u64, String>>);
    static CACHE: std::sync::Mutex<Option<Placements>> = std::sync::Mutex::new(None);
    let project = lib.projects.first().ok_or("Project was removed")?;
    let key = (lib.root.join(&project.id), project.revision);
    let cached = CACHE.lock().ok().and_then(|c| {
        c.as_ref()
            .filter(|c| (&c.0, c.1) == (&key.0, key.1))
            .map(|c| c.2.clone())
    });
    let placements = match cached {
        Some(placements) => placements,
        None => {
            let placements = Arc::new(lib.impl_placements(0)?);
            if let Ok(mut cache) = CACHE.lock() {
                *cache = Some((key.0, key.1, placements.clone()));
            }
            placements
        }
    };
    Ok(placements.get(&id).cloned())
}

/// What one project contributes to the refresh fingerprint: its catalog's stamp, plus the stamps
/// of its completed reports (their hashes decide acceptance) and of the selected record.
fn signature(
    root: &Path,
    id: &str,
    known: Option<(&Project, &catalog_browser::BrowserData)>,
    selected: &(String, u64),
) -> String {
    let stamp = |path: &Path| {
        format!(
            "{:?};",
            fs::metadata(path).map(|m| (m.len(), m.modified().ok()))
        )
    };
    let mut signature = stamp(&root.join(id).join("project.json"));
    if let Some((project, browser)) = known {
        for n in &project.nodes {
            if (n.status == Status::Complete || (project.id == selected.0 && n.id == selected.1))
                && let Some(path) = browser.paths.get(&n.id)
            {
                signature += &stamp(&root.join(id).join(path));
            }
        }
    }
    signature
}

/// One project in a refresh: its catalog, meters and browser data, reused or read anew.
struct Entry {
    project: Arc<Project>,
    view: Option<ProjectView>,
    browser: Option<Arc<catalog_browser::BrowserData>>,
    sidecars: Option<Sidecars>,
    /// Read, or reused unchanged, successfully: the next refresh may reuse it.
    fresh: bool,
}

pub(super) fn snapshot(
    previous: String,
    signatures: HashMap<String, String>,
    projects: Vec<Arc<Project>>,
    browsers: Vec<Arc<catalog_browser::BrowserData>>,
    previous_views: Vec<ProjectView>,
    selected: (String, u64),
) -> Result<Snapshot> {
    let home = crate::paths::home(None)?;
    crate::paths::initialize(&home)?;
    let refresh = Refresh {
        previous,
        signatures,
        projects,
        browsers,
        previous_views,
        selected,
    };
    snapshot_in(home.join("projects"), refresh)
}

/// What the dashboard showed after its last refresh.
pub(super) struct Refresh {
    pub previous: String,
    pub signatures: HashMap<String, String>,
    pub projects: Vec<Arc<Project>>,
    pub browsers: Vec<Arc<catalog_browser::BrowserData>>,
    pub previous_views: Vec<ProjectView>,
    pub selected: (String, u64),
}

/// The next snapshot of the projects under `root`, reusing what `last` shows of every project
/// unchanged since it was last read.
pub(super) fn snapshot_in(root: PathBuf, last: Refresh) -> Result<Snapshot> {
    snapshot_observed(root, last, &|_| {})
}

/// `snapshot_in`, calling `after_read` with each project's ID right after its catalog is read
/// (tests commit there, to land a change inside the refresh).
fn snapshot_observed(root: PathBuf, last: Refresh, after_read: &dyn Fn(&str)) -> Result<Snapshot> {
    let Refresh {
        previous,
        signatures,
        projects,
        browsers,
        previous_views,
        selected,
    } = last;
    let mut ids = fs::read_dir(&root)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter(|e| e.path().join("project.json").is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    ids.sort();
    let known: HashMap<&str, (&Arc<Project>, &Arc<catalog_browser::BrowserData>)> = projects
        .iter()
        .zip(&browsers)
        .map(|(p, b)| (p.id.as_str(), (p, b)))
        .collect();
    let current: Vec<(String, String)> = ids
        .iter()
        .map(|id| {
            let pair = known
                .get(id.as_str())
                .map(|(p, b)| (p.as_ref(), b.as_ref()));
            (id.clone(), signature(&root, id, pair, &selected))
        })
        .collect();
    let mut jobs = HashMap::new();
    for p in &projects {
        if let Some(id) = p.active_import.as_ref().or(p.last_import.as_ref())
            && let Ok(job) = imports::status(&root, &p.id, id)
        {
            jobs.insert(p.id.clone(), job);
        }
    }
    let mut fingerprint = hash(
        &current
            .iter()
            .map(|(id, signature)| format!("{id}:{signature}"))
            .collect::<String>(),
    );
    if fingerprint == previous {
        return Ok(Snapshot {
            root: None,
            projects: vec![],
            browser: vec![],
            views: vec![],
            jobs,
            fingerprint,
            signatures,
            unreadable: vec![],
        });
    }
    let reuse = |id: &str| -> Option<Entry> {
        let (project, browser) = known.get(id)?;
        let view = previous_views.iter().find(|v| v.id == id)?.clone();
        Some(Entry {
            project: Arc::clone(project),
            view: Some(view),
            browser: Some(Arc::clone(browser)),
            sidecars: None,
            fresh: false,
        })
    };
    let mut entries: Vec<Entry> = vec![];
    let mut unreadable = vec![];
    let mut retry = false;
    let mut read_any = false;
    for (id, signature) in &current {
        // Unchanged since it was last read successfully: nothing to read again.
        if signatures.get(id) == Some(signature)
            && let Some(mut entry) = reuse(id)
        {
            entry.fresh = true;
            entries.push(entry);
            continue;
        }
        match Library::read_project(root.clone(), id) {
            Ok(mut lib) => {
                let Some(project) = lib.projects.first() else {
                    continue;
                };
                // Read under the same shared lock as the catalog, so both come from one revision.
                let sidecars = Sidecars::read(&root.join(&project.id), project);
                // Meters hash reports and walk the tree; do that without holding the lock,
                // which would otherwise delay agents' writes on every refresh.
                lib.release();
                after_read(id);
                read_any = true;
                let view = lib.views().pop();
                if let Some(project) = lib.projects.pop() {
                    entries.push(Entry {
                        project: Arc::new(project),
                        view,
                        browser: None,
                        sidecars: Some(sidecars),
                        fresh: true,
                    });
                }
            }
            Err(e) => {
                // A busy project keeps what was shown, and so does one that cannot be read;
                // both are tried again at the next refresh.
                retry = true;
                if !e.contains("is busy") {
                    unreadable.push((id.clone(), e));
                }
                if let Some(entry) = reuse(id) {
                    entries.push(entry);
                }
            }
        }
    }
    // The signature each project had before anything was read: one taken after reading could
    // already describe a commit that landed meanwhile, which would then never be read.
    let before: HashMap<&str, &String> = current
        .iter()
        .map(|(id, signature)| (id.as_str(), signature))
        .collect();
    let next: HashMap<String, String> = entries
        .iter()
        .filter(|e| e.fresh)
        .filter_map(|e| {
            let id = e.project.id.as_str();
            Some((id.to_string(), before.get(id)?.to_string()))
        })
        .collect();
    if retry {
        // The fingerprint of nothing that was read: the next poll looks again.
        fingerprint.push('*');
    }
    // Nothing read anew and every shown project still shown: the dashboard has nothing to
    // replace (republishing would reload the open record and reset selections every poll).
    if !read_any && entries.len() == projects.len() {
        return Ok(Snapshot {
            root: None,
            projects: vec![],
            browser: vec![],
            views: vec![],
            jobs,
            fingerprint,
            signatures: next,
            unreadable,
        });
    }
    entries.sort_by(|a, b| a.project.name.cmp(&b.project.name));
    // Browser data for the projects read anew, built off this thread.
    let pending: Vec<(usize, Arc<Project>, Option<Sidecars>)> = entries
        .iter_mut()
        .enumerate()
        .filter(|(_, e)| e.browser.is_none())
        .map(|(i, e)| (i, Arc::clone(&e.project), e.sidecars.take()))
        .collect();
    let flag_root = root.clone();
    let built: Vec<(usize, Arc<catalog_browser::BrowserData>)> =
        join(spawn(SWExecutionClass::Mid, move || {
            pending
                .into_iter()
                .map(|(i, p, sidecars)| {
                    let read =
                        sidecars.unwrap_or_else(|| Sidecars::read(&flag_root.join(&p.id), &p));
                    let data = catalog_browser::BrowserData::new(
                        &p,
                        &read.flags,
                        &read.queued,
                        &super::quarantine::descriptions(&p, &read.quarantine),
                    );
                    (i, Arc::new(data))
                })
                .collect()
        })?)?;
    for (i, browser) in built {
        entries[i].browser = Some(browser);
    }
    let mut views: Vec<ProjectView> = entries.iter().filter_map(|e| e.view.clone()).collect();
    views.sort_by_key(|p| p.name.clone());
    let (projects, browser) = entries
        .into_iter()
        .map(|e| (e.project, e.browser.unwrap_or_default()))
        .unzip();
    Ok(Snapshot {
        root: Some(root),
        projects,
        browser,
        views,
        jobs,
        fingerprint,
        signatures: next,
        unreadable,
    })
}

pub(super) fn launch_import(root: &Path, project: &str, id: &str) -> Result<()> {
    let executable = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .with_file_name(if cfg!(windows) {
            "research.exe"
        } else {
            "research"
        });
    let mut command = std::process::Command::new(executable);
    command
        .arg("--home")
        .arg(root.parent().ok_or("Missing research home")?)
        .args(["import", "resume", project, id]);
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command.spawn().map_err(|e| {
        format!("Job {id} is reserved; runner launch failed: {e}. Resume using the CLI.")
    })?;
    Ok(())
}

/// The GUI's refresh thread: one named thread that runs refresh jobs in order for the life of
/// the dashboard. Being long-lived, it keeps report stamps in memory between refreshes; a new
/// thread per refresh would start empty every time. A job that panics is caught, so later
/// refreshes still run; its result channel closes, which the caller reports.
pub(super) struct RefreshWorker {
    jobs: std::sync::mpsc::Sender<RefreshJob>,
}
pub(super) type RefreshJob = Box<dyn FnOnce() + Send>;
impl RefreshWorker {
    pub fn start() -> Self {
        let (jobs, queue) = std::sync::mpsc::channel::<RefreshJob>();
        // If the thread cannot start, `queue` is dropped with the closure and every `run` fails.
        let _ = std::thread::Builder::new()
            .name("scopework-refresh".into())
            .spawn(move || {
                super::report_hashes::retain_on_this_thread();
                for job in queue {
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
                }
            });
        Self { jobs }
    }
    /// Queues `job`, or hands it back if the thread is gone.
    pub fn run(&self, job: RefreshJob) -> std::result::Result<(), RefreshJob> {
        self.jobs.send(job).map_err(|unsent| unsent.0)
    }
}

/// Only the GUI owner pumps this inbox; no CPU task or filesystem work is helped.
pub(super) struct SnapshotInbox {
    owner: solworker::SWOwner<Option<Result<Snapshot>>>,
}
impl SnapshotInbox {
    pub fn new() -> Result<Self> {
        let mut owner = shared()?
            .owner(None, std::num::NonZeroUsize::new(1).unwrap())
            .map_err(|e| format!("Snapshot owner: {e:?}"))?;
        owner
            .set_phase(solworker::SWPhase(1))
            .map_err(|e| format!("Snapshot phase: {e:?}"))?;
        Ok(Self { owner })
    }
    pub fn sender(&self) -> solworker::SWOwnerSender<Option<Result<Snapshot>>> {
        self.owner.sender()
    }
    pub fn take(&mut self) -> Result<Option<Result<Snapshot>>> {
        self.owner
            .pump(solworker::SWPhase(1), solworker::SWPumpBudget::new(1))
            .map_err(|e| format!("Snapshot publication: {e:?}"))?;
        Ok(self.owner.state_mut().take())
    }
}
impl Drop for SnapshotInbox {
    fn drop(&mut self) {
        // The shared runtime outlives this inbox; closing the owner rejects late deliveries.
        self.owner.close();
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

    /// What the dashboard shows: it keeps its projects when an answer is unchanged.
    #[derive(Clone)]
    struct Shown {
        fingerprint: String,
        signatures: HashMap<String, String>,
        projects: Vec<Arc<Project>>,
        browsers: Vec<Arc<catalog_browser::BrowserData>>,
        views: Vec<ProjectView>,
    }
    impl Shown {
        fn empty() -> Self {
            Self {
                fingerprint: String::new(),
                signatures: HashMap::new(),
                projects: vec![],
                browsers: vec![],
                views: vec![],
            }
        }
        /// One refresh; `previous` overrides the fingerprint, as a forced refresh does.
        fn refresh(&mut self, root: &Path, previous: Option<&str>) -> Snapshot {
            let answer = snapshot_in(
                root.to_path_buf(),
                Refresh {
                    previous: previous.map_or(self.fingerprint.clone(), str::to_string),
                    signatures: self.signatures.clone(),
                    projects: self.projects.clone(),
                    browsers: self.browsers.clone(),
                    previous_views: self.views.clone(),
                    selected: (String::new(), 0),
                },
            )
            .unwrap();
            self.fingerprint = answer.fingerprint.clone();
            self.signatures = answer.signatures.clone();
            if answer.root.is_some() {
                self.projects = answer.projects.clone();
                self.browsers = answer.browser.clone();
                self.views = answer.views.clone();
            }
            answer
        }
    }

    #[test]
    fn a_refresh_reads_only_changed_projects_and_skips_unreadable_ones() {
        let home = Home(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target/test-workspaces")
                .join(format!("refresh-{}", uuid::Uuid::new_v4())),
        );
        let root = home.0.join("projects");
        fs::create_dir_all(&root).unwrap();
        let mut lib = Library::open(root.clone()).unwrap();
        lib.create("alpha", "Alpha", "synthetic").unwrap();
        lib.create("beta", "Beta", "synthetic").unwrap();
        drop(lib);
        let mut shown = Shown::empty();
        let first = shown.refresh(&root, None);
        assert_eq!(first.projects.len(), 2);
        assert_eq!(first.signatures.len(), 2);
        // Forced past the fingerprint, unchanged projects are reused, not read again: the answer
        // is "unchanged", so the dashboard keeps what it shows.
        let second = shown.refresh(&root, Some("forced"));
        assert!(
            second.root.is_none(),
            "nothing was read, nothing is republished"
        );
        // A changed project that is busy keeps what was shown and is retried; the others are
        // still reused. (An unchanged one is reused without being read, busy or not.)
        let mut beta = Library::open_project(root.clone(), Some("beta")).unwrap();
        let revision = beta.projects[0].revision;
        beta.set_scope(0, "Changed", revision).unwrap();
        drop(beta);
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(root.join(".project-locks/beta.lock"))
            .unwrap();
        lock.lock().unwrap();
        let busy = shown.refresh(&root, None);
        let again = shown.refresh(&root, None);
        lock.unlock().unwrap();
        for answer in [&busy, &again] {
            assert!(answer.root.is_none(), "a busy project republishes nothing");
            assert!(
                answer.fingerprint.ends_with('*'),
                "the next poll looks again"
            );
            assert!(!answer.signatures.contains_key("beta"));
        }
        assert_eq!(shown.projects.len(), 2);
        // A stray copy of a project folder is reported, and hides nothing.
        let stray = root.join("Alpha - Copy");
        fs::create_dir_all(&stray).unwrap();
        fs::copy(root.join("alpha/project.json"), stray.join("project.json")).unwrap();
        let skipped = shown.refresh(&root, None);
        assert_eq!(skipped.unreadable.len(), 1);
        assert_eq!(skipped.unreadable[0].0, "Alpha - Copy");
        assert!(
            skipped.signatures.contains_key("beta"),
            "beta is read again"
        );
        let beta = shown.projects.iter().find(|p| p.id == "beta").unwrap();
        assert_eq!(beta.research_scope, "Changed");
        assert_eq!(shown.projects.len(), 2);
        // With only the stray folder left unreadable, refreshes answer "unchanged" but keep
        // reporting it.
        let quiet = shown.refresh(&root, None);
        assert!(quiet.root.is_none());
        assert_eq!(quiet.unreadable.len(), 1);
    }

    #[test]
    fn a_commit_landing_during_a_refresh_is_read_by_the_next_one() {
        let home = Home(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target/test-workspaces")
                .join(format!("refresh-race-{}", uuid::Uuid::new_v4())),
        );
        let root = home.0.join("projects");
        fs::create_dir_all(&root).unwrap();
        let mut lib = Library::open(root.clone()).unwrap();
        lib.create("alpha", "Alpha", "synthetic").unwrap();
        drop(lib);
        // An agent commits right after the refresh has read the catalog.
        let committed = std::cell::Cell::new(false);
        let commit = |id: &str| {
            if id == "alpha" && !committed.replace(true) {
                let mut lib = Library::open_project(root.clone(), Some("alpha")).unwrap();
                let revision = lib.projects[0].revision;
                lib.set_scope(0, "Landed meanwhile", revision).unwrap();
            }
        };
        let first = snapshot_observed(
            root.clone(),
            Refresh {
                previous: String::new(),
                signatures: HashMap::new(),
                projects: vec![],
                browsers: vec![],
                previous_views: vec![],
                selected: (String::new(), 0),
            },
            &commit,
        )
        .unwrap();
        assert_eq!(
            first.projects[0].research_scope, "",
            "read before the commit"
        );
        let mut shown = Shown {
            fingerprint: first.fingerprint.clone(),
            signatures: first.signatures.clone(),
            projects: first.projects.clone(),
            browsers: first.browser.clone(),
            views: first.views.clone(),
        };
        shown.refresh(&root, None);
        assert_eq!(shown.projects[0].research_scope, "Landed meanwhile");
    }

    #[test]
    fn where_a_record_is_implemented_follows_the_tracker_and_never_fails_the_load() {
        let home = Home(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target/test-workspaces")
                .join(format!("placements-{}", uuid::Uuid::new_v4())),
        );
        let root = home.0.join("projects");
        fs::create_dir_all(&root).unwrap();
        let mut lib = Library::open(root.clone()).unwrap();
        lib.create("alpha", "Alpha", "synthetic").unwrap();
        drop(lib);
        let mut lib = Library::open_project(root.clone(), Some("alpha")).unwrap();
        let arch = lib.add(0, 0, "Arch", "").unwrap();
        let chunk = lib.add(0, arch, "Chunk", "").unwrap();
        let function = lib.add(0, chunk, "Alloc", "1000").unwrap();
        assert_eq!(implemented_in(&lib, function), Ok(None));
        let krate = lib.impl_add(0, None, "alpha_rs").unwrap();
        lib.impl_place(0, krate, &[function]).unwrap();
        assert_eq!(implemented_in(&lib, function), Ok(Some("alpha_rs".into())));
        // A tracker change raises the revision, so the cached map is replaced.
        lib.impl_rename(0, krate, "alpha_core").unwrap();
        assert_eq!(
            implemented_in(&lib, function),
            Ok(Some("alpha_core".into()))
        );
        // An unreadable tracker answers an error for this field alone.
        fs::write(root.join("alpha/implementation/tree.json"), "{not json").unwrap();
        lib.set_scope(0, "Bump", lib.projects[0].revision).unwrap();
        assert!(implemented_in(&lib, function).is_err());
    }
}
