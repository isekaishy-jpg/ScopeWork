//! The coordinator: one process per research home that answers reads from immutable
//! snapshots, queues writes per project by priority class, and merges queued record
//! operations into shared commits. See docs/proposal-coordinator.md and
//! docs/design-coordinator.md.
//!
//! It keeps no state that is not on disk. Each project's writer takes the project's file lock
//! only to commit, so direct-mode commands and older Scopework versions stay correct beside
//! it; their commits are picked up before the next read or commit.
pub mod audit;
pub mod client;
mod drift;
pub mod job;
mod leases;
pub mod manifest;
pub mod pipe;

use crate::cli::{self, Route, WriteRoute};
use crate::research::{self, Library, Overlay, Project};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock, Weak};
use std::time::{Duration, Instant, SystemTime};

/// Protocol number in the handshake; both sides must match.
pub const PROTOCOL: u32 = 2;
/// The product version, compared to retire an older coordinator.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// How long a coordinator with no clients and no queued work stays up.
const IDLE: Duration = Duration::from_secs(600);
/// Waiting this long raises an operation one priority class.
const AGEING: Duration = Duration::from_secs(5);
/// The most operations, and report bytes, one merged commit takes.
const MERGE_OPERATIONS: usize = 256;
const MERGE_BYTES: usize = 8 << 20;

/// Priority classes, most urgent first.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Class {
    Interactive,
    Normal,
    Bulk,
    Job,
}
impl Class {
    fn rank(self) -> u64 {
        self as u64
    }
    fn name(self) -> &'static str {
        match self {
            Class::Interactive => "interactive",
            Class::Normal => "normal",
            Class::Bulk => "bulk",
            Class::Job => "job",
        }
    }
}

/// The answer to one request, as the CLI would print it.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Answer {
    pub exit: i32,
    pub stdout: Option<Value>,
    pub stderr: Vec<Value>,
}
impl Answer {
    fn of(result: Result<Value, String>) -> Self {
        match result {
            Ok(value) => Self {
                exit: 0,
                stdout: Some(value),
                stderr: vec![],
            },
            Err(error) => Self {
                exit: 1,
                stdout: None,
                stderr: vec![json!({"error": error, "kind": cli::error_kind(&error)})],
            },
        }
    }
}

/// A queued write.
struct Operation {
    id: u64,
    arguments: Vec<String>,
    route: WriteRoute,
    class: Class,
    submitter: String,
    queued: Instant,
    reply: std::sync::mpsc::Sender<Answer>,
}
impl Operation {
    fn effective_rank(&self, now: Instant) -> u64 {
        let aged = now.duration_since(self.queued).as_secs() / AGEING.as_secs();
        self.class.rank().saturating_sub(aged)
    }
    fn view(&self, state: &str, position: usize, now: Instant) -> Value {
        json!({"operation": self.id, "kind": self.route.kind, "class": self.class, "state": state,
            "position": position, "submitter": self.submitter, "nodes": self.route.nodes,
            "affects": self.route.affects, "waited_ms": now.duration_since(self.queued).as_millis() as u64})
    }
}

/// A committed revision of a project, as readers see it.
struct Snapshot {
    project: Project,
    /// Size and modification time of `project.json` when this snapshot was taken.
    stamp: Option<(u64, SystemTime)>,
    /// Texts that commits after this snapshot replaced (see `operations::with_commit_observer`).
    overlay: Overlay,
}

#[derive(Default)]
struct Queue {
    waiting: Vec<Operation>,
    running: Vec<Value>,
}
/// Read counts per record class since the last flush to the tooling log.
#[derive(Default)]
struct ReadLog {
    /// Class -> (reads, reads that carried a pending entry).
    classes: std::collections::BTreeMap<&'static str, (u64, u64)>,
    /// The UTC day these counts belong to.
    day: u64,
}
impl ReadLog {
    fn count(&mut self, class: &'static str, pending: bool) {
        let entry = self.classes.entry(class).or_default();
        entry.0 += 1;
        entry.1 += u64::from(pending);
    }
    fn totals(&self) -> (u64, u64) {
        self.classes
            .values()
            .fold((0, 0), |(r, p), (reads, pending)| (r + reads, p + pending))
    }
    fn view(&self) -> Value {
        Value::Object(
            self.classes
                .iter()
                .map(|(class, (reads, pending))| {
                    (
                        class.to_string(),
                        json!({"reads": reads, "with_pending": pending}),
                    )
                })
                .collect(),
        )
    }
}
/// Commit measurements since the last flush, for the phase 3 gates (batch sizes, fixed commit
/// cost by stage, queue wait by class).
#[derive(Default)]
struct CommitLog {
    sizes: Vec<u64>,
    reused: u64,
    open_us: Vec<u64>,
    delta_us: Vec<u64>,
    journal_us: Vec<u64>,
    apply_us: Vec<u64>,
    wait_ms: std::collections::BTreeMap<&'static str, Vec<u64>>,
}
impl CommitLog {
    fn view(&self) -> Value {
        let waits: serde_json::Map<String, Value> = self
            .wait_ms
            .iter()
            .map(|(class, waits)| (class.to_string(), spread(waits)))
            .collect();
        json!({"batches": self.sizes.len(), "resident_reused": self.reused,
            "operations_per_batch": spread(&self.sizes), "open_us": spread(&self.open_us),
            "delta_us": spread(&self.delta_us), "journal_us": spread(&self.journal_us),
            "apply_us": spread(&self.apply_us), "wait_ms": waits})
    }
}
/// Count, median and p90 of a set of measurements.
fn spread(values: &[u64]) -> Value {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let at = |q: usize| {
        sorted
            .get((sorted.len() * q / 100).min(sorted.len().saturating_sub(1)))
            .copied()
    };
    json!({"count": sorted.len(), "median": at(50), "p90": at(90)})
}
fn micros(time: Duration) -> u64 {
    u64::try_from(time.as_micros()).unwrap_or(u64::MAX)
}
fn today() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() / 86_400)
}
/// The record class a read concerns: its first node's kind, or the project as a whole.
fn read_class(project: &Project, nodes: Option<&[u64]>) -> &'static str {
    let kind = nodes
        .and_then(|nodes| nodes.first())
        .and_then(|id| project.nodes.iter().find(|n| n.id == *id))
        .map(|n| n.kind);
    match kind {
        Some(research::Kind::App) => "app",
        Some(research::Kind::Architecture) => "architecture",
        Some(research::Kind::Chunk) => "chunk",
        Some(research::Kind::Function) => "function",
        None => "project",
    }
}

/// One open project: its current snapshot, queue and writer.
struct ProjectState {
    id: String,
    snapshot: RwLock<Arc<Snapshot>>,
    /// Older snapshots still held by readers; commits record replaced texts in them too.
    live: Mutex<Vec<Weak<Snapshot>>>,
    queue: Mutex<Queue>,
    wake: Condvar,
    /// True while this coordinator's writer holds the project lock (see `Writing`).
    writing: AtomicBool,
    /// Changes by other processes noticed so far. Their file writes bypass the overlays, so a
    /// read that overlaps one is answered again under the lock.
    external: AtomicU64,
    reads: Mutex<ReadLog>,
    /// The writer's catalog kept between batches (see `Resident`).
    resident: Mutex<Option<Resident>>,
    commits: Mutex<CommitLog>,
    /// Leases on the project's tool resources (phase 2).
    leases: Mutex<leases::Leases>,
    /// Drift entries recorded per resource since this coordinator started.
    drift: Mutex<std::collections::BTreeMap<String, u64>>,
}

/// How long the writer may reuse its in-memory catalog before re-opening it from disk.
const RESIDENT_FOR: Duration = Duration::from_secs(60);
/// Within this time of a `project.json` modification, its stamp is not trusted alone.
const RACY: Duration = Duration::from_secs(2);
/// The writer's library between batches, without its locks, and the `project.json` stamp it
/// last wrote or read. Reused only while that stamp still holds under a re-taken lock.
struct Resident {
    lib: Library,
    stamp: Option<(u64, SystemTime)>,
    opened: Instant,
}
impl Resident {
    /// Re-takes the lock and returns the library if the catalog on disk is still the one held.
    fn resume(mut self, root: &Path) -> Option<Library> {
        if self.opened.elapsed() >= RESIDENT_FOR {
            return None;
        }
        research::with_lock_wait(Duration::from_secs(600), || self.lib.relock()).ok()?;
        let id = self.lib.projects[0].id.clone();
        let path = root.join(&id).join("project.json");
        let now = stamp(&path);
        if pending_journal(root, &id) || now != self.stamp {
            return None;
        }
        // A modification time this recent could hide a same-size rewrite: compare the bytes.
        let racy =
            now.is_some_and(|(_, modified)| modified.elapsed().map_or(true, |age| age < RACY));
        if racy && std::fs::read(&path).ok() != research::catalog_bytes(&self.lib.projects[0]).ok()
        {
            return None;
        }
        Some(self.lib)
    }
}

struct Server {
    root: PathBuf,
    started: Instant,
    projects: Mutex<HashMap<String, Arc<ProjectState>>>,
    admitting: AtomicBool,
    connections: AtomicUsize,
    last_activity: Mutex<Instant>,
    next_operation: AtomicU64,
}
static SERVER: std::sync::OnceLock<Arc<Server>> = std::sync::OnceLock::new();

fn stamp(path: &Path) -> Option<(u64, SystemTime)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()?))
}
fn new_overlay() -> Overlay {
    Arc::new(RwLock::new(HashMap::new()))
}
fn pending_journal(root: &Path, project: &str) -> bool {
    root.join(project)
        .join("tools/workspaces/catalog-transactions-v4/pending")
        .exists()
}

impl Server {
    fn touch(&self) {
        if let Ok(mut last) = self.last_activity.lock() {
            *last = Instant::now();
        }
    }
    /// The project's state, loading its first snapshot and starting its writer on first use.
    fn project(self: &Arc<Self>, id: &str) -> Result<Arc<ProjectState>, String> {
        let mut projects = self
            .projects
            .lock()
            .map_err(|_| "Coordinator state poisoned")?;
        if let Some(state) = projects.get(id) {
            return Ok(state.clone());
        }
        let snapshot = self.load(id)?;
        let state = Arc::new(ProjectState {
            id: id.to_string(),
            snapshot: RwLock::new(snapshot),
            live: Mutex::new(vec![]),
            queue: Mutex::new(Queue::default()),
            wake: Condvar::new(),
            writing: AtomicBool::new(false),
            external: AtomicU64::new(0),
            resident: Mutex::new(None),
            commits: Mutex::new(CommitLog::default()),
            leases: Mutex::new(leases::Leases::default()),
            drift: Mutex::new(Default::default()),
            reads: Mutex::new(ReadLog {
                day: today(),
                ..ReadLog::default()
            }),
        });
        projects.insert(id.to_string(), state.clone());
        let server = self.clone();
        let writer = state.clone();
        std::thread::spawn(move || server.write_loop(&writer));
        let server = self.clone();
        let watched = state.clone();
        std::thread::spawn(move || server.drift_loop(&watched));
        Ok(state)
    }
    /// A snapshot from the committed catalog, read the way direct mode reads it (shared lock,
    /// recovery of an interrupted transaction).
    fn load(&self, id: &str) -> Result<Arc<Snapshot>, String> {
        // Stamped before reading: a commit landing in between makes the snapshot look stale,
        // never current.
        let stamp = stamp(&self.root.join(id).join("project.json"));
        let lib = research::with_lock_wait(Duration::from_secs(120), || {
            Library::read_project(self.root.clone(), id)
        })?;
        let project = lib
            .projects
            .first()
            .cloned()
            .ok_or_else(|| format!("Unknown project: {id}"))?;
        Ok(Arc::new(Snapshot {
            project,
            stamp,
            overlay: new_overlay(),
        }))
    }
    fn publish(&self, state: &ProjectState, project: Project) {
        let snapshot = Arc::new(Snapshot {
            stamp: stamp(&self.root.join(&state.id).join("project.json")),
            project,
            overlay: new_overlay(),
        });
        Self::replace(state, snapshot);
    }
    /// Makes `snapshot` current, keeping the previous one reachable for overlay recording
    /// while readers still hold it.
    fn replace(state: &ProjectState, snapshot: Arc<Snapshot>) {
        if let Ok(mut current) = state.snapshot.write() {
            let previous = std::mem::replace(&mut *current, snapshot);
            if let Ok(mut live) = state.live.lock() {
                live.retain(|w| w.strong_count() > 0);
                live.push(Arc::downgrade(&previous));
            }
        }
    }
    /// The snapshot to read from: the current one, refreshed first if another process
    /// (direct mode, an older version) committed since it was taken.
    fn snapshot(&self, state: &ProjectState) -> Result<Arc<Snapshot>, String> {
        let current = state
            .snapshot
            .read()
            .map_err(|_| "Snapshot poisoned")?
            .clone();
        if state.writing.load(Ordering::Acquire)
            || stamp(&self.root.join(&state.id).join("project.json")) == current.stamp
        {
            return Ok(current);
        }
        state.external.fetch_add(1, Ordering::AcqRel);
        let fresh = self.load(&state.id)?;
        Self::replace(state, fresh.clone());
        Ok(fresh)
    }

    fn read(&self, state: &ProjectState, arguments: &[String], nodes: Option<Vec<u64>>) -> Answer {
        let command = match cli::parse(arguments) {
            Ok(args) => args.command,
            Err(e) => return Answer::of(Err(e)),
        };
        let snapshot = match self.snapshot(state) {
            Ok(s) => s,
            Err(e) => return Answer::of(Err(e)),
        };
        // Another process's transaction in progress: its report writes are not recorded in our
        // overlays, so answer the way direct mode does, after it finishes. The same applies when
        // one overlaps this read; that is checked afterwards, so reads never wait for this
        // coordinator's own commits.
        let locked = || -> Result<(Value, u64), String> {
            let mut lib = research::with_lock_wait(Duration::from_secs(120), || {
                Library::read_project(self.root.clone(), &state.id)
            })?;
            let revision = lib.projects.first().map_or(0, |p| p.revision);
            let command = cli::parse(arguments)?.command;
            cli::execute(command, &mut lib).map(|value| (value, revision))
        };
        let epoch = state.external.load(Ordering::Acquire);
        let result = if self.external_activity(state, &snapshot) {
            locked()
        } else {
            let mut lib = Library::snapshot_view(
                self.root.clone(),
                snapshot.project.clone(),
                snapshot.overlay.clone(),
            );
            // Debug builds only: widens the window in which another process's commit overlaps
            // this answer, for the regression test of that case.
            #[cfg(debug_assertions)]
            if let Some(ms) = std::env::var("SCOPEWORK_COORDINATOR_READ_DELAY_MS")
                .ok()
                .and_then(|v| v.parse().ok())
            {
                std::thread::sleep(Duration::from_millis(ms));
            }
            let answer = cli::execute(command, &mut lib);
            let overlapped = state.external.load(Ordering::Acquire) != epoch
                || self.external_activity(state, &snapshot);
            if overlapped {
                locked()
            } else {
                answer.map(|value| (value, snapshot.project.revision))
            }
        };
        let (result, revision) = match result {
            Ok((value, revision)) => (Ok(value), revision),
            Err(e) => (Err(e), snapshot.project.revision),
        };
        let mut answer = Answer::of(result);
        if let Some(stdout) = answer.stdout.as_mut() {
            let pending = self.pending(state, nodes.as_deref());
            if let Ok(mut log) = state.reads.lock() {
                log.count(
                    read_class(&snapshot.project, nodes.as_deref()),
                    !pending.is_empty(),
                );
            }
            // Array answers keep their shape; objects carry the revision and pending work.
            if stdout.is_object() {
                stdout["revision"] = json!(revision);
                stdout["pending"] = Value::Array(pending);
            }
        }
        answer
    }
    /// Whether another process is changing the project's files right now: its catalog
    /// transaction or import publication is in flight, or it committed a catalog this
    /// coordinator has not published. Never true for this coordinator's own commits.
    fn external_activity(&self, state: &ProjectState, snapshot: &Snapshot) -> bool {
        if state.writing.load(Ordering::Acquire) {
            return false;
        }
        let dir = self.root.join(&state.id);
        let importing = snapshot
            .project
            .active_import
            .iter()
            .chain(snapshot.project.last_import.iter())
            .any(|job| {
                dir.join("tools/workspaces/import-jobs")
                    .join(job)
                    .join("publication/pending.json")
                    .is_file()
            });
        let published = state.snapshot.read().ok().map(|s| s.stamp);
        pending_journal(&self.root, &state.id)
            || importing
            || published.is_some_and(|published| stamp(&dir.join("project.json")) != published)
    }
    /// Queued and running operations that concern `nodes` (all of them when `None`).
    fn pending(&self, state: &ProjectState, nodes: Option<&[u64]>) -> Vec<Value> {
        let Ok(queue) = state.queue.lock() else {
            return vec![];
        };
        let now = Instant::now();
        let concerns = |v: &Value| {
            nodes.is_none_or(|nodes| {
                v["nodes"].as_array().is_some_and(|ns| {
                    ns.iter()
                        .any(|n| n.as_u64().is_some_and(|n| nodes.contains(&n)))
                })
            })
        };
        let mut out: Vec<Value> = queue
            .running
            .iter()
            .filter(|v| concerns(v))
            .cloned()
            .collect();
        out.extend(
            ordered(&queue.waiting, now)
                .into_iter()
                .enumerate()
                .map(|(i, op)| op.view("queued", i + 1, now))
                .filter(|v| concerns(v)),
        );
        out
    }
    fn queue_view(&self, state: &ProjectState) -> Value {
        let now = Instant::now();
        let queue = state.queue.lock();
        let (running, waiting) = match &queue {
            Ok(q) => (
                q.running.clone(),
                ordered(&q.waiting, now)
                    .into_iter()
                    .enumerate()
                    .map(|(i, op)| op.view("queued", i + 1, now))
                    .collect::<Vec<_>>(),
            ),
            Err(_) => (vec![], vec![]),
        };
        let leases = state
            .leases
            .lock()
            .map(|l| l.view())
            .unwrap_or_else(|_| json!({"held": [], "waiting": []}));
        json!({"project": state.id, "source": "coordinator", "running": running, "waiting": waiting, "leases": leases})
    }

    fn enqueue(
        &self,
        state: &ProjectState,
        arguments: Vec<String>,
        route: WriteRoute,
        class: Class,
        submitter: String,
    ) -> std::sync::mpsc::Receiver<Answer> {
        let (reply, answer) = std::sync::mpsc::channel();
        let op = Operation {
            id: self.next_operation.fetch_add(1, Ordering::Relaxed),
            arguments,
            route,
            class,
            submitter,
            queued: Instant::now(),
            reply,
        };
        if let Ok(mut queue) = state.queue.lock() {
            queue.waiting.push(op);
        }
        state.wake.notify_all();
        answer
    }

    /// One project's writer: takes the most urgent operation, plus every queued record
    /// operation that may join it, and commits them together.
    fn write_loop(self: Arc<Self>, state: &Arc<ProjectState>) {
        // This thread lives as long as the coordinator and evaluates completion on every commit.
        research::retain_report_hashes_on_this_thread();
        loop {
            let batch = {
                let Ok(mut queue) = state.queue.lock() else {
                    return;
                };
                while queue.waiting.is_empty() {
                    queue = match state.wake.wait_timeout(queue, Duration::from_secs(5)) {
                        Ok((q, _)) => q,
                        Err(_) => return,
                    };
                    // Idle: a new day's first idle moment writes yesterday's counts.
                    if queue.waiting.is_empty()
                        && state.reads.lock().is_ok_and(|log| log.day != today())
                    {
                        drop(queue);
                        self.flush_logs(state);
                        queue = match state.queue.lock() {
                            Ok(q) => q,
                            Err(_) => return,
                        };
                    }
                }
                let batch = select(&mut queue.waiting, Instant::now());
                let now = Instant::now();
                queue.running = batch
                    .iter()
                    .enumerate()
                    .map(|(i, op)| op.view("running", i + 1, now))
                    .collect();
                batch
            };
            self.commit_batch(state, batch);
            if let Ok(mut queue) = state.queue.lock() {
                queue.running.clear();
            }
            self.touch();
        }
    }

    /// Appends the read counts and commit measurements gathered so far to the project's
    /// tooling log (`coordinator-reads`, `coordinator-commits`), then starts afresh.
    fn flush_logs(&self, state: &ProjectState) {
        let Some((day, classes, (reads, with_pending))) = state.reads.lock().ok().map(|mut log| {
            let taken = std::mem::take(&mut log.classes);
            let day = std::mem::replace(&mut log.day, today());
            let view = ReadLog {
                classes: taken,
                day,
            };
            (day, view.view(), view.totals())
        }) else {
            return;
        };
        let commits = state
            .commits
            .lock()
            .map(|mut log| std::mem::take(&mut *log))
            .unwrap_or_default();
        let leases = state
            .leases
            .lock()
            .map(|mut l| std::mem::take(&mut l.stats))
            .unwrap_or_default();
        if reads == 0 && commits.sizes.is_empty() && leases.is_empty() {
            return;
        }
        let _ = research::with_lock_wait(Duration::from_secs(10), || {
            Library::open_project(self.root.clone(), Some(&state.id))
        })
        .and_then(|mut lib| {
            if reads > 0 {
                let summary = format!(
                    "{reads} reads through the coordinator, {with_pending} with pending writes (day {day})"
                );
                let diff = json!({"day_unix": day * 86_400, "reads_by_class": classes});
                lib.append_history(0, "coordinator-reads", &summary, &[], &[], Some(diff))?;
            }
            if !commits.sizes.is_empty() {
                let summary = format!(
                    "{} commits of {} operations through the coordinator (day {day})",
                    commits.sizes.len(),
                    commits.sizes.iter().sum::<u64>()
                );
                let diff = json!({"day_unix": day * 86_400, "commits": commits.view()});
                lib.append_history(0, "coordinator-commits", &summary, &[], &[], Some(diff))?;
            }
            if !leases.is_empty() {
                let summary = format!("Tool leases through the coordinator (day {day})");
                let diff = json!({"day_unix": day * 86_400, "kinds": leases.view()});
                lib.append_history(0, "coordinator-leases", &summary, &[], &[], Some(diff))?;
            }
            Ok(())
        });
    }
    /// Polls the project's shared files every 2 s and records changes made outside their
    /// owners as `drift` history entries.
    fn drift_loop(&self, state: &ProjectState) {
        let dir = self.root.join(&state.id);
        let mut watch = drift::Watch::default();
        loop {
            let found = watch.check(&dir, &|resource, since| {
                state
                    .leases
                    .lock()
                    .is_ok_and(|l| l.excused(resource, since))
            });
            for change in found {
                if let Ok(mut counts) = state.drift.lock() {
                    *counts.entry(change.resource.clone()).or_insert(0) += 1;
                }
                let owner = if change.resource == "catalog" {
                    "a catalog transaction".to_string()
                } else {
                    format!("a {} lease", change.resource)
                };
                let summary = format!("{} changed without {owner}", change.file);
                let diff = json!({"resource": change.resource, "file": change.file,
                    "before_sha256": change.before, "after_sha256": change.after});
                let _ = research::with_lock_wait(Duration::from_secs(10), || {
                    Library::open_project(self.root.clone(), Some(&state.id))
                })
                .and_then(|mut lib| {
                    lib.append_history(
                        0,
                        "drift",
                        &summary,
                        &[],
                        std::slice::from_ref(&change.file),
                        Some(diff),
                    )
                });
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    }
    fn commit_batch(&self, state: &Arc<ProjectState>, batch: Vec<Operation>) {
        if let Ok(mut log) = state.commits.lock() {
            log.sizes.push(batch.len() as u64);
            for op in &batch {
                let waited = u64::try_from(op.queued.elapsed().as_millis()).unwrap_or(u64::MAX);
                log.wait_ms.entry(op.class.name()).or_default().push(waited);
            }
        }
        let answers = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.apply(state, &batch)
        }))
        .unwrap_or_else(|_| {
            // The library may hold anything now: the next batch re-opens from disk, and
            // readers get what is committed.
            if let Ok(mut resident) = state.resident.lock() {
                *resident = None;
            }
            if let Some(committed) = self.committed_catalog(&state.id) {
                self.publish(state, committed);
            }
            let error = "writer_failed: the coordinator's writer failed while applying this operation; it may or may not have been applied. Check the record before retrying.";
            batch.iter().map(|_| Answer::of(Err(error.to_string()))).collect()
        });
        for (op, answer) in batch.into_iter().zip(answers) {
            let _ = op.reply.send(answer);
        }
    }

    fn apply(&self, state: &Arc<ProjectState>, batch: &[Operation]) -> Vec<Answer> {
        let fail_all = |error: String| -> Vec<Answer> {
            batch
                .iter()
                .map(|_| Answer::of(Err(error.clone())))
                .collect()
        };
        let opening = Instant::now();
        let resident = state.resident.lock().ok().and_then(|mut r| r.take());
        let mut reused = false;
        let (mut lib, opened) = match resident.and_then(|r| {
            let opened = r.opened;
            r.resume(&self.root).map(|lib| (lib, opened))
        }) {
            Some(resumed) => {
                reused = true;
                resumed
            }
            None => match research::with_lock_wait(Duration::from_secs(600), || {
                Library::open_project(self.root.clone(), Some(&state.id))
            }) {
                Ok(lib) if !lib.projects.is_empty() => (lib, Instant::now()),
                Ok(_) => return fail_all(format!("Unknown project: {}", state.id)),
                Err(e) => return fail_all(e),
            },
        };
        let open_time = opening.elapsed();
        // Declared after `lib`, so it is dropped first: the flag is down before the lock goes.
        let writing = Writing::start(&state.writing);
        #[cfg(debug_assertions)]
        if std::env::var("SCOPEWORK_COORDINATOR_FAIL_AT").as_deref() == Ok("apply") {
            panic!("injected writer failure");
        }
        research::operations::take_commit_times();
        // Another process committed since the last snapshot: publish its state first, so
        // readers never see our next commit's texts against an older catalog.
        let unchanged = state
            .snapshot
            .read()
            .map(|s| s.project == lib.projects[0])
            .unwrap_or(false);
        if !unchanged {
            state.external.fetch_add(1, Ordering::AcqRel);
            self.publish(state, lib.projects[0].clone());
        }
        // Record each file's previous text in every live snapshot before a commit changes it.
        let observed = state.clone();
        let answers = research::operations::with_commit_observer(
            move |previous| record_previous(&observed, previous),
            || {
                if batch.len() == 1 && !batch[0].route.merge {
                    return vec![execute(&batch[0].arguments, &mut lib)];
                }
                lib.begin_staging(0);
                let answers: Vec<Answer> = batch
                    .iter()
                    .map(|op| lib.stage_one(0, |lib| execute(&op.arguments, lib), |a| a.exit != 0))
                    .collect();
                match lib.commit_staged(0) {
                    Ok(_) => answers,
                    Err(e) => {
                        lib.discard_staged(0);
                        answers
                            .into_iter()
                            .map(|a| {
                                if a.exit == 0 {
                                    Answer::of(Err(e.clone()))
                                } else {
                                    a
                                }
                            })
                            .collect()
                    }
                }
            },
        );
        let current = state.snapshot.read().map(|s| s.project.clone()).ok();
        if current.as_ref() != Some(&lib.projects[0]) {
            if answers.iter().all(|a| a.exit == 0) {
                self.publish(state, lib.projects[0].clone());
            } else if let Some(committed) = self.committed_catalog(&state.id)
                && current.as_ref() != Some(&committed)
            {
                // A failure may leave the in-memory catalog ahead of the disk: readers get
                // what was actually committed (the lock is still held, so it is current).
                self.publish(state, committed);
            }
        }
        let [delta, journal, applied] = research::operations::take_commit_times();
        if let Ok(mut log) = state.commits.lock() {
            log.reused += u64::from(reused);
            log.open_us.push(micros(open_time));
            if !applied.is_zero() {
                log.delta_us.push(micros(delta));
                log.journal_us.push(micros(journal));
                log.apply_us.push(micros(applied));
            }
        }
        // Keep the catalog for the next batch unless anything failed: a failure may leave the
        // in-memory catalog ahead of or behind the disk, so the next batch re-opens instead.
        if answers.iter().all(|a| a.exit == 0) {
            let stamp = stamp(&self.root.join(&state.id).join("project.json"));
            drop(writing);
            lib.release_locks();
            if let Ok(mut slot) = state.resident.lock() {
                *slot = Some(Resident { lib, stamp, opened });
            }
        }
        answers
    }
}

/// `ProjectState::writing` for exactly as long as this writer holds the project lock: raised
/// once the lock is taken and lowered before it is released, also when unwinding. Readers skip
/// their checks for outside writers only then, when every file change is this coordinator's
/// and recorded in its overlays; while the writer still waits for the lock, another process may
/// be mid-commit.
struct Writing<'a>(&'a AtomicBool);
impl<'a> Writing<'a> {
    fn start(flag: &'a AtomicBool) -> Self {
        flag.store(true, Ordering::Release);
        Self(flag)
    }
}
impl Drop for Writing<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl Server {
    /// The catalog as committed on disk, or `None` if it cannot be read.
    fn committed_catalog(&self, id: &str) -> Option<Project> {
        let bytes = std::fs::read(self.root.join(id).join("project.json")).ok()?;
        serde_json::from_slice(&bytes).ok()
    }
}

/// Runs one queued command on the writer's library.
fn execute(arguments: &[String], lib: &mut Library) -> Answer {
    Answer::of(cli::parse(arguments).and_then(|args| cli::execute(args.command, lib)))
}

/// Inserts the previous texts a commit is about to replace into every live snapshot's overlay
/// (the current one and older ones still held by readers). The first recorded text wins: it is
/// the one that snapshot was taken with.
fn record_previous(state: &ProjectState, previous: &[(String, Option<String>)]) {
    let mut snapshots: Vec<Arc<Snapshot>> = vec![];
    if let Ok(current) = state.snapshot.read() {
        snapshots.push(current.clone());
    }
    if let Ok(live) = state.live.lock() {
        snapshots.extend(live.iter().filter_map(Weak::upgrade));
    }
    for snapshot in snapshots {
        if let Ok(mut overlay) = snapshot.overlay.write() {
            for (path, text) in previous {
                overlay.entry(path.clone()).or_insert_with(|| text.clone());
            }
        }
    }
}

/// Queue order: most urgent effective class first (with ageing), then arrival.
fn ordered(waiting: &[Operation], now: Instant) -> Vec<&Operation> {
    let mut order: Vec<&Operation> = waiting.iter().collect();
    order.sort_by_key(|op| (op.effective_rank(now), op.id));
    order
}
/// Removes the next commit's operations from the queue: the most urgent operation and, if it
/// is a record operation, every queued record operation in arrival order up to the bounds.
fn select(waiting: &mut Vec<Operation>, now: Instant) -> Vec<Operation> {
    let Some(head) = ordered(waiting, now).first().map(|op| op.id) else {
        return vec![];
    };
    let position = waiting.iter().position(|op| op.id == head).unwrap_or(0);
    let first = waiting.remove(position);
    if !first.route.merge {
        return vec![first];
    }
    let mut bytes = first.route.bytes;
    let mut batch = vec![first];
    waiting.sort_by_key(|op| op.id);
    let mut i = 0;
    while i < waiting.len() && batch.len() < MERGE_OPERATIONS {
        if waiting[i].route.merge && bytes + waiting[i].route.bytes <= MERGE_BYTES {
            bytes += waiting[i].route.bytes;
            batch.push(waiting.remove(i));
        } else {
            i += 1;
        }
    }
    batch.sort_by_key(|op| op.id);
    batch
}

/// The direct-mode queue view: who holds the project lock and who waits for it.
pub fn lock_queue(root: &Path, project: &str) -> Result<Value, String> {
    let locks = research::lock_status(root, project)?;
    // The holder file is advisory: a held lock without one is still a running writer.
    let running = match (locks["writer_active"].as_bool(), &locks["writer"]) {
        (Some(true), Value::Object(_)) => vec![locks["writer"].clone()],
        (Some(true), _) => vec![json!({"command": "another operation"})],
        _ => vec![],
    };
    Ok(
        json!({"project": project, "source": "locks", "running": running, "waiting": locks["waiting"]}),
    )
}

/// Handshake, as exchanged at the start of every connection.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Hello {
    pub protocol: u32,
    pub version: String,
    #[serde(default)]
    pub build: u64,
    pub role: String,
    #[serde(default)]
    pub submitter: String,
}

/// Runs the coordinator for `home` until it is stopped or idle.
pub fn serve(home: &Path) -> Result<Value, String> {
    let name = pipe::pipe_name(home)?;
    let lock_path = home.join(".coordinator.lock");
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|e| e.to_string())?;
    if lock.try_lock().is_err() {
        return Ok(json!({"serving": false, "reason": "another coordinator serves this home"}));
    }
    // The shutdown window must exist before the first pipe instance: a client that can connect may
    // start Restart Manager or a session end immediately, and both need the window to close us gracefully.
    pipe::shutdown_window(
        || {
            if let Some(server) = SERVER.get() {
                server.admitting.store(false, Ordering::Release);
            }
        },
        || {
            if let Some(server) = SERVER.get() {
                server.drain_and_exit();
            }
        },
    );
    let first = match pipe::listen(&name, true) {
        Ok(instance) => instance,
        Err(e) if e.starts_with("pipe_taken") => {
            return Ok(json!({"serving": false, "reason": e}));
        }
        Err(e) => return Err(e),
    };
    let server = Arc::new(Server {
        root: home.join("projects"),
        started: Instant::now(),
        projects: Mutex::new(HashMap::new()),
        admitting: AtomicBool::new(true),
        connections: AtomicUsize::new(0),
        last_activity: Mutex::new(Instant::now()),
        next_operation: AtomicU64::new(1),
    });
    let _ = SERVER.set(server.clone());
    let accepting = server.clone();
    std::thread::spawn(move || {
        let mut instance = Some(first);
        loop {
            let pipe = match instance.take() {
                Some(pipe) => pipe,
                None => match pipe::listen(&name, false) {
                    Ok(pipe) => pipe,
                    Err(_) => {
                        std::thread::sleep(Duration::from_millis(100));
                        continue;
                    }
                },
            };
            if pipe::accept(&pipe).is_err() {
                continue;
            }
            let server = accepting.clone();
            server.connections.fetch_add(1, Ordering::AcqRel);
            std::thread::spawn(move || {
                server.serve_connection(pipe);
                server.connections.fetch_sub(1, Ordering::AcqRel);
            });
        }
    });
    // Idle watch: exit when nothing has happened for a while and nobody is connected.
    loop {
        std::thread::sleep(Duration::from_secs(5));
        let idle = server
            .last_activity
            .lock()
            .map(|last| last.elapsed() >= IDLE)
            .unwrap_or(false);
        if idle && server.connections.load(Ordering::Acquire) == 0 && server.queues_empty() {
            server.drain_and_exit();
        }
    }
}

impl Server {
    fn queues_empty(&self) -> bool {
        self.projects.lock().is_ok_and(|projects| {
            projects.values().all(|state| {
                state
                    .queue
                    .lock()
                    .is_ok_and(|q| q.waiting.is_empty() && q.running.is_empty())
                    && state.leases.lock().is_ok_and(|l| l.is_empty())
            })
        })
    }
    /// Stops admitting work, lets queued work finish, then exits.
    fn drain_and_exit(&self) -> ! {
        self.admitting.store(false, Ordering::Release);
        let deadline = Instant::now() + Duration::from_secs(60);
        while !self.queues_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        let states: Vec<Arc<ProjectState>> = self
            .projects
            .lock()
            .map(|projects| projects.values().cloned().collect())
            .unwrap_or_default();
        for state in states {
            self.flush_logs(&state);
        }
        std::process::exit(0)
    }
    fn status(&self) -> Value {
        let projects: Vec<Value> = self
            .projects
            .lock()
            .map(|projects| {
                projects
                    .values()
                    .map(|state| {
                        let ((reads, with_pending), classes) = state
                            .reads
                            .lock()
                            .map(|r| (r.totals(), r.view()))
                            .unwrap_or_default();
                        let (waiting, running) = state
                            .queue
                            .lock()
                            .map(|q| (q.waiting.len(), q.running.len()))
                            .unwrap_or_default();
                        json!({"project": state.id, "waiting": waiting, "running": running,
                            "reads": reads, "reads_with_pending": with_pending, "reads_by_class": classes,
                            "commits": state.commits.lock().map(|log| log.view()).unwrap_or_default(),
                            "leases": state.leases.lock().map(|l| l.stats.view()).unwrap_or_default(),
                            "drift": state.drift.lock().map(|d| json!(*d)).unwrap_or_default()})
                    })
                    .collect()
            })
            .unwrap_or_default();
        json!({"running": true, "version": VERSION, "protocol": PROTOCOL, "pid": std::process::id(),
            "uptime_s": self.started.elapsed().as_secs(),
            "idle_s": self.last_activity.lock().map_or(0, |last| last.elapsed().as_secs()), "admitting": self.admitting.load(Ordering::Acquire),
            "connections": self.connections.load(Ordering::Acquire), "projects": projects})
    }

    /// Serves one lease request: waits for the grant or for another holder's answer. A granted
    /// connection holds the lease until it sends `release` or closes. Returns false when the
    /// connection has ended.
    fn serve_lease(
        self: &Arc<Self>,
        reader: &mut BufReader<std::fs::File>,
        send: &mut dyn FnMut(Value) -> bool,
        id: Value,
        request: &Value,
        submitter: &str,
    ) -> bool {
        let lease = &request["lease"];
        let refuse = |send: &mut dyn FnMut(Value) -> bool, error: String| {
            send(json!({"id": id, "error": error, "kind": cli::error_kind(&error)}))
        };
        if !self.admitting.load(Ordering::Acquire) {
            return send(json!({"id": id, "unavailable": "stopping"}));
        }
        let project = lease["project"].as_str().unwrap_or_default();
        let state = match self.project(project) {
            Ok(state) => state,
            Err(e) => return refuse(send, e),
        };
        let resources: std::collections::BTreeSet<String> = lease["resources"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| r.as_str().map(String::from))
            .collect();
        if resources.is_empty() || !resources.iter().all(|r| manifest::valid_name(r)) {
            return refuse(
                send,
                "lease_refused: a lease names one or more resources, lowercase words joined by hyphens".into(),
            );
        }
        let payload = lease.get("payload").filter(|p| !p.is_null()).cloned();
        let bytes = payload.as_ref().map_or(0, |p| p.to_string().len());
        if bytes > leases::PAYLOAD_BYTES {
            return refuse(send, "lease_refused: a payload may not exceed 1 MB".into());
        }
        let class = match lease["class"].as_str() {
            Some("bulk") => Class::Bulk,
            Some("job") => Class::Job,
            _ => Class::Normal,
        };
        let kind = lease["kind"].as_str().unwrap_or("lease").to_string();
        let (reply, events) = std::sync::mpsc::channel();
        let request_id = self.next_operation.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut leases) = state.leases.lock() {
            leases.enqueue(leases::Request {
                id: request_id,
                kind,
                resources,
                key: lease["key"].as_str().map(String::from),
                class,
                submitter: submitter.to_string(),
                payload,
                bytes,
                queued: Instant::now(),
                reply,
            });
        }
        let granted = match events.recv() {
            Err(_) => return refuse(send, "The lease queue failed".into()),
            Ok(leases::Event::Done { by, result }) => {
                return send(json!({"id": id, "done": {"by": by, "result": result}}));
            }
            Ok(leases::Event::Covered { by, result }) => {
                return send(json!({"id": id, "covered": {"by": by, "result": result}}));
            }
            Ok(leases::Event::Granted { lease, payloads }) => {
                if !send(json!({"id": id, "granted": {"lease": lease, "payloads": payloads}})) {
                    if let Ok(mut leases) = state.leases.lock() {
                        leases.release(lease);
                    }
                    return false;
                }
                lease
            }
        };
        let release = || {
            if let Ok(mut leases) = state.leases.lock() {
                leases.release(granted);
            }
        };
        let mut line = String::new();
        loop {
            line.clear();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                release();
                return false;
            }
            self.touch();
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                send(json!({"error": "Each line must be a JSON object"}));
                continue;
            };
            let message_id = message["id"].clone();
            if let Some(results) = message["results"].as_array() {
                let reported = state
                    .leases
                    .lock()
                    .map_err(|_| "The lease queue failed".to_string())
                    .and_then(|mut l| l.results(granted, results, &message["overall"]));
                let answer = match reported {
                    Ok(()) => json!({"id": message_id, "reported": granted}),
                    Err(e) => json!({"id": message_id, "error": e}),
                };
                if !send(answer) {
                    release();
                    return false;
                }
            } else if message.get("release").is_some() {
                release();
                return send(json!({"id": message_id, "released": granted}));
            } else {
                send(
                    json!({"id": message_id, "error": "A lease holder may only report results or release its lease"}),
                );
            }
        }
    }
    fn serve_connection(self: &Arc<Self>, pipe: std::fs::File) {
        let Ok(reader) = pipe.try_clone() else { return };
        let mut reader = BufReader::new(reader);
        let mut writer = pipe;
        let mut send = |value: Value| -> bool {
            writeln!(writer, "{value}").is_ok() && writer.flush().is_ok()
        };
        let mut line = String::new();
        // Handshake.
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let Ok(hello) = serde_json::from_str::<Value>(&line)
            .map_err(|_| ())
            .and_then(|v| serde_json::from_value::<Hello>(v["hello"].clone()).map_err(|_| ()))
        else {
            send(json!({"hello": {"accepted": false, "reason": "bad-hello"}}));
            return;
        };
        // Numbered packages order by build number; source builds by version and stage.
        let theirs = crate::version::compare(
            &hello.version,
            u32::try_from(hello.build).unwrap_or(u32::MAX),
            VERSION,
            crate::build_info::BUILD,
        );
        if hello.protocol != PROTOCOL || theirs.is_lt() {
            send(
                json!({"hello": {"accepted": false, "reason": "older-client", "version": VERSION}}),
            );
            return;
        }
        if theirs.is_gt() {
            send(json!({"hello": {"accepted": false, "reason": "retiring", "version": VERSION}}));
            self.drain_and_exit();
        }
        send(
            json!({"hello": {"accepted": true, "version": VERSION, "build": crate::build_info::BUILD}}),
        );
        let role = hello.role;
        let submitter = if hello.submitter.is_empty() {
            "unknown".to_string()
        } else {
            hello.submitter
        };
        loop {
            line.clear();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            let Ok(request) = serde_json::from_str::<Value>(&line) else {
                send(json!({"error": "Each line must be a JSON object"}));
                continue;
            };
            let id = request["id"].clone();
            if request["stop"] == true {
                send(json!({"id": id, "exit": 0, "stdout": {"stopping": true}, "stderr": []}));
                self.drain_and_exit();
            }
            if request["status"] == true {
                send(json!({"id": id, "exit": 0, "stdout": self.status(), "stderr": []}));
                continue;
            }
            if request.get("lease").is_some() {
                self.touch();
                if !self.serve_lease(&mut reader, &mut send, id, &request, &submitter) {
                    return;
                }
                continue;
            }
            let arguments: Vec<String> = request["args"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            // The GUI's queue polls only watch: they never keep an idle coordinator alive.
            if !(role == "gui" && arguments.first().is_some_and(|a| a == "queue")) {
                self.touch();
            }
            let class = match request["class"].as_str() {
                Some("interactive") if role == "gui" => Class::Interactive,
                Some("bulk") => Class::Bulk,
                Some("job") => Class::Job,
                _ => Class::Normal,
            };
            let answer = self.handle(arguments, class, &submitter);
            let mut out = serde_json::to_value(&answer).unwrap_or(Value::Null);
            out["id"] = id;
            if !send(out) {
                return;
            }
        }
    }

    fn handle(self: &Arc<Self>, arguments: Vec<String>, class: Class, submitter: &str) -> Answer {
        let args = match cli::parse(&arguments) {
            Ok(args) => args,
            Err(e) => {
                return Answer {
                    exit: 2,
                    stdout: None,
                    stderr: vec![json!({"error": e, "kind": "usage"})],
                };
            }
        };
        let stopping = || Answer {
            exit: 75,
            stdout: None,
            stderr: vec![
                json!({"error": "coordinator_stopping: use direct mode", "kind": "coordinator_stopping"}),
            ],
        };
        match cli::route(&args.command) {
            Route::Direct => Answer::of(Err(
                "direct_only: this command runs in the client process".into(),
            )),
            Route::Read { project, nodes } => match self.project(&project) {
                Ok(state) => self.read(&state, &arguments, nodes),
                Err(e) => Answer::of(Err(e)),
            },
            Route::Queue { project } => match self.project(&project) {
                Ok(state) => Answer::of(Ok(self.queue_view(&state))),
                Err(e) => Answer::of(Err(e)),
            },
            Route::Write(route) => {
                if !self.admitting.load(Ordering::Acquire) {
                    return stopping();
                }
                let state = match self.project(&route.project) {
                    Ok(state) => state,
                    Err(e) => return Answer::of(Err(e)),
                };
                let class = if class == Class::Normal && route.bulk {
                    Class::Bulk
                } else {
                    class
                };
                let answer = self.enqueue(&state, arguments, route, class, submitter.to_string());
                answer.recv().unwrap_or_else(|_| stopping())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(id: u64, class: Class, merge: bool, waited: Duration) -> Operation {
        let (reply, _) = std::sync::mpsc::channel();
        Operation {
            id,
            arguments: vec![],
            route: WriteRoute {
                project: "p".into(),
                kind: "save",
                nodes: vec![id],
                affects: &["text"],
                merge,
                bulk: false,
                bytes: 10,
            },
            class,
            submitter: "test".into(),
            queued: Instant::now() - waited,
            reply,
        }
    }

    #[test]
    fn interactive_work_goes_first_and_ageing_prevents_starvation() {
        let now = Instant::now();
        let waiting = vec![
            op(1, Class::Bulk, false, Duration::ZERO),
            op(2, Class::Interactive, false, Duration::ZERO),
            op(3, Class::Normal, false, Duration::ZERO),
        ];
        let order: Vec<u64> = ordered(&waiting, now).iter().map(|o| o.id).collect();
        assert_eq!(order, vec![2, 3, 1]);
        // A bulk operation that has waited 10 s ranks with interactive work and wins on age.
        let waiting = vec![
            op(1, Class::Bulk, false, Duration::from_secs(10)),
            op(2, Class::Interactive, false, Duration::ZERO),
        ];
        assert_eq!(ordered(&waiting, Instant::now())[0].id, 1);
    }

    #[test]
    fn record_operations_merge_and_structural_ones_commit_alone() {
        let mut waiting = vec![
            op(1, Class::Normal, true, Duration::ZERO),
            op(2, Class::Normal, false, Duration::ZERO),
            op(3, Class::Normal, true, Duration::ZERO),
            op(4, Class::Bulk, true, Duration::ZERO),
        ];
        let batch: Vec<u64> = select(&mut waiting, Instant::now())
            .iter()
            .map(|o| o.id)
            .collect();
        assert_eq!(
            batch,
            vec![1, 3, 4],
            "record operations merge in arrival order"
        );
        let alone: Vec<u64> = select(&mut waiting, Instant::now())
            .iter()
            .map(|o| o.id)
            .collect();
        assert_eq!(alone, vec![2]);
    }
}
