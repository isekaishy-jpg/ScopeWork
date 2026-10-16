//! Versioned, resumable migration of the current catalog; never imports historical research.
use super::*;
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize)]
struct ReportMove {
    id: u64,
    old: String,
    new: String,
    original: String,
    rewritten: String,
}
#[derive(Serialize, Deserialize)]
struct Journal {
    project: Project,
    reports: Vec<ReportMove>,
    #[serde(default)]
    annotations: Option<String>,
}

pub(super) fn link(from: &str, to: &str) -> String {
    let mut a: Vec<_> = from.split('/').collect();
    a.pop();
    let b: Vec<_> = to.split('/').collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    format!(
        "{}{}",
        "../".repeat(a.len() - common),
        b[common..].join("/")
    )
}
fn legacy(n: &Node) -> String {
    match n.kind {
        Kind::App => "app.md".into(),
        Kind::Architecture => format!("architectures/{:06}/research.md", n.id),
        Kind::Chunk => format!("chunks/{:06}/research.md", n.id),
        Kind::Function => format!("functions/{:08X}/research.md", n.rva.unwrap()),
    }
}
/// Indexed path rewriting: cost follows the number of path components, not catalog size.
pub(super) struct Rewriter {
    moves: std::collections::HashMap<String, String>,
    root: String,
    destinations: regex::Regex,
    paths: regex::Regex,
}
impl Rewriter {
    pub fn new(moves: &[(String, String)], root: &Path) -> Self {
        Self {
            moves: moves.iter().cloned().collect(),
            root: format!("{}/", root.to_string_lossy().replace('\\', "/").trim_end_matches('/')),
            destinations: regex::Regex::new(r"\]\((<[^>]+>|[^\s)]+)\)").unwrap(),
            paths: regex::Regex::new(r#"(?:[A-Za-z][A-Za-z0-9+.-]*://|[A-Za-z]:[\\/]|/|\.{1,2}[\\/]|architectures/|functions/|chunks/|tools/)[^\s`<>\"'()\[\]]+"#).unwrap(),
        }
    }
    fn remap(&self, path: &str) -> String {
        let mut prefix = path;
        loop {
            if let Some(to) = self.moves.get(prefix) {
                return format!("{to}{}", &path[prefix.len()..]);
            }
            let Some((parent, _)) = prefix.rsplit_once('/') else {
                return path.into();
            };
            prefix = parent;
        }
    }
    fn target(&self, target: &str, old: &str, new: &str, prose: bool) -> String {
        let angle = target.starts_with('<') && target.ends_with('>');
        let bare = if angle {
            &target[1..target.len() - 1]
        } else {
            target
        };
        let normalized = bare.replace('\\', "/").replace("%20", " ");
        let uri_prefix = if normalized.starts_with("file:///") {
            "file:///"
        } else {
            ""
        };
        let local = normalized.strip_prefix(uri_prefix).unwrap();
        let absolute = local.to_lowercase().starts_with(&self.root.to_lowercase());
        let rewritten = if absolute {
            let local = &local[self.root.len()..];
            let end = local.find(['#', '?']).unwrap_or(local.len());
            format!(
                "{uri_prefix}{}{}{}",
                self.root,
                self.remap(&local[..end]),
                &local[end..]
            )
        } else {
            if local.starts_with(['#', '/']) || local.contains(':') {
                return target.into();
            }
            let end = local.find(['#', '?']).unwrap_or(local.len());
            let (path, suffix) = local.split_at(end);
            if prose
                && ["architectures/", "functions/", "chunks/", "tools/"]
                    .iter()
                    .any(|p| path.starts_with(p))
            {
                format!("{}{suffix}", self.remap(path))
            } else {
                let mut parts: Vec<_> = old.split('/').collect();
                parts.pop();
                for component in path.split('/') {
                    match component {
                        ".." => {
                            if parts.pop().is_none() {
                                return target.into();
                            }
                        }
                        "." | "" => {}
                        _ => parts.push(component),
                    }
                }
                format!("{}{suffix}", link(new, &self.remap(&parts.join("/"))))
            }
        };
        let rewritten = if bare.contains("%20") {
            rewritten.replace(' ', "%20")
        } else {
            rewritten
        };
        if angle {
            format!("<{rewritten}>")
        } else {
            rewritten
        }
    }
    pub fn rewrite(&self, text: &str, old: &str, new: &str) -> String {
        // Process plain text and Markdown destinations separately; never rewrite an external URL substring.
        let mut output = String::new();
        let mut last = 0;
        for captures in self.destinations.captures_iter(text) {
            let matched = captures.get(0).unwrap();
            output += &self
                .paths
                .replace_all(&text[last..matched.start()], |c: &regex::Captures<'_>| {
                    self.target(&c[0], old, new, true)
                });
            output += &format!("]({})", self.target(&captures[1], old, new, false));
            last = matched.end();
        }
        output += &self
            .paths
            .replace_all(&text[last..], |c: &regex::Captures<'_>| {
                self.target(&c[0], old, new, true)
            });
        output
    }
}
pub(super) fn inside(root: &Path, path: &Path) -> Result<()> {
    let root = fs::canonicalize(root).map_err(|e| e.to_string())?;
    let mut ancestor = path;
    loop {
        match fs::symlink_metadata(ancestor) {
            Ok(_) => break,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                ancestor = ancestor.parent().ok_or("Invalid migration path")?
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    let actual = fs::canonicalize(ancestor).map_err(|e| e.to_string())?;
    if !actual.starts_with(root) {
        return Err(format!(
            "Migration path escapes project: {}",
            path.display()
        ));
    }
    Ok(())
}
pub(super) fn migrate(root: &Path, project: &mut Project) -> Result<()> {
    if matches!(project.layout_version, 2..=5) {
        return Ok(());
    }
    if project.layout_version != 1 {
        return Err("This project needs a newer Scopework version.".into());
    }
    let backup = root.join("tools/workspaces/layout-migration-v2");
    inside(root, &backup)?;
    fs::create_dir_all(&backup).map_err(|e| e.to_string())?;
    let journal_path = backup.join("journal.json");
    let journal: Journal = if journal_path.exists() {
        serde_json::from_slice(&fs::read(&journal_path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?
    } else {
        let moves: Vec<_> = project
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, Kind::Chunk | Kind::Function))
            .map(|n| {
                (
                    legacy(n).trim_end_matches("/research.md").into(),
                    relative(project, n).trim_end_matches("/research.md").into(),
                )
            })
            .collect();
        let rewriter = Rewriter::new(&moves, root);
        let mut reports = vec![];
        for n in &project.nodes {
            let old = legacy(n);
            let new = relative(project, n);
            inside(root, &root.join(&old))?;
            let original = fs::read_to_string(root.join(&old))
                .map_err(|e| format!("Cannot migrate {old}: {e}"))?;
            let rewritten = rewriter.rewrite(&original, &old, &new);
            if old != new && root.join(&new).exists() {
                return Err(format!("Migration destination already exists: {new}"));
            }
            reports.push(ReportMove {
                id: n.id,
                old,
                new,
                original,
                rewritten,
            });
        }
        reports.sort_by_key(|r| r.new.matches('/').count());
        let journal = Journal {
            project: project.clone(),
            reports,
            annotations: if root.join("tools/profiles/annotations.json").is_file() {
                Some(
                    fs::read_to_string(root.join("tools/profiles/annotations.json"))
                        .map_err(|e| e.to_string())?,
                )
            } else {
                None
            },
        };
        atomic(
            &journal_path,
            &serde_json::to_vec_pretty(&journal).map_err(|e| e.to_string())?,
        )?;
        journal
    };
    if serde_json::to_value(&journal.project).unwrap() != serde_json::to_value(&*project).unwrap() {
        return Err("Project changed during folder migration. Keep the migration journal and restore its catalog before resuming.".into());
    }
    // Validate every report before moving any further; accept only a recorded original or output.
    for r in &journal.reports {
        let path = if root.join(&r.old).exists() {
            root.join(&r.old)
        } else {
            root.join(&r.new)
        };
        inside(root, &path)?;
        let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        if text != r.original && text != r.rewritten {
            return Err(format!(
                "Report changed during migration: {}",
                path.display()
            ));
        }
    }
    for r in &journal.reports {
        if r.old != r.new {
            let from = root.join(&r.old).parent().unwrap().to_path_buf();
            let to = root.join(&r.new).parent().unwrap().to_path_buf();
            inside(root, &from)?;
            inside(root, &to)?;
            if from.exists() {
                if to.exists() {
                    return Err(format!(
                        "Refusing to overwrite migration folder: {}",
                        to.display()
                    ));
                }
                fs::create_dir_all(to.parent().unwrap()).map_err(|e| e.to_string())?;
                fs::rename(&from, &to).map_err(|e| e.to_string())?;
            }
        }
        atomic(&root.join(&r.new), r.rewritten.as_bytes())?;
    }
    let hashes: BTreeMap<_, _> = journal
        .reports
        .iter()
        .map(|r| (r.id, (hash(&r.original), hash(&r.rewritten))))
        .collect();
    for n in &mut project.nodes {
        let (old, new) = &hashes[&n.id];
        if n.accepted_hash.as_ref() == Some(old) {
            n.accepted_hash = Some(new.clone());
        }
        for (id, stamp) in &mut n.accepted_prerequisites {
            let source = journal
                .project
                .nodes
                .iter()
                .find(|v| v.id == *id)
                .ok_or("Unknown prerequisite")?;
            let (old, new) = &hashes[id];
            if *stamp == format!("{}:{old}", source.revision) {
                *stamp = format!("{}:{new}", source.revision);
            }
        }
    }
    if let Some(original) = &journal.annotations {
        let mut ledger: serde_json::Value =
            serde_json::from_str(original).map_err(|e| e.to_string())?;
        if ledger["schema"] == 1 {
            if let Some(rows) = ledger["annotations"].as_array_mut() {
                for row in rows {
                    if let Some(r) = journal
                        .reports
                        .iter()
                        .find(|r| row["report"].as_str() == Some(&r.old))
                    {
                        row["report"] = r.new.clone().into();
                        if row["report_sha256"].as_str() == Some(&hash(&r.original)) {
                            row["report_sha256"] = hash(&r.rewritten).into();
                            row["layout_migration"] = "v2; path references only; originals in tools/workspaces/layout-migration-v2/journal.json".into();
                        }
                    }
                }
            }
            let path = root.join("tools/profiles/annotations.json");
            inside(root, &path)?;
            let current = fs::read_to_string(&path).map_err(|e| e.to_string())?;
            let rewritten = serde_json::to_string_pretty(&ledger).map_err(|e| e.to_string())?;
            if current != *original && current != rewritten {
                return Err("Annotations changed during migration; preserve the journal and resolve the concurrent edit.".into());
            }
            atomic(&path, rewritten.as_bytes())?;
        }
    }
    // Refresh generated dependency views before committing the new catalog.
    let temporary = Library {
        busy_projects: vec![],
        root: root.parent().ok_or("Missing library directory")?.into(),
        projects: vec![project.clone()],
        _lock: None,
    };
    temporary.sync_dependency_files(0)?;
    // The catalog is the commit point. The retained journal contains the original catalog and reports.
    project.layout_version = 2;
    project.revision += 1;
    atomic(
        &root.join("project.json"),
        &serde_json::to_vec_pretty(project).map_err(|e| e.to_string())?,
    )?;
    for folder in ["functions", "chunks"] {
        let path = root.join(folder);
        if inside(root, &path).is_ok() {
            let _ = fs::remove_dir(path);
        }
    }
    Ok(())
}
