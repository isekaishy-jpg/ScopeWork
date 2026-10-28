//! A project's artifacts (input files) and address spaces besides the default one
//! (docs/design-storage-promotion.md §2). Each change is a journaled catalog transaction, so
//! it leaves a receipt like any other; the first one raises the catalog to format 7.
use super::*;

/// The space named `name` or numbered `id` in `project`, for messages.
pub(crate) fn space_label(project: &Project, id: u32) -> String {
    project
        .spaces
        .iter()
        .find(|s| s.id == id)
        .map_or_else(|| format!("S{id}"), |s| format!("{} (S{id})", s.name))
}

/// An optional text field as given on the command line: absent leaves it, empty clears it.
fn set_text(field: &mut Option<String>, value: Option<&str>) {
    if let Some(v) = value {
        *field = Some(v.trim().to_string()).filter(|v| !v.is_empty());
    }
}

/// Fields of an artifact or space change; `None` leaves a field as it is.
#[derive(Default)]
pub struct InputFields<'a> {
    pub name: Option<&'a str>,
    pub sha256: Option<&'a str>,
    pub size: Option<u64>,
    pub format: Option<&'a str>,
    pub inside: Option<u32>,
    pub path: Option<&'a str>,
    pub kind: Option<SpaceKind>,
    pub artifact: Option<u32>,
    pub processor: Option<&'a str>,
    pub base: Option<u64>,
    pub description: Option<&'a str>,
}
impl InputFields<'_> {
    fn artifact(&self, a: &mut Artifact) {
        if let Some(name) = self.name {
            a.name = name.trim().into();
        }
        if let Some(sha) = self.sha256 {
            a.sha256 = sha.trim().to_ascii_lowercase();
        }
        a.size = self.size.or(a.size);
        a.inside = self.inside.or(a.inside);
        set_text(&mut a.format, self.format);
        set_text(&mut a.path, self.path);
        set_text(&mut a.description, self.description);
    }
    fn space(&self, s: &mut Space) {
        if let Some(name) = self.name {
            s.name = name.trim().into();
        }
        s.kind = self.kind.unwrap_or(s.kind);
        s.artifact = self.artifact.or(s.artifact);
        s.base = self.base.or(s.base);
        set_text(&mut s.processor, self.processor);
        set_text(&mut s.description, self.description);
    }
}

impl Library {
    /// Applies `edit` to project `p`'s artifacts and spaces as one catalog transaction, when
    /// `expected_revision` is still current. An edit that changes nothing commits nothing.
    fn edit_inputs<T>(
        &mut self,
        p: usize,
        expected_revision: u64,
        edit: impl FnOnce(&mut Project) -> Result<T>,
    ) -> Result<T> {
        if !self.writable(p) {
            return self.write_project(p, |l, p| l.edit_inputs(p, expected_revision, edit));
        }
        if self.projects[p].revision != expected_revision {
            return Err(
                "Project changed. Reload and review its artifacts and spaces before saving.".into(),
            );
        }
        let before = self.projects[p].clone();
        let mut after = before.clone();
        let out = edit(&mut after)?;
        validate_inputs(&after)?;
        if after == before {
            return Ok(out);
        }
        raise_format(&mut after);
        after.revision += 1;
        self.projects[p] = after;
        self.commit_catalog(p, before)?;
        Ok(out)
    }
    /// Adds an artifact; answers its identity.
    pub fn add_artifact(&mut self, p: usize, fields: &InputFields, rev: u64) -> Result<u32> {
        if fields.name.is_none() || fields.sha256.is_none() {
            return Err("An artifact needs --name and --sha256.".into());
        }
        self.edit_inputs(p, rev, |project| {
            let id = project
                .artifacts
                .iter()
                .map(|a| a.id + 1)
                .max()
                .unwrap_or(1);
            let mut a = Artifact {
                id,
                name: String::new(),
                sha256: String::new(),
                size: None,
                format: None,
                inside: None,
                path: None,
                description: None,
            };
            fields.artifact(&mut a);
            project.artifacts.push(a);
            Ok(id)
        })
    }
    pub fn update_artifact(
        &mut self,
        p: usize,
        id: u32,
        fields: &InputFields,
        rev: u64,
    ) -> Result<()> {
        self.edit_inputs(p, rev, |project| {
            let a = project
                .artifacts
                .iter_mut()
                .find(|a| a.id == id)
                .ok_or_else(|| format!("Artifact {id} does not exist."))?;
            fields.artifact(a);
            Ok(())
        })
    }
    /// Removes an artifact no space names and no artifact lies inside.
    pub fn remove_artifact(&mut self, p: usize, id: u32, rev: u64) -> Result<()> {
        self.edit_inputs(p, rev, |project| {
            if project.spaces.iter().any(|s| s.artifact == Some(id))
                || project.artifacts.iter().any(|a| a.inside == Some(id))
            {
                return Err(format!(
                    "Artifact {id} is in use by a space or another artifact; change those first."
                ));
            }
            let before = project.artifacts.len();
            project.artifacts.retain(|a| a.id != id);
            if project.artifacts.len() == before {
                return Err(format!("Artifact {id} does not exist."));
            }
            Ok(())
        })
    }
    /// Adds an address space; answers its identity.
    pub fn add_space(&mut self, p: usize, fields: &InputFields, rev: u64) -> Result<u32> {
        let (Some(_), Some(kind)) = (fields.name, fields.kind) else {
            return Err("A space needs --name and --kind.".into());
        };
        self.edit_inputs(p, rev, |project| {
            let id = project.spaces.iter().map(|s| s.id + 1).max().unwrap_or(1);
            let mut s = Space {
                id,
                name: String::new(),
                kind,
                artifact: None,
                processor: None,
                base: None,
                description: None,
            };
            fields.space(&mut s);
            project.spaces.push(s);
            Ok(id)
        })
    }
    pub fn update_space(
        &mut self,
        p: usize,
        id: u32,
        fields: &InputFields,
        rev: u64,
    ) -> Result<()> {
        self.edit_inputs(p, rev, |project| {
            let s = project
                .spaces
                .iter_mut()
                .find(|s| s.id == id)
                .ok_or_else(|| format!("Space {id} does not exist."))?;
            fields.space(s);
            Ok(())
        })
    }
    /// Removes a space no record is in.
    pub fn remove_space(&mut self, p: usize, id: u32, rev: u64) -> Result<()> {
        self.edit_inputs(p, rev, |project| {
            if project.nodes.iter().any(|n| n.space == Some(id)) {
                return Err(format!(
                    "Space {id} still holds functions or data items; delete them first."
                ));
            }
            let before = project.spaces.len();
            project.spaces.retain(|s| s.id != id);
            if project.spaces.len() == before {
                return Err(format!("Space {id} does not exist."));
            }
            Ok(())
        })
    }
}
