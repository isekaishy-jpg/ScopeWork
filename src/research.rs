use eframe::egui;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Kind {
    App,
    Architecture,
    Chunk,
    Function,
}
impl Kind {
    pub fn completion_criteria(self) -> &'static str {
        match self {
            Self::Function => {
                "Explain this function's behavior, inputs, outputs, side effects, boundaries and connections, supported by evidence. Complete only the prerequisites necessary to establish this function's behavior."
            }
            Self::Chunk => {
                "Synthesize the completed function reports into an explanation of this chunk's behavior, control flow, data flow and external interactions. Every required function must be complete."
            }
            Self::Architecture => {
                "Synthesize the completed chunk reports into an explanation of this subsystem's behavior and interfaces. Every required chunk must be complete."
            }
            Self::App => {
                "Synthesize the completed architecture reports to explain the project's research subject and relevant subsystem interactions. Every required architecture must be complete. The subject may be a selected part of the binary."
            }
        }
    }
    fn child(self) -> Option<Self> {
        match self {
            Self::App => Some(Self::Architecture),
            Self::Architecture => Some(Self::Chunk),
            Self::Chunk => Some(Self::Function),
            Self::Function => None,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::App => "App",
            Self::Architecture => "Architecture",
            Self::Chunk => "Chunk",
            Self::Function => "Function",
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Status {
    NotStarted,
    InProgress,
    Complete,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct Node {
    #[serde(default)]
    pub context_only: bool,
    pub id: u64,
    pub parent: Option<u64>,
    pub kind: Kind,
    pub name: String,
    pub status: Status,
    pub rva: Option<u32>,
    pub revision: u64,
    pub accepted_hash: Option<String>,
    #[serde(default)]
    pub accepted_prerequisites: std::collections::BTreeMap<u64, String>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct Project {
    #[serde(default = "legacy_layout")]
    pub layout_version: u32,
    pub id: String,
    pub name: String,
    pub binary: String,
    #[serde(default)]
    pub research_scope: String,
    pub next_id: u64,
    pub revision: u64,
    #[serde(default)]
    pub structural_epoch: u64,
    #[serde(default)]
    pub active_import: Option<String>,
    #[serde(default)]
    pub last_import: Option<String>,
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
}
fn legacy_layout() -> u32 {
    1
}
#[path = "background.rs"]
mod background;
#[path = "catalog_batch.rs"]
mod catalog_batch;
#[path = "catalog_browser.rs"]
mod catalog_browser;
#[path = "catalog_index.rs"]
mod catalog_index;
#[path = "coordination.rs"]
pub mod coordination;
#[path = "edit_tokens.rs"]
mod edit_tokens;
#[path = "history.rs"]
pub mod history;
#[path = "imports.rs"]
pub mod imports;
#[path = "layout.rs"]
mod layout;
#[path = "ledger.rs"]
mod ledger;
#[path = "operations.rs"]
pub(crate) mod operations;
#[path = "records_ui.rs"]
mod records_ui;
#[path = "reopen.rs"]
mod reopen;
#[path = "report_hashes.rs"]
mod report_hashes;
pub use catalog_batch::Batch;
pub use catalog_batch::Removal;
pub use catalog_batch::ReviewFlag;
// Reports contain their editable source in Markdown; the catalog stores no duplicate report prose.
fn hash(text: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(text.as_bytes()))
}
fn atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let temp = path.with_extension(format!("{}.pending", uuid::Uuid::new_v4()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    rename_retrying(&temp, path).map_err(|e| {
        let _ = fs::remove_file(&temp);
        format!("Cannot replace {}: {e}", path.display())
    })
}
/// Renames `from` to `to` (replacing a file). On Windows a rename fails while any other
/// process (an indexer, antivirus, a tool reading a file inside) has the file or folder open;
/// those holds are usually brief, so sharing, lock and access violations are retried for a few
/// seconds before the error is returned.
fn rename_retrying(from: &Path, to: &Path) -> std::io::Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let mut pause = std::time::Duration::from_millis(5);
    loop {
        match fs::rename(from, to) {
            Err(e)
                if cfg!(windows)
                    && matches!(e.raw_os_error(), Some(5 | 32 | 33))
                    && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(pause);
                pause = (pause * 2).min(std::time::Duration::from_millis(200));
            }
            other => return other,
        }
    }
}
/// Catalog bytes for `project.json`. Compact JSON: the catalog is rewritten on every commit,
/// and pretty printing roughly doubled both its size and the time the write lock is held.
pub(crate) fn catalog_bytes(project: &Project) -> Result<Vec<u8>> {
    serde_json::to_vec(project).map_err(|e| e.to_string())
}
/// Removes `project.<uuid>.pending` temporaries left by interrupted catalog writes. Only call
/// while holding the project's exclusive lock: then no writer can own one of them.
fn remove_orphaned_catalog_temporaries(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("project.") && name.ends_with(".pending") && name.len() > 16 {
            let _ = fs::remove_file(entry.path());
        }
    }
}
pub(crate) fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !["con", "prn", "aux", "nul"].contains(&id)
        && !(id.len() == 4
            && (id.starts_with("com") || id.starts_with("lpt"))
            && matches!(id.as_bytes()[3], b'1'..=b'9'))
}
fn relative(project: &Project, n: &Node) -> String {
    if n.kind == Kind::App {
        return "app.md".into();
    }
    let folder = match n.kind {
        Kind::Function => format!("{:08X}", n.rva.unwrap()),
        _ => format!("{:06}", n.id),
    };
    if n.kind == Kind::Architecture {
        return format!("architectures/{folder}/research.md");
    }
    let parent = project
        .nodes
        .iter()
        .find(|v| Some(v.id) == n.parent)
        .unwrap();
    let parent_path = relative(project, parent);
    format!(
        "{}/{folder}/research.md",
        parent_path.trim_end_matches("/research.md")
    )
}

// One project-local destination for each kind of reversing material.
const PROJECT_FOLDERS: &[&str] = &[
    "architectures",
    "tools/profiles",
    "tools/scripts",
    "tools/instrumentation",
    "tools/bin",
    "tools/workspaces",
    "tools/logs",
    "tools/scratch",
];
fn ensure_folders(dir: &Path) -> Result<()> {
    for folder in PROJECT_FOLDERS {
        fs::create_dir_all(dir.join(folder)).map_err(|e| format!("Cannot create {folder}: {e}"))?;
    }
    // Preserve an existing project tool guide; no tool or evidence files are replaced.
    let guide = dir.join("tools/README.md");
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&guide)
    {
        Ok(mut f) => {
            use std::io::Write;
            f.write_all(b"# Project reversing tools\n\nThis directory belongs only to its enclosing research project.\n\nprofiles/: compiler, ABI, type and analysis profiles.\nscripts/: analysis drivers and exporters.\ninstrumentation/: observational hooks and capture utilities.\nbin/: tool binaries or launchers.\nworkspaces/: shared Ghidra projects and whole-binary analysis databases.\nlogs/: shared tool run logs and provenance.\nscratch/: shared temporary working files.\n\nFunction-specific captures, exports and evidence belong in ../architectures/<category>/<chunk>/<RVA>/data/, beside that function's research.md. Link to them from research.md using data/<filename>. Do not create project-level evidence or data dumps. Chunks and architecture reports reference the canonical reports below them. Source installations remain read-only. Record input binary hashes and tool/profile versions for tool runs.\n").map_err(|e|e.to_string())?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.to_string()),
    }
    Ok(())
}
fn open_folder(path: &Path) -> Result<()> {
    #[cfg(target_os = "windows")]
    let mut command = std::process::Command::new("explorer.exe");
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let mut command = std::process::Command::new("xdg-open");
    command.arg(path).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

/// Report and sidecar texts by project-relative path, standing in for the files on disk.
pub(crate) type Overlay =
    std::sync::Arc<std::sync::RwLock<std::collections::HashMap<String, Option<String>>>>;
pub struct Library {
    pub root: PathBuf,
    pub projects: Vec<Project>,
    pub busy_projects: Vec<String>,
    /// Why a pending transaction could not resume when this read-only view was opened; the
    /// view shows the committed catalog. Writes stay blocked until recovery succeeds.
    pub recovery_blocked: Option<String>,
    /// Texts this view uses instead of the files on disk: reports and sidecars staged by earlier
    /// operations of a merged commit, or replaced by commits newer than a coordinator snapshot.
    /// `None` in an entry means the file does not exist for this view.
    pub(crate) overlay: Option<Overlay>,
    /// Operations collected for one merged commit (see `operations.rs`); `None` commits each.
    staging: Option<operations::Staged>,
    _lock: Option<OperationLocks>,
}
// Keep the legacy gate shared for the lifetime of every project operation. Older
// executables take it exclusively, so mixed-version processes cannot race writes.
struct OperationLocks {
    project: Option<String>,
    writable: bool,
    _project: Option<fs::File>,
    _gate: fs::File,
    /// Holder record written with an exclusive project lock; removed before the lock is released.
    holder: Option<PathBuf>,
}
impl Drop for OperationLocks {
    fn drop(&mut self) {
        if let Some(holder) = self.holder.take() {
            let _ = fs::remove_file(holder);
        }
    }
}
fn holder_path(root: &Path, id: &str, writable: bool) -> Option<PathBuf> {
    writable.then(|| root.join(".project-locks").join(format!("{id}.holder")))
}
/// Longest time a lock acquisition waits for another process before reporting `busy`, in
/// milliseconds. Zero keeps the immediate `busy` answer the GUI refresh relies on to retain
/// snapshots; the CLI sets a wait so callers no longer respawn the process in a retry loop.
static LOCK_WAIT_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
thread_local! {
    /// A per-thread wait, so GUI actions can wait while its refresh keeps answering at once.
    static THREAD_LOCK_WAIT_MS: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}
pub fn set_lock_wait(wait: std::time::Duration) {
    LOCK_WAIT_MS.store(
        wait.as_millis().min(u64::MAX as u128) as u64,
        std::sync::atomic::Ordering::Relaxed,
    );
}
/// Runs `work` with its own lock wait on this thread.
pub fn with_lock_wait<T>(wait: std::time::Duration, work: impl FnOnce() -> T) -> T {
    let previous = THREAD_LOCK_WAIT_MS
        .with(|w| w.replace(Some(wait.as_millis().min(u64::MAX as u128) as u64)));
    let result = work();
    THREAD_LOCK_WAIT_MS.with(|w| w.set(previous));
    result
}
fn lock_wait_ms() -> u64 {
    THREAD_LOCK_WAIT_MS
        .with(|w| w.get())
        .unwrap_or_else(|| LOCK_WAIT_MS.load(std::sync::atomic::Ordering::Relaxed))
}
/// Takes `file`'s lock, waiting up to the configured lock wait with a short backoff. Used for
/// the library gate, which only legacy writers take exclusively.
fn wait_lock(file: &fs::File, exclusive: bool) -> std::result::Result<(), ()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(lock_wait_ms());
    let mut pause = std::time::Duration::from_millis(2);
    loop {
        let taken = if exclusive {
            file.try_lock()
        } else {
            file.try_lock_shared()
        };
        if taken.is_ok() {
            return Ok(());
        }
        let now = std::time::Instant::now();
        if now >= deadline {
            return Err(());
        }
        std::thread::sleep(pause.min(deadline - now));
        pause = (pause * 2).min(std::time::Duration::from_millis(50));
    }
}
/// The writer currently holding a project's exclusive lock, as recorded by that writer.
fn lock_holder(folder: &Path, id: &str) -> Option<String> {
    let text = fs::read_to_string(folder.join(format!("{id}.holder"))).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(format!(
        "held by pid {} ({}) since {}",
        v["pid"],
        v["command"].as_str().unwrap_or("?"),
        v["since"].as_str().unwrap_or("?")
    ))
}

// The project lock queue. A process that cannot take a project lock at once registers a ticket
// in `<id>.waiters/`: `<nanoseconds>-<pid>-<w|r>-<unique>.lock`, which it keeps locked while it
// waits, plus a `.json` record for `research locks`. Tickets order the waiters. A writer goes
// when no older live ticket exists; a reader goes when no older live writer ticket exists, so
// readers share a turn and a waiting writer is never overtaken by new readers. A ticket whose
// lock can be taken belongs to a process that died; it is removed. Older Scopework versions do
// not queue but still exclude correctly through the project lock itself.
struct Ticket {
    name: String,
    lock: PathBuf,
    record: PathBuf,
    _held: fs::File,
}
impl Drop for Ticket {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.record);
        let _ = self._held.unlock();
        let _ = fs::remove_file(&self.lock);
    }
}
fn take_ticket(folder: &Path, id: &str, writable: bool) -> Option<Ticket> {
    let waiters = folder.join(format!("{id}.waiters"));
    fs::create_dir_all(&waiters).ok()?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let name = format!(
        "{nanos:020}-{}-{}-{}",
        std::process::id(),
        if writable { 'w' } else { 'r' },
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    );
    let lock = waiters.join(format!("{name}.lock"));
    let held = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&lock)
        .ok()?;
    held.try_lock().ok()?;
    let record = waiters.join(format!("{name}.json"));
    let command: Vec<String> = std::env::args().skip(1).take(3).collect();
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let info = serde_json::json!({"pid": std::process::id(), "command": command.join(" "), "since_unix": since, "write": writable});
    let _ = fs::write(&record, info.to_string());
    Some(Ticket {
        name,
        lock,
        record,
        _held: held,
    })
}
/// Live tickets in queue order as (name, writer). Dead tickets are removed on the way.
fn live_tickets(folder: &Path, id: &str) -> Vec<(String, bool)> {
    let waiters = folder.join(format!("{id}.waiters"));
    let mut names: Vec<String> = fs::read_dir(&waiters)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            e.file_name()
                .to_string_lossy()
                .strip_suffix(".lock")
                .map(String::from)
        })
        .collect();
    names.sort();
    names
        .into_iter()
        .filter(|name| {
            let path = waiters.join(format!("{name}.lock"));
            let Ok(file) = fs::OpenOptions::new().read(true).write(true).open(&path) else {
                return false;
            };
            if file.try_lock_shared().is_err() {
                return true;
            }
            // Nobody holds it, so its waiter died, unless it was created a moment ago and is
            // about to be locked: give a fresh ticket a second.
            let _ = file.unlock();
            drop(file);
            let young = name
                .split('-')
                .next()
                .and_then(|n| n.parse::<u128>().ok())
                .is_some_and(|n| {
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_nanos())
                        .saturating_sub(n)
                        < 1_000_000_000
                });
            if !young {
                let _ = fs::remove_file(&path);
                let _ = fs::remove_file(waiters.join(format!("{name}.json")));
            }
            young
        })
        .map(|name| {
            let writer = name.split('-').nth(2) == Some("w");
            (name, writer)
        })
        .collect()
}
/// Whether a live ticket ahead of `me` (all tickets when `me` is `None`) must go first.
fn queued_ahead(tickets: &[(String, bool)], me: Option<&str>, writable: bool) -> bool {
    tickets
        .iter()
        .filter(|(name, _)| me.is_none_or(|me| name.as_str() < me))
        .any(|(_, writer)| writable || *writer)
}
/// Who holds and who waits for a project's lock, without taking it.
pub fn lock_status(projects: &Path, id: &str) -> Result<serde_json::Value> {
    if !valid_id(id) {
        return Err("Invalid project ID".into());
    }
    let mut status = lock_entry(projects, id);
    // Tool resource locks taken by `job run` without a coordinator.
    let prefix = format!("{id}.res-");
    let mut resources = serde_json::Map::new();
    for entry in fs::read_dir(projects.join(".project-locks"))
        .into_iter()
        .flatten()
        .flatten()
    {
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(resource) = name
            .strip_prefix(&prefix)
            .and_then(|rest| rest.strip_suffix(".lock"))
        {
            let mut entry = lock_entry(projects, &resource_lock_id(id, resource));
            if let Some(object) = entry.as_object_mut() {
                object.remove("project");
            }
            resources.insert(resource.to_string(), entry);
        }
    }
    status["resources"] = resources.into();
    Ok(status)
}
/// The lock name of a project's tool resource.
fn resource_lock_id(project: &str, resource: &str) -> String {
    format!("{project}.res-{resource}")
}
/// Exclusive locks on a project's tool resources for `job run` without a coordinator, taken
/// in name order so that two requests never deadlock. They wait like project locks.
pub(crate) fn resource_locks(
    projects: &Path,
    project: &str,
    resources: &[String],
) -> Result<Vec<ResourceLock>> {
    if !valid_id(project) {
        return Err("Invalid project ID".into());
    }
    let mut names = resources.to_vec();
    names.sort();
    names.dedup();
    names
        .iter()
        .map(|resource| {
            let id = resource_lock_id(project, resource);
            Ok(ResourceLock {
                _file: project_lock(projects, &id, true)?,
                holder: projects.join(".project-locks").join(format!("{id}.holder")),
            })
        })
        .collect()
}
/// A lock from `resource_locks`. Like a project lock, it removes its holder record before
/// the lock itself is released.
pub(crate) struct ResourceLock {
    _file: fs::File,
    holder: PathBuf,
}
impl Drop for ResourceLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.holder);
    }
}
fn lock_entry(projects: &Path, id: &str) -> serde_json::Value {
    let folder = projects.join(".project-locks");
    let path = folder.join(format!("{id}.lock"));
    let (writer, readers) = match fs::OpenOptions::new().read(true).write(true).open(&path) {
        Err(_) => (false, false),
        Ok(file) => {
            let writer = file.try_lock_shared().is_err();
            if !writer {
                let _ = file.unlock();
            }
            let readers = !writer && file.try_lock().is_err();
            if !writer && !readers {
                let _ = file.unlock();
            }
            (writer, readers)
        }
    };
    let holder = writer
        .then(|| fs::read_to_string(folder.join(format!("{id}.holder"))).ok())
        .flatten()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
    let waiters = folder.join(format!("{id}.waiters"));
    let waiting: Vec<serde_json::Value> = live_tickets(&folder, id)
        .into_iter()
        .enumerate()
        .filter_map(|(position, (name, _))| {
            let mut v: serde_json::Value = serde_json::from_str(
                &fs::read_to_string(waiters.join(format!("{name}.json"))).ok()?,
            )
            .ok()?;
            v["position"] = (position + 1).into();
            Some(v)
        })
        .collect();
    serde_json::json!({"project": id, "writer_active": writer, "readers_active": readers, "writer": holder, "waiting": waiting})
}
/// The legacy library gate (shared unless `exclusive_gate`) plus the project's own lock.
fn acquire_locks(
    root: &Path,
    id: &str,
    exclusive_gate: bool,
    write: bool,
) -> Result<OperationLocks> {
    let gate = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join(".library.lock"))
        .map_err(|e| e.to_string())?;
    wait_lock(&gate, exclusive_gate).map_err(|_| {
        "Research library is busy with another operation. Retry shortly.".to_string()
    })?;
    Ok(OperationLocks {
        project: Some(id.into()),
        writable: write,
        _project: Some(project_lock(root, id, write)?),
        _gate: gate,
        holder: holder_path(root, id, write),
    })
}
fn project_lock(root: &Path, id: &str, writable: bool) -> Result<fs::File> {
    let folder = root.join(".project-locks");
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(folder.join(format!("{id}.lock")))
        .map_err(|e| e.to_string())?;
    let take = |file: &fs::File| {
        if writable {
            file.try_lock()
        } else {
            file.try_lock_shared()
        }
        .is_ok()
    };
    let wait = lock_wait_ms();
    let mut taken = !queued_ahead(&live_tickets(&folder, id), None, writable) && take(&file);
    if !taken && wait > 0 {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(wait);
        let ticket = take_ticket(&folder, id, writable);
        let mut pause = std::time::Duration::from_millis(2);
        while !taken && std::time::Instant::now() < deadline {
            std::thread::sleep(pause);
            pause = (pause * 2).min(std::time::Duration::from_millis(50));
            let tickets = live_tickets(&folder, id);
            let me = ticket.as_ref().map(|t| t.name.as_str());
            // Without a ticket (unwritable lock folder) fall back to plain polling.
            taken = (me.is_none() || !queued_ahead(&tickets, me, writable)) && take(&file);
        }
    }
    if !taken {
        return Err(match lock_holder(&folder, id) {
            Some(holder) => format!(
                "Research project '{id}' is busy with another operation ({holder}). Retry shortly."
            ),
            None => {
                format!("Research project '{id}' is busy with another operation. Retry shortly.")
            }
        });
    }
    if writable {
        // Waiters read this to say who holds the lock; it is advisory and never trusted for safety.
        let command: Vec<String> = std::env::args().skip(1).take(3).collect();
        let since = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let holder = serde_json::json!({"pid": std::process::id(), "command": command.join(" "), "since": format!("unix {since}")});
        let _ = fs::write(folder.join(format!("{id}.holder")), holder.to_string());
    }
    Ok(file)
}
impl Library {
    fn commit_lease(root: &Path, id: &str) -> Result<OperationLocks> {
        if !valid_id(id) {
            return Err("Invalid project ID".into());
        }
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_millis(lock_wait_ms().max(5000));
        loop {
            let result: Result<OperationLocks> = (|| {
                let gate = fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(root.join(".library.lock"))
                    .map_err(|e| e.to_string())?;
                gate.try_lock_shared()
                    .map_err(|_| "Research library is busy".to_string())?;
                Ok(OperationLocks {
                    project: Some(id.into()),
                    writable: true,
                    _project: Some(project_lock(root, id, true)?),
                    _gate: gate,
                    holder: holder_path(root, id, true),
                })
            })();
            match result {
                Err(e) if e.contains("is busy") && std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(20))
                }
                other => return other,
            }
        }
    }
    pub fn set_scope(&mut self, p: usize, scope: &str, expected_revision: u64) -> Result<()> {
        if !self.writable(p) {
            return self.write_project(p, |l, p| l.set_scope(p, scope, expected_revision));
        }
        if self.projects[p].revision != expected_revision {
            return Err("Project changed. Reload and review its scope before saving.".into());
        }
        let scope = scope.trim();
        if scope.chars().any(char::is_control) {
            return Err(
                "Research scope is a single-line topic label, such as Rendering or Asset platform."
                    .into(),
            );
        }
        if scope == self.projects[p].research_scope {
            return Ok(());
        }
        self.projects[p].research_scope = scope.into();
        self.projects[p].revision += 1;
        self.persist(p)
    }
    pub fn release(&mut self) {
        self._lock = None;
    }
    fn fresh(&self) -> Result<Self> {
        let mut fresh = Self::open(self.root.clone())?;
        for old in &self.projects {
            if fresh.busy_projects.contains(&old.id) {
                fresh.projects.push(old.clone());
            }
        }
        fresh.projects.sort_by_key(|p| {
            self.projects
                .iter()
                .position(|old| old.id == p.id)
                .unwrap_or(usize::MAX)
        });
        Ok(fresh)
    }
    fn write_project<T>(
        &mut self,
        p: usize,
        operation: impl FnOnce(&mut Self, usize) -> Result<T>,
    ) -> Result<T> {
        let id = self
            .projects
            .get(p)
            .ok_or("Project no longer exists")?
            .id
            .clone();
        self.release();
        let mut fresh = Self::open_project(self.root.clone(), Some(&id))?;
        let current = fresh
            .projects
            .iter()
            .position(|v| v.id == id)
            .ok_or("Project was removed. Refresh before continuing.")?;
        let result = operation(&mut fresh, current);
        fresh.release();
        self.projects[p] = fresh.projects.remove(current);
        self.busy_projects.retain(|busy| busy != &id);
        result
    }
    pub fn refresh(&mut self) -> Result<()> {
        let mut fresh = self.fresh()?;
        fresh.release();
        *self = fresh;
        Ok(())
    }
    pub fn set_binary(&mut self, p: usize, binary: &str) -> Result<()> {
        if !self.writable(p) {
            return self.write_project(p, |l, p| l.set_binary(p, binary));
        }
        self.ensure_structural(p)?;
        if binary.trim().is_empty() {
            return Err("Binary identity cannot be empty.".into());
        }
        if self.projects[p]
            .nodes
            .iter()
            .any(|n| n.kind == Kind::Function)
        {
            return Err("Binary identity is locked after adding functions.".into());
        }
        let before = self.projects[p].clone();
        let mut after = before.clone();
        after.structural_epoch += 1;
        after.binary = binary.into();
        after.revision += 1;
        let old_index = catalog_index::Index::new(&before);
        let index = catalog_index::Index::new(&after);
        let mut reports = vec![];
        for node in &before.nodes {
            let original = fs::read_to_string(self.dir(p).join(&old_index.paths[&node.id]))
                .map_err(|e| e.to_string())?;
            edit_tokens::draft(&before, &old_index, node.id, &original)?;
            let (body, evidence) = edit_tokens::parts(&original)?;
            reports.push((
                node.id,
                original.clone(),
                index.render(&after, node.id, body, evidence),
            ));
        }
        for node in &mut after.nodes {
            node.accepted_hash = None;
            node.accepted_prerequisites.clear();
            if node.status == Status::Complete {
                node.status = Status::InProgress;
            }
            node.revision += 1;
        }
        self.commit_metadata(p, before, after, reports, vec![])
    }

    fn writable(&self, p: usize) -> bool {
        self._lock.as_ref().is_some_and(|l| {
            l.writable && l.project.as_deref() == Some(self.projects[p].id.as_str())
        })
    }
    fn ensure_structural(&self, p: usize) -> Result<()> {
        if let Some(job) = &self.projects[p].active_import {
            return Err(format!(
                "structural_job_conflict: import {job} owns the structural reservation; pause does not release it. Commit or cancel the job first."
            ));
        }
        Ok(())
    }
    /// A read-only view of a committed catalog, as a coordinator snapshot holds it. It takes
    /// no lock: reports and sidecars are read through `overlay`, which keeps the texts newer
    /// commits replaced, so answers always come from this one revision.
    pub(crate) fn snapshot_view(root: PathBuf, project: Project, overlay: Overlay) -> Self {
        Self {
            root,
            projects: vec![project],
            busy_projects: vec![],
            recovery_blocked: None,
            overlay: Some(overlay),
            staging: None,
            _lock: None,
        }
    }
    pub fn open(root: PathBuf) -> Result<Self> {
        Self::open_project(root, None)
    }
    pub fn open_project(root: PathBuf, selected: Option<&str>) -> Result<Self> {
        Self::open_mode(root, selected, true, true)
    }
    /// Releases this library's locks while keeping its catalog in memory (the coordinator's
    /// writer between batches). Only `relock` makes it writable again.
    pub(crate) fn release_locks(&mut self) {
        self._lock = None;
    }
    /// Re-takes the writable lock of the one project this library holds in memory. The caller
    /// must then check that the catalog on disk is still the one held.
    pub(crate) fn relock(&mut self) -> Result<()> {
        let id = self.projects.first().ok_or("No project held")?.id.clone();
        self._lock = Some(acquire_locks(&self.root, &id, false, true)?);
        Ok(())
    }
    pub fn read_project(root: PathBuf, selected: &str) -> Result<Self> {
        Self::open_mode(root, Some(selected), false, true)
    }
    fn open_mode(
        root: PathBuf,
        selected: Option<&str>,
        writable: bool,
        recover: bool,
    ) -> Result<Self> {
        fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let Some(id) = selected else {
            let mut lib = Self {
                root: root.clone(),
                projects: vec![],
                busy_projects: vec![],
                _lock: None,
                recovery_blocked: None,
                overlay: None,
                staging: None,
            };
            for entry in fs::read_dir(&root).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                if entry.file_type().map_err(|e| e.to_string())?.is_symlink()
                    || !entry.path().join("project.json").is_file()
                {
                    continue;
                }
                let id = entry.file_name().to_string_lossy().into_owned();
                match Self::read_project(root.clone(), &id) {
                    Ok(mut project) => lib.projects.append(&mut project.projects),
                    Err(e) if e.contains("is busy") => lib.busy_projects.push(id),
                    Err(e) => return Err(e),
                }
            }
            lib.projects.sort_by_key(|p| p.name.clone());
            return Ok(lib);
        };
        if !valid_id(id) {
            return Err("Invalid project identity".into());
        }
        let acquire =
            |exclusive_gate: bool, write: bool| acquire_locks(&root, id, exclusive_gate, write);
        let mut locks = acquire(false, writable)?;
        let dir = root.join(id);
        if dir.exists() {
            layout::inside(&root, &dir)?;
        }
        let path = dir.join("project.json");
        let mut project: Option<Project> = if path.is_file() {
            Some(
                serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?,
            )
        } else {
            None
        };
        if project.as_ref().is_some_and(|p| p.layout_version < 5) {
            if !recover {
                return Err("migration_required: open this project with the current CLI before planning an import".into());
            }
            // Never upgrade a held shared lease. Exclude legacy writers during migration.
            drop(locks);
            locks = acquire(true, true)?;
            let mut p: Project =
                serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            catalog_batch::resume(&dir, &mut p)?;
            imports::recover(&dir, &mut p)?;
            if p.active_import.is_some() {
                return Err("Finish or cancel the active import with the previous Scopework version before upgrading this project.".into());
            }
            Self::validate_project(&p, id)?;
            layout::migrate(&dir, &mut p)?;
            if p.layout_version < 5 {
                let backup = dir.join("tools/workspaces/layout-migration-v5");
                fs::create_dir_all(&backup).map_err(|e| e.to_string())?;
                if !backup.join("before.json").exists() {
                    atomic(
                        &backup.join("before.json"),
                        &serde_json::to_vec_pretty(&p).map_err(|e| e.to_string())?,
                    )?;
                }
                p.layout_version = 5;
                atomic(&path, &catalog_bytes(&p)?)?;
                atomic(
                    &backup.join("migration.json"),
                    b"{\"format\":5,\"authored_reports_changed\":false}",
                )?;
            }
            drop(locks);
            locks = acquire(false, writable)?;
            project = Some(
                serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?,
            );
        }
        let mut recovery_blocked = None;
        if let Some(p) = &mut project {
            if p.layout_version != 5 {
                return Err("Unsupported catalog format; upgrade Scopework.".into());
            }
            let pending = dir
                .join("tools/workspaces/catalog-transactions/pending/journal.json")
                .is_file()
                || dir
                    .join("tools/workspaces/catalog-transactions-v4/pending/journal.json")
                    .is_file()
                || imports::needs_recovery(&dir, p);
            if pending && !recover {
                return Err(
                    "recovery_required: resolve pending publication before planning an import"
                        .into(),
                );
            }
            if pending && !locks.writable {
                drop(locks);
                locks = acquire(false, true)?;
                *p = serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            }
            if locks.writable {
                remove_orphaned_catalog_temporaries(&dir);
                let recovered =
                    catalog_batch::resume(&dir, p).and_then(|_| imports::recover(&dir, p));
                match recovered {
                    Ok(()) => {}
                    Err(e) if writable => return Err(e),
                    // A blocked recovery must not stop reads: answer from the committed catalog.
                    Err(e) => {
                        drop(locks);
                        locks = acquire(false, false)?;
                        *p = serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?)
                            .map_err(|e| e.to_string())?;
                        recovery_blocked = Some(e);
                    }
                }
            }
            Self::validate_project(p, id)?;
            if !dir.join("app.md").is_file() {
                return Err("Canonical app report missing. Restore it before continuing.".into());
            }
        }
        Ok(Self {
            root,
            projects: project.into_iter().collect(),
            busy_projects: vec![],
            recovery_blocked,
            overlay: None,
            staging: None,
            _lock: Some(locks),
        })
    }
    fn validate_project(p: &Project, id: &str) -> Result<()> {
        use std::collections::{HashMap, HashSet};
        if p.id != id || !valid_id(&p.id) {
            return Err("Invalid project identity".into());
        }
        let lookup: HashMap<_, _> = p.nodes.iter().map(|n| (n.id, n)).collect();
        let mut ids = HashSet::new();
        let mut rvas = HashSet::new();
        for n in &p.nodes {
            if !ids.insert(n.id) {
                return Err("Duplicate node identity".into());
            }
            if n.kind == Kind::Function && (n.rva.is_none() || !rvas.insert(n.rva)) {
                return Err("Invalid or duplicate function RVA".into());
            }
            if n.kind != Kind::App {
                let parent = n
                    .parent
                    .and_then(|id| lookup.get(&id))
                    .ok_or("Missing report parent")?;
                if parent.kind.child() != Some(n.kind) {
                    return Err("Invalid research hierarchy".into());
                }
            } else if n.parent.is_some() {
                return Err("App cannot have a parent".into());
            }
        }
        if p.nodes.iter().filter(|n| n.kind == Kind::App).count() != 1
            || !p.nodes.iter().any(|n| n.id == 0 && n.kind == Kind::App)
            || p.nodes.iter().any(|n| n.id >= p.next_id)
        {
            return Err("Invalid app ID or next node ID".into());
        }
        let validation = Self {
            root: PathBuf::new(),
            projects: vec![p.clone()],
            busy_projects: vec![],
            _lock: None,
            recovery_blocked: None,
            overlay: None,
            staging: None,
        };
        validation.validate_dependencies(0, &p.dependencies)
    }
    fn dir(&self, p: usize) -> PathBuf {
        self.root.join(&self.projects[p].id)
    }
    fn persist(&self, p: usize) -> Result<()> {
        atomic(
            &self.dir(p).join("project.json"),
            &catalog_bytes(&self.projects[p])?,
        )
    }
    pub fn create(&mut self, id: &str, name: &str, binary: &str) -> Result<()> {
        if self
            ._lock
            .as_ref()
            .is_none_or(|locks| !locks.writable || locks.project.as_deref() != Some(id))
        {
            let mut fresh = Self::open_project(self.root.clone(), Some(id))?;
            fresh.create(id, name, binary)?;
            fresh.release();
            self.projects.extend(fresh.projects);
            return Ok(());
        }
        if !valid_id(id) || name.trim().is_empty() {
            return Err("Use a project ID with lowercase letters, digits, and hyphens, and a nonempty name.".into());
        }
        if self.root.join(id).exists() {
            return Err("That project already exists.".into());
        }
        let dir = self.root.join(id);
        let stage = self
            .root
            .join(".project-staging")
            .join(uuid::Uuid::new_v4().to_string());
        layout::inside(&self.root, &stage)?;
        fs::create_dir_all(&stage).map_err(|e| e.to_string())?;
        ensure_folders(&stage)?;
        let project = Project {
            layout_version: 5,
            structural_epoch: 0,
            active_import: None,
            last_import: None,
            id: id.into(),
            name: name.into(),
            binary: binary.into(),
            research_scope: String::new(),
            dependencies: vec![],
            next_id: 1,
            revision: 0,
            nodes: vec![Node {
                context_only: false,
                id: 0,
                parent: None,
                kind: Kind::App,
                name: format!("{name} research"),
                status: Status::NotStarted,
                rva: None,
                revision: 0,
                accepted_hash: None,
                accepted_prerequisites: Default::default(),
            }],
        };
        let text = catalog_index::Index::new(&project).render(&project, 0, "", "");
        atomic(&stage.join("app.md"), text.as_bytes())?;
        atomic(&stage.join("project.json"), &catalog_bytes(&project)?)?;
        fs::rename(&stage, &dir).map_err(|e| e.to_string())?;
        self.projects.push(project);
        Ok(())
    }
    pub fn path(&self, p: usize, id: u64) -> Result<PathBuf> {
        let n = self.projects[p]
            .nodes
            .iter()
            .find(|n| n.id == id)
            .ok_or("Unknown report")?;
        let path = self.dir(p).join(relative(&self.projects[p], n));
        layout::inside(&self.dir(p), &path)?;
        Ok(path)
    }
    /// A report's path relative to its project folder.
    pub(crate) fn report_rel(&self, p: usize, id: u64) -> Result<String> {
        let n = self.projects[p]
            .nodes
            .iter()
            .find(|n| n.id == id)
            .ok_or("Unknown report")?;
        Ok(relative(&self.projects[p], n))
    }
    pub fn read(&self, p: usize, id: u64) -> Result<(String, String)> {
        self.path(p, id)?;
        let full = self.text(p, &self.report_rel(p, id)?)?;
        let marker = "\n<!-- research-content -->\n";
        let content = full
            .split_once(marker)
            .ok_or("Report marker is missing; restore the canonical report header before editing.")?
            .1;
        let (body, evidence) = content
            .split_once("\n<!-- research-evidence -->\n")
            .ok_or("Evidence marker missing")?;
        Ok((body.to_string(), evidence.to_string()))
    }
    /// Reopens the complete records a change to `id` reaches (see `reopen.rs`).
    fn reopen_affected(&mut self, p: usize, id: u64) {
        let consumers = reopen::consumers_of(&self.projects[p].dependencies);
        reopen::reopen_affected(
            &mut self.projects[p],
            &[id].into(),
            &Default::default(),
            &consumers,
        );
    }
    pub fn effective_all(&self, p: usize) -> std::collections::HashMap<u64, Status> {
        catalog_index::Index::new(&self.projects[p])
            .states_all_with(&self.dir(p), &|| self.hash_overrides())
    }
    pub fn required_nodes(&self, p: usize) -> std::collections::HashSet<u64> {
        catalog_index::Index::new(&self.projects[p]).required
    }
    pub fn report_paths(&self, p: usize) -> std::collections::HashMap<u64, PathBuf> {
        catalog_index::Index::new(&self.projects[p])
            .paths
            .into_iter()
            .map(|(id, path)| (id, self.dir(p).join(path)))
            .collect()
    }
    pub fn effective(&self, p: usize, id: u64) -> Status {
        let Some(n) = self.projects[p].nodes.iter().find(|n| n.id == id) else {
            return Status::NotStarted;
        };
        if n.kind == Kind::Function && n.status != Status::Complete {
            return n.status;
        }
        catalog_index::Index::new(&self.projects[p])
            .states_with(&self.dir(p), [id], &|| self.hash_overrides())
            .get(&id)
            .copied()
            .unwrap_or(Status::NotStarted)
    }
    /// What keeps `id` from being completed, apart from its own report text: a review flag,
    /// incomplete required children and incomplete prerequisites. Empty when nothing blocks it.
    pub fn completion_blockers(&self, p: usize, id: u64) -> Result<Vec<String>> {
        let project = &self.projects[p];
        let node = project
            .nodes
            .iter()
            .find(|n| n.id == id)
            .ok_or("Unknown node")?;
        let states = self.completion_states(p, id);
        let mut blockers = vec![];
        if self.review_flags(p)?.contains_key(&id) {
            blockers.push("Flagged for review: clear the flag first.".to_string());
        }
        if let Some(kind) = node.kind.child() {
            let kids: Vec<_> = project
                .nodes
                .iter()
                .filter(|k| k.parent == Some(id) && !k.context_only)
                .collect();
            let open = kids
                .iter()
                .filter(|k| states.get(&k.id) != Some(&Status::Complete))
                .count();
            let noun = kind.label().to_lowercase();
            if kids.is_empty() {
                blockers.push(format!("No required {noun} records yet."));
            } else if open > 0 {
                blockers.push(format!(
                    "{open} of {} required {noun} records not complete.",
                    kids.len()
                ));
            }
        }
        let prerequisites = project
            .dependencies
            .iter()
            .filter(|e| e.dependent == id)
            .filter(|e| states.get(&e.prerequisite) != Some(&Status::Complete))
            .count();
        if prerequisites > 0 {
            blockers.push(format!(
                "{prerequisites} prerequisite function(s) not complete."
            ));
        }
        Ok(blockers)
    }
    /// Effective status of `id` and everything its completion depends on: required children
    /// and tagged prerequisites. Evaluates only that closure, not the whole project.
    fn completion_states(&self, p: usize, id: u64) -> std::collections::HashMap<u64, Status> {
        let project = &self.projects[p];
        let inputs = project
            .nodes
            .iter()
            .filter(|n| n.parent == Some(id))
            .map(|n| n.id)
            .chain(
                project
                    .dependencies
                    .iter()
                    .filter(|e| e.dependent == id)
                    .map(|e| e.prerequisite),
            )
            .chain([id]);
        catalog_index::Index::new(project)
            .states_with(&self.dir(p), inputs, &|| self.hash_overrides())
    }
    pub fn add(&mut self, p: usize, parent: u64, name: &str, rva: &str) -> Result<u64> {
        if !self.writable(p) {
            return self.write_project(p, |l, p| l.add(p, parent, name, rva));
        }
        self.ensure_structural(p)?;
        if name.trim().is_empty() {
            return Err("Enter a name.".into());
        }
        let kind = self.projects[p]
            .nodes
            .iter()
            .find(|n| n.id == parent)
            .ok_or("Unknown parent")?
            .kind
            .child()
            .ok_or("Functions cannot have children")?;
        let address = if kind == Kind::Function {
            if self.projects[p].binary.trim().is_empty() {
                return Err("Set the project's binary build/hash before adding functions.".into());
            }
            let x = u32::from_str_radix(
                rva.trim().trim_start_matches("0x").trim_start_matches("0X"),
                16,
            )
            .map_err(|_| "Enter an RVA in hexadecimal, not a VA.")?;
            if self.projects[p].nodes.iter().any(|n| n.rva == Some(x)) {
                return Err("That function already has a canonical file in this project.".into());
            }
            Some(x)
        } else {
            None
        };
        let batch = Batch {
            schema: 1,
            binary: self.projects[p].binary.clone(),
            expect_revision: Some(self.projects[p].revision),
            create: vec![catalog_batch::Create {
                parent,
                name: name.trim().into(),
                rva: address,
                context_only: false,
            }],
            update: vec![],
            reorder: vec![],
            repair_legacy_links: false,
        };
        self.batch(p, &batch)?
            .into_iter()
            .next()
            .ok_or_else(|| "Creation returned no identity".into())
    }
    pub fn save_report(
        &mut self,
        p: usize,
        id: u64,
        body: &str,
        evidence: &str,
        complete: bool,
        expected: &str,
    ) -> Result<()> {
        if !self.writable(p) {
            return self.write_project(p, |l, p| {
                l.save_report(p, id, body, evidence, complete, expected)
            });
        }
        self.check_edit_token(
            p,
            id,
            expected,
            if complete { "completion" } else { "draft" },
        )?;
        if !complete {
            let batch = catalog_batch::ReportBatch {
                schema: 2,
                binary: self.projects[p].binary.clone(),
                expect_revision: None,
                reports: vec![catalog_batch::ReportSave {
                    id,
                    expect: expected.into(),
                    body: body.into(),
                    evidence: evidence.into(),
                }],
                evidence_only: false,
            };
            self.save_reports(p, &batch)?;
            return Ok(());
        }
        let n = self.projects[p].nodes.iter().find(|n| n.id == id).unwrap();
        let (old_body, old_evidence) = self.read(p, id)?;
        if old_body == body
            && old_evidence == evidence
            && complete == (n.status == Status::Complete)
            && self.completion_states(p, id)[&id] == n.status
        {
            return Ok(());
        }
        let states = if complete {
            self.completion_states(p, id)
        } else {
            Default::default()
        };
        if complete && self.review_flags(p)?.contains_key(&id) {
            return Err(
                "This record is flagged for review. Clear the review flag before completing it."
                    .into(),
            );
        }
        if complete {
            if body.trim().is_empty() {
                return Err(
                    "Write the compiled understanding report before marking complete.".into(),
                );
            }
            if n.kind == Kind::Function && evidence.trim().is_empty() {
                return Err("A function needs evidence references and claim classification before completion.".into());
            }
            let kids: Vec<_> = self.projects[p]
                .nodes
                .iter()
                .filter(|k| k.parent == Some(id) && !k.context_only)
                .collect();
            if n.kind != Kind::Function
                && (kids.is_empty() || kids.iter().any(|k| states[&k.id] != Status::Complete))
            {
                return Err("Every required child must be complete, and the parent needs its own compiled report.".into());
            }
        }
        if complete
            && self.projects[p]
                .dependencies
                .iter()
                .filter(|e| e.dependent == id)
                .any(|e| states[&e.prerequisite] != Status::Complete)
        {
            return Err(
                "Every tagged prerequisite must be complete before this function can be complete."
                    .into(),
            );
        }
        let stamps = if complete {
            self.dependency_stamps(p, id)
        } else {
            Default::default()
        };
        if body.contains("<!-- research-") || evidence.contains("<!-- research-") {
            return Err("Reserved report markers cannot appear in report content.".into());
        }
        let before = self.projects[p].clone();
        let original = self.text(p, &self.report_rel(p, id)?)?;
        let report = catalog_index::Index::new(&before).render(&before, id, body, evidence);
        let n = self.projects[p]
            .nodes
            .iter_mut()
            .find(|n| n.id == id)
            .unwrap();
        n.status = if complete {
            Status::Complete
        } else if body.trim().is_empty() && evidence.trim().is_empty() {
            Status::NotStarted
        } else {
            Status::InProgress
        };
        n.accepted_hash = complete.then(|| hash(&report));
        n.accepted_prerequisites = stamps;
        n.revision += 1;
        self.reopen_affected(p, id);
        self.projects[p].revision += 1;
        let after = std::mem::replace(&mut self.projects[p], before.clone());
        self.commit_report(p, before, after, id, original, report)
    }
}

impl Library {
    /// Save generated evidence without changing a Not Started or In Progress status.
    pub fn save_evidence(
        &mut self,
        p: usize,
        id: u64,
        body: &str,
        evidence: &str,
        expected: &str,
    ) -> Result<()> {
        if !self.writable(p) {
            return self.write_project(p, |l, p| l.save_evidence(p, id, body, evidence, expected));
        }
        self.check_edit_token(p, id, expected, "draft")?;
        let batch = catalog_batch::ReportBatch {
            schema: 2,
            binary: self.projects[p].binary.clone(),
            expect_revision: None,
            reports: vec![catalog_batch::ReportSave {
                id,
                expect: expected.into(),
                body: body.into(),
                evidence: evidence.into(),
            }],
            evidence_only: true,
        };
        self.save_reports(p, &batch).map(|_| ())
    }
}

type ActionResult = Result<Option<(String, u64)>>;

pub struct ResearchUi {
    browser: catalog_browser::Browser,
    ui_projects: Vec<std::sync::Arc<Project>>,
    pub open: bool,
    root: Option<PathBuf>,
    pub changed: bool,
    project: usize,
    project_id: String,
    last_poll: std::time::Instant,
    fingerprint: String,
    views: Vec<ProjectView>,
    browser_data: Vec<std::sync::Arc<catalog_browser::BrowserData>>,
    refresh_task: Option<(u64, std::sync::mpsc::Receiver<Result<()>>)>,
    snapshot_inbox: Option<background::SnapshotInbox>,
    load_task: Option<(u64, std::sync::mpsc::Receiver<Result<background::Loaded>>)>,
    action_task: Option<std::sync::mpsc::Receiver<ActionResult>>,
    generation: u64,
    refresh_generation: u64,
    requested: Option<(String, u64)>,
    import_file: String,
    jobs: std::collections::HashMap<String, imports::Job>,
    node: u64,
    body: String,
    evidence: String,
    loaded_hash: String,
    completion_hash: String,
    dirty: bool,
    message: String,
    new_name: String,
    rva: String,
    dependency_rvas: String,
    new_project_name: String,
    new_project_id: String,
    binary: String,
    scope: String,
    scope_dirty: bool,
    scope_revision: u64,
    complete: bool,
    flag_reason: String,
    /// The loaded record's effective status and what blocks its completion.
    status: Status,
    blockers: Vec<String>,
    /// Message styling: errors in the alert colour.
    message_error: bool,
    tab: usize,
    confirm_complete: bool,
    reviewed: bool,
    new_project_open: bool,
    /// Write queues of the projects on screen, started once the research home is known.
    queue: Option<crate::queue_watch::QueueWatch>,
    /// The project whose queue list is open.
    queue_open: Option<String>,
}
impl Default for ResearchUi {
    fn default() -> Self {
        Self::new()
    }
}

impl ResearchUi {
    pub fn new() -> Self {
        Self {
            browser: Default::default(),
            ui_projects: vec![],
            open: false,
            root: None,
            changed: true,
            project: 0,
            project_id: String::new(),
            last_poll: std::time::Instant::now() - std::time::Duration::from_secs(3),
            fingerprint: String::new(),
            views: vec![],
            browser_data: vec![],
            refresh_task: None,
            snapshot_inbox: None,
            load_task: None,
            action_task: None,
            refresh_generation: 0,
            generation: 0,
            requested: None,
            import_file: String::new(),
            jobs: Default::default(),
            node: 0,
            body: String::new(),
            evidence: String::new(),
            loaded_hash: String::new(),
            completion_hash: String::new(),
            dirty: false,
            message: String::new(),
            new_name: String::new(),
            rva: String::new(),
            dependency_rvas: String::new(),
            new_project_name: String::new(),
            new_project_id: String::new(),
            binary: String::new(),
            scope: String::new(),
            scope_dirty: false,
            scope_revision: 0,
            complete: false,
            flag_reason: String::new(),
            status: Status::NotStarted,
            blockers: vec![],
            message_error: false,
            tab: 0,
            confirm_complete: false,
            reviewed: false,
            new_project_open: false,
            queue: None,
            queue_open: None,
        }
    }
    fn load(&mut self, p: usize, id: u64) {
        if let Some(project) = self.ui_projects.get(p) {
            self.generation += 1;
            self.requested = Some((project.id.clone(), id));
            self.dirty = false;
            self.scope_dirty = false;
        }
    }
    fn action(
        &mut self,
        work: impl FnOnce(PathBuf) -> Result<Option<(String, u64)>> + Send + 'static,
    ) {
        if self.action_task.is_some() {
            return;
        }
        let (send, recv) = std::sync::mpsc::sync_channel(1);
        self.action_task = Some(recv);
        std::thread::spawn(move || {
            // Human actions wait for agents' operations instead of failing `busy`.
            let result = crate::paths::home(None)
                .map(|h| h.join("projects"))
                .and_then(|root| with_lock_wait(std::time::Duration::from_secs(60), || work(root)));
            let _ = send.send(result);
        });
    }
    fn mutate(
        &mut self,
        work: impl FnOnce(&mut Library, usize, u64) -> Result<u64> + Send + 'static,
    ) {
        let project = self.project_id.clone();
        let id = self.node;
        self.action(move |root| {
            let mut lib = Library::open_project(root, Some(&project))?;
            if lib.projects.is_empty() {
                return Err("Project was removed".into());
            }
            let id = work(&mut lib, 0, id)?;
            Ok(Some((project, id)))
        });
    }
}

#[derive(Clone)]
pub struct Meter {
    pub id: u64,
    pub name: String,
    /// Required functions, and how many are complete or in progress by their own effective
    /// status; the rest are not started.
    pub functions: usize,
    pub completed_functions: usize,
    pub in_progress_functions: usize,
    pub states: Vec<usize>,
}
#[derive(Clone)]
pub struct ProjectView {
    pub inventory_functions: usize,
    pub required_functions: usize,
    pub app_status: Status,
    pub id: String,
    pub name: String,
    pub meters: Vec<Meter>,
    pub research_scope: String,
}
impl ResearchUi {
    pub fn poll(&mut self, ctx: &egui::Context) {
        use std::sync::mpsc::TryRecvError;
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        if self.queue.is_none()
            && let Some(home) = self.root.as_ref().and_then(|root| root.parent())
        {
            let ctx = ctx.clone();
            self.queue = Some(crate::queue_watch::QueueWatch::start(
                home.to_path_buf(),
                move || ctx.request_repaint(),
            ));
        }
        if let Some(task) = &self.action_task {
            match task.try_recv() {
                Ok(result) => {
                    self.action_task = None;
                    match result {
                        Ok(target) => {
                            self.message = "Saved. Refreshing...".into();
                            self.message_error = false;
                            if let Some(target) = target {
                                self.generation += 1;
                                self.requested = Some(target);
                                self.dirty = false;
                                self.scope_dirty = false;
                            }
                            self.refresh_library();
                        }
                        Err(e) => {
                            self.message = e;
                            self.message_error = true;
                        }
                    }
                }
                Err(TryRecvError::Disconnected) => {
                    self.action_task = None;
                    self.message =
                        "Background operation failed; reload to inspect committed state.".into();
                    self.message_error = true;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        if let Some((generation, task)) = &self.load_task {
            let generation = *generation;
            match task.try_recv() {
                Ok(result) => {
                    self.load_task = None;
                    if generation == self.generation {
                        match result {
                            Ok(v) => {
                                self.project_id = v.project;
                                self.node = v.id;
                                self.body = v.body;
                                self.evidence = v.evidence;
                                self.loaded_hash = v.draft;
                                self.completion_hash = v.completion;
                                self.complete = v.complete;
                                self.binary = v.binary;
                                self.scope = v.scope;
                                self.scope_revision = v.revision;
                                self.dependency_rvas = v.dependencies;
                                self.status = v.status;
                                self.blockers = v.blockers;
                                self.dirty = false;
                                self.scope_dirty = false;
                                self.confirm_complete = false;
                                self.message.clear();
                            }
                            Err(e) => {
                                self.message = e;
                                self.message_error = true;
                            }
                        }
                    }
                }
                Err(TryRecvError::Disconnected) => {
                    self.load_task = None;
                    self.message = "Report loader failed".into();
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        if self.load_task.is_none()
            && let Some((project, id)) = self.requested.take()
        {
            let (send, recv) = std::sync::mpsc::sync_channel(1);
            self.load_task = Some((self.generation, recv));
            std::thread::spawn(move || {
                let loaded = with_lock_wait(std::time::Duration::from_secs(30), || {
                    background::load(&project, id)
                });
                let _ = send.send(loaded);
            });
        }
        let delivered = self
            .snapshot_inbox
            .as_mut()
            .and_then(|inbox| match inbox.take() {
                Ok(v) => v,
                Err(e) => Some(Err(e)),
            });
        if let Some((generation, task)) = &self.refresh_task {
            let generation = *generation;
            let outcome = match delivered {
                Some(value) => Ok(value),
                None => match task.try_recv() {
                    Ok(Err(e)) => Ok(Err(e)),
                    Ok(Ok(())) | Err(TryRecvError::Empty) => Err(TryRecvError::Empty),
                    Err(TryRecvError::Disconnected) => Err(TryRecvError::Disconnected),
                },
            };
            match outcome {
                Ok(Ok(v)) if generation == self.refresh_generation => {
                    self.refresh_task = None;
                    self.fingerprint = v.fingerprint;
                    self.jobs = v.jobs;
                    if let Some(root) = v.root {
                        self.root = Some(root);
                        let retired = (
                            std::mem::replace(&mut self.ui_projects, v.projects),
                            std::mem::replace(&mut self.browser_data, v.browser),
                            std::mem::replace(&mut self.views, v.views),
                        );
                        std::thread::spawn(move || drop(retired));
                        self.changed = true;
                        if let Some(p) = self
                            .ui_projects
                            .iter()
                            .position(|p| p.id == self.project_id)
                        {
                            self.project = p;
                        }
                        if !self.has_unsaved()
                            && self.load_task.is_none()
                            && self.requested.is_none()
                            && let Some(p) = self.ui_projects.get(self.project)
                        {
                            let id = if self.browser_data[self.project].node(p, self.node).is_some()
                            {
                                self.node
                            } else {
                                0
                            };
                            self.load(self.project, id);
                        }
                    }
                }
                Ok(Ok(stale)) => {
                    std::thread::spawn(move || drop(stale));
                    self.refresh_task = None;
                }
                Ok(Err(e)) => {
                    self.refresh_task = None;
                    self.message = e;
                    self.message_error = true;
                }
                Err(TryRecvError::Disconnected) => {
                    self.refresh_task = None;
                    self.message = "Snapshot worker failed".into();
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        if self.refresh_task.is_none()
            && self.last_poll.elapsed() >= std::time::Duration::from_secs(2)
        {
            if self.snapshot_inbox.is_none() {
                match background::SnapshotInbox::new() {
                    Ok(inbox) => self.snapshot_inbox = Some(inbox),
                    Err(e) => {
                        self.message = e;
                        self.message_error = true;
                        return;
                    }
                }
            }
            let owner = self.snapshot_inbox.as_ref().unwrap().sender();
            self.last_poll = std::time::Instant::now();
            let (send, recv) = std::sync::mpsc::sync_channel(1);
            self.refresh_task = Some((self.refresh_generation, recv));
            let fingerprint = self.fingerprint.clone();
            let projects = self.ui_projects.clone();
            let browser = self.browser_data.clone();
            let selected = (self.project_id.clone(), self.node);
            let views = self.views.clone();
            std::thread::spawn(move || {
                let result = background::snapshot(fingerprint, projects, browser, views, selected);
                let posted = owner
                    .try_post(solworker::SWPhase(1), move |slot| *slot = Some(result))
                    .map(|_| ())
                    .map_err(|e| format!("Snapshot delivery rejected: {:?}", e.reason));
                let _ = send.send(posted);
            });
        }
    }
    pub fn refresh_library(&mut self) {
        self.refresh_generation += 1;
        self.fingerprint.clear();
        self.last_poll = std::time::Instant::now() - std::time::Duration::from_secs(3);
    }
    pub fn snapshot(&self) -> Vec<ProjectView> {
        self.views.clone()
    }
    pub fn open_record(&mut self, project_id: &str, id: u64) {
        self.open = true;
        if self.has_unsaved() {
            self.message =
                "Save or discard the current edits before opening another report.".into();
            return;
        }
        if let Some(p) = self.ui_projects.iter().position(|p| p.id == project_id) {
            self.load(p, id);
        }
    }
}

impl ResearchUi {
    pub fn has_unsaved(&self) -> bool {
        self.dirty || self.scope_dirty
    }
    pub fn prevent_close(&mut self) {
        self.open = true;
        self.message = "Save or discard your report edits before closing the app.".into();
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Dependency {
    pub dependent: u64,
    pub prerequisite: u64,
}
#[derive(Serialize)]
struct FunctionTag {
    node_id: u64,
    rva: String,
    report: String,
}
#[derive(Serialize)]
struct DependencyFile {
    schema: u32,
    project: String,
    binary: String,
    function_rva: String,
    requires: Vec<FunctionTag>,
}
#[derive(Serialize)]
struct PrerequisiteFile {
    schema: u32,
    project: String,
    binary: String,
    function_rva: String,
    required_by: Vec<FunctionTag>,
}
impl Library {
    fn validate_dependencies(&self, p: usize, edges: &[Dependency]) -> Result<()> {
        let functions: std::collections::HashSet<_> = self.projects[p]
            .nodes
            .iter()
            .filter(|n| n.kind == Kind::Function)
            .map(|n| n.id)
            .collect();
        let mut pairs = std::collections::HashSet::new();
        for e in edges {
            if e.dependent == e.prerequisite
                || !functions.contains(&e.dependent)
                || !functions.contains(&e.prerequisite)
                || !pairs.insert((e.dependent, e.prerequisite))
            {
                return Err(
                    "Prerequisites must be unique, different functions in this same project."
                        .into(),
                );
            }
        }
        fn visit(
            id: u64,
            edges: &[Dependency],
            active: &mut std::collections::HashSet<u64>,
            done: &mut std::collections::HashSet<u64>,
        ) -> bool {
            if done.contains(&id) {
                return true;
            }
            if !active.insert(id) {
                return false;
            }
            for e in edges.iter().filter(|e| e.dependent == id) {
                if !visit(e.prerequisite, edges, active, done) {
                    return false;
                }
            }
            active.remove(&id);
            done.insert(id);
            true
        }
        let mut done = std::collections::HashSet::new();
        for id in functions {
            if !visit(id, edges, &mut Default::default(), &mut done) {
                return Err("That creates a prerequisite cycle. These tags describe research prerequisites, not every call edge.".into());
            }
        }
        Ok(())
    }
    fn sync_dependency_files(&self, p: usize) -> Result<()> {
        self.sync_dependency_subset(p, None)
    }
    fn sync_dependency_subset(
        &self,
        p: usize,
        only: Option<&std::collections::HashSet<u64>>,
    ) -> Result<()> {
        let project = &self.projects[p];
        let index = catalog_index::Index::new(project);
        let mut incoming: std::collections::HashMap<u64, Vec<u64>> = Default::default();
        for e in &project.dependencies {
            incoming
                .entry(e.prerequisite)
                .or_default()
                .push(e.dependent);
        }
        for node in project
            .nodes
            .iter()
            .filter(|n| n.kind == Kind::Function && only.is_none_or(|ids| ids.contains(&n.id)))
        {
            let tag = |id| {
                let n = index.nodes[&id];
                FunctionTag {
                    node_id: id,
                    rva: format!("0x{:08X}", n.rva.unwrap()),
                    report: layout::link(&index.paths[&node.id], &index.paths[&n.id]),
                }
            };
            let deps = DependencyFile {
                schema: 1,
                project: project.id.clone(),
                binary: project.binary.clone(),
                function_rva: format!("0x{:08X}", node.rva.unwrap()),
                requires: index
                    .requires
                    .get(&node.id)
                    .into_iter()
                    .flatten()
                    .map(|id| tag(*id))
                    .collect(),
            };
            let prereq = PrerequisiteFile {
                schema: 1,
                project: project.id.clone(),
                binary: project.binary.clone(),
                function_rva: format!("0x{:08X}", node.rva.unwrap()),
                required_by: incoming
                    .get(&node.id)
                    .into_iter()
                    .flatten()
                    .map(|id| tag(*id))
                    .collect(),
            };
            let folder = self
                .dir(p)
                .join(&index.paths[&node.id])
                .parent()
                .unwrap()
                .to_owned();
            fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
            for (name, body) in [
                ("dep.toml", toml::to_string_pretty(&deps)),
                ("prereq.toml", toml::to_string_pretty(&prereq)),
            ] {
                let content = format!(
                    "# Managed by Scopework. Edit through the app or research deps set.\n{}",
                    body.map_err(|e| e.to_string())?
                );
                let path = folder.join(name);
                if fs::read_to_string(&path).ok().as_deref() != Some(content.as_str()) {
                    atomic(&path, content.as_bytes())?;
                }
            }
        }
        Ok(())
    }
    fn dependency_stamps(&self, p: usize, id: u64) -> std::collections::BTreeMap<u64, String> {
        self.projects[p]
            .dependencies
            .iter()
            .filter(|e| e.dependent == id)
            .map(|e| {
                let n = self.projects[p]
                    .nodes
                    .iter()
                    .find(|n| n.id == e.prerequisite)
                    .unwrap();
                let content = self
                    .report_rel(p, n.id)
                    .ok()
                    .and_then(|rel| self.text(p, &rel).ok())
                    .unwrap_or_default();
                (n.id, format!("{}:{}", n.revision, hash(&content)))
            })
            .collect()
    }
    pub fn set_dependencies(&mut self, p: usize, id: u64, rvas: &str) -> Result<()> {
        if !self.writable(p) {
            return self.write_project(p, |l, p| l.set_dependencies(p, id, rvas));
        }
        self.ensure_structural(p)?;
        if !self.projects[p]
            .nodes
            .iter()
            .any(|n| n.id == id && n.kind == Kind::Function)
        {
            return Err("Select a function report.".into());
        }
        let mut edges: Vec<_> = self.projects[p]
            .dependencies
            .iter()
            .filter(|e| e.dependent != id)
            .cloned()
            .collect();
        for rva in rvas
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|s| !s.is_empty())
        {
            let address =
                u32::from_str_radix(rva.trim_start_matches("0x").trim_start_matches("0X"), 16)
                    .map_err(|_| format!("Invalid hexadecimal RVA: {rva}"))?;
            let n = self.projects[p]
                .nodes
                .iter()
                .find(|n| n.rva == Some(address))
                .ok_or_else(|| format!("RVA {rva} has no function report in this project."))?;
            edges.push(Dependency {
                dependent: id,
                prerequisite: n.id,
            });
        }
        edges.sort_by_key(|e| (e.dependent, e.prerequisite));
        self.validate_dependencies(p, &edges)?;
        let mut previous = self.projects[p].dependencies.clone();
        previous.sort_by_key(|e| (e.dependent, e.prerequisite));
        if edges == previous {
            return Ok(());
        }
        let affected: std::collections::HashSet<_> = previous
            .iter()
            .chain(&edges)
            .filter(|e| e.dependent == id)
            .flat_map(|e| [e.dependent, e.prerequisite])
            .chain([id])
            .collect();
        let before = self.projects[p].clone();
        self.projects[p].structural_epoch += 1;
        self.projects[p].dependencies = edges;
        let n = self.projects[p]
            .nodes
            .iter_mut()
            .find(|n| n.id == id)
            .unwrap();
        if n.status == Status::Complete {
            n.status = Status::InProgress;
        }
        n.accepted_hash = None;
        n.accepted_prerequisites.clear();
        n.revision += 1;
        self.reopen_affected(p, id);
        self.projects[p].revision += 1;
        let after = std::mem::replace(&mut self.projects[p], before.clone());
        self.commit_metadata(p, before, after, vec![], affected.into_iter().collect())
    }
    pub fn set_dependencies_by_rva(
        &mut self,
        project: &str,
        function: &str,
        dependencies: &str,
    ) -> Result<()> {
        let p = self
            .projects
            .iter()
            .position(|p| p.id == project)
            .ok_or("Unknown project ID")?;
        let rva = u32::from_str_radix(
            function.trim_start_matches("0x").trim_start_matches("0X"),
            16,
        )
        .map_err(|_| "Invalid function RVA")?;
        let id = self.projects[p]
            .nodes
            .iter()
            .find(|n| n.rva == Some(rva))
            .ok_or("Unknown function RVA in this project")?
            .id;
        self.set_dependencies(p, id, dependencies)
    }
}

impl Library {
    fn views(&self) -> Vec<ProjectView> {
        let lib = self;
        lib.projects
            .iter()
            .enumerate()
            .map(|(p, project)| {
                let index = catalog_index::Index::new(project);
                let states_all = index.states(&lib.dir(p));
                let mut counts = std::collections::HashMap::<u64, (usize, usize, usize)>::new();
                let mut inventory_functions = 0;
                let mut required_functions = 0;
                for n in &project.nodes {
                    if n.kind == Kind::Function {
                        inventory_functions += 1;
                        if index.required.contains(&n.id) {
                            required_functions += 1;
                            if let Some(arch) = n.parent.and_then(|id| index.nodes[&id].parent) {
                                let count = counts.entry(arch).or_default();
                                count.0 += 1;
                                count.1 += usize::from(states_all[&n.id] == Status::Complete);
                                count.2 += usize::from(states_all[&n.id] == Status::InProgress);
                            }
                        }
                    }
                }
                let meters = project
                    .nodes
                    .iter()
                    .filter(|n| n.kind == Kind::Architecture)
                    .map(|a| {
                        let (functions, completed_functions, in_progress_functions) =
                            counts.get(&a.id).copied().unwrap_or_default();
                        let states = index
                            .children
                            .get(&a.id)
                            .into_iter()
                            .flatten()
                            .filter(|id| index.required.contains(id))
                            .map(|id| match states_all[id] {
                                Status::Complete => 0,
                                Status::InProgress => 1,
                                Status::NotStarted => 2,
                            })
                            .collect();
                        Meter {
                            id: a.id,
                            name: if index.required.contains(&a.id) {
                                a.name.clone()
                            } else {
                                format!("{} [context]", a.name)
                            },
                            functions,
                            completed_functions,
                            in_progress_functions,
                            states,
                        }
                    })
                    .collect();
                ProjectView {
                    inventory_functions,
                    required_functions,
                    app_status: states_all[&0],
                    id: project.id.clone(),
                    name: project.name.clone(),
                    meters,
                    research_scope: project.research_scope.clone(),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod background_ui_tests {
    use super::*;
    fn isolated_ui() -> ResearchUi {
        let mut ui = ResearchUi::new();
        ui.root = Some(PathBuf::from("C:/unused-background-ui-fixture"));
        ui.last_poll = std::time::Instant::now();
        ui
    }
    fn loaded(project: &str, id: u64) -> background::Loaded {
        background::Loaded {
            project: project.into(),
            id,
            body: "loaded".into(),
            evidence: String::new(),
            draft: "d4:test".into(),
            completion: "c4:test".into(),
            complete: false,
            binary: "fixture".into(),
            scope: String::new(),
            revision: 1,
            dependencies: String::new(),
            status: Status::NotStarted,
            blockers: vec![],
        }
    }
    fn fixture_project(id: &str) -> std::sync::Arc<Project> {
        std::sync::Arc::new(Project {
            layout_version: 5,
            id: id.into(),
            name: id.into(),
            binary: "synthetic".into(),
            research_scope: String::new(),
            next_id: 1,
            revision: 1,
            structural_epoch: 0,
            active_import: None,
            last_import: None,
            nodes: vec![Node {
                context_only: false,
                id: 0,
                parent: None,
                kind: Kind::App,
                name: "App".into(),
                status: Status::NotStarted,
                rva: None,
                revision: 0,
                accepted_hash: None,
                accepted_prerequisites: Default::default(),
            }],
            dependencies: vec![],
        })
    }
    #[test]
    fn the_records_window_follows_the_dashboard_project_unless_edits_are_unsaved() {
        let mut ui = isolated_ui();
        ui.ui_projects = vec![fixture_project("alpha"), fixture_project("beta")];
        ui.project_id = "alpha".into();
        ui.follow_project("beta");
        assert_eq!(ui.requested, Some(("beta".to_string(), 0)));
        let mut ui = isolated_ui();
        ui.ui_projects = vec![fixture_project("alpha"), fixture_project("beta")];
        ui.project_id = "alpha".into();
        ui.dirty = true;
        ui.open = true;
        ui.follow_project("beta");
        assert!(ui.requested.is_none(), "unsaved edits keep their project");
        assert!(ui.message.contains("Save or discard"));
    }
    #[test]
    fn the_records_window_renders_its_empty_state_and_dialogs_headless() {
        let mut ui = isolated_ui();
        ui.open = true;
        ui.new_project();
        let ctx = egui::Context::default();
        for _ in 0..2 {
            let _ = ctx.run(Default::default(), |ctx| ui.show(ctx));
        }
        assert!(ui.new_project_open, "the dialog stays open until closed");
        assert!(ui.open);
    }
    #[test]
    fn one_solworker_runtime_serves_every_class_and_thread() {
        let here = background::shared().unwrap() as *const _ as usize;
        let there = std::thread::spawn(|| background::shared().unwrap() as *const _ as usize)
            .join()
            .unwrap();
        assert_eq!(here, there, "one runtime per process");
        for class in [
            solworker::SWExecutionClass::Low,
            solworker::SWExecutionClass::Mid,
            solworker::SWExecutionClass::High,
        ] {
            let task = background::spawn(class, move || 21 * 2).unwrap();
            assert_eq!(background::join(task).unwrap(), 42);
        }
    }
    #[test]
    fn obsolete_report_load_cannot_replace_current_selection_or_draft() {
        let mut ui = isolated_ui();
        ui.generation = 2;
        ui.project_id = "newer".into();
        ui.body = "unsaved prose".into();
        ui.dirty = true;
        let (send, recv) = std::sync::mpsc::sync_channel(1);
        send.send(Ok(loaded("older", 42))).unwrap();
        ui.load_task = Some((1, recv));
        ui.poll(&egui::Context::default());
        assert_eq!(ui.project_id, "newer");
        assert_eq!(ui.body, "unsaved prose");
        assert!(ui.dirty);
        assert!(ui.load_task.is_none());
    }
    #[test]
    fn newer_meters_publish_without_discarding_unsaved_text() {
        let mut ui = isolated_ui();
        ui.body = "unsaved prose".into();
        ui.dirty = true;
        let inbox = background::SnapshotInbox::new().unwrap();
        let value = background::Snapshot {
            root: Some(ui.root.clone().unwrap()),
            projects: vec![],
            browser: vec![],
            views: vec![ProjectView {
                inventory_functions: 12,
                required_functions: 10,
                app_status: Status::InProgress,
                id: "fixture".into(),
                name: "Fixture".into(),
                meters: vec![],
                research_scope: "Scope".into(),
            }],
            jobs: Default::default(),
            fingerprint: "new".into(),
        };
        assert!(
            inbox
                .sender()
                .try_post(solworker::SWPhase(1), move |slot| *slot = Some(Ok(value)))
                .is_ok()
        );
        let (_send, recv) = std::sync::mpsc::sync_channel(1);
        ui.refresh_task = Some((0, recv));
        ui.snapshot_inbox = Some(inbox);
        ui.poll(&egui::Context::default());
        assert_eq!(ui.views[0].inventory_functions, 12);
        assert_eq!(ui.body, "unsaved prose");
        assert!(ui.dirty);
        assert!(ui.requested.is_none());
    }
    #[test]
    fn obsolete_snapshot_is_rejected_and_owner_shutdown_rejects_late_delivery() {
        let mut ui = isolated_ui();
        ui.refresh_generation = 2;
        ui.fingerprint = "current".into();
        let inbox = background::SnapshotInbox::new().unwrap();
        let sender = inbox.sender();
        let value = background::Snapshot {
            root: None,
            projects: vec![],
            browser: vec![],
            views: vec![],
            jobs: Default::default(),
            fingerprint: "obsolete".into(),
        };
        assert!(
            sender
                .try_post(solworker::SWPhase(1), move |slot| *slot = Some(Ok(value)))
                .is_ok()
        );
        // Capacity is one; providers cannot accumulate snapshot generations.
        assert!(sender.try_post(solworker::SWPhase(1), |_| {}).is_err());
        let (_send, recv) = std::sync::mpsc::sync_channel(1);
        ui.refresh_task = Some((1, recv));
        ui.snapshot_inbox = Some(inbox);
        ui.poll(&egui::Context::default());
        assert_eq!(ui.fingerprint, "current");
        drop(ui);
        assert!(sender.try_post(solworker::SWPhase(1), |_| {}).is_err());
    }
}
