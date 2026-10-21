//! Report paths in the project annotation ledger follow catalog moves.
//!
//! The ledger belongs to project tooling, which writes it outside Scopework's lock. A move
//! therefore never stores or replaces whole ledger copies: when the catalog commits, the
//! ledger's `"report"` values that name a canonical path at an outdated location are replaced
//! in the existing text. Formatting, key order and line endings stay as the tool wrote them.
//! The rewrite is idempotent, so recovery can repeat it, and it also repairs rows that a
//! concurrent tool write left on an old path.
use super::*;
use std::collections::HashMap;

pub(super) const LEDGER: &str = "tools/profiles/annotations.json";

/// Node lookups for canonical-path resolution.
pub(super) struct Resolver<'a> {
    index: catalog_index::Index<'a>,
    rvas: HashMap<u32, u64>,
}
impl<'a> Resolver<'a> {
    pub fn new(project: &'a Project) -> Self {
        Self {
            rvas: project
                .nodes
                .iter()
                .filter_map(|n| Some((n.rva?, n.id)))
                .collect(),
            index: catalog_index::Index::new(project),
        }
    }
    /// The node a canonical-shaped path names (`architectures/<id>/<id>/<RVA>/...`, old or
    /// current) and that path at the node's current location. `None` when the path has no
    /// canonical folder or names a node that does not exist.
    pub fn current(&self, path: &str) -> Option<(u64, String)> {
        let parts: Vec<&str> = path.split('/').collect();
        if parts.first() != Some(&"architectures") {
            return None;
        }
        let mut deepest = None;
        for (depth, part) in parts.iter().enumerate().skip(1).take(3) {
            let id = match depth {
                1 | 2 if part.len() == 6 && part.bytes().all(|b| b.is_ascii_digit()) => {
                    let id: u64 = part.parse().ok()?;
                    let expected = if depth == 1 {
                        Kind::Architecture
                    } else {
                        Kind::Chunk
                    };
                    (self.index.nodes.get(&id)?.kind == expected).then_some(id)?
                }
                3 if part.len() == 8 && part.bytes().all(|b| b.is_ascii_hexdigit()) => {
                    *self.rvas.get(&u32::from_str_radix(part, 16).ok()?)?
                }
                _ => break,
            };
            deepest = Some((depth, id));
        }
        let (depth, id) = deepest?;
        let folder = self.index.paths[&id].trim_end_matches("/research.md");
        let rest = &parts[depth + 1..];
        Some((
            id,
            if rest.is_empty() {
                folder.to_string()
            } else {
                format!("{folder}/{}", rest.join("/"))
            },
        ))
    }
}

impl Library {
    /// Maps report or data paths, old or current, project-relative or absolute, to their
    /// node and current location. Tools that keep paths use this after other sessions' moves.
    pub fn resolve_paths(&self, p: usize, paths: &[String]) -> serde_json::Value {
        let project = &self.projects[p];
        let resolver = Resolver::new(project);
        let dir = self.dir(p);
        let prefix = format!(
            "{}/",
            dir.to_string_lossy()
                .replace('\\', "/")
                .trim_end_matches('/')
        );
        let rows: Vec<serde_json::Value> = paths
            .iter()
            .map(|input| {
                let mut path = input.replace('\\', "/");
                if let Some(rest) = layout::strip_prefix_ignore_case(&path, &prefix) {
                    path = rest.to_string();
                }
                match resolver.current(&path) {
                    Some((id, current)) => {
                        let node = resolver.index.nodes[&id];
                        serde_json::json!({"path": input, "node": id, "kind": node.kind, "name": node.name,
                            "current": current, "absolute": dir.join(&current), "moved": current != path})
                    }
                    None => serde_json::json!({"path": input, "error": "Not a canonical path of an existing record in this project"}),
                }
            })
            .collect();
        serde_json::json!({"project": project.id, "revision": project.revision, "paths": rows})
    }
}

/// Replaces outdated `"report"` values in ledger text. `explicit` maps old folders to new ones
/// first (removed nodes have no canonical location); everything else resolves canonically.
/// Returns the new text and the number of values replaced.
pub(super) fn repath(
    text: &str,
    project: &Project,
    explicit: &[(String, String)],
) -> (String, usize) {
    static REPORT: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    static SCHEMA: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let report =
        REPORT.get_or_init(|| regex::Regex::new(r#""report"\s*:\s*"((?:[^"\\]|\\.)*)""#).unwrap());
    let schema =
        SCHEMA.get_or_init(|| regex::Regex::new(r#""schema"\s*:\s*1\s*[,}\r\n]"#).unwrap());
    if !schema.is_match(text) {
        return (text.to_string(), 0);
    }
    let resolver = Resolver::new(project);
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut replaced = 0;
    for captures in report.captures_iter(text) {
        let value = captures.get(1).unwrap();
        let Ok(path) = serde_json::from_str::<String>(&format!("\"{}\"", value.as_str())) else {
            continue;
        };
        let explicit_target = explicit.iter().find_map(|(from, to)| {
            let rest = path.strip_prefix(from.as_str())?;
            (rest.is_empty() || rest.starts_with('/')).then(|| format!("{to}{rest}"))
        });
        let Some(current) =
            explicit_target.or_else(|| resolver.current(&path).map(|(_, current)| current))
        else {
            continue;
        };
        if current == path {
            continue;
        }
        let quoted = serde_json::to_string(&current).unwrap_or_default();
        out.push_str(&text[last..value.start()]);
        out.push_str(&quoted[1..quoted.len() - 1]);
        last = value.end();
        replaced += 1;
    }
    out.push_str(&text[last..]);
    (out, replaced)
}

/// Applies [`repath`] to the project's ledger. When another writer replaces the file between
/// reading and writing, the pass starts again on the new text, so a tool's own edit is never
/// overwritten with an older copy. A ledger that keeps changing is left for the next move.
pub(super) fn apply(
    root: &Path,
    project: &Project,
    explicit: &[(String, String)],
) -> Result<usize> {
    let path = root.join(LEDGER);
    let stamp = |path: &Path| {
        fs::metadata(path)
            .ok()
            .map(|m| (m.len(), m.modified().ok()))
    };
    for _ in 0..5 {
        let Some(before) = stamp(&path) else {
            return Ok(0);
        };
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(format!("Cannot read {LEDGER}: {e}")),
        };
        let (updated, replaced) = repath(&text, project, explicit);
        if replaced == 0 {
            return Ok(0);
        }
        if stamp(&path) != Some(before) {
            continue;
        }
        atomic(&path, updated.as_bytes())?;
        return Ok(replaced);
    }
    Ok(0)
}

/// Replaces `"report_sha256"` values equal to an old hash in `pairs` with the matching new hash.
/// A report hash identifies one exact report text, so the replacement only touches rows that
/// were pinned to a report this transaction rewrote mechanically. Returns the new text and count.
pub(super) fn repin_text(text: &str, pairs: &[(String, String)]) -> (String, usize) {
    static PIN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let pin =
        PIN.get_or_init(|| regex::Regex::new(r#""report_sha256"\s*:\s*"([0-9a-f]{64})""#).unwrap());
    let map: HashMap<&str, &str> = pairs
        .iter()
        .filter(|(old, new)| old != new)
        .map(|(old, new)| (old.as_str(), new.as_str()))
        .collect();
    if map.is_empty() {
        return (text.to_string(), 0);
    }
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut replaced = 0;
    for captures in pin.captures_iter(text) {
        let value = captures.get(1).unwrap();
        if let Some(new) = map.get(value.as_str()) {
            out.push_str(&text[last..value.start()]);
            out.push_str(new);
            last = value.end();
            replaced += 1;
        }
    }
    out.push_str(&text[last..]);
    (out, replaced)
}

/// Applies [`repin_text`] to the project's ledger, with the same reread-on-change rule as
/// [`apply`], so a tool's concurrent write is never overwritten with an older copy.
pub(super) fn repin(root: &Path, pairs: &[(String, String)]) -> Result<usize> {
    let path = root.join(LEDGER);
    let stamp = |path: &Path| {
        fs::metadata(path)
            .ok()
            .map(|m| (m.len(), m.modified().ok()))
    };
    for _ in 0..5 {
        let Some(before) = stamp(&path) else {
            return Ok(0);
        };
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(format!("Cannot read {LEDGER}: {e}")),
        };
        let (updated, replaced) = repin_text(&text, pairs);
        if replaced == 0 {
            return Ok(0);
        }
        if stamp(&path) != Some(before) {
            continue;
        }
        atomic(&path, updated.as_bytes())?;
        return Ok(replaced);
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u64, parent: Option<u64>, kind: Kind, rva: Option<u32>) -> Node {
        Node {
            context_only: false,
            id,
            parent,
            kind,
            name: format!("N{id}"),
            status: Status::NotStarted,
            rva,
            revision: 0,
            accepted_hash: None,
            accepted_prerequisites: Default::default(),
        }
    }
    fn project() -> Project {
        Project {
            layout_version: 5,
            id: "fixture".into(),
            name: "Fixture".into(),
            binary: "synthetic".into(),
            research_scope: String::new(),
            next_id: 5,
            revision: 1,
            structural_epoch: 0,
            active_import: None,
            last_import: None,
            nodes: vec![
                node(0, None, Kind::App, None),
                node(1, Some(0), Kind::Architecture, None),
                node(2, Some(1), Kind::Chunk, None),
                // The function now lives in chunk 4; ledgers may still cite chunk 2.
                node(4, Some(1), Kind::Chunk, None),
                node(3, Some(4), Kind::Function, Some(0x003F_4930)),
            ],
            dependencies: vec![],
        }
    }

    #[test]
    fn resolves_old_and_current_canonical_paths() {
        let p = project();
        let r = Resolver::new(&p);
        let current = "architectures/000001/000004/003F4930/research.md";
        assert_eq!(
            r.current("architectures/000001/000002/003F4930/research.md"),
            Some((3, current.to_string()))
        );
        assert_eq!(r.current(current), Some((3, current.to_string())));
        assert_eq!(
            r.current("architectures/000001/000002/003F4930/data/x.json"),
            Some((3, "architectures/000001/000004/003F4930/data/x.json".into()))
        );
        assert_eq!(
            r.current("architectures/000001/000002/research.md"),
            Some((2, "architectures/000001/000002/research.md".into()))
        );
        assert_eq!(
            r.current("architectures/000001/000002/00009999/research.md"),
            None,
            "a path naming an unknown node is never guessed"
        );
        assert_eq!(r.current("tools/profiles/x.json"), None);
        assert_eq!(r.current("architectures/000009/research.md"), None);
    }

    #[test]
    fn repaths_only_outdated_values_and_keeps_the_text() {
        let p = project();
        let text = "{\r\n  \"annotations\": [\r\n    {\r\n      \"kind\": \"plate-comment\",\r\n      \"report\": \"architectures/000001/000002/003F4930/research.md\",\r\n      \"note\": \"source .\\\\Movement_C.cpp\"\r\n    },\r\n    {\"report\": \"architectures/000001/000002/research.md\"}\r\n  ],\r\n  \"schema\": 1\r\n}\r\n";
        let (out, replaced) = repath(text, &p, &[]);
        assert_eq!(replaced, 1);
        assert_eq!(
            out,
            text.replace("000002/003F4930/research.md", "000004/003F4930/research.md")
        );
        assert_eq!(repath(&out, &p, &[]), (out.clone(), 0), "idempotent");
        let (archived, n) = repath(
            text,
            &p,
            &[(
                "architectures/000001/000002".into(),
                "tools/workspaces/deleted-nodes/revision-2/2".into(),
            )],
        );
        assert_eq!(n, 2);
        assert!(
            archived
                .contains("\"tools/workspaces/deleted-nodes/revision-2/2/003F4930/research.md\"")
        );
        let other =
            "{\"schema\": 2, \"report\": \"architectures/000001/000002/003F4930/research.md\"}";
        assert_eq!(
            repath(other, &p, &[]).1,
            0,
            "unknown ledger schemas are left alone"
        );
    }
}
