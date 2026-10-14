#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod research;

use eframe::egui::{self, Color32, Vec2};

const GREEN: Color32 = Color32::from_rgb(0, 210, 48);
const RED: Color32 = Color32::from_rgb(228, 48, 40);
const YELLOW: Color32 = Color32::from_rgb(238, 193, 55);

#[derive(Clone)]
struct Row {
    name: String,
    groups: Vec<(String, usize, usize, usize)>,
}

fn rows() -> Vec<Row> {
    vec![
        Row {
            name: "Rendering".into(),
            groups: vec![
                ("Frame".into(), 42, 0, 42),
                ("Views".into(), 68, 0, 68),
                ("Passes".into(), 95, 1, 37),
                ("Present".into(), 24, 2, 0),
            ],
        },
        Row {
            name: "Assets".into(),
            groups: vec![
                ("Requests".into(), 53, 0, 53),
                ("Dependencies".into(), 84, 1, 51),
                ("Publication".into(), 37, 2, 0),
                ("Lifetime".into(), 65, 2, 0),
            ],
        },
        Row {
            name: "World".into(),
            groups: vec![
                ("Maps".into(), 31, 0, 31),
                ("Spatial data".into(), 102, 1, 26),
                ("Visibility".into(), 73, 2, 0),
                ("Lifetime".into(), 46, 2, 0),
            ],
        },
        Row {
            name: "Animation".into(),
            groups: vec![
                ("States".into(), 28, 0, 28),
                ("Poses".into(), 64, 1, 41),
                ("Blending".into(), 51, 2, 0),
                ("Integration".into(), 39, 2, 0),
            ],
        },
        Row {
            name: "Interface".into(),
            groups: vec![
                ("Input".into(), 47, 0, 47),
                ("Widgets".into(), 88, 1, 19),
                ("Layout".into(), 56, 2, 0),
                ("Drawing".into(), 71, 2, 0),
            ],
        },
        Row {
            name: "Unclassified".into(),
            groups: vec![
                ("Region A".into(), 210, 2, 0),
                ("Region B".into(), 190, 2, 0),
                ("Region C".into(), 160, 2, 0),
                ("Region D".into(), 140, 2, 0),
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
    chunk_counts: Vec<usize>,
    stock_preview: Option<Vec<Row>>,
    research: research::ResearchUi,
    projects: Vec<research::ProjectView>,
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
            chunk_counts: vec![64; 48],
            stock_preview: None,
            research: research::ResearchUi::new(),
            projects: vec![],
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
const METER_HEIGHT: f32 = 46.0;
fn meter_width(segments: usize) -> f32 {
    6.0 + segments as f32 * 3.0
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
        eframe::set_value(storage, "chunk_counts", &self.chunk_counts);
    }
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if ctx.input(|i| i.viewport().close_requested()) && self.research.has_unsaved() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.research.prevent_close();
        }
        if self.research.changed {
            self.projects = self.research.snapshot();
            self.research.changed = false;
            self.selection = None;
        }
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
                for (i, project) in self.projects.iter().enumerate() {
                    let name=&project.name;
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
                let project_view=self.projects.get(self.project).cloned();
                let live=project_view.as_ref().is_some_and(|p| !p.meters.is_empty() || (p.id!="stock"&&p.id!="forever"));
                let live_meters=project_view.as_ref().map(|p|p.meters.clone()).unwrap_or_default();
                let data = if live {
                    let mut rows=Vec::new();
                    for batch in live_meters.chunks(4) {
                        rows.push(Row{name:String::new(),groups:batch.iter().map(|m|(m.name.clone(),m.functions,if m.states.iter().all(|&s|s==0)&&!m.states.is_empty(){0}else if m.states.iter().any(|&s|s!=2){1}else{2},m.completed_functions)).collect()});
                    }
                    if rows.is_empty(){rows.push(Row{name:String::new(),groups:vec![("No architecture records yet".into(),0,2,0)]});}rows
                } else if self.project==0 {self.stock_preview.clone().unwrap_or_else(rows)} else {rows()};
                let meter_count:usize=data.iter().map(|r|r.groups.len()).sum();
                let scale=if live{1}else{self.project+1};
                let mut counts = [0usize; 3];
                for row in &data {
                    for g in &row.groups {
                        counts[0] += g.3 * scale;
                        if g.2 != 0 {
                            counts[g.2] += (g.1 - g.3) * scale;
                        }
                    }
                }
                let total: usize = counts.iter().sum();
                let project_counts:Vec<usize>=if live {live_meters.iter().map(|m|m.states.len()).chain(std::iter::repeat(0)).take(meter_count).collect()} else {self.chunk_counts[self.project*24..self.project*24+meter_count].to_vec()};
                let total_chunks:usize=project_counts.iter().sum();
                let done_chunks:usize=if live{live_meters.iter().flat_map(|m|m.states.iter()).filter(|&&s|s==0).count()}else{data.iter().flat_map(|r|r.groups.iter()).zip(&project_counts).map(|(g,&count)|g.3*count/g.1.max(1)).sum()};
                let selected=self.selection.filter(|&(r,g)|r<data.len()&&g<data[r].groups.len()).unwrap_or((0,0));
                let (name,n,status,completed)=data[selected.0].groups[selected.1].clone();
                let top=82.0;
                let meter_h=(h-270.0).max(218.0);
                let detail=r(15.0,top,174.0,meter_h);
                group_box(&p,detail,"Selected meter");
                let well=detail.shrink2(Vec2::new(10.0,17.0));
                p.rect_filled(well,0.0,Color32::BLACK);bevel(&p,well,true);
                let selected_index=selected.0*4+selected.1;
                let selected_chunks=project_counts[selected_index];
                let selected_done=if live {live_meters.get(selected_index).map(|m|m.states.iter().filter(|&&s|s==0).count()).unwrap_or(0)}else{completed*selected_chunks/n.max(1)};
                let title=if live {name.clone()} else {format!("{} / {name}",data[selected.0].name)};
                let heading=p.layout(title,egui::FontId::proportional(11.0),GREEN,well.width()-14.0);
                let heading_height=heading.size().y;
                p.galley(well.min+Vec2::new(7.0,8.0),heading,GREEN);
                let bar=egui::Rect::from_min_size(well.min+Vec2::new(6.0,heading_height+20.0),Vec2::new(well.width()-12.0,32.0));
                led(&p,bar,selected_done,selected_chunks,[GREEN,YELLOW,RED][status],((bar.width()-6.0)/3.0).floor() as usize);
                let readout=bar.bottom()+19.0;
                text(&p,well.left()+7.0,readout,["Complete","In progress","Not done"][status],GREEN);
                text(&p,well.left()+7.0,readout+24.0,format!("Chunks: {selected_done} / {selected_chunks}"),GREEN);
                text(&p,well.left()+7.0,readout+45.0,format!("Funcs: {} / {}",completed*scale,n*scale),GREEN);
                text(&p,well.left()+7.0,readout+69.0,format!("{}% chunks complete",selected_done*100/selected_chunks.max(1)),GREEN);
                let hint=p.layout("Click a meter to inspect it.".into(),egui::FontId::proportional(11.0),Color32::from_gray(150),well.width()-14.0);
                p.galley(egui::pos2(well.left()+7.0,readout+96.0),hint,Color32::from_gray(150));
                let bank=r(201.0,top,w-216.0,meter_h);
                group_box(&p, bank, "Research chunks");
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
                            .show(ui, |ui| {
                                let padding=8.0;
                                let gap=12.0;
                                let row_height=104.0;
                                let panel_widths:Vec<f32>=data.iter().flat_map(|row|row.groups.iter().map(move |g|(row,g))).enumerate().map(|(index,(row,g))|{
                                    let title=if live{g.0.clone()}else{format!("{} / {}",row.name,g.0)};
                                    let count=project_counts[index];
                                    let footer=format!("Chunks: {} / {}    Funcs: {} / {}",count,count,g.1*scale,g.1*scale);
                                    let font=egui::FontId::proportional(11.0);
                                    let title_w=ui.painter().layout_no_wrap(title,font.clone(),ink).size().x;
                                    let footer_w=ui.painter().layout_no_wrap(footer,font,ink).size().x;
                                    meter_width(count).max(title_w).max(footer_w)+padding*2.0
                                }).collect();
                                let content_width=panel_widths.chunks(2).map(|pair|pair.iter().sum::<f32>()+if pair.len()==2{gap}else{0.0}).fold(0.0_f32,f32::max);
                                let (area,_)=ui.allocate_exact_size(Vec2::new(content_width,(row_height*meter_count.div_ceil(2) as f32-gap).max(0.0)),egui::Sense::hover());
                                let base = area.min;
                                let p = ui.painter().clone();
                                for ri in 0..data.len() {
                                    for gi in 0..data[ri].groups.len() {
                                        let index = ri * 4 + gi;
                                        let col = index % 2;
                                        let slot = index / 2;
                                        let (chunk, num, state, finished) = data[ri].groups[gi].clone();
                                        let panel_x=base.x+if col==0{0.0}else{panel_widths[index-1]+gap};
                                        let panel_y=base.y+slot as f32*row_height;
                                        let panel_rect=egui::Rect::from_min_size(egui::pos2(panel_x,panel_y),Vec2::new(panel_widths[index],row_height-gap));
                                        p.rect_filled(panel_rect,0.0,bg);
                                        bevel(&p,panel_rect,true);
                                        let x=panel_x+padding;
                                        let y=panel_y+padding;
                                        let count_index = self.project * 24 + index;
                                        let segment_count = project_counts[index];
                                        let meter = egui::Rect::from_min_size(
                                            egui::pos2(x, y + 15.0),
                                            Vec2::new(meter_width(segment_count), METER_HEIGHT),
                                        );
                                        if live {
                                            draw_chunk_states(&p,meter,live_meters.get(index).map(|m|m.states.as_slice()).unwrap_or(&[]));
                                        } else {led(&p,meter,finished,num,[GREEN,YELLOW,RED][state],segment_count);}

                                        text(
                                            &p,
                                            x,
                                            y + 5.0,
                                            if live {chunk.clone()} else {format!("{} / {chunk}",data[ri].name)},
                                            Color32::BLACK,
                                        );
                                        text(
                                            &p,
                                            x,
                                            meter.bottom() + 10.0,
                                            format!(
                                                "Chunks: {} / {}    Funcs: {} / {}",
                                                if live {live_meters.get(index).map(|m|m.states.iter().filter(|&&s|s==0).count()).unwrap_or(0)}else{finished * segment_count / num.max(1)},
                                                segment_count,
                                                finished * scale,
                                                num * scale
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
                                        if live && hit.double_clicked() {
                                            if let (Some(project),Some(meter))=(&project_view,live_meters.get(index)){self.research.open_record(&project.id,meter.id);}
                                        }
                                        hit.context_menu(|ui| {
                                            if live {
                                                if ui.button("Open architecture report").clicked(){if let (Some(project),Some(meter))=(&project_view,live_meters.get(index)){self.research.open_record(&project.id,meter.id);}}
                                                ui.label("Segment counts come from this architecture's chunk records.");
                                                return;
                                            }
                                            ui.label("Chunk segments");
                                            ui.add(
                                                egui::DragValue::new(
                                                    &mut self.chunk_counts[count_index],
                                                )
                                                .range(0..=4096),
                                            );
                                            ui.label(
                                                "One segment per chunk. Saved for this project.",
                                            );
                                        });
                                        hit.on_hover_text(format!(
                                            "{} / {chunk}: {} functions - {}",
                                            data[ri].name,
                                            num * scale,
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
                t(26.0,bottom+122.0,format!("Selected: {}{}{}    Funcs: {} / {}    {}",data[selected.0].name,if live{""}else{" / "},name,completed*scale,n*scale,["Done","In progress","Not done"][status]));
                let mut sx = 0.0;
                for (width, label) in [
                    (w * 0.34, format!("Funcs: {} / {total}", counts[0])),
                    (
                        w * 0.33,
                        format!("Chunks done: {done_chunks} / {total_chunks}"),
                    ),
                    (
                        w * 0.33,
                        if live {format!("App report: {:?}",project_view.as_ref().map(|p|p.app_status).unwrap_or(research::Status::NotStarted))} else if self.project == 0 && self.stock_preview.is_some() {
                            "Stock assumptions".to_string()
                        } else {
                            "Sample data".to_string()
                        },
                    ),
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
                            if menu == 2
                                && ui
                                    .add_sized(
                                        [185.0, 23.0],
                                        egui::Button::new("Research records...").frame(false),
                                    )
                                    .clicked()
                            {
                                self.research.open = true;
                                self.menu = None;
                            }
                            if menu == 2
                                && ui
                                    .add_sized(
                                        [185.0, 23.0],
                                        egui::Button::new("Refresh research progress").frame(false),
                                    )
                                    .clicked()
                            {
                                self.research.changed = true;
                                self.menu = None;
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
        self.research.show(ctx);
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
    let arguments: Vec<String> = std::env::args().collect();
    if arguments.get(1).is_some_and(|a| a == "--set-prerequisites") {
        if arguments.len() < 4 {
            eprintln!("Usage: --set-prerequisites PROJECT FUNCTION_RVA [PREREQUISITE_RVA ...]");
            std::process::exit(2);
        }
        let root = std::env::var_os("RESEARCH_DASHBOARD_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/projects")
            });
        let result = research::Library::open(root).and_then(|mut lib| {
            lib.set_dependencies_by_rva(&arguments[2], &arguments[3], &arguments[4..].join(","))
        });
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return Ok(());
    }

    if std::env::args().any(|arg| arg == "--migrate-library") {
        let root = std::env::var_os("RESEARCH_DASHBOARD_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/projects")
            });
        if let Err(error) = research::Library::open(root) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return Ok(());
    }
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
            if let Some(saved) = cc
                .storage
                .and_then(|s| eframe::get_value::<Vec<usize>>(s, "chunk_counts"))
            {
                if saved.len() == 48 {
                    app.chunk_counts = saved.into_iter().map(|n| n.min(4096)).collect();
                }
            }
            // Local preview data stays outside Git; no historical completion states are accepted.
            let preview_path =
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/stock-preview.json");
            if let Ok(bytes) = std::fs::read(preview_path) {
                if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                    if let Some(meters) = value["meters"]
                        .as_array()
                        .filter(|m| !m.is_empty() && m.len() <= 24)
                    {
                        let mut preview = Vec::new();
                        for (i, batch) in meters.chunks(4).enumerate() {
                            let mut groups = Vec::new();
                            for (j, m) in batch.iter().enumerate() {
                                let name: &'static str = Box::leak(
                                    m["name"]
                                        .as_str()
                                        .unwrap_or("Unclassified")
                                        .to_owned()
                                        .into_boxed_str(),
                                );
                                let functions =
                                    m["functions"].as_u64().unwrap_or(1).max(1) as usize;
                                groups.push((name.to_string(), functions, 2, 0));
                                app.chunk_counts[i * 4 + j] =
                                    m["chunks"].as_u64().unwrap_or(1).min(4096) as usize;
                            }
                            preview.push(Row {
                                name: "Stock".into(),
                                groups,
                            });
                        }
                        app.stock_preview = Some(preview);
                    }
                }
            }
            Ok(Box::new(app))
        }),
    )
}
