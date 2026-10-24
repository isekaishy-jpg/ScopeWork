//! Research-home settings shared by the GUI and the CLI: today, the dashboard's project groups.
//!
//! `settings.json` in the research home holds presentation choices only; nothing in it is
//! project data or history. Writers take `.settings.lock` and replace the file atomically.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

const FILE: &str = "settings.json";
const LOCK: &str = ".settings.lock";
/// The longest group name, in characters; a tab has to show it.
pub const GROUP_NAME_MAX: usize = 40;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "schema")]
    pub schema: u32,
    #[serde(default)]
    pub groups: Vec<Group>,
}
fn schema() -> u32 {
    1
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: schema(),
            groups: vec![],
        }
    }
}
/// A named set of projects shown together on the dashboard.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub name: String,
    pub projects: Vec<String>,
}

impl Settings {
    /// The group holding `project`, if any.
    pub fn group_of(&self, project: &str) -> Option<&str> {
        self.groups
            .iter()
            .find(|g| g.projects.iter().any(|p| p == project))
            .map(|g| g.name.as_str())
    }
    /// Puts `project` in group `name` (created if new), or makes it ungrouped with `None`.
    /// It leaves any other group; a group left without projects is removed.
    pub fn assign(&mut self, project: &str, name: Option<&str>) -> Result<(), String> {
        let name = name.map(valid_name).transpose()?;
        for group in &mut self.groups {
            group.projects.retain(|p| p != project);
        }
        if let Some(name) = name {
            match self.groups.iter_mut().find(|g| g.name == name) {
                Some(group) => group.projects.push(project.to_string()),
                None => self.groups.push(Group {
                    name,
                    projects: vec![project.to_string()],
                }),
            }
        }
        self.groups.retain(|g| !g.projects.is_empty());
        for group in &mut self.groups {
            group.projects.sort();
        }
        self.groups.sort_by_key(|g| g.name.to_lowercase());
        Ok(())
    }
    /// Drops projects that are not in `existing` and the groups this empties.
    pub fn retain_projects(&mut self, existing: &[String]) {
        for group in &mut self.groups {
            group.projects.retain(|p| existing.contains(p));
        }
        self.groups.retain(|g| !g.projects.is_empty());
    }
}

/// A group name as stored: trimmed, one line, 1 to `GROUP_NAME_MAX` characters.
pub fn valid_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().any(char::is_control) {
        return Err("A group name is a single line of text, such as WoW or Populous.".into());
    }
    if name.chars().count() > GROUP_NAME_MAX {
        return Err(format!(
            "A group name has at most {GROUP_NAME_MAX} characters."
        ));
    }
    Ok(name.to_string())
}

fn file(home: &Path) -> PathBuf {
    home.join(FILE)
}

/// The settings of `home`; defaults when there is no settings file yet.
pub fn load(home: &Path) -> Result<Settings, String> {
    match fs::read(file(home)) {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", file(home).display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(e) => Err(format!("{}: {e}", file(home).display())),
    }
}

/// The settings file's modification time, to notice changes cheaply.
pub fn stamp(home: &Path) -> Option<std::time::SystemTime> {
    fs::metadata(file(home)).and_then(|m| m.modified()).ok()
}

/// Reads, changes and writes the settings under the settings lock.
pub fn update<T>(
    home: &Path,
    change: impl FnOnce(&mut Settings) -> Result<T, String>,
) -> Result<T, String> {
    fs::create_dir_all(home).map_err(|e| e.to_string())?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(home.join(LOCK))
        .map_err(|e| format!("{}: {e}", home.join(LOCK).display()))?;
    lock.lock().map_err(|e| format!("Settings lock: {e}"))?;
    let mut settings = load(home)?;
    let result = change(&mut settings)?;
    let bytes = serde_json::to_vec_pretty(&settings).map_err(|e| e.to_string())?;
    crate::research::atomic(&file(home), &bytes)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(s: &Settings) -> Vec<(String, Vec<String>)> {
        s.groups
            .iter()
            .map(|g| (g.name.clone(), g.projects.clone()))
            .collect()
    }

    #[test]
    fn a_project_is_in_at_most_one_group_and_empty_groups_go() {
        let mut s = Settings::default();
        s.assign("b", Some(" WoW ")).unwrap();
        s.assign("a", Some("WoW")).unwrap();
        s.assign("p", Some("Populous")).unwrap();
        assert_eq!(
            names(&s),
            vec![
                ("Populous".into(), vec!["p".into()]),
                ("WoW".into(), vec!["a".into(), "b".into()]),
            ]
        );
        s.assign("p", Some("WoW")).unwrap();
        assert_eq!(
            names(&s),
            vec![("WoW".into(), vec!["a".into(), "b".into(), "p".into()])]
        );
        assert_eq!(s.group_of("p"), Some("WoW"));
        s.assign("a", None).unwrap();
        assert_eq!(s.group_of("a"), None);
        s.retain_projects(&["a".into()]);
        assert!(s.groups.is_empty());
    }

    #[test]
    fn group_names_are_one_short_line() {
        assert!(valid_name("").is_err());
        assert!(valid_name("Two\nlines").is_err());
        assert!(valid_name(&"x".repeat(GROUP_NAME_MAX + 1)).is_err());
        assert_eq!(valid_name("  Populous ").unwrap(), "Populous");
    }

    #[test]
    fn concurrent_writers_keep_each_others_changes() {
        let home = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-workspaces")
            .join(format!("settings-{}", uuid::Uuid::new_v4()));
        let writers: Vec<_> = (0..8)
            .map(|i| {
                let home = home.clone();
                std::thread::spawn(move || {
                    update(&home, |s| s.assign(&format!("p{i}"), Some("All"))).unwrap();
                })
            })
            .collect();
        for writer in writers {
            writer.join().unwrap();
        }
        let settings = load(&home).unwrap();
        assert_eq!(settings.groups.len(), 1);
        assert_eq!(settings.groups[0].projects.len(), 8);
        fs::remove_dir_all(&home).unwrap();
    }
}
