//! `research tools audit`: an advisory report of what a project's tool sources appear to use,
//! against its manifest. It is a textual scan, deliberately: it finds imports of the project's
//! own modules, `research` subcommands and subprocess executables, but cannot say which files a
//! tool writes (research found 89% of write targets unresolvable even with a parser). Runtime
//! drift entries cover writes; this report only hints at declarations to add or remove.
use super::manifest;
use regex::Regex;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const SOURCES: [&str; 2] = ["tools/scripts", "tools/instrumentation"];
const EXTENSIONS: [&str; 8] = ["py", "java", "ps1", "cmd", "bat", "js", "ts", "sh"];
const LARGEST: u64 = 2 << 20;

fn sources(dir: &Path) -> Vec<PathBuf> {
    let mut found = vec![];
    let mut pending: Vec<PathBuf> = SOURCES.iter().map(|s| dir.join(s)).collect();
    while let Some(folder) = pending.pop() {
        for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file()
                && path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
                && entry.metadata().is_ok_and(|m| m.len() <= LARGEST)
            {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// Counts, per name, the files in which a pattern's first capture appears.
fn count(
    files: &[(PathBuf, String)],
    pattern: &Regex,
    keep: impl Fn(&str) -> bool,
) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for (_, text) in files {
        let names: BTreeSet<String> = pattern
            .captures_iter(text)
            .filter_map(|c| c.get(1).map(|m| m.as_str().trim().to_string()))
            .filter(|n| keep(n))
            .collect();
        for name in names {
            *counts.entry(name).or_insert(0) += 1;
        }
    }
    counts
}

pub fn run(dir: &Path) -> Result<Value, String> {
    let manifest = manifest::load(dir)?;
    let files: Vec<(PathBuf, String)> = sources(dir)
        .into_iter()
        .filter_map(|p| {
            std::fs::read(&p)
                .ok()
                .map(|b| (p, String::from_utf8_lossy(&b).into_owned()))
        })
        .collect();
    // The project's own modules: Python files anywhere in the scanned trees.
    let local: BTreeSet<String> = files
        .iter()
        .filter(|(p, _)| p.extension().is_some_and(|e| e == "py"))
        .filter_map(|(p, _)| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect();
    let import =
        Regex::new(r"(?m)^\s*(?:from\s+([A-Za-z_]\w*)[\w.]*\s+import|import\s+([A-Za-z_]\w*))")
            .unwrap();
    let mut modules: BTreeMap<String, usize> = BTreeMap::new();
    for (_, text) in &files {
        let names: BTreeSet<String> = import
            .captures_iter(text)
            .filter_map(|c| {
                c.get(1)
                    .or_else(|| c.get(2))
                    .map(|m| m.as_str().to_string())
            })
            .filter(|n| local.contains(n))
            .collect();
        for name in names {
            *modules.entry(name).or_insert(0) += 1;
        }
    }
    let commands = count(
        &files,
        &Regex::new(r#"['"]((?:node|deps|claim|project|history|review-queue|status|job|journal|import|criteria|wait|locks|queue)['"]\s*,\s*['"][a-z][a-z-]*)['"]"#).unwrap(),
        |_| true,
    )
    .into_iter()
    .map(|(k, v)| (k.replace(['"', '\''], "").split(',').map(str::trim).collect::<Vec<_>>().join(" "), v))
    .collect::<BTreeMap<_, _>>();
    let executables = count(
        &files,
        &Regex::new(
            r"subprocess\.(?:run|Popen|call|check_call|check_output)\(\s*\[\s*([^,\]\n]{1,60})",
        )
        .unwrap(),
        |_| true,
    );
    let referenced = count(
        &files,
        &Regex::new(r#"--kind['"]?\s*[,= ]\s*['"]?([a-z0-9][a-z0-9-]*)"#).unwrap(),
        |_| true,
    );

    // Kinds that leased through a coordinator, and drift recorded, from the tooling history.
    let mut leased = BTreeSet::new();
    let mut drift: BTreeMap<String, usize> = BTreeMap::new();
    if let Ok(log) = std::fs::read_to_string(dir.join(crate::research::history::TOOLING_LOG)) {
        for entry in log
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        {
            match entry["entry_kind"].as_str() {
                Some("coordinator-leases") => {
                    if let Some(kinds) = entry["diff"]["kinds"].as_object() {
                        leased.extend(kinds.keys().cloned());
                    }
                }
                Some("drift") => {
                    let resource = entry["diff"]["resource"]
                        .as_str()
                        .unwrap_or("?")
                        .to_string();
                    *drift.entry(resource).or_insert(0) += 1;
                }
                _ => {}
            }
        }
    }
    let declared: BTreeSet<String> = manifest.kinds.keys().cloned().collect();
    let used: BTreeSet<String> = leased
        .iter()
        .cloned()
        .chain(referenced.keys().cloned())
        .collect();
    Ok(json!({
        "advisory": true,
        "note": "A textual scan of tool sources: it shows what tools appear to use, not which files they write. Drift entries record writes at runtime.",
        "files_scanned": files.len(),
        "modules": modules,
        "research_commands": commands,
        "executables": executables,
        "kinds": {
            "declared": declared,
            "referenced_in_sources": referenced,
            "leased": leased,
            "declared_unused": declared.difference(&used).collect::<Vec<_>>(),
            "used_undeclared": used.difference(&declared).collect::<Vec<_>>(),
        },
        "resources": manifest.resources.keys().collect::<Vec<_>>(),
        "drift_entries": drift,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_reports_usage_against_declarations() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-workspaces")
            .join(format!("audit-{}", uuid::Uuid::new_v4()));
        struct Clean(PathBuf);
        impl Drop for Clean {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _clean = Clean(dir.clone());
        let scripts = dir.join("tools/scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::write(scripts.join("shared.py"), "import json\n").unwrap();
        std::fs::write(
            scripts.join("tool.py"),
            "import shared, os\nfrom shared import x\nimport subprocess\nrun('node','save',p)\nsubprocess.run(['rizin','-q'])\nargs=['job','run','--kind','ledger-put']\n",
        )
        .unwrap();
        std::fs::write(
            dir.join(manifest::MANIFEST),
            "version = 1\n[resources.ledger]\nfiles = [\"a.json\"]\n[kinds.ledger-put]\nresources = [\"ledger\"]\n[kinds.old]\nresources = [\"ledger\"]\n",
        )
        .unwrap();
        let report = run(&dir).unwrap();
        assert_eq!(report["files_scanned"], 2);
        assert_eq!(report["modules"], json!({"shared": 1}));
        assert_eq!(report["research_commands"]["node save"], 1);
        assert_eq!(report["executables"]["'rizin'"], 1);
        assert_eq!(report["kinds"]["declared_unused"], json!(["old"]));
        assert_eq!(report["kinds"]["used_undeclared"], json!([]));
    }
}
