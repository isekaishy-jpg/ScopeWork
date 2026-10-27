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

/// `text` after `prefix`, compared ignoring ASCII case (Windows paths), or `None`. Never splits
/// a character, whatever `text` holds.
pub(super) fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
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
/// The project path a relative `path` names from the report at `from`, or `None` above the root.
fn resolve(from: &str, path: &str) -> Option<String> {
    let mut parts: Vec<_> = from.split('/').collect();
    parts.pop();
    for component in path.split('/') {
        match component {
            ".." => {
                parts.pop()?;
            }
            "." | "" => {}
            _ => parts.push(component),
        }
    }
    Some(parts.join("/"))
}
/// A record's report path in format 1. That format predates data items (`migrate` refuses a
/// format 1 catalog holding one), so only the four original kinds have a location there.
fn legacy(n: &Node) -> String {
    match n.kind {
        Kind::App => "app.md".into(),
        Kind::Architecture => format!("architectures/{:06}/research.md", n.id),
        Kind::Chunk => format!("chunks/{:06}/research.md", n.id),
        Kind::Function | Kind::Data => format!("functions/{:08X}/research.md", n.rva.unwrap()),
    }
}
/// Indexed path rewriting: cost follows the number of path components, not catalog size.
///
/// A reference is rewritten only when it must change: the project path it names lies under a
/// moved folder, or the report holding a relative reference moved and the named project file
/// exists. Every other reference, including quoted source strings such as `.\Movement_C.cpp`
/// or `..\src\file.cpp`, keeps its exact text.
pub(super) struct Rewriter {
    moves: std::collections::HashMap<String, String>,
    root: String,
    project: PathBuf,
    destinations: regex::Regex,
    paths: regex::Regex,
}
impl Rewriter {
    pub fn new(moves: &[(String, String)], root: &Path) -> Self {
        Self {
            moves: moves.iter().cloned().collect(),
            root: format!("{}/", root.to_string_lossy().replace('\\', "/").trim_end_matches('/')),
            project: root.to_path_buf(),
            destinations: regex::Regex::new(r"\]\((<[^>]+>|[^\s)]+)\)").unwrap(),
            paths: regex::Regex::new(r#"(?:[A-Za-z][A-Za-z0-9+.-]*://|[A-Za-z]:[\\/]|/|\.{1,2}[\\/]|architectures/|functions/|chunks/|tools/)[^\s`<>\"'()\[\]]+"#).unwrap(),
        }
    }
    /// The moved location of `path`, or `None` when no moved folder contains it.
    fn remap(&self, path: &str) -> Option<String> {
        let mut prefix = path;
        loop {
            if let Some(to) = self.moves.get(prefix) {
                return Some(format!("{to}{}", &path[prefix.len()..]));
            }
            let (parent, _) = prefix.rsplit_once('/')?;
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
        let rewritten = if let Some(local) = strip_prefix_ignore_case(local, &self.root) {
            let end = local.find(['#', '?']).unwrap_or(local.len());
            let Some(moved) = self.remap(&local[..end]) else {
                return target.into();
            };
            format!("{uri_prefix}{}{moved}{}", self.root, &local[end..])
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
                let Some(moved) = self.remap(path) else {
                    return target.into();
                };
                format!("{moved}{suffix}")
            } else {
                let Some(resolved) = resolve(old, path) else {
                    return target.into();
                };
                let destination = match self.remap(&resolved) {
                    Some(moved) => moved,
                    // Only a moved report's links to existing project files need rebasing.
                    None if old != new && self.project.join(&resolved).exists() => resolved,
                    None => return target.into(),
                };
                // A reference that already names the destination from the new location stays as written.
                if resolve(new, path).as_deref() == Some(destination.as_str()) {
                    return target.into();
                }
                format!("{}{suffix}", link(new, &destination))
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
    if matches!(project.layout_version, 2..=DATA_FORMAT) {
        return Ok(());
    }
    if project.layout_version != 1 {
        return Err("This project needs a newer Scopework version.".into());
    }
    if project.nodes.iter().any(|n| n.kind == Kind::Data) {
        return Err("Invalid research hierarchy: format 1 has no data items.".into());
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
                rename_retrying(&from, &to)
                    .map_err(|e| format!("Cannot move {}: {e}", from.display()))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn fixture() -> Fixture {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-workspaces")
            .join(format!("rewriter-{}", uuid::Uuid::new_v4()));
        for file in [
            "architectures/000001/000002/research.md",
            "architectures/000001/000002/00002000/research.md",
        ] {
            fs::create_dir_all(root.join(file).parent().unwrap()).unwrap();
            fs::write(root.join(file), "report").unwrap();
        }
        Fixture(root)
    }
    const MOVED: (&str, &str) = (
        "architectures/000001/000002/00001000",
        "architectures/000001/000003/00001000",
    );

    #[test]
    fn unrelated_references_and_source_literals_keep_their_bytes() {
        let f = fixture();
        let rewriter = Rewriter::new(&[(MOVED.0.into(), MOVED.1.into())], &f.0);
        let report = "architectures/000001/000004/research.md";
        let text = concat!(
            r"Strings `.\Movement_C.cpp`, .\MovementShared.cpp and `..\src\fmod_file_cdda.cpp`; ",
            "[chunk](../000002/research.md), [same](./research.md), see architectures/000001/000002/research.md."
        );
        assert_eq!(rewriter.rewrite(text, report, report), text);
    }

    #[test]
    fn paths_whose_case_folding_changes_length_never_split_a_character() {
        let f = fixture();
        let rewriter = Rewriter::new(&[(MOVED.0.into(), MOVED.1.into())], &f.0);
        let report = "architectures/000001/000004/research.md";
        // The Kelvin sign lowercases to an ASCII k but takes three bytes: a project path
        // spelled with it is not this project's path, and must not be sliced mid-character.
        let root = format!("{}/", f.0.to_string_lossy().replace('\\', "/"));
        assert!(root.contains('k'));
        let text = format!(
            "see {}{}/research.md",
            root.replace('k', "\u{212A}"),
            MOVED.0
        );
        assert_eq!(rewriter.rewrite(&text, report, report), text);
    }

    #[test]
    fn references_to_a_moved_folder_follow_it() {
        let f = fixture();
        let rewriter = Rewriter::new(&[(MOVED.0.into(), MOVED.1.into())], &f.0);
        let report = "architectures/000001/000004/research.md";
        let text = r"[f](../000002/00001000/research.md) and architectures/000001/000002/00001000/data/x.json `.\a.cpp`";
        assert_eq!(
            rewriter.rewrite(text, report, report),
            r"[f](../000003/00001000/research.md) and architectures/000001/000003/00001000/data/x.json `.\a.cpp`"
        );
    }

    #[test]
    fn a_moved_report_rebases_only_links_to_existing_project_files() {
        let f = fixture();
        let rewriter = Rewriter::new(&[(MOVED.0.into(), MOVED.1.into())], &f.0);
        let old = format!("{}/research.md", MOVED.0);
        let new = format!("{}/research.md", MOVED.1);
        let text = concat!(
            "[chunk](../research.md) [sibling](../00002000/research.md) [data](./data/x.json) ",
            r"`..\src\a.cpp` `.\b.cpp` ../missing/file.md"
        );
        assert_eq!(
            rewriter.rewrite(text, &old, &new),
            concat!(
                "[chunk](../../000002/research.md) [sibling](../../000002/00002000/research.md) [data](./data/x.json) ",
                r"`..\src\a.cpp` `.\b.cpp` ../missing/file.md"
            )
        );
    }
}
