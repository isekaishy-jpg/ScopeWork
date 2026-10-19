//! The coordinator: one process per research home that answers reads from immutable
//! snapshots, queues writes per project by priority class, and merges queued record
//! operations into shared commits. See docs/proposal-coordinator.md and
//! docs/design-coordinator.md.
//!
//! It keeps no state that is not on disk. Each project's writer takes the project's file lock
//! only to commit, so direct-mode commands and older Scopework versions stay correct beside
//! it; their commits are picked up before the next read or commit.
pub mod client;
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
pub const PROTOCOL: u32 = 1;
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
    /// True while this coordinator's writer is committing (its own pending journal is expected).
    writing: AtomicBool,
    reads: Mutex<ReadLog>,
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
            reads: Mutex::new(ReadLog {
                day: today(),
                ..ReadLog::default()
            }),
        });
        projects.insert(id.to_string(), state.clone());
        let server = self.clone();
        let writer = state.clone();
        std::thread::spawn(move || server.write_loop(&writer));
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
        // overlays, so answer the way direct mode does, after it finishes.
        let external =
            !state.writing.load(Ordering::Acquire) && pending_journal(&self.root, &state.id);
        let result = if external {
            research::with_lock_wait(Duration::from_secs(120), || {
                Library::read_project(self.root.clone(), &state.id)
            })
            .and_then(|mut lib| cli::execute(command, &mut lib))
        } else {
            let mut lib = Library::snapshot_view(
                self.root.clone(),
                snapshot.project.clone(),
                snapshot.overlay.clone(),
            );
            cli::execute(command, &mut lib)
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
                stdout["revision"] = json!(snapshot.project.revision);
                stdout["pending"] = Value::Array(pending);
            }
        }
        answer
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
        json!({"project": state.id, "source": "coordinator", "running": running, "waiting": waiting})
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
                    // Idle: a new day's first idle moment writes yesterday's read counts.
                    if queue.waiting.is_empty()
                        && state.reads.lock().is_ok_and(|log| log.day != today())
                    {
                        drop(queue);
                        self.flush_reads(state);
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

    /// Appends the read counts gathered so far to the project's tooling log as one
    /// `coordinator-reads` entry, then starts counting afresh.
    fn flush_reads(&self, state: &ProjectState) {
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
        if reads == 0 {
            return;
        }
        let summary = format!(
            "{reads} reads through the coordinator, {with_pending} with pending writes (day {day})"
        );
        let diff = json!({"day_unix": day * 86_400, "reads_by_class": classes});
        let _ = research::with_lock_wait(Duration::from_secs(10), || {
            Library::open_project(self.root.clone(), Some(&state.id))
        })
        .and_then(|mut lib| {
            lib.append_history(0, "coordinator-reads", &summary, &[], &[], Some(diff))
        });
    }

    fn commit_batch(&self, state: &Arc<ProjectState>, batch: Vec<Operation>) {
        state.writing.store(true, Ordering::Release);
        let answers = self.apply(state, &batch);
        state.writing.store(false, Ordering::Release);
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
        let mut lib = match research::with_lock_wait(Duration::from_secs(600), || {
            Library::open_project(self.root.clone(), Some(&state.id))
        }) {
            Ok(lib) if !lib.projects.is_empty() => lib,
            Ok(_) => return fail_all(format!("Unknown project: {}", state.id)),
            Err(e) => return fail_all(e),
        };
        // Another process committed since the last snapshot: publish its state first, so
        // readers never see our next commit's texts against an older catalog.
        let unchanged = state
            .snapshot
            .read()
            .map(|s| s.project == lib.projects[0])
            .unwrap_or(false);
        if !unchanged {
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
            self.publish(state, lib.projects[0].clone());
        }
        answers
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
/// `a.b.c` as comparable numbers.
fn version_key(version: &str) -> Vec<u64> {
    version
        .split(['.', '-', '+'])
        .map(|part| {
            part.trim_matches(|c: char| !c.is_ascii_digit())
                .parse()
                .unwrap_or(0)
        })
        .collect()
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
            self.flush_reads(&state);
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
                            "reads": reads, "reads_with_pending": with_pending, "reads_by_class": classes})
                    })
                    .collect()
            })
            .unwrap_or_default();
        json!({"running": true, "version": VERSION, "protocol": PROTOCOL, "pid": std::process::id(),
            "uptime_s": self.started.elapsed().as_secs(),
            "idle_s": self.last_activity.lock().map_or(0, |last| last.elapsed().as_secs()), "admitting": self.admitting.load(Ordering::Acquire),
            "connections": self.connections.load(Ordering::Acquire), "projects": projects})
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
        let theirs = version_key(&hello.version);
        let ours = version_key(VERSION);
        if hello.protocol != PROTOCOL || theirs < ours {
            send(
                json!({"hello": {"accepted": false, "reason": "older-client", "version": VERSION}}),
            );
            return;
        }
        if theirs > ours {
            send(json!({"hello": {"accepted": false, "reason": "retiring", "version": VERSION}}));
            self.drain_and_exit();
        }
        send(json!({"hello": {"accepted": true, "version": VERSION, "build": 0}}));
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

    #[test]
    fn versions_compare_numerically() {
        assert!(version_key("0.10.0") > version_key("0.9.3"));
        assert!(version_key("0.9.0") == version_key("0.9.0"));
    }
}
