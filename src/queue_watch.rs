//! The GUI's view of projects' write queues: the coordinator's queue when one serves the
//! research home, otherwise the project lock queue. Polled in the background every 2 s.
use crate::coordinator::{self, client};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const POLL: Duration = Duration::from_secs(2);

/// One poll's result, ready to draw.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QueueView {
    pub project: String,
    /// `coordinator` or `locks`.
    pub source: String,
    pub running: Vec<String>,
    pub waiting: Vec<String>,
}
impl QueueView {
    /// `Queue: n running, m waiting`.
    pub fn label(&self) -> String {
        format!(
            "Queue: {} running, {} waiting",
            self.running.len(),
            self.waiting.len()
        )
    }
    /// Reads `research queue` output (either source).
    pub fn from_json(value: &Value) -> Self {
        let lines = |list: &Value| -> Vec<String> {
            list.as_array()
                .map(|entries| entries.iter().map(describe).collect())
                .unwrap_or_default()
        };
        // Tool leases count with operations: held ones run, waiting ones wait.
        let mut running = lines(&value["running"]);
        running.extend(lines(&value["leases"]["held"]));
        let mut waiting = lines(&value["waiting"]);
        waiting.extend(lines(&value["leases"]["waiting"]));
        Self {
            project: value["project"].as_str().unwrap_or_default().to_string(),
            source: value["source"].as_str().unwrap_or_default().to_string(),
            running,
            waiting,
        }
    }
}
/// One queue entry as a line: a coordinator operation or a lock holder/waiter.
fn describe(entry: &Value) -> String {
    if let Some(resources) = entry["resources"].as_array() {
        let kind = entry["kind"].as_str().unwrap_or("lease");
        let names: Vec<&str> = resources.iter().filter_map(Value::as_str).collect();
        let who = entry["submitter"].as_str().unwrap_or("?");
        let joined = entry["joined"]
            .as_u64()
            .map_or(String::new(), |n| format!(", {n} joined"));
        format!("{kind} lease on {}{joined} - {who}", names.join(", "))
    } else if let Some(kind) = entry["kind"].as_str() {
        let nodes = entry["nodes"].as_array().map_or(0, Vec::len);
        let who = entry["submitter"].as_str().unwrap_or("?");
        let class = entry["class"].as_str().unwrap_or("normal");
        let waited = entry["waited_ms"].as_u64().unwrap_or(0) / 1000;
        format!("{kind} ({class}, {nodes} records, {waited} s) - {who}")
    } else {
        let command = entry["command"].as_str().unwrap_or("another operation");
        match entry["pid"].as_u64() {
            Some(pid) => format!("{command} - pid {pid}"),
            None => command.to_string(),
        }
    }
}

#[derive(Default)]
struct Watched {
    projects: Vec<String>,
    views: HashMap<String, QueueView>,
    stop: bool,
}
/// A background poller; dropping it stops the thread at its next poll.
pub struct QueueWatch {
    shared: Arc<Mutex<Watched>>,
}
impl QueueWatch {
    /// Starts polling `home`; `repaint` is called whenever the view changes.
    pub fn start(home: PathBuf, repaint: impl Fn() + Send + 'static) -> Self {
        let shared = Arc::new(Mutex::new(Watched::default()));
        let watched = shared.clone();
        std::thread::spawn(move || {
            loop {
                // One connection per round, closed before sleeping: a held connection would
                // keep an otherwise idle coordinator running.
                let mut connection = None;
                let projects = match watched.lock() {
                    Ok(w) if w.stop => return,
                    Ok(w) => w.projects.clone(),
                    Err(_) => return,
                };
                let views: HashMap<String, QueueView> = projects
                    .into_iter()
                    .map(|id| {
                        let view = QueueView::from_json(&poll(&home, &id, &mut connection));
                        (id, view)
                    })
                    .collect();
                drop(connection);
                let changed = match watched.lock() {
                    Ok(mut w) => {
                        let changed = w.views != views;
                        w.views = views;
                        changed
                    }
                    Err(_) => return,
                };
                if changed {
                    repaint();
                }
                std::thread::sleep(POLL);
            }
        });
        Self { shared }
    }
    /// Selects the projects to watch.
    pub fn watch(&self, projects: &[&str]) {
        if let Ok(mut w) = self.shared.lock()
            && w.projects != projects
        {
            w.projects = projects.iter().map(|p| p.to_string()).collect();
        }
    }
    /// The latest view of `project`, once polled.
    pub fn view(&self, project: &str) -> Option<QueueView> {
        self.shared.lock().ok()?.views.get(project).cloned()
    }
}
impl Drop for QueueWatch {
    fn drop(&mut self) {
        if let Ok(mut w) = self.shared.lock() {
            w.stop = true;
        }
    }
}

/// The coordinator's queue over this round's connection, or the lock queue without one. The GUI
/// never starts a coordinator just to watch.
fn poll(
    home: &std::path::Path,
    project: &str,
    connection: &mut Option<client::Connection>,
) -> Value {
    if connection.is_none() {
        *connection = client::connect(home, "gui", false).ok().flatten();
    }
    if let Some(open) = connection.as_mut() {
        let arguments = ["queue".to_string(), project.to_string()];
        match open.request(&arguments, Some("interactive")) {
            Ok(answer) if answer.exit == 0 => {
                if let Some(stdout) = answer.stdout {
                    return stdout;
                }
            }
            _ => *connection = None,
        }
    }
    coordinator::lock_queue(&home.join("projects"), project)
        .unwrap_or_else(|_| serde_json::json!({"project": project, "source": "locks"}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn both_queue_sources_read_as_lines() {
        let coordinated = QueueView::from_json(&json!({"project": "p", "source": "coordinator",
            "running": [{"kind": "node save", "class": "normal", "nodes": [3, 4], "submitter": "agent-1", "waited_ms": 2500}],
            "waiting": [{"kind": "node claim", "class": "interactive", "nodes": [5], "submitter": "gui", "waited_ms": 0}]}));
        assert_eq!(coordinated.label(), "Queue: 1 running, 1 waiting");
        assert_eq!(
            coordinated.running,
            ["node save (normal, 2 records, 2 s) - agent-1"]
        );
        let locks = QueueView::from_json(&json!({"project": "p", "source": "locks",
            "running": [{"pid": 7, "command": "node save p"}], "waiting": []}));
        assert_eq!(locks.label(), "Queue: 1 running, 0 waiting");
        assert_eq!(locks.running, ["node save p - pid 7"]);
        assert_eq!(
            QueueView::from_json(&json!({})).label(),
            "Queue: 0 running, 0 waiting"
        );
        let leased = QueueView::from_json(
            &json!({"project": "p", "source": "coordinator", "running": [], "waiting": [],
            "leases": {"held": [{"lease": 7, "kind": "ledger-put", "resources": ["ledger"], "submitter": "agent-2", "joined": 3}],
                "waiting": [{"request": 9, "kind": "annotations-apply", "resources": ["ghidra", "ledger"], "submitter": "agent-4"}]}}),
        );
        assert_eq!(leased.label(), "Queue: 1 running, 1 waiting");
        assert_eq!(
            leased.running,
            ["ledger-put lease on ledger, 3 joined - agent-2"]
        );
        assert_eq!(
            leased.waiting,
            ["annotations-apply lease on ghidra, ledger - agent-4"]
        );
    }
}
