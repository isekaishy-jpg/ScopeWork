#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui::{self, Color32, Vec2};

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

fn bevel(p: &egui::Painter, r: egui::Rect, inset: bool) {
    let (light, dark) = if inset {
        (Color32::from_gray(110), Color32::WHITE)
    } else {
        (Color32::WHITE, Color32::from_gray(110))
    };
    p.line_segment(
        [r.left_bottom(), r.left_top()],
        egui::Stroke::new(1.0, light),
    );
    p.line_segment([r.left_top(), r.right_top()], egui::Stroke::new(1.0, light));
    p.line_segment(
        [r.right_top(), r.right_bottom()],
        egui::Stroke::new(1.0, dark),
    );
    p.line_segment(
        [r.right_bottom(), r.left_bottom()],
        egui::Stroke::new(1.0, dark),
    );
}
fn text(p: &egui::Painter, x: f32, y: f32, s: impl ToString, color: Color32) {
    p.text(
        egui::pos2(x, y),
        egui::Align2::LEFT_CENTER,
        s,
        egui::FontId::proportional(11.0),
        color,
    );
}
impl eframe::App for Dashboard {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(212, 208, 200))
                    .inner_margin(8.0),
            )
            .show(ctx, |ui| {
                let origin = ui.min_rect().min;
                let w = ui.available_width();
                let p = ui.painter().clone();
                let black = Color32::BLACK;
                let bg = Color32::from_rgb(212, 208, 200);
                let at = |x, y| origin + Vec2::new(x, y);
                let rect = |x, y, width, height| {
                    egui::Rect::from_min_size(at(x, y), Vec2::new(width, height))
                };
                for (i, name) in ["Stock", "Forever"].iter().enumerate() {
                    let r = rect(i as f32 * 60.0, 2.0, 60.0, 22.0);
                    p.rect_filled(r, 0.0, bg);
                    bevel(&p, r, false);
                    if i == self.project {
                        p.line_segment(
                            [r.left_bottom(), r.right_bottom()],
                            egui::Stroke::new(2.0, bg),
                        );
                    }
                    text(&p, r.left() + 12.0, r.center().y, *name, black);
                    if ui
                        .interact(r, ui.id().with(i), egui::Sense::click())
                        .clicked()
                    {
                        self.project = i;
                        self.selection = None;
                    }
                }
                text(
                    &p,
                    origin.x + w - 72.0,
                    origin.y + 12.0,
                    "Sample data",
                    Color32::from_gray(85),
                );
                let body = rect(0.0, 25.0, w, 550.0);
                bevel(&p, body, false);
                // One recessed field of closely stacked meters, like the reference's CPU meters rotated 90 degrees.
                let field = rect(10.0, 43.0, w - 20.0, 466.0);
                bevel(&p, field, true);
                p.rect_filled(rect(18.0, 37.0, 90.0, 13.0), 0.0, bg);
                text(
                    &p,
                    origin.x + 22.0,
                    origin.y + 43.0,
                    "Function groups",
                    black,
                );
                text(
                    &p,
                    origin.x + 20.0,
                    origin.y + 61.0,
                    "Architecture / chunk",
                    black,
                );
                text(&p, origin.x + w - 116.0, origin.y + 61.0, "Funcs", black);
                text(&p, origin.x + w - 65.0, origin.y + 61.0, "Total", black);
                let scale = self.project + 1;
                let data = rows();
                let colors = [GREEN, RED, GREY];
                let mut y = 76.0;
                let mut total = 0;
                let mut done = 0;
                for (ri, row) in data.iter().enumerate() {
                    let category_total: usize = row.groups.iter().map(|g| g.1 * scale).sum();
                    total += category_total;
                    for (gi, &(name, n, status)) in row.groups.iter().enumerate() {
                        if status == 0 {
                            done += n * scale;
                        }
                        let r = rect(176.0, y, w - 307.0, 13.0);
                        p.rect_filled(r, 0.0, black);
                        bevel(&p, r, true);
                        let inner = r.shrink(2.0);
                        let cells = (inner.width() / 4.0).floor() as usize;
                        for k in 0..cells {
                            let cell = egui::Rect::from_min_size(
                                egui::pos2(inner.left() + k as f32 * 4.0, inner.top()),
                                Vec2::new(2.5, inner.height()),
                            );
                            p.rect_filled(cell, 0.0, colors[status]);
                        }
                        let label = if gi == 0 {
                            format!("{} / {name}", row.name)
                        } else {
                            format!("    {name}")
                        };
                        text(&p, origin.x + 20.0, origin.y + y + 6.5, label, black);
                        text(
                            &p,
                            origin.x + w - 116.0,
                            origin.y + y + 6.5,
                            format!("{}", n * scale),
                            black,
                        );
                        if gi == 0 {
                            text(
                                &p,
                                origin.x + w - 65.0,
                                origin.y + y + 6.5,
                                category_total.to_string(),
                                black,
                            );
                        }
                        let hit =
                            ui.interact(r, ui.id().with((ri, gi, "meter")), egui::Sense::click());
                        if hit.clicked() {
                            self.selection = Some((ri, gi));
                        }
                        if self.selection == Some((ri, gi)) {
                            p.rect_stroke(
                                r.expand(1.0),
                                0.0,
                                egui::Stroke::new(1.0, Color32::from_rgb(0, 0, 128)),
                                egui::StrokeKind::Outside,
                            );
                        }
                        hit.on_hover_text(format!(
                            "{} / {name}: {} functions - {}",
                            row.name,
                            n * scale,
                            ["Done", "In progress", "Not done"][status]
                        ));
                        y += 17.0;
                    }
                    y += 3.0;
                }
                let selected = self.selection.or_else(|| {
                    data.iter().enumerate().find_map(|(r, row)| {
                        row.groups.iter().position(|g| g.2 == 1).map(|g| (r, g))
                    })
                });
                text(
                    &p,
                    origin.x + 16.0,
                    origin.y + 527.0,
                    format!("Total functions: {total}    Done: {done}"),
                    black,
                );
                if let Some((r, g)) = selected {
                    let (name, count, _) = data[r].groups[g];
                    text(
                        &p,
                        origin.x + 16.0,
                        origin.y + 547.0,
                        format!(
                            "{} chunk: {} / {name}    Functions: {}",
                            if self.selection.is_some() {
                                "Selected"
                            } else {
                                "Current"
                            },
                            data[r].name,
                            count * scale
                        ),
                        black,
                    );
                }
                let mut x = w - 252.0;
                for (c, label) in [(GREEN, "Done"), (RED, "In progress"), (GREY, "Not done")] {
                    p.rect_filled(rect(x, 523.0, 8.0, 8.0), 0.0, c);
                    text(&p, origin.x + x + 12.0, origin.y + 527.0, label, black);
                    x += match label {
                        "In progress" => 94.0,
                        _ => 70.0,
                    };
                }
                ui.allocate_space(Vec2::new(w, 578.0));
            });
    }
}

fn main() -> eframe::Result {
    eframe::run_native(
        "Research Dashboard",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([800.0, 600.0])
                .with_min_inner_size([720.0, 600.0]),
            persist_window: false,
            ..Default::default()
        },
        Box::new(|cc| {
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
