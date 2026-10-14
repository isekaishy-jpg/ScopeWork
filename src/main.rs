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
    menu: Option<usize>,
    on_top: bool,
    about: bool,
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
const BG: Color32 = Color32::from_rgb(212, 208, 200);
fn group_box(p: &egui::Painter, r: egui::Rect, title: &str) {
    bevel(p, r, true);
    let galley = p.layout_no_wrap(
        title.into(),
        egui::FontId::proportional(11.0),
        Color32::BLACK,
    );
    p.rect_filled(
        egui::Rect::from_min_size(
            r.min + Vec2::new(7.0, -6.0),
            galley.size() + Vec2::new(6.0, 0.0),
        ),
        0.0,
        BG,
    );
    p.galley(r.min + Vec2::new(10.0, -6.0), galley, Color32::BLACK);
}
fn led(p: &egui::Painter, r: egui::Rect, portions: &[(usize, Color32)]) {
    p.rect_filled(r, 0.0, Color32::BLACK);
    bevel(p, r, true);
    let inner = r.shrink(3.0);
    let count = (inner.width() / 4.0).floor().max(1.0) as usize;
    let total = portions.iter().map(|x| x.0).sum::<usize>().max(1);
    for i in 0..count {
        let fraction = (i as f32 + 0.5) / count as f32;
        let mut boundary = 0;
        let color = portions
            .iter()
            .find_map(|&(n, c)| {
                boundary += n;
                (fraction <= boundary as f32 / total as f32).then_some(c)
            })
            .unwrap_or(GREY);
        p.rect_filled(
            egui::Rect::from_min_size(
                inner.min + Vec2::new(i as f32 * 4.0, 0.0),
                Vec2::new(2.0, inner.height()),
            ),
            0.0,
            color,
        );
    }
}
impl eframe::App for Dashboard {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(4.0))
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
                p.rect_filled(title, 0.0, Color32::from_rgb(0, 0, 128));
                let icon = r(3.0, 3.0, 14.0, 14.0);
                p.rect_filled(icon, 0.0, BG);
                bevel(&p, icon, false);
                p.rect_filled(icon.shrink(3.0), 0.0, Color32::BLACK);
                p.line_segment(
                    [
                        icon.min + Vec2::new(3.0, 9.0),
                        icon.min + Vec2::new(6.0, 5.0),
                    ],
                    egui::Stroke::new(1.0, GREEN),
                );
                text(
                    &p,
                    o.x + 22.0,
                    o.y + 10.0,
                    "Research Task Manager",
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
                    p.rect_filled(b, 0.0, BG);
                    bevel(&p, b, false);
                    match i {
                        0 => {
                            p.line_segment(
                                [b.min + Vec2::new(4.0, 10.0), b.min + Vec2::new(11.0, 10.0)],
                                egui::Stroke::new(2.0, Color32::BLACK),
                            );
                        }
                        1 => {
                            p.rect_stroke(
                                b.shrink(3.0),
                                0.0,
                                egui::Stroke::new(1.0, Color32::BLACK),
                                egui::StrokeKind::Inside,
                            );
                        }
                        _ => {
                            p.line_segment(
                                [b.min + Vec2::new(4.0, 3.0), b.max - Vec2::new(4.0, 3.0)],
                                egui::Stroke::new(1.0, Color32::BLACK),
                            );
                            p.line_segment(
                                [b.min + Vec2::new(4.0, 11.0), b.min + Vec2::new(12.0, 3.0)],
                                egui::Stroke::new(1.0, Color32::BLACK),
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
                        p.rect_filled(mr, 0.0, Color32::from_gray(190));
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
                for (i, name) in ["Stock", "Forever"].iter().enumerate() {
                    let tab = r(5.0 + i as f32 * 61.0, 43.0, 61.0, 22.0);
                    p.rect_filled(tab, 0.0, BG);
                    bevel(&p, tab, false);
                    if self.project == i {
                        p.line_segment(
                            [tab.left_bottom(), tab.right_bottom()],
                            egui::Stroke::new(2.0, BG),
                        );
                    }
                    t(17.0 + i as f32 * 61.0, 54.0, name.to_string());
                    if ui
                        .interact(tab, ui.id().with(("tab", i)), egui::Sense::click())
                        .clicked()
                    {
                        self.project = i;
                        self.selection = None;
                    }
                }
                let data = rows();
                let scale = self.project + 1;
                let mut counts = [0usize; 3];
                for row in &data {
                    for g in row.groups {
                        counts[g.2] += g.1 * scale;
                    }
                }
                let total: usize = counts.iter().sum();
                let selected = self.selection.unwrap_or((0, 2));
                let (name, n, status) = data[selected.0].groups[selected.1];
                let top = 82.0;
                let meter_h = (h - 282.0).max(218.0);
                let left = r(15.0, top, 122.0, meter_h);
                group_box(&p, left, "Function coverage");
                let well = left.shrink2(Vec2::new(10.0, 17.0));
                p.rect_filled(well, 0.0, Color32::BLACK);
                bevel(&p, well, true);
                text(
                    &p,
                    well.left() + 7.0,
                    well.top() + 15.0,
                    "All functions",
                    GREEN,
                );
                led(
                    &p,
                    egui::Rect::from_min_size(
                        well.min + Vec2::new(5.0, 29.0),
                        Vec2::new(well.width() - 10.0, 22.0),
                    ),
                    &[(counts[0], GREEN), (counts[1], RED), (counts[2], GREY)],
                );
                text(
                    &p,
                    well.left() + 7.0,
                    well.top() + 67.0,
                    format!("{}% done", counts[0] * 100 / total),
                    GREEN,
                );
                text(
                    &p,
                    well.left() + 7.0,
                    well.top() + 90.0,
                    format!("Total: {total}"),
                    GREEN,
                );
                text(
                    &p,
                    well.left() + 7.0,
                    well.top() + 112.0,
                    "Current chunk",
                    GREEN,
                );
                led(
                    &p,
                    egui::Rect::from_min_size(
                        well.min + Vec2::new(5.0, 125.0),
                        Vec2::new(well.width() - 10.0, 22.0),
                    ),
                    &[(n, [GREEN, RED, GREY][status])],
                );
                text(
                    &p,
                    well.left() + 7.0,
                    well.top() + 164.0,
                    format!("{} functions", n * scale),
                    GREEN,
                );
                let bank = r(149.0, top, w - 164.0, meter_h);
                group_box(&p, bank, "Function groups");
                // The reference's narrow upright meter bank, rotated into thin horizontal strips.
                let row_height = (bank.height() - 28.0) / 12.0;
                let col_width = (bank.width() - 26.0) / 2.0;
                for ri in 0..6 {
                    for gi in 0..4 {
                        let index = ri * 4 + gi;
                        let col = index / 12;
                        let slot = index % 12;
                        let (chunk, num, state) = data[ri].groups[gi];
                        let x = bank.left() + 8.0 + col as f32 * (col_width + 10.0);
                        let y = bank.top() + 16.0 + slot as f32 * row_height;
                        let meter = egui::Rect::from_min_size(
                            egui::pos2(x, y),
                            Vec2::new(col_width - 32.0, (row_height - 3.0).min(18.0)),
                        );
                        led(&p, meter, &[(num, [GREEN, RED, GREY][state])]);
                        text(
                            &p,
                            meter.right() + 5.0,
                            meter.center().y,
                            (num * scale).to_string(),
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
                                egui::Stroke::new(1.0, Color32::from_rgb(0, 0, 128)),
                                egui::StrokeKind::Outside,
                            );
                        }
                        hit.on_hover_text(format!(
                            "{} / {chunk}: {} functions - {}",
                            data[ri].name,
                            num * scale,
                            ["Done", "In progress", "Not done"][state]
                        ));
                    }
                }
                let bottom = top + meter_h + 18.0;
                let stats = r(15.0, bottom, 220.0, 116.0);
                group_box(&p, stats, "Functions");
                for (i, (label, value)) in [
                    ("Total", total),
                    ("Done", counts[0]),
                    ("In progress", counts[1]),
                    ("Not done", counts[2]),
                ]
                .iter()
                .enumerate()
                {
                    t(26.0, bottom + 19.0 + i as f32 * 22.0, label.to_string());
                    t(165.0, bottom + 19.0 + i as f32 * 22.0, value.to_string());
                }
                let detail = r(247.0, bottom, w - 262.0, 116.0);
                group_box(&p, detail, "Current chunk");
                for (i, (label, value)) in [
                    ("Architecture", data[selected.0].name.to_string()),
                    ("Chunk", name.to_string()),
                    ("Functions", (n * scale).to_string()),
                    (
                        "Status",
                        ["Done", "In progress", "Not done"][status].to_string(),
                    ),
                ]
                .iter()
                .enumerate()
                {
                    t(258.0, bottom + 19.0 + i as f32 * 22.0, label.to_string());
                    t(346.0, bottom + 19.0 + i as f32 * 22.0, value.clone());
                }
                let mut sx = 0.0;
                for (width, label) in [
                    (w * 0.34, format!("Functions: {total}")),
                    (w * 0.33, format!("Done: {}%", counts[0] * 100 / total)),
                    (w * 0.33, "Sample data".to_string()),
                ] {
                    let sr = r(sx, h - 20.0, width - 2.0, 19.0);
                    bevel(&p, sr, true);
                    t(sx + 5.0, h - 10.0, label);
                    sx += width;
                }
                for i in 0..3 {
                    let d = i as f32 * 4.0;
                    p.line_segment(
                        [
                            o + Vec2::new(w - 11.0 + d, h - 3.0),
                            o + Vec2::new(w - 3.0, h - 11.0 + d),
                        ],
                        egui::Stroke::new(1.0, Color32::from_gray(110)),
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
            let mut action = false;
            let popup = egui::Area::new(egui::Id::new("classic menu"))
                .fixed_pos(egui::pos2(x, 44.0))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    egui::Frame::new()
                        .fill(BG)
                        .stroke(egui::Stroke::new(1.0, Color32::from_gray(100)))
                        .inner_margin(3.0)
                        .show(ui, |ui| {
                            let label = match menu {
                                0 => "Exit",
                                1 => {
                                    if self.on_top {
                                        "✓ Always on top"
                                    } else {
                                        "Always on top"
                                    }
                                }
                                2 => "Clear chunk selection",
                                _ => "About Research Task Manager",
                            };
                            if ui
                                .add_sized([185.0, 23.0], egui::Button::new(label).frame(false))
                                .clicked()
                            {
                                action = true;
                            }
                        });
                });
            if action {
                match menu {
                    0 => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                    1 => {
                        self.on_top = !self.on_top;
                        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(if self.on_top {
                            egui::WindowLevel::AlwaysOnTop
                        } else {
                            egui::WindowLevel::Normal
                        }));
                    }
                    2 => self.selection = None,
                    _ => self.about = true,
                }
                self.menu = None;
            } else if ctx.input(|i| i.key_pressed(egui::Key::Escape))
                || (ctx.input(|i| i.pointer.any_pressed()) && !popup.response.contains_pointer())
            {
                self.menu = None;
            }
        }
        if self.about {
            egui::Window::new("About Research Task Manager")
                .collapsible(false)
                .resizable(false)
                .open(&mut self.about)
                .show(ctx, |ui| {
                    ui.label("Research Task Manager 0.1");
                    ui.label("Stock and Forever research dashboard.");
                    ui.label("Displayed counts and states are illustrative sample data.");
                });
        }
    }
}

fn main() -> eframe::Result {
    eframe::run_native(
        "Research Dashboard",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_decorations(false)
                .with_inner_size([740.0, 520.0])
                .with_min_inner_size([640.0, 520.0]),
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
