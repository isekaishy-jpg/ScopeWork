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

impl eframe::App for Dashboard {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        let colors = [GREEN, RED, GREY];
        let states = ["Accepted", "In progress", "Unreviewed"];
        let scale = self.project + 1;
        let data = rows();
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (i, name) in ["Stock", "Forever"].iter().enumerate() {
                    if ui
                        .add_sized(
                            [90.0, 25.0],
                            egui::Button::new(*name).selected(self.project == i),
                        )
                        .clicked()
                    {
                        self.project = i;
                        self.selection = None;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new("SAMPLE DATA")
                            .size(11.0)
                            .color(Color32::from_gray(85)),
                    );
                });
            });
            ui.separator();
            egui::Frame::new()
                .fill(Color32::from_rgb(0, 0, 128))
                .inner_margin(6.0)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(
                        RichText::new(format!("{} — Research", ["Stock", "Forever"][self.project]))
                            .strong()
                            .color(Color32::WHITE),
                    );
                });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add_sized(
                    [110.0, 18.0],
                    egui::Label::new(RichText::new("Category").strong()),
                );
                ui.label("Function groups");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label("Accepted / Total");
                });
            });
            ui.separator();
            let mut accepted = 0;
            let mut total = 0;
            for (row_index, row) in data.iter().enumerate() {
                let count: usize = row.groups.iter().map(|g| g.1 * scale).sum();
                let done: usize = row
                    .groups
                    .iter()
                    .filter(|g| g.2 == 0)
                    .map(|g| g.1 * scale)
                    .sum();
                accepted += done;
                total += count;
                ui.horizontal(|ui| {
                    ui.add_sized([110.0, 32.0], egui::Label::new(row.name));
                    let width = (ui.available_width() - 124.0).max(80.0);
                    let (rect, _) =
                        ui.allocate_exact_size(Vec2::new(width, 27.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 0.0, Color32::BLACK);
                    let inner = rect.shrink(3.0);
                    let usable = inner.width() - 9.0;
                    let mut x = inner.left();
                    for (group_index, &(name, n, status)) in row.groups.iter().enumerate() {
                        let w = usable * (n * scale) as f32 / count as f32;
                        let group_rect = egui::Rect::from_min_size(
                            egui::pos2(x, inner.top()),
                            Vec2::new(w, inner.height()),
                        );
                        let response = ui.interact(
                            group_rect,
                            ui.id().with((row_index, group_index)),
                            egui::Sense::click(),
                        );
                        // Small vertical blocks give the bars their classic resource-meter appearance.
                        let blocks = (w / 7.0).floor().max(1.0) as usize;
                        let block_width = w / blocks as f32;
                        for b in 0..blocks {
                            let block = egui::Rect::from_min_size(
                                egui::pos2(x + b as f32 * block_width, inner.top()),
                                Vec2::new((block_width - 2.0).max(1.0), inner.height()),
                            );
                            ui.painter().rect_filled(block, 0.0, colors[status]);
                        }
                        if self.selection == Some((row_index, group_index)) {
                            ui.painter().rect_stroke(
                                group_rect,
                                0.0,
                                egui::Stroke::new(1.0, Color32::WHITE),
                                egui::StrokeKind::Inside,
                            );
                        }
                        if response.clicked() {
                            self.selection = Some((row_index, group_index));
                        }
                        response.on_hover_text(format!(
                            "{name}: {} functions\n{} (sample)",
                            n * scale,
                            states[status]
                        ));
                        x += w + 3.0;
                    }
                    ui.add_sized(
                        [112.0, 27.0],
                        egui::Label::new(
                            RichText::new(format!("{done:>4} / {count:<4}")).monospace(),
                        ),
                    );
                });
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.strong("Total");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.monospace(format!("{accepted} / {total} functions"));
                });
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                for (i, name) in states.iter().enumerate() {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(10.0), egui::Sense::hover());
                    ui.painter().rect_filled(r, 0.0, colors[i]);
                    ui.label(*name);
                    ui.add_space(10.0);
                }
            });
            ui.add_space(8.0);
            egui::Frame::new()
                .stroke(egui::Stroke::new(1.0, Color32::from_gray(128)))
                .inner_margin(6.0)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if let Some((r, g)) = self.selection {
                        let (name, n, status) = data[r].groups[g];
                        ui.label(format!(
                            "{} / {name} · {} functions · {}",
                            data[r].name,
                            n * scale,
                            states[status]
                        ));
                    } else {
                        ui.label("Click a group to inspect it.");
                    }
                    ui.label(
                        RichText::new(
                            "Illustrative counts only. Deliverables and real inventory come next.",
                        )
                        .size(11.0),
                    );
                });
        });
    }
}

fn main() -> eframe::Result {
    eframe::run_native(
        "Research Dashboard",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([760.0, 560.0])
                .with_min_inner_size([650.0, 560.0]),
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
