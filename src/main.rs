#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui::{self, Color32, Vec2};

const GREEN: Color32 = Color32::from_rgb(0, 210, 48);
const RED: Color32 = Color32::from_rgb(228, 48, 40);
const GREY: Color32 = Color32::from_rgb(82, 82, 82);

struct Row {
    name: &'static str,
    groups: [(&'static str, usize, usize, usize); 4],
}

fn rows() -> [Row; 6] {
    [
        Row {
            name: "Rendering",
            groups: [
                ("Frame", 42, 0, 42),
                ("Views", 68, 0, 68),
                ("Passes", 95, 1, 37),
                ("Present", 24, 2, 0),
            ],
        },
        Row {
            name: "Assets",
            groups: [
                ("Requests", 53, 0, 53),
                ("Dependencies", 84, 1, 51),
                ("Publication", 37, 2, 0),
                ("Lifetime", 65, 2, 0),
            ],
        },
        Row {
            name: "World",
            groups: [
                ("Maps", 31, 0, 31),
                ("Spatial data", 102, 1, 26),
                ("Visibility", 73, 2, 0),
                ("Lifetime", 46, 2, 0),
            ],
        },
        Row {
            name: "Animation",
            groups: [
                ("States", 28, 0, 28),
                ("Poses", 64, 1, 41),
                ("Blending", 51, 2, 0),
                ("Integration", 39, 2, 0),
            ],
        },
        Row {
            name: "Interface",
            groups: [
                ("Input", 47, 0, 47),
                ("Widgets", 88, 1, 19),
                ("Layout", 56, 2, 0),
                ("Drawing", 71, 2, 0),
            ],
        },
        Row {
            name: "Unclassified",
            groups: [
                ("Region A", 210, 2, 0),
                ("Region B", 190, 2, 0),
                ("Region C", 160, 2, 0),
                ("Region D", 140, 2, 0),
            ],
        },
    ]
}

struct Dashboard {
    project: usize,
    selection: Option<(usize, usize)>,
    menu: Option<usize>,
    on_top: bool,
    about: bool,
    dark: bool,
}

impl Default for Dashboard {
    fn default() -> Self {
        Self {
            project: 0,
            selection: None,
            menu: None,
            on_top: false,
            about: false,
            dark: true,
        }
    }
}
fn background(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(27, 30, 36)
    } else {
        BG
    }
}
fn foreground(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(221, 226, 234)
    } else {
        Color32::BLACK
    }
}

fn bevel(p: &egui::Painter, r: egui::Rect, inset: bool) {
    let dark_mode = p.ctx().style().visuals.dark_mode;
    let highlight = if dark_mode {
        Color32::from_rgb(58, 64, 75)
    } else {
        Color32::WHITE
    };
    let shadow = if dark_mode {
        Color32::from_rgb(15, 18, 23)
    } else {
        Color32::from_gray(110)
    };
    let (light, dark) = if inset {
        (shadow, highlight)
    } else {
        (highlight, shadow)
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
const BG: Color32 = Color32::from_rgb(212, 208, 200);
fn group_box(p: &egui::Painter, r: egui::Rect, title: &str) {
    bevel(p, r, true);
    let galley = p.layout_no_wrap(
        title.into(),
        egui::FontId::proportional(11.0),
        foreground(p.ctx().style().visuals.dark_mode),
    );
    p.rect_filled(
        egui::Rect::from_min_size(
            r.min + Vec2::new(7.0, -6.0),
            galley.size() + Vec2::new(6.0, 0.0),
        ),
        0.0,
        background(p.ctx().style().visuals.dark_mode),
    );
    p.galley(
        r.min + Vec2::new(10.0, -6.0),
        galley,
        foreground(p.ctx().style().visuals.dark_mode),
    );
}
// CPU Usage meter from the reference, rotated clockwise: thin vertical
// segments fill left-to-right, with the unlit lattice remaining visible.
fn led(p: &egui::Painter, r: egui::Rect, complete: usize, total: usize, color: Color32) {
    p.rect_filled(r, 0.0, Color32::BLACK);
    bevel(p, r, true);
    let inner = r.shrink(3.0);
    let cells = (inner.width() / 3.0).floor().max(1.0) as usize;
    let fraction = if total == 0 {
        0.0
    } else {
        complete.min(total) as f32 / total as f32
    };
    let lit = (fraction * cells as f32).floor() as usize;
    let dark = Color32::from_rgb(color.r() / 6, color.g() / 6, color.b() / 6);
    for i in 0..cells {
        // A black horizontal seam separates the two banks of the classic meter.
        let cell_color = if i < lit { color } else { dark };
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

impl eframe::App for Dashboard {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string("dark_mode", self.dark.to_string());
    }
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if ctx.style().visuals.dark_mode != self.dark {
            let mut visuals = if self.dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            };
            visuals.override_text_color = Some(foreground(self.dark));
            visuals.panel_fill = background(self.dark);
            for widget in [
                &mut visuals.widgets.noninteractive,
                &mut visuals.widgets.inactive,
                &mut visuals.widgets.hovered,
                &mut visuals.widgets.active,
                &mut visuals.widgets.open,
            ] {
                widget.corner_radius = egui::CornerRadius::ZERO;
            }
            ctx.set_visuals(visuals);
        }
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
                let icon = r(3.0, 3.0, 14.0, 14.0);
                p.rect_filled(icon, 0.0, bg);
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
                    p.rect_filled(b, 0.0, bg);
                    bevel(&p, b, false);
                    match i {
                        0 => {
                            p.line_segment(
                                [b.min + Vec2::new(4.0, 10.0), b.min + Vec2::new(11.0, 10.0)],
                                egui::Stroke::new(2.0, ink),
                            );
                        }
                        1 => {
                            p.rect_stroke(
                                b.shrink(3.0),
                                0.0,
                                egui::Stroke::new(1.0, ink),
                                egui::StrokeKind::Inside,
                            );
                        }
                        _ => {
                            p.line_segment(
                                [b.min + Vec2::new(4.0, 3.0), b.max - Vec2::new(4.0, 3.0)],
                                egui::Stroke::new(1.0, ink),
                            );
                            p.line_segment(
                                [b.min + Vec2::new(4.0, 11.0), b.min + Vec2::new(12.0, 3.0)],
                                egui::Stroke::new(1.0, ink),
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
                for (i, name) in ["Stock", "Forever"].iter().enumerate() {
                    let tab = r(5.0 + i as f32 * 61.0, 43.0, 61.0, 22.0);
                    p.rect_filled(tab, 0.0, bg);
                    bevel(&p, tab, false);
                    if self.project == i {
                        p.line_segment(
                            [tab.left_bottom(), tab.right_bottom()],
                            egui::Stroke::new(2.0, bg),
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
                        counts[0] += g.3 * scale;
                        if g.2 != 0 {
                            counts[g.2] += (g.1 - g.3) * scale;
                        }
                    }
                }
                let total: usize = counts.iter().sum();
                let selected = self.selection.unwrap_or((0, 2));
                let (name, n, status, completed) = data[selected.0].groups[selected.1];
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
                    counts[0],
                    total,
                    GREEN,
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
                    completed,
                    n,
                    [GREEN, RED, GREY][status],
                );
                text(
                    &p,
                    well.left() + 7.0,
                    well.top() + 164.0,
                    format!("{} / {} funcs", completed * scale, n * scale),
                    GREEN,
                );
                let bank = r(149.0, top, w - 164.0, meter_h);
                group_box(&p, bank, "Function groups");
                // Four named chunks per architecture row, with readable meters and counts.
                let row_height = (bank.height() - 28.0) / 6.0;
                let col_width = (bank.width() - 46.0) / 4.0;
                for ri in 0..6 {
                    for gi in 0..4 {
                        let index = ri * 4 + gi;
                        let col = index % 4;
                        let slot = index / 4;
                        let (chunk, num, state, finished) = data[ri].groups[gi];
                        let x = bank.left() + 8.0 + col as f32 * (col_width + 10.0);
                        let y = bank.top() + 16.0 + slot as f32 * row_height;
                        let meter = egui::Rect::from_min_size(
                            egui::pos2(x, y + 15.0),
                            Vec2::new(col_width, 32.0),
                        );
                        led(&p, meter, finished, num, [GREEN, RED, GREY][state]);
                        text(
                            &p,
                            x,
                            y + 5.0,
                            format!("{} / {chunk}", data[ri].name),
                            Color32::BLACK,
                        );
                        text(
                            &p,
                            x,
                            meter.bottom() + 10.0,
                            format!("{} / {} funcs done", finished * scale, num * scale),
                            Color32::BLACK,
                        );
                        text(
                            &p,
                            x,
                            meter.bottom() + 24.0,
                            ["Done", "In progress", "Not done"][state],
                            Color32::from_gray(65),
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
                                    1.0,
                                    if self.dark {
                                        Color32::from_rgb(125, 170, 239)
                                    } else {
                                        Color32::from_rgb(0, 0, 128)
                                    },
                                ),
                                egui::StrokeKind::Outside,
                            );
                        }
                        hit.on_hover_text(format!(
                            "{} / {chunk}: {} / {} functions done - {}",
                            data[ri].name,
                            finished * scale,
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
                    (
                        "Done / total",
                        format!("{} / {}", completed * scale, n * scale),
                    ),
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
            let mut toggle_theme = false;
            let popup = egui::Area::new(egui::Id::new("classic menu"))
                .fixed_pos(egui::pos2(x, 44.0))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    egui::Frame::new()
                        .fill(bg)
                        .stroke(egui::Stroke::new(1.0, Color32::from_gray(100)))
                        .inner_margin(3.0)
                        .show(ui, |ui| {
                            if menu == 1
                                && ui
                                    .add_sized(
                                        [185.0, 23.0],
                                        egui::Button::new(if self.dark {
                                            "✓ Dark mode"
                                        } else {
                                            "Dark mode"
                                        })
                                        .frame(false),
                                    )
                                    .clicked()
                            {
                                toggle_theme = true;
                            }
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
            if toggle_theme {
                self.dark = !self.dark;
                self.menu = None;
                ctx.request_repaint();
            } else if action {
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
                .with_inner_size([1100.0, 820.0])
                .with_min_inner_size([1060.0, 820.0]),
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
            let mut app = Dashboard::default();
            if let Some(value) = cc.storage.and_then(|s| s.get_string("dark_mode")) {
                app.dark = value != "false";
            }
            Ok(Box::new(app))
        }),
    )
}
