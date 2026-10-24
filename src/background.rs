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
    /// Where the record is implemented, from the implementation tracker.
    pub implemented_in: Option<String>,
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
        implemented_in: lib.impl_where(0, id)?["path"].as_str().map(str::to_string),
    })
}

pub(super) fn snapshot(
    previous: String,
    projects: Vec<Arc<Project>>,
    browsers: Vec<Arc<catalog_browser::BrowserData>>,
    previous_views: Vec<ProjectView>,
    selected: (String, u64),
) -> Result<Snapshot> {
    let home = crate::paths::home(None)?;
    crate::paths::initialize(&home)?;
    let root = home.join("projects");
    let mut paths = fs::read_dir(&root)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path().join("project.json"))
        .filter(|p| p.is_file())
        .collect::<Vec<_>>();
    paths.sort();
    let mut signature = String::new();
    let mut jobs = HashMap::new();
    for path in paths {
        signature += &format!(
            "{}:{:?};",
            path.display(),
            fs::metadata(&path).map(|m| (m.len(), m.modified().ok()))
        );
    }
    for (p, b) in projects.iter().zip(&browsers) {
        for n in &p.nodes {
            if n.status == Status::Complete || (p.id == selected.0 && n.id == selected.1) {
                signature += &format!(
                    "{:?};",
                    fs::metadata(root.join(&p.id).join(&b.paths[&n.id]))
                        .map(|m| (m.len(), m.modified().ok()))
                );
            }
        }
        if let Some(id) = p.active_import.as_ref().or(p.last_import.as_ref())
            && let Ok(job) = imports::status(&root, &p.id, id)
        {
            jobs.insert(p.id.clone(), job);
        }
    }
    let mut fingerprint = hash(&signature);
    if fingerprint == previous {
        return Ok(Snapshot {
            root: None,
            projects: vec![],
            browser: vec![],
            views: vec![],
            jobs,
            fingerprint,
        });
    }
    // Keep each shared lease through catalog, accepted-report and meter reads.
    let mut library = Library {
        root: root.clone(),
        projects: vec![],
        busy_projects: vec![],
        _lock: None,
        recovery_blocked: None,
        overlay: None,
        staging: None,
    };
    let mut views = vec![];
    // Each project's sidecars, read under the same shared lock as its catalog so both come from
    // one revision.
    let mut sidecars: HashMap<String, Sidecars> = HashMap::new();
    let mut ids = fs::read_dir(&root)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter(|e| e.path().join("project.json").is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    ids.sort();
    for id in ids {
        match Library::read_project(root.clone(), &id) {
            Ok(mut lib) => {
                if let Some(project) = lib.projects.first() {
                    let read = Sidecars::read(&root.join(&project.id), project);
                    sidecars.insert(project.id.clone(), read);
                }
                // Meters hash reports and walk the tree; do that without holding the lock,
                // which would otherwise delay agents' writes on every refresh.
                lib.release();
                views.extend(lib.views());
                library.projects.append(&mut lib.projects);
            }
            Err(e) if e.contains("is busy") => {
                if let Some(old) = projects.iter().find(|p| p.id == id) {
                    library.projects.push((**old).clone());
                }
                if let Some(old) = previous_views.iter().find(|p| p.id == id) {
                    views.push(old.clone());
                }
                library.busy_projects.push(id);
            }
            Err(e) => return Err(e),
        }
    }
    if !library.busy_projects.is_empty() {
        fingerprint.clear();
    }
    library.projects.sort_by_key(|p| p.name.clone());
    views.sort_by_key(|p| p.name.clone());
    let projects = library
        .projects
        .into_iter()
        .map(Arc::new)
        .collect::<Vec<_>>();
    let input = projects.clone();
    let flag_root = root.clone();
    let browser = join(spawn(SWExecutionClass::Mid, move || {
        input
            .iter()
            .map(|p| {
                // A busy project keeps its previous catalog; its sidecars are read as they are.
                let read = sidecars.remove(&p.id).unwrap_or_else(|| {
                    let dir = flag_root.join(&p.id);
                    Sidecars::read(&dir, p)
                });
                Arc::new(catalog_browser::BrowserData::new(
                    p,
                    &read.flags,
                    &read.queued,
                    &super::quarantine::descriptions(p, &read.quarantine),
                ))
            })
            .collect()
    })?)?;
    Ok(Snapshot {
        root: Some(root),
        projects,
        browser,
        views,
        jobs,
        fingerprint,
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
