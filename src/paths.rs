use std::{fs, path::PathBuf};

pub fn home(explicit: Option<PathBuf>) -> Result<PathBuf, String> {
    let path = explicit
        .or_else(|| std::env::var_os("RESEARCH_DASHBOARD_HOME").map(PathBuf::from))
        .or_else(installed_home)
        .or_else(|| {
            std::env::var_os("LOCALAPPDATA").map(|p| {
                let base = PathBuf::from(p);
                let previous = base.join("ResearchDashboard");
                if previous.join("projects").is_dir() {
                    previous
                } else {
                    base.join("Scopework")
                }
            })
        })
        .ok_or("Set RESEARCH_DASHBOARD_HOME or pass --home to select a research home.")?;
    if !path.is_absolute() {
        return Err("Research home must be an absolute path.".into());
    }
    Ok(path)
}

fn installed_home() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use winreg::{
            RegKey,
            enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WOW64_64KEY},
        };
        // Keep the original internal settings key across the product rename.
        let key = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags("Software\\ResearchDashboard", KEY_READ | KEY_WOW64_64KEY)
            .ok()?;
        let path: String = key.get_value("ResearchHome").ok()?;
        if !path.trim().is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    None
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
