//! The Project history window: catalog receipts and tooling entries of the dashboard's project,
//! newest first, filtered by kind, record, RVA and UTC day, with the selected entry's details.
//! Entries are read on a background thread from the committed snapshot.
use super::history::{HistoryFilter, KINDS, day_start, entry_line};
use super::*;
use crate::classic;
use serde_json::Value;

/// Entries shown at most; filters narrow further.
const LIMIT: usize = 200;

#[derive(Default)]
pub(super) struct HistoryView {
    pub(super) open: bool,
    project: String,
    kind: String,
    node: String,
    rva: String,
    from: String,
    to: String,
    entries: Vec<Value>,
    lines: Vec<String>,
    selected: Option<usize>,
    message: String,
    error: bool,
    reload: bool,
    task: Option<std::sync::mpsc::Receiver<Result<Value>>>,
}

impl HistoryView {
    /// Follows the dashboard's project; a different project reloads the list.
    pub(super) fn follow(&mut self, project: &str) {
        if self.project != project {
            self.project = project.to_string();
            self.entries.clear();
            self.lines.clear();
            self.selected = None;
            self.reload = true;
        }
    }
    fn filter(&self) -> Result<HistoryFilter> {
        let text = |s: &str| Some(s.trim()).filter(|s| !s.is_empty()).map(str::to_string);
        let node = text(&self.node)
            .map(|n| {
                n.parse::<u64>()
                    .map_err(|_| format!("Node is a record ID: {n}"))
            })
            .transpose()?;
        let rva = text(&self.rva)
            .map(|r| {
                let digits = r.trim_start_matches("0x").trim_start_matches("0X");
                u32::from_str_radix(digits, 16).map_err(|_| format!("RVA is hexadecimal: {r}"))
            })
            .transpose()?;
        let from = text(&self.from).map(|d| day_start(&d)).transpose()?;
        let to = text(&self.to)
            .map(|d| day_start(&d).map(|t| t + 86_400))
            .transpose()?;
        Ok(HistoryFilter {
            node,
            rva,
            from,
            to,
            limit: LIMIT,
            catalog: true,
            tooling: true,
            kind: text(&self.kind),
            ..Default::default()
        })
    }
    fn start(&mut self) {
        self.reload = false;
        if self.project.is_empty() {
            return;
        }
        let filter = match self.filter() {
            Ok(filter) => filter,
            Err(e) => {
                self.message = e;
                self.error = true;
                return;
            }
        };
        let project = self.project.clone();
        let (send, recv) = std::sync::mpsc::sync_channel(1);
        self.task = Some(recv);
        self.message = "Reading history...".into();
        self.error = false;
        std::thread::spawn(move || {
            let result = crate::paths::home(None).and_then(|home| {
                let lib = Library::read_project(home.join("projects"), &project)?;
                if lib.projects.is_empty() {
                    return Err("Project was removed".into());
                }
                lib.history(0, &filter)
            });
            let _ = send.send(result);
        });
    }
    fn poll(&mut self) {
        use std::sync::mpsc::TryRecvError;
        let Some(task) = &self.task else {
            return;
        };
        match task.try_recv() {
            // An answer for the project shown before a switch: the new one is read next.
            Ok(Ok(value)) if value["project"].as_str() != Some(self.project.as_str()) => {
                self.task = None;
                self.reload = true;
            }
            Ok(Ok(value)) => {
                self.task = None;
                self.entries = value["entries"].as_array().cloned().unwrap_or_default();
                self.lines = self.entries.iter().map(entry_line).collect();
                self.selected = None;
                self.message = format!(
                    "{} entr{} (newest first, at most {LIMIT}). Times are UTC.",
                    self.entries.len(),
                    if self.entries.len() == 1 { "y" } else { "ies" }
                );
                self.error = false;
            }
            Ok(Err(e)) => {
                self.task = None;
                self.message = e;
                self.error = true;
            }
            Err(TryRecvError::Disconnected) => {
                self.task = None;
                self.message = "Reading history failed.".into();
                self.error = true;
            }
            Err(TryRecvError::Empty) => {}
        }
    }
    /// Kinds offered by the filter: all, catalog, the known kinds, then any other kind listed.
    fn kinds(&self) -> Vec<String> {
        let mut kinds: Vec<String> = ["", "catalog"].map(String::from).to_vec();
        kinds.extend(KINDS.iter().map(|k| k.name.to_string()));
        for e in &self.entries {
            if let Some(k) = e["entry_kind"].as_str()
                && !kinds.iter().any(|x| x == k)
            {
                kinds.push(k.to_string());
            }
        }
        if !self.kind.is_empty() && !kinds.contains(&self.kind) {
            kinds.push(self.kind.clone());
        }
        kinds
    }
}

/// The record an entry concerns, for Open record: the filtered node, else its only (or first) one.
pub(super) fn entry_node(entry: &Value, filtered: Option<u64>) -> Option<u64> {
    let ids: Vec<u64> = if entry["kind"] == "catalog" {
        entry["changes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| c["id"].as_u64())
            .collect()
    } else {
        entry["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
            .collect()
    };
    filtered
        .filter(|n| ids.contains(n))
        .or_else(|| ids.first().copied())
}

impl ResearchUi {
    /// Opens the Project history window (Research > Project history…).
    pub fn open_history(&mut self) {
        self.history.open = true;
        self.history.reload = true;
    }
    pub(super) fn show_history(&mut self, ctx: &egui::Context) {
        if !self.history.open {
            return;
        }
        self.history.poll();
        if self.history.reload && self.history.task.is_none() {
            self.history.start();
        }
        let title = match self
            .ui_projects
            .iter()
            .find(|p| p.id == self.history.project)
        {
            Some(p) => format!("Project history - {}", p.name),
            None => "Project history".to_string(),
        };
        let mut open = true;
        let mut open_record = None;
        let view = &mut self.history;
        classic::Window::new("project-history", &title)
            .resizable(true)
            .default_size([880.0, 580.0])
            .min_size([660.0, 420.0])
            .show(ctx, &mut open, |ui| {
                ui.add_space(4.0);
                let mut submit = false;
                ui.horizontal(|ui| {
                    classic::label(ui, "Kind:", None);
                    let shown = if view.kind.is_empty() {
                        "All".to_string()
                    } else {
                        view.kind.clone()
                    };
                    egui::ComboBox::from_id_salt("history-kind")
                        .selected_text(shown)
                        .width(180.0)
                        .show_ui(ui, |ui| {
                            for kind in view.kinds() {
                                let label = if kind.is_empty() { "All" } else { &kind };
                                if ui.selectable_label(view.kind == kind, label).clicked() {
                                    view.kind = kind.clone();
                                    submit = true;
                                }
                            }
                        });
                    for (label, text, width, hint) in [
                        ("Node:", &mut view.node, 64.0, "ID"),
                        ("RVA:", &mut view.rva, 72.0, "hex"),
                        ("From:", &mut view.from, 84.0, "YYYY-MM-DD"),
                        ("To:", &mut view.to, 84.0, "YYYY-MM-DD"),
                    ] {
                        classic::label(ui, label, None);
                        let field = classic::field(ui, text, width, hint);
                        submit |=
                            field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    }
                    submit |= classic::button(ui, view.task.is_none(), "Refresh").clicked();
                });
                if submit && view.task.is_none() {
                    view.reload = true;
                }
                ui.add_space(4.0);
                let list_height = (ui.available_height() * 0.55).max(140.0);
                classic::sunken(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("history-list")
                        .max_height(list_height)
                        .min_scrolled_height(list_height)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for (i, line) in view.lines.iter().enumerate() {
                                let row = classic::list_row(
                                    ui,
                                    true,
                                    view.selected == Some(i),
                                    2.0,
                                    line,
                                );
                                if row.clicked() {
                                    view.selected = Some(i);
                                }
                                if row.double_clicked() {
                                    open_record = view.entries.get(i).and_then(|e| {
                                        entry_node(e, view.filter().ok().and_then(|f| f.node))
                                    });
                                }
                            }
                        });
                });
                ui.add_space(4.0);
                let entry = view.selected.and_then(|i| view.entries.get(i));
                let node =
                    entry.and_then(|e| entry_node(e, view.filter().ok().and_then(|f| f.node)));
                ui.horizontal(|ui| {
                    let label =
                        node.map_or("Open record".to_string(), |n| format!("Open record {n}"));
                    if classic::button(ui, node.is_some(), &label).clicked() {
                        open_record = node;
                    }
                });
                let mut details = entry
                    .map(|e| serde_json::to_string_pretty(e).unwrap_or_default())
                    .unwrap_or_default();
                let rows = ((ui.available_height() - 40.0) / 14.0).max(4.0) as usize;
                egui::ScrollArea::vertical()
                    .id_salt("history-detail")
                    .max_height(ui.available_height() - 30.0)
                    .show(ui, |ui| {
                        // Read-only: edits to the text are discarded every frame.
                        classic::editor(ui, &mut details, rows);
                    });
                classic::label(
                    ui,
                    &view.message,
                    view.error.then(|| classic::alert(ui.visuals().dark_mode)),
                );
            });
        self.history.open = open;
        if let Some(id) = open_record {
            let project = self.history.project.clone();
            self.open_record(&project, id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lines_summarise_catalog_and_tooling_entries() {
        let catalog = json!({"kind":"catalog","revision":42,"at_unix":86_400 + 3_600,"records_changed":1,
            "changes":[{"id":7,"status":{"before":"InProgress","after":"Complete"}}]});
        assert_eq!(
            entry_line(&catalog),
            "r42  1970-01-02 01:00  catalog: 1 record, 1 status, #7 -> Complete"
        );
        let tooling = json!({"kind":"tooling","entry_kind":"reviewed-type","revision":43,
            "summary":"Reviewed SpellCast\nsecond line","nodes":[5]});
        assert_eq!(
            entry_line(&tooling),
            "r43  ----------------  reviewed-type: Reviewed SpellCast"
        );
        assert_eq!(entry_node(&tooling, None), Some(5));
        let flag = json!({"kind":"catalog","revision":44,"records_changed":0,"changes":[],
            "sidecars":["review-flags.json"]});
        assert_eq!(
            entry_line(&flag),
            "r44  ----------------  catalog: review-flags.json"
        );
        assert_eq!(entry_node(&catalog, Some(9)), Some(7));
    }

    #[test]
    fn an_answer_for_the_previous_project_is_discarded() {
        let mut view = HistoryView::default();
        view.follow("alpha");
        let (send, recv) = std::sync::mpsc::sync_channel(1);
        view.task = Some(recv);
        view.reload = false;
        view.follow("beta");
        send.send(Ok(
            json!({"project": "alpha", "entries": [{"kind": "catalog"}]}),
        ))
        .unwrap();
        view.poll();
        assert!(
            view.entries.is_empty(),
            "alpha's history is not shown under beta"
        );
        assert!(
            view.task.is_none() && view.reload,
            "beta's history is read next"
        );
    }

    #[test]
    fn days_are_utc_midnights() {
        assert_eq!(day_start("1970-01-01"), Ok(0));
        assert_eq!(day_start("2026-10-21"), Ok(1_792_540_800));
        assert_eq!(day_start("2024-02-29"), Ok(1_709_164_800));
        assert!(day_start("2025-02-29").is_err());
        assert!(day_start("21-10-2026").is_err());
    }
}
