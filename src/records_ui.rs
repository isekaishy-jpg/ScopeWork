//! The Research records window and its dialogs, in the dashboard's classic style.
//!
//! The window follows the dashboard's selected project. A records list sits on the left; the
//! selected record's header and three tabs sit on the right: Report (the editors, Save,
//! Mark complete… and Discard), Organize (children, prerequisites, membership, review flag
//! and data folder) and Project (scope, binary identity, folders and import). A status bar
//! shows messages and edit state. Completion is a separate, confirmed action.
use super::*;
use crate::classic;
use std::sync::Arc;

const STATUS_BAR: f32 = 26.0;
const LEFT: f32 = 300.0;

impl ResearchUi {
    /// Opens the New project dialog (File > New project…).
    pub fn new_project(&mut self) {
        self.new_project_open = true;
    }
    /// Keeps the window on the dashboard's selected project while nothing is unsaved.
    pub fn follow_project(&mut self, id: &str) {
        self.history.follow(id);
        if id == self.project_id || self.requested.is_some() || self.load_task.is_some() {
            return;
        }
        if self.has_unsaved() {
            if self.open && !self.message_error {
                self.message =
                    "Save or discard your edits to follow the dashboard's project.".into();
            }
            return;
        }
        if let Some(p) = self.ui_projects.iter().position(|p| p.id == id) {
            self.load(p, 0);
        }
    }
    /// Selects a tab and dialog directly, for the screenshot qualification example.
    #[doc(hidden)]
    pub fn qualification_view(&mut self, tab: usize, confirm_complete: bool, new_project: bool) {
        self.tab = tab;
        self.confirm_complete = confirm_complete;
        self.new_project_open = new_project;
    }
    /// Watches the write queue of the dashboard's project and, while this window is open, of
    /// its own project.
    pub fn watch_queues(&self, dashboard: Option<&str>) {
        let Some(queue) = &self.queue else {
            return;
        };
        let mut projects: Vec<&str> = dashboard.into_iter().collect();
        if self.open && !self.project_id.is_empty() && dashboard != Some(self.project_id.as_str()) {
            projects.push(&self.project_id);
        }
        queue.watch(&projects);
    }
    /// `Queue: n running, m waiting` for `project`, once polled.
    pub fn queue_label(&self, project: &str) -> String {
        self.queue
            .as_ref()
            .and_then(|q| q.view(project))
            .map_or_else(|| "Queue: -".into(), |v| v.label())
    }
    /// Opens the write-queue list for `project`.
    pub fn open_queue(&mut self, project: &str) {
        self.queue_open = Some(project.to_string());
    }
    fn show_queue(&mut self, ctx: &egui::Context) {
        let Some(project) = self.queue_open.clone() else {
            return;
        };
        let view = self.queue.as_ref().and_then(|q| q.view(&project));
        let mut open = true;
        classic::Window::new("write-queue", "Write queue")
            .default_size([520.0, 300.0])
            .show(ctx, &mut open, |ui| {
                ui.add_space(4.0);
                classic::label(ui, format!("Project: {project}"), None);
                let Some(view) = view else {
                    classic::label(ui, "Waiting for the first update...", None);
                    return;
                };
                classic::label(
                    ui,
                    if view.source == "coordinator" {
                        "Source: coordinator"
                    } else {
                        "Source: project lock (no coordinator is running)"
                    },
                    None,
                );
                for (title, lines) in [("Running", &view.running), ("Waiting", &view.waiting)] {
                    classic::group(ui, title, |ui| {
                        if lines.is_empty() {
                            classic::label(ui, "None", None);
                        }
                        for line in lines {
                            classic::label(ui, line, None);
                        }
                    });
                }
            });
        if !open {
            self.queue_open = None;
        }
    }
    fn set_error(&mut self, error: String) {
        self.message = error;
        self.message_error = true;
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        self.show_new_project(ctx);
        self.show_queue(ctx);
        self.show_history(ctx);
        if !self.open {
            return;
        }
        let title = match self.ui_projects.get(self.project) {
            Some(p) if p.id == self.project_id => format!("Research records - {}", p.name),
            _ => "Research records".to_string(),
        };
        let mut open = self.open;
        let mut nav = None;
        classic::Window::new("research-records", &title)
            .resizable(true)
            .default_size([980.0, 680.0])
            .min_size([760.0, 500.0])
            .show(ctx, &mut open, |ui| {
                // Slack keeps the content inside the window, so it never grows by itself.
                let body = (ui.available_height() - STATUS_BAR - 12.0).max(320.0);
                ui.allocate_ui(egui::vec2(ui.available_width(), body), |ui| {
                    ui.set_min_height(body);
                    nav = self.show_body(ui, body);
                });
                self.show_status_bar(ui);
            });
        self.show_confirm_complete(ctx);
        if let Some((p, id)) = nav {
            self.load(p, id);
        }
        self.open = open;
    }

    /// The window body; returns a record to load.
    fn show_body(&mut self, ui: &mut egui::Ui, height: f32) -> Option<(usize, u64)> {
        if self.root.is_none() {
            classic::label(ui, "Loading research library...", None);
            return None;
        }
        let projects = self.ui_projects.clone();
        if projects.is_empty() {
            ui.add_space(12.0);
            classic::label(ui, "No research projects yet. Create one to begin.", None);
            if classic::button(ui, true, "New project...").clicked() {
                self.new_project_open = true;
            }
            return None;
        }
        let found = projects.iter().position(|p| p.id == self.project_id);
        if let Some(p) = found {
            self.project = p;
        }
        let orphan = found.is_none()
            || self.browser_data[self.project]
                .node(&projects[self.project], self.node)
                .is_none();
        if orphan && self.has_unsaved() {
            return self.show_orphan(ui);
        }
        if orphan {
            // Ask once; a load already on its way answers this.
            if self.requested.is_some() || self.load_task.is_some() {
                classic::label(ui, "Loading record...", None);
                return None;
            }
            return Some((found.unwrap_or(0), 0));
        }
        let project = projects[self.project].clone();
        let mut nav = None;
        ui.horizontal_top(|ui| {
            ui.allocate_ui(egui::vec2(LEFT, height), |ui| {
                ui.set_width(LEFT);
                ui.vertical(|ui| {
                    classic::group(ui, "Records", |ui| {
                        let list = (height - 150.0).max(160.0);
                        let dirty = self.has_unsaved();
                        if let Some(id) = self.browser.show(
                            ui,
                            &self.browser_data[self.project],
                            self.node,
                            dirty,
                            list,
                        ) {
                            nav = Some((self.project, id));
                        }
                    })
                });
            });
            ui.add_space(6.0);
            ui.allocate_ui(egui::vec2(ui.available_width(), height), |ui| {
                ui.vertical(|ui| {
                    // Text typed while a save or load runs would be replaced when it lands.
                    if self.busy() {
                        ui.disable();
                    }
                    if let Some(load) = self.show_record(ui, &project) {
                        nav = Some(load);
                    }
                });
            });
        });
        nav
    }

    /// A draft whose project or record was removed meanwhile stays recoverable.
    fn show_orphan(&mut self, ui: &mut egui::Ui) -> Option<(usize, u64)> {
        classic::group(ui, "Removed record", |ui| {
            classic::label(
                ui,
                "The project or report was removed. Your unsaved draft is kept here until you copy or discard it.",
                None,
            );
            classic::editor(ui, &mut self.body, 12);
            classic::editor(ui, &mut self.evidence, 4);
            let mut discard = false;
            ui.horizontal(|ui| {
                if classic::button(ui, true, "Copy draft").clicked() {
                    ui.ctx().copy_text(format!(
                        "Scope: {}\n\n{}\n\n{}",
                        self.scope, self.body, self.evidence
                    ));
                }
                if classic::button(ui, true, "Discard draft").clicked() {
                    discard = true;
                }
            });
            if discard {
                self.dirty = false;
                self.scope_dirty = false;
                self.project_id.clear();
                self.project = 0;
                self.node = 0;
                return (!self.ui_projects.is_empty()).then_some((0, 0));
            }
            None
        })
    }

    /// Header and tabs for the selected record; returns a record to load.
    fn show_record(&mut self, ui: &mut egui::Ui, project: &Arc<Project>) -> Option<(usize, u64)> {
        let data = self.browser_data[self.project].clone();
        let n = data.node(project, self.node)?.clone();
        let dark = ui.visuals().dark_mode;
        classic::group(ui, "Record", |ui| {
            ui.label(
                egui::RichText::new(format!("{}: {}", n.kind.label(), n.name))
                    .strong()
                    .size(13.0),
            );
            let mut facts = vec![match self.status {
                Status::Complete => "Complete".to_string(),
                Status::InProgress => "In progress".to_string(),
                Status::NotStarted => "Not started".to_string(),
            }];
            if let Some(rva) = n.rva {
                facts.push(format!("RVA 0x{rva:08X}"));
            }
            facts.push(if self.browser.required(n.id) {
                "Required research".into()
            } else {
                "Context inventory".into()
            });
            classic::label(ui, facts.join("   |   "), None);
            if let Some(reason) = data.flags.get(&n.id) {
                classic::label(
                    ui,
                    format!("⚑ Flagged for review: {reason}"),
                    Some(classic::alert(dark)),
                );
            }
            if let Some(reason) = data.pooled.get(&n.id) {
                classic::label(
                    ui,
                    format!(
                        "⊘ In a research pool ({reason}). Only the pool's members may change it; other agents may read it and build on it. The dashboard acts for you, so it can change it."
                    ),
                    Some(classic::alert(dark)),
                );
            }
            if let Some(reason) = data.queued.get(&n.id) {
                classic::label(
                    ui,
                    format!("↻ In review queue: {reason}. Completing it again clears the entry."),
                    None,
                );
            }
        });
        ui.add_space(6.0);
        let mut tab = self.tab;
        classic::tabs(ui, &mut tab, &["Report", "Organize", "Project"]);
        self.tab = tab;
        // The page fills what is left of the column: its frame adds 16 px of margin.
        let page = (ui.available_height() - 22.0).max(200.0);
        let mut nav = None;
        classic::tab_page(ui, |ui| {
            ui.set_min_height(page);
            match self.tab {
                0 => nav = self.show_report_tab(ui, &n, page),
                1 => {
                    egui::ScrollArea::vertical()
                        .id_salt("organize")
                        .max_height(page)
                        .show(ui, |ui| self.show_organize_tab(ui, project, &data, &n))
                        .inner
                }
                _ => {
                    egui::ScrollArea::vertical()
                        .id_salt("project")
                        .max_height(page)
                        .show(ui, |ui| self.show_project_tab(ui, project))
                        .inner
                }
            }
        });
        nav
    }

    fn show_report_tab(
        &mut self,
        ui: &mut egui::Ui,
        n: &Node,
        height: f32,
    ) -> Option<(usize, u64)> {
        let working = self.busy();
        // Fill the page: editors share what the labels and buttons leave, 7 : 3.
        let rows = ((height - 120.0) / 16.0).max(8.0) as usize;
        classic::label(ui, "Research and understanding (Markdown)", None);
        if classic::editor(ui, &mut self.body, rows * 7 / 10).changed() {
            self.dirty = true;
        }
        classic::label(
            ui,
            "Evidence: cite source/build, VA/RVA, artifact or capture; label observed / specification-defined / inferred",
            None,
        );
        if classic::editor(ui, &mut self.evidence, (rows * 3 / 10).max(3)).changed() {
            self.dirty = true;
        }
        let blockers = self.completion_problems(n);
        let mut nav = None;
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let can_save = !working && self.dirty && !self.scope_dirty;
            if classic::button(ui, can_save, "Save").clicked() {
                self.save(false);
            }
            let can_complete = !working
                && !self.scope_dirty
                && blockers.is_empty()
                && (self.dirty || self.status != Status::Complete);
            if classic::button(ui, can_complete, "Mark complete...").clicked() {
                self.reviewed = false;
                self.confirm_complete = true;
            }
            if self.status == Status::Complete && !self.dirty {
                if classic::button(ui, !working, "Reopen").clicked() {
                    let token = self.loaded_hash.clone();
                    self.mutate(move |lib, p, id| {
                        lib.reopen(p, id, &token)?;
                        Ok(id)
                    });
                }
            } else if classic::button(ui, !working && self.dirty, "Discard changes").clicked() {
                nav = Some((self.project, self.node));
            }
        });
        if !blockers.is_empty() && self.status != Status::Complete {
            let dark = ui.visuals().dark_mode;
            classic::label(
                ui,
                format!("Not ready to complete: {}", blockers.join(" ")),
                Some(classic::grey_ink(dark)),
            );
        }
        nav
    }
    /// Everything that blocks completion, including the record's own text.
    fn completion_problems(&self, n: &Node) -> Vec<String> {
        let mut problems = self.blockers.clone();
        if self.body.trim().is_empty() {
            problems.push("Write the understanding report first.".into());
        }
        if n.kind == Kind::Function && self.evidence.trim().is_empty() {
            problems.push("A function needs evidence and claim classification.".into());
        }
        problems
    }
    fn save(&mut self, complete: bool) {
        let body = self.body.clone();
        let evidence = self.evidence.clone();
        let token = if complete {
            self.completion_hash.clone()
        } else {
            self.loaded_hash.clone()
        };
        self.mutate(move |lib, p, id| {
            lib.save_report(p, id, &body, &evidence, complete, &token)?;
            Ok(id)
        });
    }

    fn show_organize_tab(
        &mut self,
        ui: &mut egui::Ui,
        project: &Arc<Project>,
        data: &Arc<catalog_browser::BrowserData>,
        n: &Node,
    ) {
        let enabled = !self.has_unsaved() && !self.busy();
        if let Some(kind) = n.kind.child() {
            classic::group(ui, &format!("Add {}", kind.label().to_lowercase()), |ui| {
                ui.horizontal(|ui| {
                    classic::label(ui, "Name", None);
                    classic::field(ui, &mut self.new_name, 220.0, "");
                    if kind == Kind::Function {
                        classic::label(ui, "RVA (hex)", None);
                        classic::field(ui, &mut self.rva, 90.0, "00401000");
                    }
                    let ready = enabled
                        && !self.new_name.trim().is_empty()
                        && (kind != Kind::Function || !self.rva.trim().is_empty());
                    if classic::button(ui, ready, "Create").clicked() {
                        let name = std::mem::take(&mut self.new_name);
                        let rva = std::mem::take(&mut self.rva);
                        self.mutate(move |lib, p, id| lib.add(p, id, &name, &rva));
                    }
                });
            });
        }
        if n.kind == Kind::Function {
            classic::group(ui, "Prerequisites", |ui| {
                classic::label(
                    ui,
                    "Functions whose research this one needs (RVAs, comma separated). Tag only what is necessary to establish this function's behavior.",
                    None,
                );
                ui.horizontal(|ui| {
                    classic::field(
                        ui,
                        &mut self.dependency_rvas,
                        300.0,
                        "0x00401000, 0x00402000",
                    );
                    if classic::button(ui, enabled, "Save prerequisites").clicked() {
                        let rvas = self.dependency_rvas.clone();
                        self.mutate(move |lib, p, id| {
                            lib.set_dependencies(p, id, &rvas)?;
                            Ok(id)
                        });
                    }
                });
                let inbound: Vec<_> = project
                    .dependencies
                    .iter()
                    .filter(|e| e.prerequisite == n.id)
                    .filter_map(|e| data.node(project, e.dependent))
                    .map(|d| format!("{} (0x{:08X})", d.name, d.rva.unwrap_or_default()))
                    .collect();
                classic::label(
                    ui,
                    format!(
                        "Required by: {}",
                        if inbound.is_empty() {
                            "none".to_string()
                        } else {
                            inbound.join(", ")
                        }
                    ),
                    None,
                );
            });
        }
        if n.kind != Kind::App {
            classic::group(ui, "Membership", |ui| {
                let mut context_only = n.context_only;
                if classic::check(
                    ui,
                    enabled,
                    &mut context_only,
                    "Context only: keep in the inventory without requiring it for the parent's completion",
                )
                .changed()
                {
                    let batch = Batch {
                        schema: 2,
                        repair_legacy_links: false,
                        binary: project.binary.clone(),
                        expect_revision: None,
                        create: vec![],
                        update: vec![catalog_batch::Update {
                            id: n.id,
                            parent: None,
                            name: None,
                            context_only: Some(context_only),
                            expect_node_revision: Some(n.revision),
                        }],
                        reorder: vec![],
                        directed_by: None,
                    };
                    self.mutate(move |lib, p, id| {
                        lib.batch(p, &batch)?;
                        Ok(id)
                    });
                }
            });
        }
        classic::group(ui, "Review flag", |ui| match data.flags.get(&n.id) {
            Some(reason) => {
                classic::label(ui, format!("Flagged: {reason}"), None);
                if classic::button(ui, enabled, "Clear flag").clicked() {
                    self.mutate(move |lib, p, id| {
                        lib.set_review_flag(p, id, None)?;
                        Ok(id)
                    });
                }
            }
            None => {
                ui.horizontal(|ui| {
                    classic::field(ui, &mut self.flag_reason, 300.0, "Reason to review");
                    let ready = enabled && !self.flag_reason.trim().is_empty();
                    if classic::button(ui, ready, "Flag for review").clicked() {
                        let reason = std::mem::take(&mut self.flag_reason);
                        self.mutate(move |lib, p, id| {
                            lib.set_review_flag(p, id, Some(&reason))?;
                            Ok(id)
                        });
                    }
                });
                classic::label(ui, "A flag blocks completion until it is cleared.", None);
            }
        });
        if matches!(n.kind, Kind::Chunk | Kind::Function) {
            classic::group(ui, "Research pool", |ui| {
                if data.pooled.contains_key(&n.id) {
                    ui.horizontal(|ui| {
                        classic::field(ui, &mut self.pool_reason, 300.0, "Reason to take it out");
                        let ready = enabled && !self.pool_reason.trim().is_empty();
                        if classic::button(ui, ready, "Take out of pool").clicked() {
                            let reason = std::mem::take(&mut self.pool_reason);
                            self.mutate(move |lib, p, id| {
                                lib.pool_records(p, None, &[id], "", &reason)?;
                                Ok(id)
                            });
                        }
                    });
                    classic::label(
                        ui,
                        "A function of a pooled chunk leaves with its chunk.",
                        None,
                    );
                } else {
                    ui.horizontal(|ui| {
                        classic::field(ui, &mut self.pool_name, 140.0, "Pool");
                        classic::field(ui, &mut self.pool_reason, 260.0, "Reason");
                        let ready = enabled
                            && !self.pool_name.trim().is_empty()
                            && !self.pool_reason.trim().is_empty();
                        if classic::button(ui, ready, "Put in pool").clicked() {
                            let pool = self.pool_name.trim().to_string();
                            let reason = std::mem::take(&mut self.pool_reason);
                            self.mutate(move |lib, p, id| {
                                lib.pool_records(p, Some(&pool), &[id], "human", &reason)?;
                                Ok(id)
                            });
                        }
                    });
                    classic::label(
                        ui,
                        "Pooled records are changed only by the pool's members (research pool join).",
                        None,
                    );
                }
            });
        }
        if let Some(root) = &self.root {
            let report = root.join(&project.id).join(&data.paths[&n.id]);
            classic::group(ui, "Files", |ui| {
                classic::label(ui, report.display().to_string(), None);
                if n.kind == Kind::Function {
                    let placed = match &self.implemented_in {
                        Ok(Some(path)) => format!("Implemented in: {path}"),
                        Ok(None) => "Not placed in the implementation tracker.".into(),
                        Err(e) => format!("Implemented in: unavailable ({e})"),
                    };
                    classic::label(ui, placed, None);
                    if !self.proof.is_empty() {
                        classic::label(ui, self.proof.clone(), None);
                    }
                }
                if n.kind == Kind::Function
                    && classic::button(ui, true, "Open data folder").clicked()
                    && let Err(e) = open_data_folder(&report.parent().unwrap().join("data"))
                {
                    self.set_error(e);
                }
            });
        }
    }

    fn show_project_tab(&mut self, ui: &mut egui::Ui, project: &Arc<Project>) {
        let working = self.busy();
        classic::group(ui, "Research scope", |ui| {
            classic::label(
                ui,
                "A few words naming what is researched, such as Rendering or Asset platform.",
                None,
            );
            ui.horizontal(|ui| {
                if classic::field(ui, &mut self.scope, 320.0, "Not set").changed() {
                    self.scope_dirty = true;
                }
                if classic::button(
                    ui,
                    !working && self.scope_dirty && !self.dirty,
                    "Save scope",
                )
                .clicked()
                {
                    let scope = self.scope.clone();
                    let revision = self.scope_revision;
                    self.mutate(move |lib, p, id| {
                        lib.set_scope(p, &scope, revision)?;
                        Ok(id)
                    });
                }
                if classic::button(ui, self.scope_dirty, "Discard").clicked() {
                    self.scope = project.research_scope.clone();
                    self.scope_revision = project.revision;
                    self.scope_dirty = false;
                }
            });
        });
        classic::group(ui, "Binary identity", |ui| {
            let locked = project.nodes.iter().any(|n| n.kind == Kind::Function);
            classic::label(
                ui,
                if locked {
                    "Locked: functions exist. Use another project for a different binary."
                } else {
                    "Build, SHA-256 and image base. Set it before adding functions."
                },
                None,
            );
            ui.horizontal(|ui| {
                ui.add_enabled_ui(!locked, |ui| {
                    classic::field(ui, &mut self.binary, 420.0, "");
                });
                if classic::button(
                    ui,
                    !locked && !working && !self.has_unsaved(),
                    "Save binary",
                )
                .clicked()
                {
                    let binary = self.binary.clone();
                    self.mutate(move |lib, p, id| {
                        lib.set_binary(p, &binary, None)?;
                        Ok(id)
                    });
                }
            });
        });
        if let Some(home) = self
            .root
            .as_ref()
            .and_then(|root| root.parent())
            .map(Path::to_path_buf)
        {
            if self.group_for != project.id {
                self.group_for = project.id.clone();
                self.group_field = crate::settings::load(&home)
                    .ok()
                    .and_then(|s| s.group_of(&project.id).map(str::to_string))
                    .unwrap_or_default();
            }
            classic::group(ui, "Dashboard group", |ui| {
                classic::label(
                    ui,
                    "Projects in the same group share one dashboard tab. Display only.",
                    None,
                );
                ui.horizontal(|ui| {
                    classic::field(ui, &mut self.group_field, 220.0, "Not grouped");
                    let mut change = None;
                    if classic::button(ui, !self.group_field.trim().is_empty(), "Set group")
                        .clicked()
                    {
                        change = Some(Some(self.group_field.clone()));
                    }
                    if classic::button(ui, true, "Ungroup").clicked() {
                        change = Some(None);
                    }
                    if let Some(group) = change {
                        let existing = crate::settings::existing_projects(&home);
                        let saved = crate::settings::update(&home, |s| {
                            s.retain_projects(&existing);
                            s.assign(&project.id, group.as_deref())?;
                            Ok(s.group_of(&project.id).map(str::to_string))
                        });
                        match saved {
                            Ok(group) => {
                                self.group_field = group.unwrap_or_default();
                                self.message = "Dashboard group saved.".into();
                                self.message_error = false;
                            }
                            Err(e) => self.set_error(e),
                        }
                    }
                });
            });
        }
        if let Some(root) = self.root.clone() {
            let project_dir = root.join(&project.id);
            classic::group(ui, "Folders", |ui| {
                classic::label(ui, project_dir.display().to_string(), None);
                ui.horizontal_wrapped(|ui| {
                    for (label, folder) in [
                        ("Project", ""),
                        ("Tools", "tools"),
                        ("Profiles", "tools/profiles"),
                        ("Shared analysis", "tools/workspaces"),
                        ("Tool logs", "tools/logs"),
                    ] {
                        if classic::button(ui, true, label).clicked()
                            && let Err(e) = open_folder(&project_dir.join(folder))
                        {
                            self.set_error(e);
                        }
                    }
                });
            });
        }
        classic::group(ui, "Inventory import", |ui| {
            classic::label(
                ui,
                "Creation-only JSON batch with the exact binary identity and project revision.",
                None,
            );
            ui.horizontal(|ui| {
                classic::field(ui, &mut self.import_file, 380.0, "C:\\path\\to\\batch.json");
                if classic::button(
                    ui,
                    !working && !self.import_file.trim().is_empty(),
                    "Start import",
                )
                .clicked()
                {
                    let file = self.import_file.clone();
                    let project = project.id.clone();
                    self.action(move |root| {
                        let batch: Batch =
                            serde_json::from_slice(&fs::read(file).map_err(|e| e.to_string())?)
                                .map_err(|e| e.to_string())?;
                        let job = imports::reserve(&root, &project, &batch)?;
                        background::launch_import(&root, &project, &job.id)?;
                        Ok(None)
                    });
                }
            });
            if let Some(job) = self.jobs.get(&project.id).cloned() {
                classic::label(
                    ui,
                    format!(
                        "Job {}: {:?}, {} of {} prepared{}",
                        job.id,
                        job.state,
                        job.prepared,
                        job.total,
                        if job.state == imports::JobState::Verifying {
                            format!(", {} verified", job.verified)
                        } else {
                            String::new()
                        }
                    ),
                    None,
                );
                if let Some(error) = &job.error {
                    let dark = ui.visuals().dark_mode;
                    classic::label(ui, error.clone(), Some(classic::alert(dark)));
                }
                if !matches!(
                    job.state,
                    imports::JobState::Committed | imports::JobState::Cancelled
                ) {
                    ui.horizontal(|ui| {
                        let (project, id) = (project.id.clone(), job.id.clone());
                        if classic::button(ui, !working, "Pause").clicked() {
                            let (project, id) = (project.clone(), id.clone());
                            self.action(move |root| {
                                imports::request(&root, &project, &id, false)?;
                                Ok(None)
                            });
                        }
                        if classic::button(ui, !working, "Resume").clicked() {
                            let (project, id) = (project.clone(), id.clone());
                            self.action(move |root| {
                                background::launch_import(&root, &project, &id)?;
                                Ok(None)
                            });
                        }
                        if classic::button(ui, !working, "Cancel import").clicked() {
                            self.action(move |root| {
                                imports::request(&root, &project, &id, true)?;
                                Ok(None)
                            });
                        }
                    });
                }
            }
        });
    }

    fn busy(&self) -> bool {
        self.action_task.is_some() || self.load_task.is_some() || self.requested.is_some()
    }

    fn show_status_bar(&mut self, ui: &mut egui::Ui) {
        let dark = ui.visuals().dark_mode;
        ui.add_space(3.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let width = ui.available_width();
            let message = if self.message.is_empty() {
                "Ready".to_string()
            } else {
                self.message.clone()
            };
            let colour = self.message_error.then(|| classic::alert(dark));
            classic::status_pane(ui, (width - 404.0).max(120.0), &message, colour);
            let queue = self.queue_label(&self.project_id);
            if classic::status_pane_button(ui, 178.0, &queue).clicked() {
                let project = self.project_id.clone();
                self.open_queue(&project);
            }
            classic::status_pane(
                ui,
                110.0,
                if self.busy() { "Working..." } else { "Idle" },
                None,
            );
            classic::status_pane(
                ui,
                110.0,
                if self.has_unsaved() {
                    "Unsaved edits"
                } else {
                    "No changes"
                },
                None,
            );
        });
    }

    /// Mark complete…: completion is an attestation, so it asks once more.
    fn show_confirm_complete(&mut self, ctx: &egui::Context) {
        let Some(project) = self.ui_projects.get(self.project).cloned() else {
            return;
        };
        let Some(n) = self.browser_data[self.project]
            .node(&project, self.node)
            .cloned()
        else {
            return;
        };
        let mut open = self.confirm_complete;
        let mut confirmed = false;
        classic::Window::new("confirm-complete", "Mark complete")
            .modal(true)
            .default_size([460.0, 220.0])
            .show(ctx, &mut open, |ui| {
                ui.add_space(4.0);
                classic::label(
                    ui,
                    format!("{}: {}", n.kind.label(), n.name),
                    None,
                );
                classic::group(ui, "Completion criteria", |ui| {
                    classic::label(ui, n.kind.completion_criteria(), None);
                    classic::label(
                        ui,
                        "Completed children make a parent ready for synthesis; its own understanding is reviewed separately.",
                        None,
                    );
                });
                let mut reviewed = self.reviewed;
                classic::check(
                    ui,
                    true,
                    &mut reviewed,
                    "I reviewed the scope criteria, the understanding and the evidence",
                );
                self.reviewed = reviewed;
                if self.dirty {
                    classic::label(ui, "Your unsaved edits are saved with the completion.", None);
                }
                ui.add_space(4.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if classic::button(ui, true, "Cancel").clicked() {
                        self.confirm_complete = false;
                    }
                    if classic::button(ui, self.reviewed && !self.busy(), "Mark complete")
                        .clicked()
                    {
                        confirmed = true;
                    }
                });
            });
        self.confirm_complete &= open;
        if confirmed {
            self.confirm_complete = false;
            self.save(true);
        }
    }

    /// File > New project…
    fn show_new_project(&mut self, ctx: &egui::Context) {
        let mut open = self.new_project_open;
        let mut create = false;
        classic::Window::new("new-project", "New project")
            .modal(true)
            .default_size([420.0, 150.0])
            .show(ctx, &mut open, |ui| {
                ui.add_space(4.0);
                classic::label(
                    ui,
                    "Each project researches one exact binary in its own folder.",
                    None,
                );
                egui::Grid::new("new-project-fields")
                    .num_columns(2)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        classic::label(ui, "Name", None);
                        classic::field(ui, &mut self.new_project_name, 260.0, "WoW 3.3.5a");
                        ui.end_row();
                        classic::label(ui, "Project ID", None);
                        classic::field(ui, &mut self.new_project_id, 260.0, "lowercase-and-digits");
                        ui.end_row();
                    });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if classic::button(ui, true, "Cancel").clicked() {
                        self.new_project_open = false;
                    }
                    let ready = !self.busy()
                        && !self.has_unsaved()
                        && !self.new_project_name.trim().is_empty()
                        && !self.new_project_id.trim().is_empty();
                    if classic::button(ui, ready, "Create").clicked() {
                        create = true;
                    }
                });
            });
        self.new_project_open &= open;
        if create {
            let id = self.new_project_id.trim().to_string();
            let name = self.new_project_name.trim().to_string();
            self.new_project_open = false;
            self.open = true;
            self.action(move |root| {
                let mut lib = Library::empty(root)?;
                lib.create(&id, &name, "")?;
                Ok(Some((id, 0)))
            });
        }
    }
}
