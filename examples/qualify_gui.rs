//! Manual headless egui frame qualification against an explicitly selected fixture.
use eframe::egui;
use research_dashboard::research::ResearchUi;
use std::time::{Duration, Instant};
fn main() {
    assert!(
        std::env::var_os("RESEARCH_DASHBOARD_HOME").is_some(),
        "Set an isolated fixture home explicitly"
    );
    let context = egui::Context::default();
    let mut ui = ResearchUi::new();
    ui.open = true;
    let start = Instant::now();
    let mut frames = vec![];
    let mut selected = false;
    let mut loaded_at = None;
    while start.elapsed() < Duration::from_secs(15) {
        let before = Instant::now();
        let _ = context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280., 900.),
                )),
                ..Default::default()
            },
            |ctx| {
                ui.poll(ctx);
                ui.show(ctx);
            },
        );
        frames.push(before.elapsed().as_secs_f64() * 1000.);
        let projects = ui.snapshot();
        if !selected && let Some(project) = projects.first() {
            ui.open_record(&project.id, 3);
            selected = true;
            loaded_at = Some(start.elapsed().as_millis());
        }
        std::thread::sleep(Duration::from_millis(8));
    }
    frames.sort_by(f64::total_cmp);
    println!(
        "{}",
        serde_json::json!({"frames":frames.len(),"p95_ms":frames[frames.len()*95/100],"max_ms":frames.last(),"first_snapshot_ms":loaded_at,"projects":ui.snapshot().len(),"headless":true})
    );
}
