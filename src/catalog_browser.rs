use super::*;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub(super) struct Browser {
    key: Option<(String, u64)>,
    rows: Vec<Row>,
    positions: HashMap<u64, usize>,
    children: HashMap<u64, Vec<u64>>,
    expanded: HashSet<u64>,
    focusable: HashSet<u64>,
    query: String,
    focus: bool,
    visible: Vec<(u64, usize)>,
    needs_layout: bool,
    selection: u64,
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
        self.positions
            .get(&id)
            .is_some_and(|i| self.rows[*i].required)
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        project: &Project,
        selected: u64,
        dirty: bool,
    ) -> Option<u64> {
        if self.key.as_ref() != Some(&(project.id.clone(), project.revision)) {
            let index = catalog_index::Index::new(project);
            if self.key.as_ref().is_none_or(|(id, _)| id != &project.id) {
                self.expanded.clear();
                self.expanded.insert(0);
                self.query.clear();
            }
            self.key = Some((project.id.clone(), project.revision));
            self.focusable = index.required.clone();
            for id in &index.required {
                let mut cursor = index.nodes[id].parent;
                while let Some(parent) = cursor {
                    self.focusable.insert(parent);
                    cursor = index.nodes[&parent].parent;
                }
            }
            self.children = index.children;
            self.rows = project
                .nodes
                .iter()
                .map(|n| {
                    let address = n.rva.map(|r| format!(" 0x{r:08X}")).unwrap_or_default();
                    let required = index.required.contains(&n.id);
                    let label = format!(
                        "{}: {}{}{}",
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
            self.positions = self
                .rows
                .iter()
                .enumerate()
                .map(|(i, n)| (n.id, i))
                .collect();
            self.needs_layout = true;
        }
        if selected != self.selection {
            self.selection = selected;
            let mut cursor = self
                .positions
                .get(&selected)
                .and_then(|i| self.rows[*i].parent);
            while let Some(id) = cursor {
                self.expanded.insert(id);
                cursor = self.rows[self.positions[&id]].parent;
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
        if ui
            .checkbox(&mut self.focus, "Required research only")
            .changed()
        {
            self.needs_layout = true;
        }
        if self.needs_layout {
            self.visible.clear();
            let query = self.query.trim().to_lowercase();
            if query.is_empty() {
                let mut stack = vec![(0, 0)];
                while let Some((id, depth)) = stack.pop() {
                    // A context parent may contain an explicit required prerequisite.
                    let contains_required = self.focusable.contains(&id);
                    if self.focus && !contains_required {
                        continue;
                    }
                    self.visible.push((id, depth));
                    if self.expanded.contains(&id)
                        && let Some(kids) = self.children.get(&id)
                    {
                        stack.extend(kids.iter().rev().map(|id| (*id, depth + 1)));
                    }
                }
            } else {
                self.visible.extend(
                    self.rows
                        .iter()
                        .filter(|r| (!self.focus || r.required) && r.search.contains(&query))
                        .map(|r| (r.id, 0)),
                );
            }
            self.needs_layout = false;
        }
        ui.label(format!(
            "{} visible / {} catalog records",
            self.visible.len(),
            self.rows.len()
        ));
        let mut navigate = None;
        egui::ScrollArea::vertical()
            .id_salt("recordtree")
            .max_height(240.0)
            .show_rows(ui, 22.0, self.visible.len(), |ui, range| {
                for i in range {
                    let (id, depth) = self.visible[i];
                    let row = &self.rows[self.positions[&id]];
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
