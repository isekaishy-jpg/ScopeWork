use std::{fs, path::PathBuf};

pub fn home(explicit: Option<PathBuf>) -> Result<PathBuf, String> {
    let path = explicit
        .or_else(|| std::env::var_os("RESEARCH_DASHBOARD_HOME").map(PathBuf::from))
        .or_else(|| {
            std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("ResearchDashboard"))
        })
        .ok_or("Set RESEARCH_DASHBOARD_HOME or pass --home to select a research home.")?;
    if !path.is_absolute() {
        return Err("Research home must be an absolute path.".into());
    }
    Ok(path)
}

pub fn initialize(home: &std::path::Path) -> Result<(), String> {
    fs::create_dir_all(home.join("projects")).map_err(|e| e.to_string())?;
    fs::create_dir_all(home.join("complaints")).map_err(|e| e.to_string())?;
    let guide = home.join("complaints/README.md");
    use std::io::Write;
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(guide)
    {
        Ok(mut f) => f
            .write_all(include_bytes!("../complaints/README.md"))
            .map_err(|e| e.to_string())?,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.to_string()),
    }
    Ok(())
}
