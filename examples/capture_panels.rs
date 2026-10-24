//! Manual visual qualification: renders the Research records, Project history and Write queue
//! windows and the records dialogs against an
//! explicitly selected fixture home and saves screenshots (light and dark) as PNG files.
//!
//! RESEARCH_DASHBOARD_HOME=<fixture home> SCOPEWORK_CAPTURE_DIR=<folder> cargo run --example capture_panels
use eframe::egui;
use research_dashboard::{classic, research::ResearchUi};

/// (file name, dark mode, tab, confirm-complete dialog, new-project dialog, history window,
/// write-queue window)
const SHOTS: &[(&str, bool, usize, bool, bool, bool, bool)] = &[
    ("report-light", false, 0, false, false, false, false),
    ("organize-light", false, 1, false, false, false, false),
    ("project-light", false, 2, false, false, false, false),
    ("complete-light", false, 0, true, false, false, false),
    ("new-project-light", false, 0, false, true, false, false),
    ("report-dark", true, 0, false, false, false, false),
    ("organize-dark", true, 1, false, false, false, false),
    ("history-light", false, 0, false, false, true, false),
    ("history-dark", true, 0, false, false, true, false),
    ("queue-light", false, 0, false, false, false, true),
    ("queue-dark", true, 0, false, false, false, true),
];

struct Capture {
    ui: ResearchUi,
    out: std::path::PathBuf,
    shot: usize,
    frames: u32,
    requested: bool,
    selected: bool,
}
impl eframe::App for Capture {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if self.shot == SHOTS.len() {
            return;
        }
        for event in ctx.input(|i| i.raw.events.clone()) {
            if let egui::Event::Screenshot { image, .. } = event {
                let path = self.out.join(format!("{}.png", SHOTS[self.shot].0));
                image::save_buffer(
                    &path,
                    image.as_raw(),
                    image.width() as u32,
                    image.height() as u32,
                    image::ExtendedColorType::Rgba8,
                )
                .unwrap();
                println!("{}", path.display());
                self.shot += 1;
                self.frames = 0;
                self.requested = false;
                if self.shot == SHOTS.len() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    return;
                }
            }
        }
        let (_, dark, tab, confirm, new_project, history, queue) = SHOTS[self.shot];
        if ctx.style().visuals.dark_mode != dark {
            ctx.set_visuals(classic::visuals(dark));
        }
        self.ui.poll(ctx);
        let first = self.ui.snapshot().first().map(|p| p.id.clone());
        self.ui.watch_queues(first.as_deref());
        if !self.selected
            && let Some(project) = self.ui.snapshot().first()
        {
            // Open a chunk so the Organize tab shows children, flags and membership.
            self.ui.open_record(&project.id, 2);
            self.selected = true;
        }
        self.ui.qualification_view(tab, confirm, new_project);
        if history {
            // The history window alone, following the fixture's first project.
            self.ui.open = false;
            if let Some(id) = first.as_deref() {
                self.ui.follow_project(id);
            }
            if self.frames == 0 {
                self.ui.open_history();
            }
        }
        if queue && self.frames == 0 {
            // Over the records window, so a gap in its face would show what is behind it.
            if let Some(id) = first.as_deref() {
                self.ui.open_queue(id);
            }
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(classic::face(dark)))
            .show(ctx, |_| {});
        self.ui.show(ctx);
        self.frames += 1;
        // Give the snapshot, the report load and the layout time to settle.
        if self.selected && self.frames > 90 && !self.requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            self.requested = true;
        }
        ctx.request_repaint();
    }
}

fn main() -> eframe::Result {
    assert!(
        std::env::var_os("RESEARCH_DASHBOARD_HOME").is_some(),
        "Set an isolated fixture home explicitly"
    );
    let out = std::path::PathBuf::from(
        std::env::var_os("SCOPEWORK_CAPTURE_DIR").expect("Set SCOPEWORK_CAPTURE_DIR"),
    );
    std::fs::create_dir_all(&out).unwrap();
    eframe::run_native(
        "Scopework panel capture",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([1100.0, 760.0]),
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
            let mut ui = ResearchUi::new();
            ui.open = true;
            Ok(Box::new(Capture {
                ui,
                out,
                shot: 0,
                frames: 0,
                requested: false,
                selected: false,
            }))
        }),
    )
}
