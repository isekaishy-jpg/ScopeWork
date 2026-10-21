//! A GUI failure leaves a trace in the research home's logs/gui.log: the run header, the
//! panic message with its location and a backtrace (the windowed process has no console).
use std::{fs, path::PathBuf};

#[test]
fn a_gui_panic_is_written_to_the_crash_log() {
    let home = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/test-workspaces")
        .join(format!("gui-log-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&home).unwrap();
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_research-dashboard"))
        .env("RESEARCH_DASHBOARD_HOME", &home)
        .env("SCOPEWORK_GUI_PANIC_TEST", "1")
        .output()
        .unwrap();
    assert!(!o.status.success(), "the deliberate panic ends the process");
    let log = fs::read_to_string(home.join("logs/gui.log")).unwrap();
    let _ = fs::remove_dir_all(&home);
    assert!(log.starts_with("--- "), "run header first: {log}");
    assert!(log.contains("start: Scopework "), "{log}");
    assert!(log.contains("deliberate test panic"), "{log}");
    assert!(
        log.contains("main.rs:"),
        "the panic location is recorded: {log}"
    );
    assert!(log.contains("backtrace:"), "{log}");
    assert!(
        o.stderr.is_empty(),
        "standard error went to the log, not the console"
    );
}
