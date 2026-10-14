#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui::{self, Color32, RichText, Vec2};
use serde::{Deserialize, Serialize};

const GREEN: Color32 = Color32::from_rgb(79, 204, 151);
const AMBER: Color32 = Color32::from_rgb(224, 179, 93);
const MUTED: Color32 = Color32::from_rgb(144, 159, 178);
const PANEL: Color32 = Color32::from_rgb(23, 31, 43);

#[derive(Clone, Copy, PartialEq)]
enum Status {
    Accepted,
    Researching,
    Unreviewed,
}
impl Status {
    fn label(self) -> &'static str {
        match self {
            Self::Accepted => "Accepted",
            Self::Researching => "Researching",
            Self::Unreviewed => "Unreviewed",
        }
    }
    fn color(self) -> Color32 {
        match self {
            Self::Accepted => GREEN,
            Self::Researching => AMBER,
            Self::Unreviewed => Color32::from_rgb(66, 80, 99),
        }
    }
}
struct Group {
    name: &'static str,
    count: usize,
    status: Status,
}
struct Item {
    category: &'static str,
    title: &'static str,
    description: &'static str,
    groups: Vec<Group>,
}
impl Item {
    fn count(&self) -> usize {
        self.groups.iter().map(|g| g.count).sum()
    }
    fn accepted(&self) -> usize {
        self.groups
            .iter()
            .filter(|g| g.status == Status::Accepted)
            .map(|g| g.count)
            .sum()
    }
}
fn sample_items(project: usize) -> Vec<Item> {
    let scale = if project == 0 { 1 } else { 2 };
    [
        (
            "Rendering",
            "Frame architecture",
            "Understand how a frame moves from scene preparation to presentation.",
            [
                "Frame orchestration",
                "View preparation",
                "Pass submission",
                "Presentation",
            ],
            [42, 68, 95, 24],
        ),
        (
            "Assets",
            "Asset ownership and loading",
            "Understand requests, dependencies, publication, and resource lifetime.",
            [
                "Request identity",
                "Dependency traversal",
                "Publication",
                "Release and eviction",
            ],
            [53, 84, 37, 65],
        ),
        (
            "World",
            "World representation",
            "Understand spatial organization and the ownership of world data.",
            [
                "Map selection",
                "Spatial hierarchy",
                "Visibility",
                "World lifetime",
            ],
            [31, 102, 73, 46],
        ),
        (
            "Animation",
            "Animation architecture",
            "Understand state evaluation and how animation reaches the scene.",
            [
                "State selection",
                "Pose evaluation",
                "Blending",
                "Scene integration",
            ],
            [28, 64, 51, 39],
        ),
        (
            "Interface",
            "Interface architecture",
            "Understand input routing, UI state, and presentation.",
            ["Input routing", "Widget state", "Layout", "UI drawing"],
            [47, 88, 56, 71],
        ),
        (
            "Unclassified",
            "Unclassified functions",
            "Keep every inventoried function visible until a category is supported by evidence.",
            [
                "Unassigned region A",
                "Unassigned region B",
                "Unassigned region C",
                "Unassigned region D",
            ],
            [210, 190, 160, 140],
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (category, title, description, names, counts))| Item {
        category,
        title,
        description,
        groups: names
            .into_iter()
            .enumerate()
            .map(|(j, name)| Group {
                name,
                count: counts[j] * scale,
                status: if i == 5 {
                    Status::Unreviewed
                } else if j < (if i < 2 { 2 } else { 1 }) {
                    Status::Accepted
                } else if j == 2 {
                    Status::Researching
                } else {
                    Status::Unreviewed
                },
            })
            .collect(),
    })
    .collect()
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Dashboard {
    project: usize,
    demo: bool,
    category: String,
    search: String,
    unfinished_only: bool,
    selected: usize,
    selected_group: Option<usize>,
    notes: [String; 2],
}
impl Default for Dashboard {
    fn default() -> Self {
        Self {
            project: 0,
            demo: true,
            category: "All categories".into(),
            search: String::new(),
            unfinished_only: false,
            selected: 0,
            selected_group: None,
            notes: Default::default(),
        }
    }
}
fn label(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(RichText::new(text).color(MUTED).size(12.0));
}
fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL)
        .corner_radius(10.0)
        .inner_margin(18.0)
}
fn segments(ui: &mut egui::Ui, item: &Item, height: f32) -> Option<usize> {
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let available = (rect.width() - 4.0 * (item.groups.len() - 1) as f32).max(1.0);
    let mut x = rect.left();
    let mut clicked = None;
    for (i, group) in item.groups.iter().enumerate() {
        let width = available * group.count as f32 / item.count() as f32;
        let r = egui::Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(width, height));
        let response = ui.interact(
            r,
            ui.id().with((item.title, i, "segment")),
            egui::Sense::click(),
        );
        ui.painter().rect_filled(r, 3.0, group.status.color());
        if response.clicked() {
            clicked = Some(i);
        }
        response.on_hover_text(format!(
            "{}\n{} functions · {}\nIllustrative sample",
            group.name,
            group.count,
            group.status.label()
        ));
        x += width + 4.0;
    }
    clicked
}
impl eframe::App for Dashboard {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.project = self.project.min(1);
        let items = sample_items(self.project);
        self.selected = self.selected.min(items.len() - 1);
        egui::TopBottomPanel::top("toolbar")
            .frame(egui::Frame::new().fill(PANEL).inner_margin(16.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("◈  RESEARCH / ATLAS").strong().size(17.0));
                    ui.separator();
                    label(ui, "Binary research workspace");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.checkbox(&mut self.demo, "Sample preview");
                    });
                });
            });
        egui::SidePanel::left("navigation").exact_width(220.0).resizable(false).frame(egui::Frame::new().fill(Color32::from_rgb(16, 23, 33)).inner_margin(18.0)).show(ctx, |ui| {
            label(ui, "PROJECTS"); ui.add_space(10.0);
            for (i, name) in ["Stock", "Forever"].iter().enumerate() {
                if ui.add_sized([182.0, 38.0], egui::Button::new(RichText::new(*name).size(16.0)).selected(self.project == i)).clicked() { self.project = i; self.selected_group = None; }
            }
            ui.add_space(28.0); label(ui, "BINARY CATEGORIES"); ui.add_space(8.0);
            for name in ["All categories", "Rendering", "Assets", "World", "Animation", "Interface", "Unclassified"] {
                if ui.selectable_label(self.category == name, name).clicked() { self.category = name.into(); }
            }
            ui.add_space(24.0); ui.separator(); ui.add_space(12.0);
            label(ui, "Categories shown here are illustrative. The binary inventory will define the real categories.");
            ui.add_space(12.0); label(ui, "Project evidence stays separate.");
        });
        egui::SidePanel::right("details").default_width(330.0).width_range(280.0..=460.0).show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(18.0); label(ui, "ARCHITECTURE ITEM"); ui.add_space(8.0);
                if self.demo {
                    let item = &items[self.selected];
                    ui.heading(item.title); ui.add_space(8.0); ui.label(item.description);
                    ui.add_space(18.0); label(ui, "FUNCTION GROUPS · SELECT TO INSPECT");
                    for (i, g) in item.groups.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.colored_label(g.status.color(), "●");
                            if ui.selectable_label(self.selected_group == Some(i), format!("{}  ·  {}", g.name, g.count)).clicked() { self.selected_group = Some(i); }
                        });
                    }
                    ui.add_space(16.0); ui.separator(); ui.add_space(12.0);
                    let group = self.selected_group.and_then(|i| item.groups.get(i));
                    ui.heading("Deliverables");
                    label(ui, group.map_or("Architecture item", |g| g.name));
                    ui.add_space(8.0);
                    for (i, (title, body)) in [
                        ("Behavior and responsibility", "Placeholder for the agreed behavior account and its boundaries."),
                        ("Relationships and lifetime", "Placeholder for ownership, dependencies, state transitions, and failure paths."),
                        ("Evidence and open questions", "Placeholder for exact binary references, evidence links, and unresolved claims."),
                    ].iter().enumerate() {
                        egui::CollapsingHeader::new(*title).id_salt((self.project, self.selected, i)).show(ui, |ui| { ui.label(*body); ui.add_space(6.0); ui.colored_label(AMBER, "Sample only · acceptance criteria not defined"); });
                    }
                    ui.add_space(12.0); label(ui, "These are layout examples, not assigned research requirements.");
                } else { ui.heading("No research item selected"); ui.label("The project is ready for its binary inventory and agreed deliverables."); }
                ui.add_space(24.0); ui.separator(); ui.add_space(12.0); ui.heading("Project notes"); label(ui, "Saved locally · independent for each project"); ui.add_space(8.0);
                ui.add(egui::TextEdit::multiline(&mut self.notes[self.project]).desired_width(f32::INFINITY).desired_rows(8).hint_text("Notes for this project…"));
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(14.0);
                label(ui, "PROJECT OVERVIEW");
                ui.heading(RichText::new(if self.project == 0 { "Stock" } else { "Forever" }).size(32.0));
                label(ui, "Whole-binary understanding / architecture research");
                ui.add_space(18.0);
                if !self.demo {
                    card().show(ui, |ui| { ui.heading("Ready for research data"); ui.label("No inventory has been imported. Function counts and completion are unknown."); ui.add_space(12.0); if ui.button("Explore the sample dashboard").clicked() { self.demo = true; } });
                    return;
                }
                ui.colored_label(AMBER, "SAMPLE PREVIEW — all counts, categories, and completion states are illustrative.");
                ui.add_space(16.0);
                let total: usize = items.iter().map(Item::count).sum();
                let accepted: usize = items.iter().map(Item::accepted).sum();
                ui.columns(3, |columns| {
                    for (ui, (title, value, sub)) in columns.iter_mut().zip([
                        ("INVENTORIED FUNCTIONS", total.to_string(), "Sample inventory"),
                        ("ACCEPTED GROUP COVERAGE", format!("{:.0}%", accepted as f32 / total as f32 * 100.0), "Sample functions in green groups"),
                        ("DELIVERABLE COMPLETION", "Not configured".into(), "Criteria to be agreed"),
                    ]) { card().show(ui, |ui| { label(ui, title); ui.add_space(8.0); ui.label(RichText::new(value).size(24.0).strong()); label(ui, sub); }); }
                });
                ui.add_space(22.0);
                ui.horizontal_wrapped(|ui| {
                    ui.heading("Understanding / architecture");
                    ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search items or function groups…").desired_width(230.0));
                    ui.checkbox(&mut self.unfinished_only, "Has open groups");
                });
                ui.horizontal(|ui| { ui.colored_label(GREEN, "● Accepted"); ui.colored_label(AMBER, "● Researching"); label(ui, "● Unreviewed    ·    Click a segment to inspect its group"); });
                ui.add_space(12.0);
                let query = self.search.to_lowercase();
                let mut shown = 0;
                for (i, item) in items.iter().enumerate() {
                    if self.category != "All categories" && self.category != item.category { continue; }
                    if self.unfinished_only && item.accepted() == item.count() { continue; }
                    if !format!("{} {} {}", item.title, item.category, item.groups.iter().map(|g| g.name).collect::<Vec<_>>().join(" ")).to_lowercase().contains(&query) { continue; }
                    shown += 1;
                    ui.push_id(i, |ui| {
                        card().stroke(egui::Stroke::new(1.0, if self.selected == i { Color32::from_rgb(68, 118, 112) } else { Color32::from_rgb(35, 46, 61) })).show(ui, |ui| {
                            ui.horizontal(|ui| { label(ui, item.category.to_uppercase()); ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { label(ui, format!("{} functions · {} groups", item.count(), item.groups.len())); }); });
                            if ui.add(egui::Button::new(RichText::new(item.title).size(19.0).strong()).frame(false)).clicked() { self.selected = i; self.selected_group = None; }
                            ui.add_space(10.0);
                            if let Some(group) = segments(ui, item, 18.0) { self.selected = i; self.selected_group = Some(group); }
                            ui.add_space(8.0); label(ui, format!("{} / {} functions in accepted groups    ·    Sample deliverables below item details", item.accepted(), item.count()));
                        });
                    });
                    ui.add_space(10.0);
                }
                if shown == 0 { ui.label("No items match these filters."); if ui.button("Clear filters").clicked() { self.search.clear(); self.category = "All categories".into(); self.unfinished_only = false; } }
            });
        });
    }
}

fn main() -> eframe::Result {
    eframe::run_native(
        "Research Atlas",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1440.0, 960.0])
                .with_min_inner_size([1100.0, 720.0]),
            ..Default::default()
        },
        Box::new(|cc| {
            let mut visuals = egui::Visuals::dark();
            visuals.panel_fill = Color32::from_rgb(13, 19, 28);
            visuals.override_text_color = Some(Color32::from_rgb(224, 232, 242));
            visuals.selection.bg_fill = Color32::from_rgb(38, 81, 73);
            cc.egui_ctx.set_visuals(visuals);
            cc.egui_ctx.style_mut(|s| {
                s.spacing.item_spacing = Vec2::new(10.0, 9.0);
                s.spacing.button_padding = Vec2::new(10.0, 7.0);
            });
            Ok(Box::new(
                cc.storage
                    .and_then(|s| eframe::get_value(s, eframe::APP_KEY))
                    .unwrap_or_else(Dashboard::default),
            ))
        }),
    )
}
