//! The dashboard's Windows 95 Task Manager look, shared by the main view and every secondary
//! window, dialog and menu: grey faces, square bevelled edges, sunken fields and lists, etched
//! group boxes, a navy title bar and navy selection. Light and dark palettes.
use eframe::egui::{self, Color32, Rect, Response, Sense, Stroke, Ui, Vec2};

pub const LIGHT_FACE: Color32 = Color32::from_rgb(212, 208, 200);
pub const NAVY: Color32 = Color32::from_rgb(0, 0, 128);
const DARK_NAVY: Color32 = Color32::from_rgb(37, 48, 66);
/// Text size of the classic surfaces (Tahoma when installed).
pub const TEXT: f32 = 11.0;

pub fn face(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(27, 30, 36)
    } else {
        LIGHT_FACE
    }
}
pub fn ink(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(221, 226, 234)
    } else {
        Color32::BLACK
    }
}
/// Text of disabled controls.
pub fn grey_ink(dark: bool) -> Color32 {
    if dark {
        Color32::from_gray(110)
    } else {
        Color32::from_gray(128)
    }
}
/// Background of fields and list boxes.
pub fn well(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(15, 18, 23)
    } else {
        Color32::WHITE
    }
}
pub fn title_bar(dark: bool) -> Color32 {
    if dark { DARK_NAVY } else { NAVY }
}
pub fn selection(dark: bool) -> Color32 {
    title_bar(dark)
}
fn highlight(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(58, 64, 75)
    } else {
        Color32::WHITE
    }
}
fn shadow(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(15, 18, 23)
    } else {
        Color32::from_gray(110)
    }
}
/// The inner bevel pair of raised controls.
fn soft(dark: bool) -> (Color32, Color32) {
    if dark {
        (Color32::from_rgb(45, 50, 60), Color32::from_rgb(20, 23, 28))
    } else {
        (Color32::from_rgb(232, 230, 226), Color32::from_gray(150))
    }
}
/// Dark red for errors and warnings on the grey face.
pub fn alert(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(240, 150, 120)
    } else {
        Color32::from_rgb(150, 20, 20)
    }
}

/// egui styling for every surface the classic painters do not draw themselves.
pub fn visuals(dark: bool) -> egui::Visuals {
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    let face = face(dark);
    let ink = ink(dark);
    let edge = if dark {
        Color32::from_gray(90)
    } else {
        Color32::from_gray(100)
    };
    visuals.override_text_color = Some(ink);
    visuals.panel_fill = face;
    visuals.window_fill = face;
    visuals.faint_bg_color = face;
    visuals.extreme_bg_color = well(dark);
    visuals.window_stroke = Stroke::new(1.0_f32, edge);
    visuals.window_corner_radius = egui::CornerRadius::ZERO;
    visuals.menu_corner_radius = egui::CornerRadius::ZERO;
    visuals.window_shadow = egui::Shadow::NONE;
    visuals.popup_shadow = egui::Shadow::NONE;
    visuals.selection.bg_fill = selection(dark);
    visuals.selection.stroke = Stroke::new(1.0_f32, if dark { ink } else { Color32::WHITE });
    for (widget, fill) in [
        (&mut visuals.widgets.noninteractive, face),
        (&mut visuals.widgets.inactive, face),
        (
            &mut visuals.widgets.hovered,
            if dark {
                Color32::from_rgb(45, 50, 60)
            } else {
                Color32::from_rgb(212, 208, 200)
            },
        ),
        (
            &mut visuals.widgets.active,
            if dark {
                Color32::from_rgb(20, 23, 28)
            } else {
                Color32::from_rgb(170, 170, 170)
            },
        ),
        (&mut visuals.widgets.open, face),
    ] {
        widget.corner_radius = egui::CornerRadius::ZERO;
        widget.bg_fill = fill;
        widget.weak_bg_fill = fill;
        widget.bg_stroke = Stroke::new(1.0_f32, edge);
        widget.fg_stroke = Stroke::new(1.0_f32, ink);
        widget.expansion = 0.0;
    }
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, edge);
    visuals
}

/// A one-pixel bevel: raised (light top-left) or inset (light bottom-right).
pub fn bevel(p: &egui::Painter, r: Rect, inset: bool) {
    let dark_mode = p.ctx().style().visuals.dark_mode;
    let (light, dark) = if inset {
        (shadow(dark_mode), highlight(dark_mode))
    } else {
        (highlight(dark_mode), shadow(dark_mode))
    };
    p.line_segment([r.left_bottom(), r.left_top()], Stroke::new(1.0_f32, light));
    p.line_segment([r.left_top(), r.right_top()], Stroke::new(1.0_f32, light));
    p.line_segment(
        [r.right_top(), r.right_bottom()],
        Stroke::new(1.0_f32, dark),
    );
    p.line_segment(
        [r.right_bottom(), r.left_bottom()],
        Stroke::new(1.0_f32, dark),
    );
}
/// A two-pixel bevel for buttons, windows and sunken fields.
fn double_bevel(p: &egui::Painter, r: Rect, inset: bool) {
    let dark_mode = p.ctx().style().visuals.dark_mode;
    bevel(p, r, inset);
    let (a, b) = soft(dark_mode);
    let (light, dark) = if inset { (b, a) } else { (a, b) };
    let r = r.shrink(1.0);
    p.line_segment([r.left_bottom(), r.left_top()], Stroke::new(1.0_f32, light));
    p.line_segment([r.left_top(), r.right_top()], Stroke::new(1.0_f32, light));
    p.line_segment(
        [r.right_top(), r.right_bottom()],
        Stroke::new(1.0_f32, dark),
    );
    p.line_segment(
        [r.right_bottom(), r.left_bottom()],
        Stroke::new(1.0_f32, dark),
    );
}
fn font() -> egui::FontId {
    egui::FontId::proportional(TEXT)
}
/// An etched group box with its title on the frame line.
pub fn group_box(p: &egui::Painter, r: Rect, title: &str) {
    let dark = p.ctx().style().visuals.dark_mode;
    bevel(p, r, true);
    if title.is_empty() {
        return;
    }
    let galley = p.layout_no_wrap(title.into(), font(), ink(dark));
    p.rect_filled(
        Rect::from_min_size(
            r.min + Vec2::new(7.0, -6.0),
            galley.size() + Vec2::new(6.0, 0.0),
        ),
        0.0,
        face(dark),
    );
    p.galley(r.min + Vec2::new(10.0, -6.0), galley, ink(dark));
}

/// Classic text sizes and spacing for the content of a secondary window.
pub fn apply_text_style(ui: &mut Ui) {
    let style = ui.style_mut();
    for (text_style, font) in style.text_styles.iter_mut() {
        font.size = match text_style {
            egui::TextStyle::Monospace => 12.0,
            egui::TextStyle::Heading => 13.0,
            egui::TextStyle::Small => 10.0,
            _ => TEXT,
        };
    }
    style.spacing.item_spacing = Vec2::new(6.0, 5.0);
    style.spacing.interact_size.y = 21.0;
}

/// A raised push button; pressed while held. Disabled buttons show grey text.
pub fn button(ui: &mut Ui, enabled: bool, text: &str) -> Response {
    let enabled = enabled && ui.is_enabled();
    let dark = ui.visuals().dark_mode;
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), font(), ink(dark));
    let size = Vec2::new((galley.size().x + 20.0).max(75.0), 23.0);
    let (rect, response) = ui.allocate_exact_size(
        size,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        p.rect_filled(rect, 0.0, face(dark));
        let pressed = enabled && response.is_pointer_button_down_on();
        double_bevel(p, rect, pressed);
        let offset = if pressed {
            Vec2::splat(1.0)
        } else {
            Vec2::ZERO
        };
        let pos = rect.center() - galley.size() / 2.0 + offset;
        if enabled {
            p.galley(pos, galley, ink(dark));
        } else {
            if !dark {
                let emboss = ui
                    .painter()
                    .layout_no_wrap(text.to_string(), font(), Color32::WHITE);
                p.galley(pos + Vec2::splat(1.0), emboss, Color32::WHITE);
            }
            let grey = ui
                .painter()
                .layout_no_wrap(text.to_string(), font(), grey_ink(dark));
            p.galley(pos, grey, grey_ink(dark));
        }
        if response.has_focus() {
            p.rect_stroke(
                rect.shrink(4.0),
                0.0,
                Stroke::new(1.0_f32, grey_ink(dark)),
                egui::StrokeKind::Inside,
            );
        }
    }
    response
}
/// A small square title-bar button with a drawn ×.
fn close_box(ui: &mut Ui, rect: Rect) -> Response {
    let dark = ui.visuals().dark_mode;
    let response = ui.interact(rect, ui.id().with("close"), Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, 0.0, face(dark));
    double_bevel(p, rect, response.is_pointer_button_down_on());
    let c = rect.center();
    let ink = ink(dark);
    for (a, b) in [((-3.5, -3.0), (3.5, 3.0)), ((-3.5, 3.0), (3.5, -3.0))] {
        p.line_segment(
            [c + Vec2::new(a.0, a.1), c + Vec2::new(b.0, b.1)],
            Stroke::new(1.5_f32, ink),
        );
    }
    response
}

/// Paints a sunken well behind content added by `add` (fields, editors, lists).
pub fn sunken<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let dark = ui.visuals().dark_mode;
    let background = ui.painter().add(egui::Shape::Noop);
    let inner = egui::Frame::new()
        .inner_margin(egui::Margin::same(2))
        .show(ui, add);
    let rect = inner.response.rect;
    ui.painter()
        .set(background, egui::Shape::rect_filled(rect, 0.0, well(dark)));
    double_bevel(ui.painter(), rect, true);
    inner.inner
}
/// A sunken single-line text field.
pub fn field(ui: &mut Ui, text: &mut String, width: f32, hint: &str) -> Response {
    sunken(ui, |ui| {
        let hint = egui::RichText::new(hint).color(grey_ink(ui.visuals().dark_mode));
        ui.add(
            egui::TextEdit::singleline(text)
                .frame(false)
                .hint_text(hint)
                .desired_width(width)
                .margin(egui::Margin::symmetric(3, 2)),
        )
    })
}
/// A sunken multi-line editor in the monospace font.
pub fn editor(ui: &mut Ui, text: &mut String, rows: usize) -> Response {
    sunken(ui, |ui| {
        ui.add(
            egui::TextEdit::multiline(text)
                .frame(false)
                .code_editor()
                .desired_rows(rows)
                .desired_width(f32::INFINITY)
                .margin(egui::Margin::symmetric(3, 2)),
        )
    })
}
/// A square check box with its label.
pub fn check(ui: &mut Ui, enabled: bool, value: &mut bool, text: &str) -> Response {
    let enabled = enabled && ui.is_enabled();
    let dark = ui.visuals().dark_mode;
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), font(), ink(dark));
    let size = Vec2::new(19.0 + galley.size().x, 17.0_f32.max(galley.size().y));
    let (rect, mut response) = ui.allocate_exact_size(
        size,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let tick = Rect::from_min_size(rect.left_center() - Vec2::new(0.0, 6.5), Vec2::splat(13.0));
        p.rect_filled(tick, 0.0, if enabled { well(dark) } else { face(dark) });
        double_bevel(p, tick, true);
        let ink_color = if enabled { ink(dark) } else { grey_ink(dark) };
        if *value {
            let o = tick.min;
            p.line_segment(
                [o + Vec2::new(3.0, 6.5), o + Vec2::new(5.5, 9.0)],
                Stroke::new(2.0_f32, ink_color),
            );
            p.line_segment(
                [o + Vec2::new(5.5, 9.0), o + Vec2::new(10.0, 3.5)],
                Stroke::new(2.0_f32, ink_color),
            );
        }
        let label = ui
            .painter()
            .layout_no_wrap(text.to_string(), font(), ink_color);
        p.galley(
            egui::pos2(tick.right() + 6.0, rect.center().y - label.size().y / 2.0),
            label,
            ink_color,
        );
    }
    response
}
/// Static text in the classic size; `colour` overrides the ink.
pub fn label(ui: &mut Ui, text: impl Into<String>, colour: Option<Color32>) -> Response {
    let dark = ui.visuals().dark_mode;
    ui.add(
        egui::Label::new(
            egui::RichText::new(text.into())
                .size(TEXT)
                .color(colour.unwrap_or_else(|| ink(dark))),
        )
        .wrap(),
    )
}
/// A group box around `add`, with its title on the etched frame.
pub fn group<R>(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.add_space(6.0);
    let inner = egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 9,
            right: 9,
            top: 11,
            bottom: 8,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        });
    group_box(ui.painter(), inner.response.rect, title);
    inner.inner
}
/// A Task Manager tab strip; returns true when the selection changed.
pub fn tabs(ui: &mut Ui, selected: &mut usize, labels: &[&str]) -> bool {
    let dark = ui.visuals().dark_mode;
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (i, text) in labels.iter().enumerate() {
            let galley = ui
                .painter()
                .layout_no_wrap(text.to_string(), font(), ink(dark));
            let size = Vec2::new(galley.size().x + 18.0, 21.0);
            let (rect, response) = ui.allocate_exact_size(size, Sense::click());
            let active = *selected == i;
            let tab = if active {
                rect
            } else {
                Rect::from_min_max(rect.min + Vec2::new(0.0, 2.0), rect.max)
            };
            let p = ui.painter();
            p.rect_filled(tab, 0.0, face(dark));
            bevel(p, tab, false);
            if active {
                p.line_segment(
                    [tab.left_bottom(), tab.right_bottom()],
                    Stroke::new(2.0_f32, face(dark)),
                );
            }
            p.galley(tab.center() - galley.size() / 2.0, galley, ink(dark));
            if response.clicked() && !active {
                *selected = i;
                changed = true;
            }
        }
    });
    changed
}
/// The raised panel below a tab strip.
pub fn tab_page<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let inner = egui::Frame::new()
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        });
    bevel(ui.painter(), inner.response.rect, false);
    inner.inner
}
/// One selectable row of a list box: navy with white text when selected.
pub fn list_row(ui: &mut Ui, enabled: bool, selected: bool, indent: f32, text: &str) -> Response {
    let enabled = enabled && ui.is_enabled();
    let dark = ui.visuals().dark_mode;
    let width = ui.available_width().max(1.0);
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(width, 18.0),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    if ui.is_rect_visible(rect) {
        let colour = if selected { Color32::WHITE } else { ink(dark) };
        let galley = ui
            .painter()
            .layout_no_wrap(text.to_string(), font(), colour);
        let text_rect = Rect::from_min_size(
            egui::pos2(rect.left() + indent, rect.top() + 1.0),
            Vec2::new(galley.size().x + 4.0, 16.0),
        );
        if selected {
            ui.painter().rect_filled(text_rect, 0.0, selection(dark));
        } else if response.hovered() && enabled {
            ui.painter().rect_stroke(
                text_rect,
                0.0,
                Stroke::new(1.0_f32, grey_ink(dark)),
                egui::StrokeKind::Inside,
            );
        }
        ui.painter().galley(
            text_rect.min + Vec2::new(2.0, 8.0 - galley.size().y / 2.0),
            galley,
            colour,
        );
    }
    response
}
/// The [+] / [−] box of a tree row.
pub fn expander(ui: &mut Ui, at: egui::Pos2, open: bool) -> Response {
    let dark = ui.visuals().dark_mode;
    let rect = Rect::from_center_size(at, Vec2::splat(9.0));
    let response = ui.interact(
        rect.expand(3.0),
        ui.id().with(("expander", at.x as i32, at.y as i32)),
        Sense::click(),
    );
    let p = ui.painter();
    p.rect_filled(rect, 0.0, well(dark));
    p.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0_f32, grey_ink(dark)),
        egui::StrokeKind::Inside,
    );
    let c = rect.center();
    p.line_segment(
        [c - Vec2::new(2.5, 0.0), c + Vec2::new(2.5, 0.0)],
        Stroke::new(1.0_f32, ink(dark)),
    );
    if !open {
        p.line_segment(
            [c - Vec2::new(0.0, 2.5), c + Vec2::new(0.0, 2.5)],
            Stroke::new(1.0_f32, ink(dark)),
        );
    }
    response
}
/// A menu entry: flat, navy with white text while hovered. `checked` shows a tick.
pub fn menu_item(ui: &mut Ui, text: &str, checked: bool) -> Response {
    let dark = ui.visuals().dark_mode;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(185.0, 21.0), Sense::click());
    let hovered = response.hovered();
    let colour = if hovered { Color32::WHITE } else { ink(dark) };
    if hovered {
        ui.painter().rect_filled(rect, 0.0, selection(dark));
    }
    if checked {
        ui.painter().text(
            rect.left_center() + Vec2::new(6.0, 0.0),
            egui::Align2::LEFT_CENTER,
            "✓",
            font(),
            colour,
        );
    }
    ui.painter().text(
        rect.left_center() + Vec2::new(20.0, 0.0),
        egui::Align2::LEFT_CENTER,
        text,
        font(),
        colour,
    );
    response
}
/// The frame of a popup menu.
pub fn menu_frame(dark: bool) -> egui::Frame {
    egui::Frame::new()
        .fill(face(dark))
        .stroke(Stroke::new(1.0_f32, Color32::from_gray(100)))
        .inner_margin(3.0)
}

/// A sunken status-bar pane holding one line of text.
pub fn status_pane(ui: &mut Ui, width: f32, text: &str, colour: Option<Color32>) {
    let _ = pane(ui, width, text, colour, Sense::hover());
}
/// A status pane that opens something when clicked.
pub fn status_pane_button(ui: &mut Ui, width: f32, text: &str) -> Response {
    pane(ui, width, text, None, Sense::click())
}
fn pane(ui: &mut Ui, width: f32, text: &str, colour: Option<Color32>, sense: Sense) -> Response {
    let dark = ui.visuals().dark_mode;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 19.0), sense);
    bevel(ui.painter(), rect, true);
    let colour = colour.unwrap_or_else(|| ink(dark));
    let galley = ui
        .painter()
        .layout(text.to_string(), font(), colour, (width - 8.0).max(1.0));
    ui.painter().with_clip_rect(rect.shrink(1.0)).galley(
        rect.min + Vec2::new(4.0, 3.0),
        galley,
        colour,
    );
    response
}

/// A classic window: raised frame, navy title bar with a close box, grey face. Dragging the
/// title bar moves it; it is resizable when `resizable`. Returns `None` when closed.
pub struct Window<'a> {
    id: egui::Id,
    title: &'a str,
    default_size: Vec2,
    min_size: Vec2,
    resizable: bool,
    modal: bool,
}
impl<'a> Window<'a> {
    pub fn new(id: impl std::hash::Hash, title: &'a str) -> Self {
        Self {
            id: egui::Id::new(id),
            title,
            default_size: Vec2::new(420.0, 200.0),
            min_size: Vec2::new(240.0, 120.0),
            resizable: false,
            modal: false,
        }
    }
    /// A centred dialog that blocks the windows behind it; Escape closes it.
    pub fn modal(mut self, modal: bool) -> Self {
        self.modal = modal;
        self
    }
    pub fn default_size(mut self, size: impl Into<Vec2>) -> Self {
        self.default_size = size.into();
        self
    }
    pub fn min_size(mut self, size: impl Into<Vec2>) -> Self {
        self.min_size = size.into();
        self
    }
    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }
    /// Shows the window while `*open`; the close box clears it.
    pub fn show<R>(
        self,
        ctx: &egui::Context,
        open: &mut bool,
        add: impl FnOnce(&mut Ui) -> R,
    ) -> Option<R> {
        if !*open {
            return None;
        }
        let dark = ctx.style().visuals.dark_mode;
        let mut close = false;
        let title = self.title;
        let frame = egui::Frame::new()
            .fill(face(dark))
            .inner_margin(egui::Margin::same(4));
        let content = |ui: &mut Ui| {
            apply_text_style(ui);
            let width = ui.available_width();
            let (bar, _) = ui.allocate_exact_size(Vec2::new(width, 18.0), Sense::hover());
            ui.painter().rect_filled(bar, 0.0, title_bar(dark));
            ui.painter().text(
                bar.left_center() + Vec2::new(5.0, 0.0),
                egui::Align2::LEFT_CENTER,
                title,
                egui::FontId::proportional(TEXT),
                Color32::WHITE,
            );
            let boxed = Rect::from_min_size(
                egui::pos2(bar.right() - 18.0, bar.top() + 2.0),
                Vec2::new(16.0, 14.0),
            );
            if close_box(ui, boxed).clicked() {
                close = true;
            }
            let result = egui::Frame::new()
                .inner_margin(egui::Margin {
                    left: 6,
                    right: 6,
                    top: 4,
                    bottom: 4,
                })
                .show(ui, |ui| add(ui))
                .inner;
            // Frame drawn last so the bevel stays on top of the content's edges.
            double_bevel(
                &ui.ctx().layer_painter(ui.layer_id()),
                ui.max_rect().union(ui.min_rect()).expand(4.0),
                false,
            );
            result
        };
        let shown = if self.modal {
            let response = egui::Modal::new(self.id).frame(frame).show(ctx, |ui| {
                ui.set_width(self.default_size.x);
                content(ui)
            });
            if response.should_close() {
                close = true;
            }
            Some(response.inner)
        } else {
            egui::Window::new(title)
                .id(self.id)
                .title_bar(false)
                .collapsible(false)
                .resizable(self.resizable)
                .default_size(self.default_size)
                .min_size(self.min_size)
                .frame(frame)
                .show(ctx, content)
                .and_then(|r| r.inner)
        };
        if close {
            *open = false;
        }
        shown
    }
}
