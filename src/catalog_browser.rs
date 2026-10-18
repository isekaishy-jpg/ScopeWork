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
    queued_only: bool,
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
    /// Records reopened by structural changes and not complete again, with the latest reason.
    pub queued: HashMap<u64, String>,
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
        list_height: f32,
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
        let dark = ui.visuals().dark_mode;
        ui.horizontal(|ui| {
            if crate::classic::field(ui, &mut self.query, 170.0, "Find name or RVA").changed() {
                self.needs_layout = true;
            }
            if crate::classic::button(ui, !self.query.is_empty(), "Clear").clicked() {
                self.query.clear();
                self.needs_layout = true;
            }
        });
        ui.horizontal_wrapped(|ui| {
            for (value, text) in [
                (&mut self.focus, "Required only".to_string()),
                (
                    &mut self.flagged_only,
                    format!("Flagged ({})", self.data.flags.len()),
                ),
                (
                    &mut self.queued_only,
                    format!("Review queue ({})", self.data.queued.len()),
                ),
            ] {
                if crate::classic::check(ui, true, value, &text).changed() {
                    self.needs_layout = true;
                }
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
                let queued = self.queued_only;
                let query = self.query.trim().to_lowercase();
                match background::spawn(runtime, move || {
                    data.visible(&expanded, focus, flagged, queued, &query)
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
            crate::classic::label(ui, error.clone(), Some(crate::classic::alert(dark)));
            if crate::classic::button(ui, true, "Retry search").clicked() {
                self.needs_layout = true;
            }
        }
        if self.pending.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(16));
        }
        let mut navigate = None;
        crate::classic::sunken(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::ScrollArea::vertical()
                .id_salt("recordtree")
                .max_height(list_height)
                .min_scrolled_height(list_height)
                .auto_shrink([false, false])
                .show_rows(ui, 18.0, self.visible.len(), |ui, range| {
                    for i in range {
                        let (id, depth) = self.visible[i];
                        let row = &self.data.rows[self.data.positions[&id]];
                        let indent = 18.0 + depth as f32 * 14.0;
                        let response = crate::classic::list_row(
                            ui,
                            !dirty,
                            id == selected,
                            indent,
                            &row.label,
                        );
                        if response.clicked() {
                            navigate = Some(id);
                        }
                        if row.kind != Kind::Function && self.query.trim().is_empty() {
                            let at = egui::pos2(
                                response.rect.left() + indent - 9.0,
                                response.rect.center().y,
                            );
                            if crate::classic::expander(ui, at, self.expanded.contains(&id))
                                .clicked()
                            {
                                if !self.expanded.remove(&id) {
                                    self.expanded.insert(id);
                                }
                                self.needs_layout = true;
                            }
                        }
                    }
                });
        });
        crate::classic::label(
            ui,
            if self.pending.is_some() {
                "Updating results...".to_string()
            } else {
                format!(
                    "{} shown of {} records",
                    self.visible.len(),
                    self.data.rows.len()
                )
            },
            None,
        );
        navigate
    }
}

impl BrowserData {
    pub fn new(
        project: &Project,
        flags: &std::collections::BTreeMap<u64, ReviewFlag>,
        queued: &HashMap<u64, String>,
    ) -> Self {
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
        data.queued = queued.clone();
        data.children = index.children;
        data.rows = project
            .nodes
            .iter()
            .map(|n| {
                let address = n.rva.map(|r| format!(" 0x{r:08X}")).unwrap_or_default();
                let required = index.required.contains(&n.id);
                let label = format!(
                    "{}{}{}: {}{}{}",
                    if flags.contains_key(&n.id) {
                        "⚑ "
                    } else {
                        ""
                    },
                    if queued.contains_key(&n.id) {
                        "↻ "
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
        queued: bool,
        query: &str,
    ) -> Vec<(u64, usize)> {
        let mut visible = vec![];
        if flagged || queued {
            // Flagged and queued records are listed flat so they can be found regardless of expansion.
            visible.extend(
                self.rows
                    .iter()
                    .filter(|r| {
                        (!flagged || self.flags.contains_key(&r.id))
                            && (!queued || self.queued.contains_key(&r.id))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u64, parent: Option<u64>, kind: Kind, status: Status) -> Node {
        Node {
            context_only: false,
            id,
            parent,
            kind,
            name: format!("N{id}"),
            status,
            rva: (kind == Kind::Function).then_some(0x1000 + id as u32),
            revision: 1,
            accepted_hash: None,
            accepted_prerequisites: Default::default(),
        }
    }

    #[test]
    fn review_queue_filter_lists_queued_records_flat() {
        let project = Project {
            layout_version: 5,
            id: "fixture".into(),
            name: "Fixture".into(),
            binary: "synthetic".into(),
            research_scope: String::new(),
            next_id: 5,
            revision: 9,
            structural_epoch: 0,
            active_import: None,
            last_import: None,
            nodes: vec![
                node(0, None, Kind::App, Status::InProgress),
                node(1, Some(0), Kind::Architecture, Status::InProgress),
                node(2, Some(1), Kind::Chunk, Status::InProgress),
                node(3, Some(2), Kind::Function, Status::InProgress),
                node(4, Some(2), Kind::Function, Status::Complete),
            ],
            dependencies: vec![],
        };
        let queued: HashMap<u64, String> = [(2, "moved".to_string()), (3, "moved".to_string())]
            .into_iter()
            .collect();
        let flags = [(
            3,
            ReviewFlag {
                reason: "check".into(),
                flagged_at_revision: 9,
            },
        )]
        .into_iter()
        .collect();
        let data = BrowserData::new(&project, &flags, &queued);
        let expanded = HashSet::new();
        let ids = |rows: Vec<(u64, usize)>| rows.into_iter().map(|(id, _)| id).collect::<Vec<_>>();
        assert_eq!(
            ids(data.visible(&expanded, false, false, true, "")),
            vec![2, 3]
        );
        assert_eq!(ids(data.visible(&expanded, false, true, true, "")), vec![3]);
        assert!(data.rows[data.positions[&2]].label.starts_with("↻ "));
        assert!(data.rows[data.positions[&3]].label.starts_with("⚑ ↻ "));
    }
}
