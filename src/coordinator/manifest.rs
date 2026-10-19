//! A project's tool declarations, `tools/coordinator.toml`: the shared resources tools lease,
//! the files that belong to each (watched for drift), and named kinds of tool work with their
//! resources, key and class. Every field can be overridden by flags; an undeclared kind can
//! still take a plain lease. Scopework knows nothing about particular tools beyond this file.
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

pub const MANIFEST: &str = "tools/coordinator.toml";

#[derive(Deserialize, Default, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u32,
    #[serde(default)]
    pub resources: BTreeMap<String, ResourceDecl>,
    #[serde(default)]
    pub kinds: BTreeMap<String, KindDecl>,
}
#[derive(Deserialize, Default, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ResourceDecl {
    /// Project-relative files that only lease holders of this resource should change.
    #[serde(default)]
    pub files: Vec<String>,
}
#[derive(Deserialize, Default, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct KindDecl {
    pub resources: Vec<String>,
    /// Requests with the same key and resources join one lease.
    pub key: Option<String>,
    pub class: Option<String>,
}

/// What one request asks for, after the manifest and flags are combined.
#[derive(Debug, Clone, PartialEq)]
pub struct LeaseSpec {
    pub kind: String,
    pub resources: Vec<String>,
    pub key: Option<String>,
    pub class: String,
}

/// Lowercase words joined by hyphens, as for history kinds.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
fn valid_class(class: &str) -> bool {
    matches!(class, "interactive" | "normal" | "bulk" | "job")
}

/// The project's manifest, or an empty one when it has none.
pub fn load(project_dir: &Path) -> Result<Manifest, String> {
    let path = project_dir.join(MANIFEST);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Manifest {
                version: 1,
                ..Manifest::default()
            });
        }
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    parse(&text).map_err(|e| format!("manifest_invalid: {MANIFEST}: {e}"))
}
pub fn parse(text: &str) -> Result<Manifest, String> {
    let manifest: Manifest = toml::from_str(text).map_err(|e| e.to_string())?;
    if manifest.version != 1 {
        return Err(format!("unsupported version {}", manifest.version));
    }
    for (name, resource) in &manifest.resources {
        if !valid_name(name) {
            return Err(format!(
                "resource name '{name}' must be lowercase words joined by hyphens"
            ));
        }
        for file in &resource.files {
            let path = Path::new(file);
            if file.is_empty()
                || path.is_absolute()
                || path
                    .components()
                    .any(|c| !matches!(c, std::path::Component::Normal(_)))
            {
                return Err(format!(
                    "resource '{name}': file '{file}' must be a plain project-relative path"
                ));
            }
        }
    }
    for (name, kind) in &manifest.kinds {
        if !valid_name(name) {
            return Err(format!(
                "kind name '{name}' must be lowercase words joined by hyphens"
            ));
        }
        if kind.resources.is_empty() {
            return Err(format!("kind '{name}' names no resources"));
        }
        if let Some(bad) = kind.resources.iter().find(|r| !valid_name(r)) {
            return Err(format!("kind '{name}': resource name '{bad}' is invalid"));
        }
        if let Some(class) = &kind.class
            && !valid_class(class)
        {
            return Err(format!("kind '{name}': unknown class '{class}'"));
        }
    }
    Ok(manifest)
}

impl Manifest {
    /// Combines a declared kind (if any) with flags; flags win field by field.
    pub fn resolve(
        &self,
        kind: Option<&str>,
        resources: &[String],
        key: Option<&str>,
        class: Option<&str>,
    ) -> Result<LeaseSpec, String> {
        let declared = kind.and_then(|k| self.kinds.get(k));
        let kind_name = kind.unwrap_or("lease").to_string();
        if !valid_name(&kind_name) {
            return Err(format!(
                "lease_refused: kind '{kind_name}' must be lowercase words joined by hyphens"
            ));
        }
        let mut resources: Vec<String> = if resources.is_empty() {
            declared.map(|d| d.resources.clone()).unwrap_or_default()
        } else {
            resources.to_vec()
        };
        resources.sort();
        resources.dedup();
        if resources.is_empty() {
            return Err(match kind {
                Some(k) => format!(
                    "lease_refused: kind '{k}' is not declared in {MANIFEST}; name its resources with --resource"
                ),
                None => "lease_refused: name a declared --kind or at least one --resource".into(),
            });
        }
        if let Some(bad) = resources.iter().find(|r| !valid_name(r)) {
            return Err(format!(
                "lease_refused: resource name '{bad}' must be lowercase words joined by hyphens"
            ));
        }
        let key = key
            .map(str::to_string)
            .or_else(|| declared.and_then(|d| d.key.clone()));
        let class = class
            .map(str::to_string)
            .or_else(|| declared.and_then(|d| d.class.clone()))
            .unwrap_or_else(|| "normal".into());
        if !valid_class(&class) {
            return Err(format!("lease_refused: unknown class '{class}'"));
        }
        Ok(LeaseSpec {
            kind: kind_name,
            resources,
            key,
            class,
        })
    }
    /// Declared files per resource, for drift detection.
    pub fn watched(&self) -> Vec<(String, String)> {
        self.resources
            .iter()
            .flat_map(|(name, r)| r.files.iter().map(move |f| (name.clone(), f.clone())))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"
version = 1

[resources.ledger]
files = ["tools/profiles/annotations.json"]

[resources.ghidra]

[kinds.ledger-put]
resources = ["ledger"]
key = "ledger-put"

[kinds.annotations-apply]
resources = ["ledger", "ghidra"]
key = "annotations-apply"
class = "job"
"#;

    #[test]
    fn declarations_resolve_and_flags_override_them() {
        let m = parse(EXAMPLE).unwrap();
        assert_eq!(
            m.watched(),
            [(
                "ledger".to_string(),
                "tools/profiles/annotations.json".to_string()
            )]
        );
        let apply = m
            .resolve(Some("annotations-apply"), &[], None, None)
            .unwrap();
        assert_eq!(apply.resources, ["ghidra", "ledger"]);
        assert_eq!(
            (apply.key.as_deref(), apply.class.as_str()),
            (Some("annotations-apply"), "job")
        );
        let overridden = m
            .resolve(
                Some("ledger-put"),
                &["ghidra".into()],
                Some("other"),
                Some("bulk"),
            )
            .unwrap();
        assert_eq!(overridden.resources, ["ghidra"]);
        assert_eq!(
            (overridden.key.as_deref(), overridden.class.as_str()),
            (Some("other"), "bulk")
        );
        // An undeclared kind leases with flags; without resources it is refused.
        assert_eq!(
            m.resolve(Some("new-tool"), &["ledger".into()], None, None)
                .unwrap()
                .class,
            "normal"
        );
        assert!(
            m.resolve(Some("new-tool"), &[], None, None)
                .unwrap_err()
                .starts_with("lease_refused")
        );
    }

    #[test]
    fn mistakes_are_refused_with_their_place() {
        let typo =
            parse("version = 1\n[kinds.x]\nresources = [\"a\"]\nclas = \"job\"\n").unwrap_err();
        assert!(typo.contains("clas") && typo.contains("line 4"), "{typo}");
        assert!(parse("version = 2").unwrap_err().contains("version"));
        assert!(
            parse("version = 1\n[resources.Ledger]")
                .unwrap_err()
                .contains("Ledger")
        );
        assert!(
            parse("version = 1\n[resources.a]\nfiles = [\"../x\"]")
                .unwrap_err()
                .contains("../x")
        );
        assert!(
            parse("version = 1\n[kinds.k]\nresources = []")
                .unwrap_err()
                .contains("no resources")
        );
        assert!(
            parse("version = 1\n[kinds.k]\nresources = [\"a\"]\nclass = \"urgent\"")
                .unwrap_err()
                .contains("urgent")
        );
    }
}
