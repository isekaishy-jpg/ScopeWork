#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use research_dashboard::research;

use eframe::egui::{self, Color32, Vec2};

const GREEN: Color32 = Color32::from_rgb(0, 210, 48);
const RED: Color32 = Color32::from_rgb(228, 48, 40);
const YELLOW: Color32 = Color32::from_rgb(238, 193, 55);

#[derive(Clone)]
struct Row {
    groups: Vec<(String, usize, usize, usize)>,
}

/// One tab in the dashboard's project row: an ungrouped project, or a group of projects.
#[derive(Debug, PartialEq)]
enum Tab {
    Project(usize),
    Group { name: String, members: Vec<usize> },
}
/// The tab row for `projects` (IDs and names, in name order) under `settings`: ungrouped projects
/// and one tab per group, ordered by label (project or group name). Without groups this is the
/// projects in their own order.
fn tab_list(
    projects: &[(&str, &str)],
    settings: &research_dashboard::settings::Settings,
) -> Vec<Tab> {
    let mut tabs: Vec<(String, Tab)> = vec![];
    for (i, (id, name)) in projects.iter().enumerate() {
        let Some(group) = settings.group_of(id) else {
            tabs.push((name.to_string(), Tab::Project(i)));
            continue;
        };
        let existing = tabs.iter_mut().find_map(|(_, tab)| match tab {
            Tab::Group { name, members } if name == group => Some(members),
            _ => None,
        });
        match existing {
            Some(members) => members.push(i),
            None => tabs.push((
                group.to_string(),
                Tab::Group {
                    name: group.to_string(),
                    members: vec![i],
                },
            )),
        }
    }
    tabs.sort_by(|a, b| a.0.cmp(&b.0));
    tabs.into_iter().map(|(_, tab)| tab).collect()
}

/// What the selected project's panel shows.
#[derive(Clone, Copy, Debug, PartialEq)]
enum InnerView {
    Research,
    Implementation,
}
/// The implementation tracker as last loaded for a project (`research impl show`).
type TrackerResult = Result<serde_json::Value, String>;

/// Loads the tracker of `project`, with node `node`'s detail when it still exists.
fn load_tracker(project: &str, node: Option<u64>) -> TrackerResult {
    let root = research_dashboard::paths::home(None)?.join("projects");
    // Waits for an agent's write like a record load does, instead of answering busy at once.
    let lib = research::with_lock_wait(std::time::Duration::from_secs(30), || {
        research::Library::read_project(root, project)
    })?;
    if lib.projects.is_empty() {
        return Err("Project was removed".into());
    }
    lib.impl_view(0, node, false)
        .or_else(|_| lib.impl_view(0, None, false))
}

/// The tree's nodes in display order (each after its parent, siblings in their order), with
/// their depth.
fn tracker_rows(view: &serde_json::Value) -> Vec<(usize, serde_json::Value)> {
    let nodes = view["nodes"].as_array().cloned().unwrap_or_default();
    fn visit(
        parent: Option<u64>,
        depth: usize,
        nodes: &[serde_json::Value],
        out: &mut Vec<(usize, serde_json::Value)>,
    ) {
        for n in nodes.iter().filter(|n| n["parent"].as_u64() == parent) {
            out.push((depth, n.clone()));
            visit(n["id"].as_u64(), depth + 1, nodes, out);
        }
    }
    let mut out = vec![];
    visit(None, 0, &nodes, &mut out);
    out
}
/// `done / total` for a counter object.
fn counter(value: &serde_json::Value) -> String {
    format!(
        "{} / {}",
        value["done"].as_u64().unwrap_or(0),
        value["total"].as_u64().unwrap_or(0)
    )
}

struct Dashboard {
    project: usize,
    inner: InnerView,
    /// The tracker shown in the Implementation view: project, data, selected node.
    tracker: Option<(String, serde_json::Value)>,
    tracker_node: Option<u64>,
    tracker_task: Option<std::sync::mpsc::Receiver<(String, TrackerResult)>>,
    tracker_stale: bool,
    tracker_error: Option<String>,
    /// The research home's settings (project groups), reread when the file changes.
    settings: research_dashboard::settings::Settings,
    settings_stamp: Option<std::time::SystemTime>,
    settings_checked: Option<std::time::Instant>,
    /// Per group, the project its tab shows.
    group_shown: std::collections::HashMap<String, String>,
    /// The group whose project menu is open, and where to draw it.
    group_menu: Option<(String, egui::Pos2)>,
    selection: Option<(usize, usize)>,
    menu: Option<usize>,
    on_top: bool,
    about: bool,
    dark: bool,
    research: research::ResearchUi,
    projects: Vec<research::ProjectView>,
    brand_icon: Option<egui::TextureHandle>,
}

impl Default for Dashboard {
    fn default() -> Self {
        Self {
            project: 0,
            inner: InnerView::Research,
            tracker: None,
            tracker_node: None,
            tracker_task: None,
            tracker_stale: true,
            tracker_error: None,
            settings: Default::default(),
            settings_stamp: None,
            settings_checked: None,
            group_shown: Default::default(),
            group_menu: None,
            selection: None,
            menu: None,
            on_top: false,
            about: false,
            dark: true,
            research: research::ResearchUi::new(),
            projects: vec![],
            brand_icon: None,
        }
    }
}
use research_dashboard::classic::{self, bevel, group_box};
fn background(dark: bool) -> Color32 {
    classic::face(dark)
}
fn foreground(dark: bool) -> Color32 {
    classic::ink(dark)
}

fn text(p: &egui::Painter, x: f32, y: f32, s: impl ToString, color: Color32) {
    let color = if p.ctx().style().visuals.dark_mode
        && (color == Color32::BLACK || color == Color32::from_gray(65))
    {
        foreground(true)
    } else {
        color
    };
    p.text(
        egui::pos2(x, y),
        egui::Align2::LEFT_CENTER,
        s,
        egui::FontId::proportional(11.0),
        color,
    );
}
const METER_HEIGHT: f32 = 46.0;
fn meter_width(segments: usize) -> f32 {
    6.0 + segments as f32 * 3.0
}

fn meter_rows(widths: &[f32], viewport: f32, gap: f32) -> (Vec<(f32, usize)>, f32, usize) {
    let mut positions = Vec::with_capacity(widths.len());
    let mut content_width = 0.0_f32;
    let (mut row, mut on_row, mut row_width) = (0, 0, 0.0);
    for width in widths {
        let next = if on_row == 0 {
            *width
        } else {
            row_width + gap + width
        };
        // Keep the baseline pair even when oversized. Extra meters must fit
        // the viewport, never the wider content created by another row.
        if on_row >= 2 && next > viewport {
            row += 1;
            on_row = 0;
            row_width = 0.0;
        }
        let x = if on_row == 0 { 0.0 } else { row_width + gap };
        positions.push((x, row));
        row_width = x + width;
        content_width = content_width.max(row_width);
        on_row += 1;
    }
    (
        positions,
        content_width,
        if widths.is_empty() { 0 } else { row + 1 },
    )
}

// CPU Usage meter from the reference, rotated clockwise: thin vertical
// segments fill left-to-right, with the unlit lattice remaining visible.
fn led(
    p: &egui::Painter,
    r: egui::Rect,
    complete: usize,
    total: usize,
    color: Color32,
    cells: usize,
) {
    p.rect_filled(r, 0.0, Color32::BLACK);
    bevel(p, r, true);
    let inner = r.shrink(3.0);

    let fraction = if total == 0 {
        0.0
    } else {
        complete.min(total) as f32 / total as f32
    };
    let lit = (fraction * cells as f32).floor() as usize;
    let unfinished = Color32::from_rgb(76, 16, 13);
    for i in 0..cells {
        // A black horizontal seam separates the two banks of the classic meter.
        let cell_color = if i < lit {
            GREEN
        } else if i == lit && color == YELLOW {
            YELLOW
        } else {
            unfinished
        };
        let half = (inner.height() - 1.0) / 2.0;
        for bank in 0..2 {
            p.rect_filled(
                egui::Rect::from_min_size(
                    inner.min + Vec2::new(i as f32 * 3.0, bank as f32 * (half + 1.0)),
                    Vec2::new(2.0, half.max(1.0)),
                ),
                0.0,
                cell_color,
            );
        }
    }
}

fn draw_chunk_states(p: &egui::Painter, r: egui::Rect, states: &[usize]) {
    p.rect_filled(r, 0.0, Color32::BLACK);
    bevel(p, r, true);
    let inner = r.shrink(3.0);
    let half = (inner.height() - 1.0) / 2.0;
    for (i, &state) in states.iter().enumerate() {
        let color = match state {
            0 => GREEN,
            1 => YELLOW,
            _ => Color32::from_rgb(76, 16, 13),
        };
        for bank in 0..2 {
            p.rect_filled(
                egui::Rect::from_min_size(
                    inner.min + Vec2::new(i as f32 * 3.0, bank as f32 * (half + 1.0)),
                    Vec2::new(2.0, half),
                ),
                0.0,
                color,
            );
        }
    }
}

impl eframe::App for Dashboard {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string("dark_mode", self.dark.to_string());
    }
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.frame(ctx);
    }
}

impl Dashboard {
    /// The Implementation view inside the project panel: the tracker's tree with counters, the
    /// selected node's placements and items, and the project's totals.
    fn implementation_view(
        &mut self,
        ui: &mut egui::Ui,
        p: &egui::Painter,
        o: egui::Pos2,
        w: f32,
        h: f32,
        ink: Color32,
    ) {
        let r = |x, y, width, height| {
            egui::Rect::from_min_size(o + Vec2::new(x, y), Vec2::new(width, height))
        };
        let top = 82.0;
        // The same space the research view's meters and total progress use.
        let content = (h - 270.0).max(218.0) + 154.0;
        let boxes = content - 74.0;
        let tree_box = r(15.0, top, 320.0, boxes);
        let detail_box = r(347.0, top, w - 362.0, boxes);
        let totals_box = r(15.0, top + boxes + 18.0, w - 30.0, 56.0);
        let project = self.projects.get(self.project).cloned();
        let view = match (&self.tracker, &project) {
            (Some((id, view)), Some(project)) if *id == project.id => Some(view.clone()),
            _ => None,
        };
        group_box(p, tree_box, "Implementation tree");
        group_box(
            p,
            detail_box,
            view.as_ref()
                .and_then(|v| v["node"]["path"].as_str())
                .unwrap_or("No node selected"),
        );
        group_box(p, totals_box, "Implementation progress");
        let Some(view) = view else {
            let message = self
                .tracker_error
                .clone()
                .unwrap_or_else(|| "Loading the implementation tracker...".into());
            text(p, o.x + 26.0, o.y + top + 24.0, message, ink);
            return;
        };
        let rows = tracker_rows(&view);
        if rows.is_empty() {
            text(
                p,
                o.x + 26.0,
                o.y + top + 24.0,
                "No implementation tree yet. Agents build it with research impl.",
                ink,
            );
        }
        let mut chosen = None;
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(tree_box.shrink2(Vec2::new(8.0, 16.0))),
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("implementation tree")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        for (depth, node) in &rows {
                            let id = node["id"].as_u64().unwrap_or(0);
                            let subtree = &node["subtree"];
                            let mut label = format!(
                                "{}{}   {}",
                                "    ".repeat(*depth),
                                node["name"].as_str().unwrap_or(""),
                                counter(&subtree["research"])
                            );
                            if subtree["implementation_only"]["total"]
                                .as_u64()
                                .unwrap_or(0)
                                > 0
                            {
                                label += &format!(
                                    "   items {}",
                                    counter(&subtree["implementation_only"])
                                );
                            }
                            let selected = self.tracker_node == Some(id);
                            if ui.selectable_label(selected, label).clicked() {
                                chosen = Some(id);
                            }
                        }
                    });
            },
        );
        if let Some(id) = chosen {
            self.tracker_node = Some(id);
            self.tracker_stale = true;
        }
        let mut open = None;
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(detail_box.shrink2(Vec2::new(8.0, 16.0))),
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("implementation detail")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        let node = &view["node"];
                        if node.is_null() {
                            ui.label("Select a node to see its research items.");
                            return;
                        }
                        let placements = node["placements"].as_array().cloned().unwrap_or_default();
                        let items = node["items"].as_array().cloned().unwrap_or_default();
                        if placements.is_empty() && items.is_empty() {
                            ui.label("Nothing is placed on this node itself.");
                        }
                        for pl in &placements {
                            let mark = if pl["completed"].is_null() {
                                "\u{2610}"
                            } else {
                                "\u{2611}"
                            };
                            let name = if pl["missing"].as_bool() == Some(true) {
                                format!("record {} (no longer in the research)", pl["id"])
                            } else {
                                format!(
                                    "{} ({})",
                                    pl["name"].as_str().unwrap_or(""),
                                    pl["rva"].as_str().unwrap_or("")
                                )
                            };
                            let response = ui
                                .selectable_label(false, format!("{mark} {name}"))
                                .on_hover_text("Open in Research records");
                            if response.clicked() {
                                open = pl["id"].as_u64();
                            }
                        }
                        if !items.is_empty() {
                            ui.add_space(6.0);
                            ui.label("Implementation-only items");
                            for item in &items {
                                let mark = if item["completed"].is_null() {
                                    "\u{2610}"
                                } else {
                                    "\u{2611}"
                                };
                                ui.label(format!("{mark} {}", item["name"].as_str().unwrap_or("")));
                            }
                        }
                    });
            },
        );
        if let (Some(record), Some(project)) = (open, &project) {
            self.research.open_record(&project.id, record);
        }
        let totals = &view["totals"];
        let mut line = format!(
            "Research items: {}    Implementation-only: {}    Not placed: {}",
            counter(&totals["research"]),
            counter(&totals["implementation_only"]),
            view["not_placed"].as_u64().unwrap_or(0)
        );
        if let Some(repository) = view["repository"].as_str().filter(|r| !r.is_empty()) {
            line += &format!("    Repository: {repository}");
        }
        text(
            p,
            o.x + 26.0,
            o.y + totals_box.top() - o.y + 30.0,
            line,
            ink,
        );
    }

    /// One frame of the dashboard (`eframe::App::update` without the eframe frame).
    fn frame(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && self.research.has_unsaved() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.research.prevent_close();
        }
        self.research.poll(ctx);
        if self.research.changed {
            let selected_id = self.projects.get(self.project).map(|p| p.id.clone());
            self.projects = self.research.snapshot();
            self.project = selected_id
                .and_then(|id| self.projects.iter().position(|p| p.id == id))
                .unwrap_or(0);
            self.research.changed = false;
            self.selection = None;
            self.tracker_stale = true;
        }
        if self
            .settings_checked
            .is_none_or(|at| at.elapsed() >= std::time::Duration::from_secs(1))
        {
            self.settings_checked = Some(std::time::Instant::now());
            if let Ok(home) = research_dashboard::paths::home(None) {
                let stamp = research_dashboard::settings::stamp(&home);
                if stamp != self.settings_stamp {
                    self.settings_stamp = stamp;
                    self.settings = research_dashboard::settings::load(&home).unwrap_or_default();
                }
            }
        }
        if ctx.style().visuals.dark_mode != self.dark {
            ctx.set_visuals(classic::visuals(self.dark));
        }
        if let Some(task) = &self.tracker_task
            && let Ok((project, result)) = task.try_recv()
        {
            self.tracker_task = None;
            match result {
                Ok(view) => {
                    self.tracker_node = view["node"]["id"].as_u64();
                    self.tracker = Some((project, view));
                    self.tracker_error = None;
                }
                Err(e) => self.tracker_error = Some(e),
            }
        }
        if self.inner == InnerView::Implementation
            && self.tracker_task.is_none()
            && let Some(project) = self.projects.get(self.project)
            && (self.tracker_stale
                || self
                    .tracker
                    .as_ref()
                    .is_none_or(|(id, _)| *id != project.id))
        {
            self.tracker_stale = false;
            let (send, recv) = std::sync::mpsc::channel();
            let id = project.id.clone();
            let shown = self.tracker.as_ref().is_some_and(|(p, _)| *p == id);
            let node = if shown { self.tracker_node } else { None };
            std::thread::spawn(move || {
                let result = load_tracker(&id, node);
                let _ = send.send((id, result));
            });
            self.tracker_task = Some(recv);
        }
        let queue_project = self.projects.get(self.project).map(|p| p.id.clone());
        self.research.watch_queues(queue_project.as_deref());
        let bg = background(self.dark);
        let ink = foreground(self.dark);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(bg).inner_margin(4.0))
            .show(ctx, |ui| {
                let o = ui.min_rect().min;
                let w = ui.available_width();
                let h = ui.available_height();
                let p = ui.painter().clone();
                let r = |x, y, width, height| {
                    egui::Rect::from_min_size(o + Vec2::new(x, y), Vec2::new(width, height))
                };
                let t = |x, y, s: String| text(&p, o.x + x, o.y + y, s, Color32::BLACK);
                bevel(&p, r(-2.0, -2.0, w + 4.0, h + 4.0), false);
                // Own the window chrome as well as the content, avoiding modern Windows decoration.
                let title = r(0.0, 0.0, w, 20.0);
                p.rect_filled(
                    title,
                    0.0,
                    if self.dark {
                        Color32::from_rgb(37, 48, 66)
                    } else {
                        Color32::from_rgb(0, 0, 128)
                    },
                );
                let icon = r(3.0, 2.0, 16.0, 16.0);
                if let Some(texture) = &self.brand_icon {
                    p.image(texture.id(), icon, egui::Rect::from_min_max(egui::pos2(0.0,0.0),egui::pos2(1.0,1.0)),Color32::WHITE);
                }
                text(
                    &p,
                    o.x + 22.0,
                    o.y + 10.0,
                    "Scopework",
                    Color32::WHITE,
                );
                let drag = ui.interact(
                    r(19.0, 0.0, w - 79.0, 20.0),
                    ui.id().with("drag"),
                    egui::Sense::click_and_drag(),
                );
                if drag.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                if drag.double_clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(
                        !ctx.input(|i| i.viewport().maximized.unwrap_or(false)),
                    ));
                }
                for i in 0..3 {
                    let b = r(w - 55.0 + i as f32 * 18.0, 3.0, 16.0, 14.0);
                    p.rect_filled(b, 0.0, bg);
                    bevel(&p, b, false);
                    match i {
                        0 => {
                            p.line_segment(
                                [b.min + Vec2::new(4.0, 10.0), b.min + Vec2::new(11.0, 10.0)],
                                egui::Stroke::new(2.0_f32, ink),
                            );
                        }
                        1 => {
                            p.rect_stroke(
                                b.shrink(3.0),
                                0.0,
                                egui::Stroke::new(1.0_f32, ink),
                                egui::StrokeKind::Inside,
                            );
                        }
                        _ => {
                            p.line_segment(
                                [b.min + Vec2::new(4.0, 3.0), b.max - Vec2::new(4.0, 3.0)],
                                egui::Stroke::new(1.0_f32, ink),
                            );
                            p.line_segment(
                                [b.min + Vec2::new(4.0, 11.0), b.min + Vec2::new(12.0, 3.0)],
                                egui::Stroke::new(1.0_f32, ink),
                            );
                        }
                    }
                    if ui
                        .interact(b, ui.id().with(("window", i)), egui::Sense::click())
                        .clicked()
                    {
                        ctx.send_viewport_cmd(match i {
                            0 => egui::ViewportCommand::Minimized(true),
                            1 => egui::ViewportCommand::Maximized(
                                !ctx.input(|i| i.viewport().maximized.unwrap_or(false)),
                            ),
                            _ => egui::ViewportCommand::Close,
                        });
                    }
                }
                for (i, (x, label)) in [
                    (4.0, "File"),
                    (38.0, "Options"),
                    (94.0, "View"),
                    (132.0, "Help"),
                ]
                .iter()
                .enumerate()
                {
                    let mr = r(*x, 22.0, if i == 1 { 54.0 } else { 34.0 }, 18.0);
                    if self.menu == Some(i) {
                        p.rect_filled(
                            mr,
                            0.0,
                            if self.dark {
                                Color32::from_rgb(50, 57, 69)
                            } else {
                                Color32::from_gray(190)
                            },
                        );
                        bevel(&p, mr, true);
                    }
                    t(*x + 4.0, 31.0, label.to_string());
                    if ui
                        .interact(mr, ui.id().with(("menu", i)), egui::Sense::click())
                        .clicked()
                    {
                        self.menu = if self.menu == Some(i) { None } else { Some(i) };
                    }
                }
                let panel = r(3.0, 64.0, w - 6.0, h - 91.0);
                bevel(&p, panel, false);
                let mut tab_x = 5.0;
                let listed: Vec<(&str, &str)> = self
                    .projects
                    .iter()
                    .map(|p| (p.id.as_str(), p.name.as_str()))
                    .collect();
                let tabs = tab_list(&listed, &self.settings);
                let mut clicked = None;
                for (i, entry) in tabs.iter().enumerate() {
                    // The project this tab shows, and for a group its name.
                    let (shown, group) = match entry {
                        Tab::Project(index) => (*index, None),
                        Tab::Group { name, members } => {
                            let remembered = self.group_shown.get(name).and_then(|id| {
                                members.iter().copied().find(|m| self.projects[*m].id == *id)
                            });
                            let shown = if members.contains(&self.project) {
                                self.project
                            } else {
                                remembered.unwrap_or(members[0])
                            };
                            (shown, Some(name.clone()))
                        }
                    };
                    let label_text = match &group {
                        Some(name) => format!("{name}: {}", self.projects[shown].name),
                        None => self.projects[shown].name.clone(),
                    };
                    let label = p
                        .layout_no_wrap(label_text.clone(), egui::FontId::proportional(11.0), ink)
                        .size()
                        .x;
                    // A group tab also holds its drop-down arrow.
                    let width = tab_width(label + if group.is_some() { 12.0 } else { 0.0 });
                    let tab = r(tab_x, 43.0, width, 22.0);
                    p.rect_filled(tab, 0.0, bg);
                    bevel(&p, tab, false);
                    let selected = self.project == shown;
                    if selected {
                        p.line_segment(
                            [tab.left_bottom(), tab.right_bottom()],
                            egui::Stroke::new(2.0_f32, bg),
                        );
                    }
                    t(tab_x + 12.0, 54.0, label_text);
                    if group.is_some() {
                        let c = egui::pos2(tab.right() - 14.0, tab.center().y + 1.0);
                        p.add(egui::Shape::convex_polygon(
                            vec![
                                c + Vec2::new(-3.5, -2.0),
                                c + Vec2::new(3.5, -2.0),
                                c + Vec2::new(0.0, 2.0),
                            ],
                            ink,
                            egui::Stroke::NONE,
                        ));
                    }
                    tab_x += width;
                    if ui
                        .interact(tab, ui.id().with(("tab", i)), egui::Sense::click())
                        .clicked()
                    {
                        let arrow = ctx
                            .pointer_interact_pos()
                            .is_some_and(|at| at.x >= tab.right() - 22.0);
                        clicked = Some((shown, group, selected || arrow, tab));
                    }
                }
                if !self.projects.is_empty() {
                    let mut x = w - 5.0;
                    for (view, label) in [
                        (InnerView::Implementation, "Implementation"),
                        (InnerView::Research, "Research"),
                    ] {
                        let size = p
                            .layout_no_wrap(label.to_string(), egui::FontId::proportional(11.0), ink)
                            .size()
                            .x;
                        let width = tab_width(size);
                        x -= width;
                        let tab = r(x, 43.0, width, 22.0);
                        p.rect_filled(tab, 0.0, bg);
                        bevel(&p, tab, false);
                        if self.inner == view {
                            p.line_segment(
                                [tab.left_bottom(), tab.right_bottom()],
                                egui::Stroke::new(2.0_f32, bg),
                            );
                        }
                        t(x + 12.0, 54.0, label.to_string());
                        if ui
                            .interact(tab, ui.id().with(("inner", label)), egui::Sense::click())
                            .clicked()
                        {
                            self.inner = view;
                        }
                    }
                }
                if let Some((shown, group, menu, tab)) = clicked {
                    if let Some(name) = group.filter(|_| menu) {
                        self.group_menu = match &self.group_menu {
                            Some((open, _)) if *open == name => None,
                            _ => Some((name, tab.left_bottom())),
                        };
                    } else {
                        self.group_menu = None;
                    }
                    self.project = shown;
                    self.selection = None;
                }
                let project_view=self.projects.get(self.project).cloned();
                let live_meters=project_view.as_ref().map(|p|p.meters.clone()).unwrap_or_default();
                let data:Vec<Row>=live_meters.chunks(4).map(|batch|Row{groups:batch.iter().map(|m|(m.name.clone(),m.functions,if m.states.iter().all(|&s|s==0)&&!m.states.is_empty(){0}else if m.states.iter().any(|&s|s!=2){1}else{2},m.completed_functions)).collect()}).collect();
                let counts = function_counts(&live_meters);
                let total: usize = counts.iter().sum();
                let project_counts:Vec<usize>=live_meters.iter().map(|m|m.states.len()).collect();
                let total_chunks:usize=project_counts.iter().sum();
                let done_chunks:usize=live_meters.iter().flat_map(|m|m.states.iter()).filter(|&&s|s==0).count();
                let selected=self.selection.filter(|&(r,g)|r<data.len()&&g<data[r].groups.len()).unwrap_or((0,0));
                let (name,n,status,completed)=data.get(selected.0).and_then(|row|row.groups.get(selected.1)).cloned().unwrap_or(("No meter selected".into(),0,2,0));
                if self.inner == InnerView::Implementation {
                    self.implementation_view(ui, &p, o, w, h, ink);
                } else {
                let top=82.0;
                let meter_h=(h-270.0).max(218.0);
                let detail=r(15.0,top,174.0,meter_h);
                group_box(&p,detail,"Selected meter");
                let well=detail.shrink2(Vec2::new(10.0,17.0));
                p.rect_filled(well,0.0,Color32::BLACK);bevel(&p,well,true);
                let selected_index=selected.0*4+selected.1;
                let selected_chunks=project_counts.get(selected_index).copied().unwrap_or(0);
                let selected_done=live_meters.get(selected_index).map(|m|m.states.iter().filter(|&&s|s==0).count()).unwrap_or(0);
                let title=name.clone();
                let heading=p.layout(title,egui::FontId::proportional(11.0),GREEN,well.width()-14.0);
                let heading_height=heading.size().y;
                p.galley(well.min+Vec2::new(7.0,8.0),heading,GREEN);
                let bar=egui::Rect::from_min_size(well.min+Vec2::new(6.0,heading_height+20.0),Vec2::new(well.width()-12.0,32.0));
                led(&p,bar,selected_done,selected_chunks,[GREEN,YELLOW,RED][status],((bar.width()-6.0)/3.0).floor() as usize);
                let readout=bar.bottom()+19.0;
                text(&p,well.left()+7.0,readout,["Complete","In progress","Not done"][status],GREEN);
                text(&p,well.left()+7.0,readout+24.0,format!("Chunks: {selected_done} / {selected_chunks}"),GREEN);
                text(&p,well.left()+7.0,readout+45.0,format!("Funcs: {} / {}",completed,n),GREEN);
                text(&p,well.left()+7.0,readout+69.0,format!("{}% chunks complete",selected_done*100/selected_chunks.max(1)),GREEN);
                let hint=p.layout("Click a meter to inspect it.".into(),egui::FontId::proportional(11.0),Color32::from_gray(150),well.width()-14.0);
                p.galley(egui::pos2(well.left()+7.0,readout+96.0),hint,Color32::from_gray(150));
                let bank=r(201.0,top,w-216.0,meter_h);
                let scope=project_view.as_ref().map(|project|project.research_scope.trim()).unwrap_or("");
                let header=if scope.is_empty(){"Research scope: Not set".to_string()}else{format!("Research scope: {scope}")};
                let mut visible_header=header.clone();
                while p.layout_no_wrap(visible_header.clone(),egui::FontId::proportional(11.0),ink).size().x>bank.width()-30.0 && visible_header.chars().count()>2 {
                    if visible_header.ends_with('\u{2026}'){visible_header.pop();}
                    visible_header.pop();visible_header.push('\u{2026}');
                }
                group_box(&p, bank, &visible_header);
                ui.interact(egui::Rect::from_min_size(bank.min-Vec2::new(0.0,8.0),Vec2::new(bank.width(),18.0)),ui.id().with("scope_heading"),egui::Sense::hover()).on_hover_text(&header);
                // Preserve meter styling; only the arrangement and configurable length change.
                ui.scope_builder(
                    egui::UiBuilder::new().max_rect(bank.shrink2(Vec2::new(8.0, 16.0))),
                    |ui| {
                        ui.set_clip_rect(ui.clip_rect().intersect(bank.shrink2(Vec2::new(8.0,16.0))));
                        egui::ScrollArea::both()
                            .max_width(bank.width()-16.0)
                            .max_height(bank.height()-32.0)
                            .min_scrolled_width(0.0)
                            .min_scrolled_height(0.0)
                            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
                            .id_salt(("meters", self.project))
                            .auto_shrink([false, false])
                            .show_viewport(ui, |ui, viewport| {
                                if live_meters.is_empty() {
                                    ui.label(if project_view.is_none(){"No projects yet. Create your first research project."}else{"No architecture records yet. Add one in Research records."});
                                    if ui.button(if project_view.is_none(){"Create project"}else{"Open research records"}).clicked(){self.research.open=true;}
                                }
                                let padding=8.0;
                                let gap=12.0;
                                let row_height=104.0;
                                let panel_widths:Vec<f32>=data.iter().flat_map(|row|row.groups.iter()).enumerate().map(|(index,g)|{
                                    let title=g.0.clone();
                                    let count=project_counts[index];
                                    let footer=format!("Chunks: {} / {}    Funcs: {} / {}",count,count,g.1,g.1);
                                    let font=egui::FontId::proportional(11.0);
                                    let title_w=ui.painter().layout_no_wrap(title,font.clone(),ink).size().x;
                                    let footer_w=ui.painter().layout_no_wrap(footer,font,ink).size().x;
                                    meter_width(count).max(title_w).max(footer_w)+padding*2.0
                                }).collect();
                                let (positions,content_width,rows)=meter_rows(&panel_widths,viewport.width(),gap);
                                let (area,_)=ui.allocate_exact_size(Vec2::new(content_width,(row_height*rows as f32-gap).max(0.0)),egui::Sense::hover());
                                let base = area.min;
                                let p = ui.painter().clone();
                                for (ri,row) in data.iter().enumerate() {
                                    for (gi,group) in row.groups.iter().enumerate() {
                                        let index = ri * 4 + gi;
                                        let (offset,slot) = positions[index];
                                        let (chunk, num, state, finished) = group.clone();
                                        let panel_x=base.x+offset;
                                        let panel_y=base.y+slot as f32*row_height;
                                        let panel_rect=egui::Rect::from_min_size(egui::pos2(panel_x,panel_y),Vec2::new(panel_widths[index],row_height-gap));
                                        p.rect_filled(panel_rect,0.0,bg);
                                        bevel(&p,panel_rect,true);
                                        let x=panel_x+padding;
                                        let y=panel_y+padding;
                                        let segment_count = project_counts[index];
                                        let meter = egui::Rect::from_min_size(
                                            egui::pos2(x, y + 15.0),
                                            Vec2::new(meter_width(segment_count), METER_HEIGHT),
                                        );
                                        draw_chunk_states(&p,meter,live_meters.get(index).map(|m|m.states.as_slice()).unwrap_or(&[]));

                                        text(
                                            &p,
                                            x,
                                            y + 5.0,
                                            chunk.clone(),
                                            Color32::BLACK,
                                        );
                                        text(
                                            &p,
                                            x,
                                            meter.bottom() + 10.0,
                                            format!(
                                                "Chunks: {} / {}    Funcs: {} / {}",
                                                live_meters.get(index).map(|m|m.states.iter().filter(|&&s|s==0).count()).unwrap_or(0),
                                                segment_count,
                                                finished,
                                                num
                                            ),
                                            Color32::BLACK,
                                        );
                                        let hit = ui.interact(
                                            meter,
                                            ui.id().with((ri, gi, "chunk")),
                                            egui::Sense::click(),
                                        );
                                        if hit.clicked() {
                                            self.selection = Some((ri, gi));
                                        }
                                        if selected == (ri, gi) {
                                            p.rect_stroke(
                                                meter.expand(1.0),
                                                0.0,
                                                egui::Stroke::new(
                                                    1.0_f32,
                                                    if self.dark {
                                                        Color32::from_rgb(125, 170, 239)
                                                    } else {
                                                        Color32::from_rgb(0, 0, 128)
                                                    },
                                                ),
                                                egui::StrokeKind::Outside,
                                            );
                                        }
                                        if hit.double_clicked()
                                            && let (Some(project),Some(meter))=(&project_view,live_meters.get(index)){self.research.open_record(&project.id,meter.id);}
                                        hit.context_menu(|ui| {
                                            if ui.button("Open architecture report").clicked()&& let (Some(project),Some(meter))=(&project_view,live_meters.get(index)){self.research.open_record(&project.id,meter.id);}
                                            ui.label("Segment counts come from this architecture's chunk records.");
                                        });
                                        hit.on_hover_text(format!(
                                            "{chunk}: {} functions - {}",
                                            num,
                                            ["Done", "In progress", "Not done"][state]
                                        ));
                                    }
                                }
                            });
                    },
                );
                let bottom=top+meter_h+18.0;
                let summary=r(15.0,bottom,w-30.0,136.0);
                group_box(&p,summary,"Total progress");
                let total_meter=r(25.0,bottom+18.0,w-50.0,METER_HEIGHT);
                led(&p,total_meter,counts[0],total,GREEN,((total_meter.width()-6.0)/3.0).floor().max(0.0) as usize);
                t(26.0,bottom+80.0,format!("Chunks: {done_chunks} / {total_chunks}    Funcs: {} / {total}    Done: {}%",counts[0],counts[0]*100/total.max(1)));
                t(26.0,bottom+101.0,format!("Functions in progress: {}    Not done: {}",counts[1],counts[2]));
                t(26.0,bottom+122.0,format!("Selected: {}    Funcs: {} / {}    {}",name,completed,n,["Done","In progress","Not done"][status]));
                }
                let mut sx = 0.0;
                for (width, label) in [
                    (w * 0.24, format!("Funcs: {} / {total}", counts[0])),
                    (
                        w * 0.24,
                        format!("Chunks done: {done_chunks} / {total_chunks}"),
                    ),
                    (
                        w * 0.34,
                        project_view.as_ref().map(|project|format!("App: {:?} | Inventory: {} funcs | Required: {} | Context: {}",project.app_status,project.inventory_functions,project.required_functions,project.inventory_functions-project.required_functions)).unwrap_or_else(||"No projects".into()),
                    ),
                ] {
                    let sr = r(sx, h - 20.0, width - 2.0, 19.0);
                    bevel(&p, sr, true);
                    t(sx + 5.0, h - 10.0, label);
                    sx += width;
                }
                // The write queue of the selected project; clicking lists it.
                if let Some(id) = &queue_project {
                    let sr = r(sx, h - 20.0, w - sx - 16.0, 19.0);
                    bevel(&p, sr, true);
                    let label = self.research.queue_label(id);
                    text(&p.with_clip_rect(sr.shrink(1.0)), o.x + sx + 5.0, o.y + h - 10.0, label, Color32::BLACK);
                    if ui.interact(sr, ui.id().with("queue"), egui::Sense::click()).clicked() {
                        self.research.open_queue(id);
                    }
                }
                for i in 0..3 {
                    let d = i as f32 * 4.0;
                    p.line_segment(
                        [
                            o + Vec2::new(w - 11.0 + d, h - 3.0),
                            o + Vec2::new(w - 3.0, h - 11.0 + d),
                        ],
                        egui::Stroke::new(1.0_f32, Color32::from_gray(110)),
                    );
                }
                if ui
                    .interact(
                        r(w - 14.0, h - 14.0, 14.0, 14.0),
                        ui.id().with("resize"),
                        egui::Sense::drag(),
                    )
                    .drag_started()
                {
                    ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(
                        egui::ResizeDirection::SouthEast,
                    ));
                }
            });
        if let Some(menu) = self.menu {
            let x = [8.0, 42.0, 98.0, 136.0][menu];
            let mut chosen = None;
            let popup = egui::Area::new(egui::Id::new("classic menu"))
                .fixed_pos(egui::pos2(x, 44.0))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    classic::menu_frame(self.dark).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        let items: &[(&str, bool)] = match menu {
                            0 => &[("New project...", false), ("Exit", false)],
                            1 => &[("Dark mode", self.dark), ("Always on top", self.on_top)],
                            2 => &[
                                ("Research records...", false),
                                ("Project history...", false),
                                ("Refresh research progress", false),
                                ("Clear chunk selection", false),
                            ],
                            _ => &[("About Scopework", false)],
                        };
                        for (i, (label, checked)) in items.iter().enumerate() {
                            if classic::menu_item(ui, label, *checked).clicked() {
                                chosen = Some(i);
                            }
                        }
                    });
                });
            match (menu, chosen) {
                (0, Some(0)) => self.research.new_project(),
                (0, Some(_)) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                (1, Some(0)) => {
                    self.dark = !self.dark;
                    ctx.request_repaint();
                }
                (1, Some(_)) => {
                    self.on_top = !self.on_top;
                    ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(if self.on_top {
                        egui::WindowLevel::AlwaysOnTop
                    } else {
                        egui::WindowLevel::Normal
                    }));
                }
                (2, Some(0)) => self.research.open = true,
                (2, Some(1)) => self.research.open_history(),
                (2, Some(2)) => self.research.refresh_library(),
                (2, Some(_)) => self.selection = None,
                (_, Some(_)) => self.about = true,
                (_, None) => {}
            }
            if chosen.is_some()
                || ctx.input(|i| i.key_pressed(egui::Key::Escape))
                || (ctx.input(|i| i.pointer.any_pressed()) && !popup.response.contains_pointer())
            {
                self.menu = None;
            }
        }
        if let Some((name, at)) = self.group_menu.clone() {
            let members: Vec<usize> = (0..self.projects.len())
                .filter(|&i| self.settings.group_of(&self.projects[i].id) == Some(name.as_str()))
                .collect();
            let mut chosen = None;
            let popup = egui::Area::new(egui::Id::new("group menu"))
                .fixed_pos(at)
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    classic::menu_frame(self.dark).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for &i in &members {
                            let current = i == self.project;
                            if classic::menu_item(ui, &self.projects[i].name, current).clicked() {
                                chosen = Some(i);
                            }
                        }
                    });
                });
            if let Some(i) = chosen {
                self.project = i;
                self.selection = None;
            }
            // The tab's own click toggles the menu; any other click closes it.
            let on_tab = ctx.pointer_interact_pos().is_some_and(|p| {
                egui::Rect::from_min_max(at - Vec2::new(0.0, 22.0), at + Vec2::new(400.0, 0.0))
                    .contains(p)
            });
            if members.is_empty()
                || chosen.is_some()
                || ctx.input(|i| i.key_pressed(egui::Key::Escape))
                || (ctx.input(|i| i.pointer.any_pressed())
                    && !popup.response.contains_pointer()
                    && !on_tab)
            {
                self.group_menu = None;
            }
        }
        // Remember which project each group's tab shows.
        if let Some(project) = self.projects.get(self.project)
            && let Some(group) = self.settings.group_of(&project.id)
        {
            self.group_shown
                .insert(group.to_string(), project.id.clone());
        }
        // Research records follow the dashboard's selected project tab.
        if let Some(project) = self.projects.get(self.project) {
            self.research.follow_project(&project.id);
        }
        self.research.show(ctx);
        let mut about = self.about;
        classic::Window::new("about", "About Scopework")
            .modal(true)
            .default_size([300.0, 120.0])
            .show(ctx, &mut about, |ui| {
                ui.add_space(6.0);
                classic::label(ui, research_dashboard::build_info::DISPLAY, None);
                classic::label(ui, "A workbench for bounded research projects.", None);
                classic::label(
                    ui,
                    "Research records drive progress. Create a project to begin.",
                    None,
                );
                ui.add_space(6.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if classic::button(ui, true, "OK").clicked() {
                        self.about = false;
                    }
                });
            });
        self.about &= about;
    }
}

/// Required functions by their own effective status: complete, in progress, not started.
fn function_counts(meters: &[research::Meter]) -> [usize; 3] {
    meters.iter().fold([0; 3], |[done, started, open], m| {
        [
            done + m.completed_functions,
            started + m.in_progress_functions,
            open + m.functions - m.completed_functions - m.in_progress_functions,
        ]
    })
}

/// A project tab's width for a label of `label` pixels: the label plus its padding, never
/// narrower than the classic 61-pixel tab.
fn tab_width(label: f32) -> f32 {
    (label + 24.0).max(61.0)
}

fn main() -> eframe::Result {
    // Standard error and panics go to the research home's logs/gui.log (a windowed process
    // would otherwise lose them); without a usable home the GUI runs without the log.
    if let Ok(home) = research_dashboard::paths::home(None) {
        research_dashboard::crash_log::install(&home);
    }
    #[cfg(debug_assertions)]
    if std::env::var_os("SCOPEWORK_GUI_PANIC_TEST").is_some() {
        panic!("deliberate test panic (SCOPEWORK_GUI_PANIC_TEST)");
    }
    let icon_image = image::load_from_memory(include_bytes!("../assets/scopework.png"))
        .expect("bundled icon")
        .into_rgba8();
    let window_icon = egui::IconData {
        rgba: icon_image.as_raw().clone(),
        width: icon_image.width(),
        height: icon_image.height(),
    };

    let result = eframe::run_native(
        "Research Dashboard",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("Scopework")
                .with_icon(window_icon)
                .with_decorations(false)
                .with_inner_size([1100.0, 820.0])
                .with_min_inner_size([1060.0, 820.0]),
            persist_window: false,
            ..Default::default()
        },
        Box::new(move |cc| {
            let mut fonts = egui::FontDefinitions::default();
            if let Ok(bytes) = std::fs::read("C:/Windows/Fonts/tahoma.ttf") {
                fonts
                    .font_data
                    .insert("Tahoma".into(), egui::FontData::from_owned(bytes).into());
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .insert(0, "Tahoma".into());
                cc.egui_ctx.set_fonts(fonts);
            }
            cc.egui_ctx.set_visuals(classic::visuals(false));
            cc.egui_ctx.style_mut(|s| {
                s.spacing.item_spacing = Vec2::new(8.0, 4.0);
            });
            let mut app = Dashboard::default();
            let small_icon = image::load_from_memory(include_bytes!("../assets/scopework-16.png"))
                .expect("bundled titlebar icon")
                .into_rgba8();
            app.brand_icon = Some(cc.egui_ctx.load_texture(
                "scopework",
                egui::ColorImage::from_rgba_unmultiplied(
                    [small_icon.width() as usize, small_icon.height() as usize],
                    small_icon.as_raw(),
                ),
                egui::TextureOptions::LINEAR,
            ));
            if let Some(value) = cc.storage.and_then(|s| s.get_string("dark_mode")) {
                app.dark = value != "false";
            }
            Ok(Box::new(app))
        }),
    );
    if let Err(error) = &result {
        eprintln!(
            "{} the dashboard stopped with an error: {error}",
            research_dashboard::crash_log::timestamp(std::time::SystemTime::now())
        );
    }
    result
}

#[cfg(test)]
mod meter_tests {
    use super::{Tab, function_counts, meter_rows, tab_list, tab_width};
    #[test]
    fn function_totals_count_each_function_by_its_own_status() {
        // One chunk holds a function in progress and one not started: one of each, not two in
        // progress because the chunk is in progress.
        let meter = |functions, completed_functions, in_progress_functions| {
            research_dashboard::research::Meter {
                id: 1,
                name: "A".into(),
                functions,
                completed_functions,
                in_progress_functions,
                states: vec![1],
            }
        };
        assert_eq!(function_counts(&[meter(2, 0, 1)]), [0, 1, 1]);
        assert_eq!(
            function_counts(&[meter(13, 7, 1), meter(6, 0, 0)]),
            [7, 1, 11]
        );
    }
    #[test]
    fn groups_share_one_tab_in_name_order() {
        use research_dashboard::settings::Settings;
        let projects = [
            ("3-3-5a", "3.3.5a"),
            ("forever", "Forever"),
            ("populous-amiga", "Populous (Amiga)"),
            ("populous-snes", "Populous (SNES)"),
            ("wow-335a-12340", "WoW 3.3.5a"),
        ];
        let none = Settings::default();
        assert_eq!(
            tab_list(&projects, &none),
            (0..5).map(Tab::Project).collect::<Vec<_>>()
        );
        let mut grouped = Settings::default();
        for id in ["3-3-5a", "wow-335a-12340"] {
            grouped.assign(id, Some("WoW")).unwrap();
        }
        for id in ["populous-amiga", "populous-snes"] {
            grouped.assign(id, Some("Populous")).unwrap();
        }
        assert_eq!(
            tab_list(&projects, &grouped),
            vec![
                Tab::Project(1),
                Tab::Group {
                    name: "Populous".into(),
                    members: vec![2, 3]
                },
                Tab::Group {
                    name: "WoW".into(),
                    members: vec![0, 4]
                },
            ]
        );
    }

    #[test]
    fn project_tabs_fit_their_names() {
        // Short names keep the classic tab; long ones widen it instead of being overdrawn.
        assert_eq!(tab_width(20.0), 61.0);
        assert_eq!(tab_width(70.0), 94.0);
        assert!(tab_width(70.0) >= 70.0 + 12.0 * 2.0);
    }
    #[test]
    fn packs_to_exact_viewport_then_wraps() {
        let (positions, width, rows) = meter_rows(&[100.0; 5], 436.0, 12.0);
        assert_eq!(
            positions,
            vec![(0.0, 0), (112.0, 0), (224.0, 0), (336.0, 0), (0.0, 1)]
        );
        assert_eq!((width, rows), (436.0, 2));
        assert_eq!(meter_rows(&[100.0; 5], 435.0, 12.0).0[3], (0.0, 1));
    }
    #[test]
    fn oversized_pair_does_not_expand_other_rows() {
        let (positions, width, rows) =
            meter_rows(&[400.0, 400.0, 100.0, 100.0, 100.0, 100.0], 350.0, 12.0);
        assert_eq!((width, rows), (812.0, 3));
        assert_eq!(positions[2], (0.0, 1));
        assert_eq!(positions[5], (0.0, 2));
    }
    #[test]
    fn empty_single_and_resize() {
        assert_eq!(meter_rows(&[], 300.0, 12.0), (vec![], 0.0, 0));
        assert_eq!(
            meter_rows(&[500.0], 300.0, 12.0),
            (vec![(0.0, 0)], 500.0, 1)
        );
        assert_eq!(meter_rows(&[100.0; 6], 212.0, 12.0).2, 3);
        assert_eq!(meter_rows(&[100.0; 6], 660.0, 12.0).2, 1);
    }
}

#[cfg(test)]
mod group_tab_tests {
    use super::*;

    fn press(at: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        }
    }

    /// Runs one dashboard frame with `events`.
    fn step(ctx: &egui::Context, dash: &mut Dashboard, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 820.0),
            )),
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| dash.frame(ctx));
    }

    /// The dashboard reads the research home from the environment, which is shared by the whole
    /// test process: tests that set it hold this lock.
    static HOME: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// A new empty research home for one test, made the process's home.
    fn fixture_home(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let home = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-workspaces")
            .join(format!("{name}-{}", uuid::Uuid::new_v4()));
        let root = home.join("projects");
        std::fs::create_dir_all(&root).unwrap();
        // SAFETY: tests that read or set the variable hold `HOME`, so no other thread of this
        // binary reads the environment concurrently.
        unsafe { std::env::set_var("RESEARCH_DASHBOARD_HOME", &home) };
        (home, root)
    }

    /// Runs frames until `ready` holds (30 s at most).
    fn settle(ctx: &egui::Context, dash: &mut Dashboard, ready: impl Fn(&Dashboard) -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !ready(dash) {
            assert!(
                std::time::Instant::now() < deadline,
                "the dashboard never settled"
            );
            step(ctx, dash, vec![]);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    /// The width of `label` in the dashboard's tab font.
    fn label_width(ctx: &egui::Context, label: &str) -> f32 {
        ctx.fonts_mut(|f| {
            f.layout_no_wrap(
                label.to_string(),
                egui::FontId::proportional(11.0),
                Color32::WHITE,
            )
            .size()
            .x
        })
    }

    #[test]
    fn the_implementation_view_shows_the_trackers_counters() {
        let _home = HOME.lock().unwrap_or_else(|e| e.into_inner());
        let (home, root) = fixture_home("dashboard-tracker");
        let mut lib = research::Library::open(root.clone()).unwrap();
        lib.create("alpha", "Alpha", "synthetic").unwrap();
        drop(lib);
        let mut lib = research::Library::open_project(root.clone(), Some("alpha")).unwrap();
        let arch = lib.add(0, 0, "Arch", "").unwrap();
        let chunk = lib.add(0, arch, "Chunk", "").unwrap();
        let first = lib.add(0, chunk, "Alloc", "1000").unwrap();
        lib.add(0, chunk, "Free", "2000").unwrap();
        let krate = lib.impl_add(0, None, "alpha_rs").unwrap();
        lib.impl_place(0, krate, &[first]).unwrap();
        lib.impl_set_complete(0, &[first], &[], true).unwrap();
        drop(lib);

        let ctx = egui::Context::default();
        let mut dash = Dashboard::default();
        settle(&ctx, &mut dash, |d| !d.projects.is_empty());
        assert_eq!(dash.inner, InnerView::Research);
        // The Implementation tab is the last one in the row, at its right end.
        let width = tab_width(label_width(&ctx, "Implementation"));
        click(
            &ctx,
            &mut dash,
            egui::pos2(4.0 + 1100.0 - 8.0 - 5.0 - width / 2.0, 4.0 + 54.0),
        );
        assert_eq!(dash.inner, InnerView::Implementation);
        settle(&ctx, &mut dash, |d| d.tracker.is_some());
        let (project, view) = dash.tracker.clone().unwrap();
        assert_eq!(project, "alpha");
        assert_eq!(counter(&view["totals"]["research"]), "1 / 1");
        assert_eq!(view["not_placed"], 1);
        let rows = tracker_rows(&view);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1["name"], "alpha_rs");
        let _ = std::fs::remove_dir_all(&home);
    }

    /// Clicks at `at`: pointer moves there, presses, releases (each in its own frame).
    fn click(ctx: &egui::Context, dash: &mut Dashboard, at: egui::Pos2) {
        step(ctx, dash, vec![egui::Event::PointerMoved(at)]);
        step(ctx, dash, vec![press(at, true)]);
        step(ctx, dash, vec![press(at, false)]);
    }

    #[test]
    fn a_group_tab_selects_its_project_and_opens_its_menu() {
        // An isolated home with three projects, two of them grouped.
        let _home = HOME.lock().unwrap_or_else(|e| e.into_inner());
        let (home, root) = fixture_home("dashboard-groups");
        let mut lib = research::Library::open(root.clone()).unwrap();
        for (id, name) in [("alpha", "Alpha"), ("demo", "Demo"), ("demo2", "Demo two")] {
            lib.create(id, name, "").unwrap();
        }
        drop(lib);
        research_dashboard::settings::update(&home, |s| {
            s.assign("demo", Some("Demos"))?;
            s.assign("demo2", Some("Demos"))
        })
        .unwrap();

        let ctx = egui::Context::default();
        let mut dash = Dashboard::default();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while dash.projects.len() < 3 || dash.settings.groups.is_empty() {
            assert!(
                std::time::Instant::now() < deadline,
                "the snapshot never arrived"
            );
            step(&ctx, &mut dash, vec![]);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let names: Vec<&str> = dash.projects.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Alpha", "Demo", "Demo two"]);
        assert_eq!(dash.project, 0);

        // The group's tab follows Alpha's: just inside its left edge.
        let alpha = dash.projects[0].name.clone();
        let width = ctx.fonts_mut(|f| {
            f.layout_no_wrap(alpha, egui::FontId::proportional(11.0), Color32::WHITE)
                .size()
                .x
        });
        let group_tab = egui::pos2(4.0 + 5.0 + tab_width(width) + 8.0, 4.0 + 54.0);
        click(&ctx, &mut dash, group_tab);
        assert_eq!(dash.projects[dash.project].id, "demo");
        assert!(dash.group_menu.is_none(), "the first click only selects");
        click(&ctx, &mut dash, group_tab);
        assert_eq!(
            dash.group_menu.as_ref().map(|(name, _)| name.as_str()),
            Some("Demos")
        );
        // Its two items stack evenly inside the menu: pick the second.
        let menu = ctx
            .memory(|m| m.area_rect(egui::Id::new("group menu")))
            .expect("the menu is drawn");
        click(
            &ctx,
            &mut dash,
            egui::pos2(menu.center().x, menu.top() + menu.height() * 0.75),
        );
        assert_eq!(dash.projects[dash.project].id, "demo2");
        assert!(dash.group_menu.is_none(), "choosing closes the menu");
        // The group remembers its project; reopen and close with Escape.
        click(&ctx, &mut dash, group_tab);
        assert!(dash.group_menu.is_some());
        step(
            &ctx,
            &mut dash,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        assert!(dash.group_menu.is_none());
        let _ = std::fs::remove_dir_all(&home);
    }
}
