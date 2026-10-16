use super::*;
use std::collections::{HashMap, HashSet};

type VisibleRows = Vec<(u64, usize)>;

#[derive(Default)]
pub(super) struct Browser {
    data: std::sync::Arc<BrowserData>,
    runtime: Option<solworker::SWRuntime>,
    pending: Option<(u64, solworker::SWTask<VisibleRows>)>,
    generation: u64,
    selection_generation: u64,
    expanded: HashSet<u64>,
    query: String,
    focus: bool,
    flagged_only: bool,
    visible: Vec<(u64, usize)>,
    needs_layout: bool,
    selection: u64,
    error: Option<String>,
}
#[derive(Default)]
pub(super) struct BrowserData {
    key: (String, u64),
    rows: Vec<Row>,
    positions: HashMap<u64, usize>,
    children: HashMap<u64, Vec<u64>>,
    focusable: HashSet<u64>,
    pub paths: HashMap<u64, String>,
    /// Review flag reasons by node ID (from the review-flags sidecar).
    pub flags: HashMap<u64, String>,
}
struct Row {
    id: u64,
    label: String,
    search: String,
    kind: Kind,
    required: bool,
    parent: Option<u64>,
}
impl Browser {
    pub fn required(&self, id: u64) -> bool {
        self.data
            .positions
            .get(&id)
            .is_some_and(|i| self.data.rows[*i].required)
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        data: &std::sync::Arc<BrowserData>,
        selected: u64,
        dirty: bool,
    ) -> Option<u64> {
        if self.data.key != data.key {
            if self.data.key.0 != data.key.0 {
                self.expanded.clear();
                self.expanded.insert(0);
                self.query.clear();
            }
            let retired = std::mem::replace(&mut self.data, data.clone());
            std::thread::spawn(move || drop(retired));
            self.visible.clear();
            self.needs_layout = true;
        }
        if selected != self.selection {
            self.selection = selected;
            let mut cursor = self
                .data
                .positions
                .get(&selected)
                .and_then(|i| self.data.rows[*i].parent);
            while let Some(id) = cursor {
                self.expanded.insert(id);
                cursor = self.data.rows[self.data.positions[&id]].parent;
            }
            self.needs_layout = true;
        }
        ui.horizontal(|ui| {
            if ui
                .add(
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text("Find name or RVA")
                        .desired_width(200.0),
                )
                .changed()
            {
                self.needs_layout = true;
            }
            if ui.button("Clear").clicked() {
                self.query.clear();
                self.needs_layout = true;
            }
        });
        ui.horizontal(|ui| {
            if ui
                .checkbox(&mut self.focus, "Required research only")
                .changed()
            {
                self.needs_layout = true;
            }
            if ui
                .checkbox(
                    &mut self.flagged_only,
                    format!("Flagged for review ({})", self.data.flags.len()),
                )
                .changed()
            {
                self.needs_layout = true;
            }
        });
        if self.needs_layout {
            self.generation += 1;
            self.needs_layout = false;
        }
        if let Some((generation, task)) = &mut self.pending
            && let Some(outcome) = task.try_take()
        {
            if *generation == self.generation {
                match outcome {
                    solworker::SWOutcome::Success(rows) => {
                        self.visible = rows;
                        self.error = None;
                    }
                    _ => {
                        self.error =
                            Some("Search worker failed or was cancelled. Retry the search.".into())
                    }
                }
            }
            self.pending = None;
        }
        if self.pending.is_none() && self.selection_generation != self.generation {
            if self.runtime.is_none() {
                match background::runtime(1) {
                    Ok(runtime) => self.runtime = Some(runtime),
                    Err(e) => self.error = Some(e),
                }
            }
            if let Some(runtime) = &self.runtime {
                let data = self.data.clone();
                let expanded = self.expanded.clone();
                let focus = self.focus;
                let flagged = self.flagged_only;
                let query = self.query.trim().to_lowercase();
                match background::spawn(runtime, move || {
                    data.visible(&expanded, focus, flagged, &query)
                }) {
                    Ok(task) => {
                        self.pending = Some((self.generation, task));
                        self.selection_generation = self.generation;
                    }
                    Err(e) => {
                        ui.label(e);
                    }
                }
            }
        }
        if let Some(error) = &self.error {
            ui.label(error);
            if ui.button("Retry search").clicked() {
                self.needs_layout = true;
            }
        }
        if self.pending.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(16));
            ui.label("Updating results...");
        }
        ui.label(format!(
            "{} visible / {} catalog records",
            self.visible.len(),
            self.data.rows.len()
        ));
        let mut navigate = None;
        egui::ScrollArea::vertical()
            .id_salt("recordtree")
            .max_height(240.0)
            .show_rows(ui, 22.0, self.visible.len(), |ui, range| {
                for i in range {
                    let (id, depth) = self.visible[i];
                    let row = &self.data.rows[self.data.positions[&id]];
                    ui.horizontal(|ui| {
                        ui.add_space(depth as f32 * 12.0);
                        if row.kind != Kind::Function
                            && self.query.trim().is_empty()
                            && ui
                                .small_button(if self.expanded.contains(&id) {
                                    "−"
                                } else {
                                    "+"
                                })
                                .clicked()
                        {
                            if !self.expanded.remove(&id) {
                                self.expanded.insert(id);
                            }
                            self.needs_layout = true;
                        }
                        if ui
                            .add_enabled(
                                !dirty,
                                egui::Button::new(&row.label).selected(id == selected),
                            )
                            .clicked()
                        {
                            navigate = Some(id);
                        }
                    });
                }
            });
        navigate
    }
}

impl BrowserData {
    pub fn new(project: &Project, flags: &std::collections::BTreeMap<u64, ReviewFlag>) -> Self {
        let index = catalog_index::Index::new(project);
        let mut data = Self {
            key: (project.id.clone(), project.revision),
            ..Default::default()
        };
        data.focusable = index.required.clone();
        for id in &index.required {
            let mut cursor = index.nodes[id].parent;
            while let Some(parent) = cursor {
                data.focusable.insert(parent);
                cursor = index.nodes[&parent].parent;
            }
        }
        data.paths = index.paths;
        data.flags = flags
            .iter()
            .map(|(id, f)| (*id, f.reason.clone()))
            .collect();
        data.children = index.children;
        data.rows = project
            .nodes
            .iter()
            .map(|n| {
                let address = n.rva.map(|r| format!(" 0x{r:08X}")).unwrap_or_default();
                let required = index.required.contains(&n.id);
                let label = format!(
                    "{}{}: {}{}{}",
                    if flags.contains_key(&n.id) {
                        "⚑ "
                    } else {
                        ""
                    },
                    n.kind.label(),
                    n.name,
                    address,
                    if required { "" } else { " [context]" }
                );
                Row {
                    id: n.id,
                    search: label.to_lowercase(),
                    label,
                    kind: n.kind,
                    required,
                    parent: n.parent,
                }
            })
            .collect();
        data.positions = data
            .rows
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id, i))
            .collect();

        data
    }
    fn visible(
        &self,
        expanded: &HashSet<u64>,
        focus: bool,
        flagged: bool,
        query: &str,
    ) -> Vec<(u64, usize)> {
        let mut visible = vec![];
        if flagged {
            // Flagged records are listed flat so they can be found regardless of expansion.
            visible.extend(
                self.rows
                    .iter()
                    .filter(|r| {
                        self.flags.contains_key(&r.id)
                            && (!focus || r.required)
                            && r.search.contains(query)
                    })
                    .map(|r| (r.id, 0)),
            );
        } else if query.is_empty() {
            let mut stack = vec![(0, 0)];
            while let Some((id, depth)) = stack.pop() {
                if focus && !self.focusable.contains(&id) {
                    continue;
                }
                visible.push((id, depth));
                if expanded.contains(&id)
                    && let Some(kids) = self.children.get(&id)
                {
                    stack.extend(kids.iter().rev().map(|id| (*id, depth + 1)));
                }
            }
        } else {
            visible.extend(
                self.rows
                    .iter()
                    .filter(|r| (!focus || r.required) && r.search.contains(query))
                    .map(|r| (r.id, 0)),
            );
        }
        visible
    }
    pub fn node<'a>(&self, project: &'a Project, id: u64) -> Option<&'a Node> {
        self.positions.get(&id).map(|i| &project.nodes[*i])
    }
}
