#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui::{self, Color32, RichText, Vec2};

const GREEN: Color32 = Color32::from_rgb(0, 210, 48);
const RED: Color32 = Color32::from_rgb(228, 48, 40);
const GREY: Color32 = Color32::from_rgb(82, 82, 82);

struct Row {
    name: &'static str,
    groups: [(&'static str, usize, usize); 4],
}

fn rows() -> [Row; 6] {
    [
        Row {
            name: "Rendering",
            groups: [
                ("Frame", 42, 0),
                ("Views", 68, 0),
                ("Passes", 95, 1),
                ("Present", 24, 2),
            ],
        },
        Row {
            name: "Assets",
            groups: [
                ("Requests", 53, 0),
                ("Dependencies", 84, 1),
                ("Publication", 37, 2),
                ("Lifetime", 65, 2),
            ],
        },
        Row {
            name: "World",
            groups: [
                ("Maps", 31, 0),
                ("Spatial data", 102, 1),
                ("Visibility", 73, 2),
                ("Lifetime", 46, 2),
            ],
        },
        Row {
            name: "Animation",
            groups: [
                ("States", 28, 0),
                ("Poses", 64, 1),
                ("Blending", 51, 2),
                ("Integration", 39, 2),
            ],
        },
        Row {
            name: "Interface",
            groups: [
                ("Input", 47, 0),
                ("Widgets", 88, 1),
                ("Layout", 56, 2),
                ("Drawing", 71, 2),
            ],
        },
        Row {
            name: "Unclassified",
            groups: [
                ("Region A", 210, 2),
                ("Region B", 190, 2),
                ("Region C", 160, 2),
                ("Region D", 140, 2),
            ],
        },
    ]
}

#[derive(Default)]
struct Dashboard {
    project: usize,
    selection: Option<(usize, usize)>,
}

fn meter(
    ui: &mut egui::Ui,
    row: &Row,
    row_index: usize,
    scale: usize,
    selection: &mut Option<(usize, usize)>,
) {
    let colors = [GREEN, RED, GREY];
    let states = ["Done", "In progress", "Not done"];
    let total: usize = row.groups.iter().map(|g| g.1 * scale).sum();
    let done: usize = row
        .groups
        .iter()
        .filter(|g| g.2 == 0)
        .map(|g| g.1 * scale)
        .sum();
    ui.strong(row.name);
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 36.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, Color32::BLACK);
    ui.painter().line_segment(
        [rect.left_top(), rect.right_top()],
        egui::Stroke::new(2.0, Color32::from_gray(100)),
    );
    ui.painter().line_segment(
        [rect.left_top(), rect.left_bottom()],
        egui::Stroke::new(2.0, Color32::from_gray(100)),
    );
    ui.painter().line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        egui::Stroke::new(1.0, Color32::WHITE),
    );
    ui.painter().line_segment(
        [rect.right_top(), rect.right_bottom()],
        egui::Stroke::new(1.0, Color32::WHITE),
    );

    let inner = rect.shrink(3.0);
    let mut x = inner.left();
    for (g, &(name, count, status)) in row.groups.iter().enumerate() {
        let width = (inner.width() - 9.0) * (count * scale) as f32 / total as f32;
        let r =
            egui::Rect::from_min_size(egui::pos2(x, inner.top()), Vec2::new(width, inner.height()));
        let blocks = (width / 6.0).floor().max(1.0) as usize;
        let step = width / blocks as f32;
        for block in 0..blocks {
            for line in 0..4 {
                let cell = egui::Rect::from_min_size(
                    egui::pos2(x + block as f32 * step, inner.top() + line as f32 * 8.0),
                    Vec2::new((step - 1.5).max(0.5), 6.0),
                );
                ui.painter().rect_filled(cell, 0.0, colors[status]);
            }
        }
        let response = ui.interact(r, ui.id().with((row_index, g)), egui::Sense::click());
        if response.clicked() {
            *selection = Some((row_index, g));
        }
        if *selection == Some((row_index, g)) {
            ui.painter().rect_stroke(
                r,
                0.0,
                egui::Stroke::new(1.0, Color32::WHITE),
                egui::StrokeKind::Inside,
            );
        }
        response.on_hover_text(format!(
            "{name} · {} functions · {}",
            count * scale,
            states[status]
        ));
        x += width + 3.0;
    }
    ui.horizontal(|ui| {
        ui.monospace(format!("Total: {total}"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.monospace(format!("Done: {done}"));
        });
    });
    let current = row.groups.iter().find(|g| g.2 == 1);
    ui.label(match current {
        Some((name, count, _)) => format!("Current chunk: {name} · {} funcs", count * scale),
        None => "Current chunk: —".to_owned(),
    });
}

impl eframe::App for Dashboard {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        let data = rows();
        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (i, name) in ["Stock", "Forever"].iter().enumerate() {
                    if ui
                        .add_sized(
                            [80.0, 24.0],
                            egui::Button::new(*name).selected(self.project == i),
                        )
                        .clicked()
                    {
                        self.project = i;
                        self.selection = None;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("Sample data").size(11.0));
                });
            });
        });
        let scale = self.project + 1;
        let total: usize = data
            .iter()
            .flat_map(|r| r.groups.iter())
            .map(|g| g.1 * scale)
            .sum();
        let done: usize = data
            .iter()
            .flat_map(|r| r.groups.iter())
            .filter(|g| g.2 == 0)
            .map(|g| g.1 * scale)
            .sum();
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.monospace(format!("Total functions: {total}    Done: {done}"));
                ui.separator();
                for (color, name) in [(GREEN, "Done"), (RED, "In progress"), (GREY, "Not done")] {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(9.0), egui::Sense::hover());
                    ui.painter().rect_filled(r, 0.0, color);
                    ui.label(name);
                }
            });
            if let Some((r, g)) = self.selection {
                let (name, count, state) = data[r].groups[g];
                ui.label(format!(
                    "{} / {name}: {} functions · {}",
                    data[r].name,
                    count * scale,
                    ["Done", "In progress", "Not done"][state]
                ));
            } else {
                ui.label("Hover or click a chunk for its function count.");
            }
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            // Add meters without adding panels; wrap to the available page width.
            let columns = (ui.available_width() / 250.0).floor().max(1.0) as usize;
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (row_number, chunk) in data.chunks(columns).enumerate() {
                    ui.columns(columns, |cols| {
                        for (column, row) in chunk.iter().enumerate() {
                            let ui = &mut cols[column];
                            egui::Frame::new()
                                .stroke(egui::Stroke::new(1.0, Color32::from_gray(128)))
                                .inner_margin(7.0)
                                .show(ui, |ui| {
                                    meter(
                                        ui,
                                        row,
                                        row_number * columns + column,
                                        scale,
                                        &mut self.selection,
                                    );
                                });
                        }
                    });
                    ui.add_space(5.0);
                }
            });
        });
    }
}

fn main() -> eframe::Result {
    eframe::run_native(
        "Research Dashboard",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([840.0, 380.0])
                .with_min_inner_size([550.0, 380.0]),
            persist_window: false,
            ..Default::default()
        },
        Box::new(|cc| {
            let mut visuals = egui::Visuals::light();
            visuals.panel_fill = Color32::from_rgb(192, 192, 192);
            visuals.override_text_color = Some(Color32::BLACK);
            visuals.selection.bg_fill = Color32::from_rgb(170, 180, 196);
            for widget in [
                &mut visuals.widgets.noninteractive,
                &mut visuals.widgets.inactive,
                &mut visuals.widgets.hovered,
                &mut visuals.widgets.active,
                &mut visuals.widgets.open,
            ] {
                widget.corner_radius = egui::CornerRadius::ZERO;
            }
            cc.egui_ctx.set_visuals(visuals);
            cc.egui_ctx.style_mut(|s| {
                s.spacing.item_spacing = Vec2::new(8.0, 4.0);
            });
            Ok(Box::new(Dashboard::default()))
        }),
    )
}
