//! Purpose-bound record edits. Project revisions remain strict legacy preconditions.
use super::*;
use std::collections::HashSet;

pub(super) fn parts(text: &str) -> Result<(&str, &str)> {
    text.split_once("\n<!-- research-content -->\n")
        .ok_or("Missing report content marker")?
        .1
        .split_once("\n<!-- research-evidence -->\n")
        .ok_or_else(|| "Missing evidence marker".into())
}

pub(super) fn draft(
    project: &Project,
    index: &catalog_index::Index<'_>,
    id: u64,
    text: &str,
) -> Result<String> {
    let (body, evidence) = parts(text)?;
    if index.render(project, id, body, evidence) != text {
        return Err(format!(
            "malformed_report: generated envelope for node {id} changed externally; restore/review it before editing"
        ));
    }
    let mut lineage = vec![];
    let mut cursor = Some(id);
    while let Some(id) = cursor {
        let node = index.nodes.get(&id).ok_or("Unknown report")?;
        lineage.push((
            node.id,
            node.parent,
            node.kind,
            node.name.as_str(),
            node.rva,
            node.context_only,
        ));
        cursor = node.parent;
    }
    let dependencies = index.requires.get(&id).cloned().unwrap_or_default();
    Ok(format!(
        "d4:{}",
        hash(
            &serde_json::to_string(&(
                project.id.as_str(),
                project.binary.as_str(),
                lineage,
                dependencies,
                body,
                evidence
            ))
            .map_err(|e| e.to_string())?
        )
    ))
}

impl Library {
    pub fn report_token(&self, p: usize, id: u64) -> Result<String> {
        let index = catalog_index::Index::new(&self.projects[p]);
        let path = index.paths.get(&id).ok_or("Unknown report")?;
        draft(
            &self.projects[p],
            &index,
            id,
            &fs::read_to_string(self.dir(p).join(path)).map_err(|e| e.to_string())?,
        )
    }
    pub fn completion_token(&self, p: usize, id: u64) -> Result<String> {
        let project = &self.projects[p];
        let index = catalog_index::Index::new(project);
        let path = index.paths.get(&id).ok_or("Unknown report")?;
        let own = draft(
            project,
            &index,
            id,
            &fs::read_to_string(self.dir(p).join(path)).map_err(|e| e.to_string())?,
        )?;
        completion(&index, &self.dir(p), id, &own)
    }
    pub fn structural_token(&self, p: usize, id: u64) -> Result<String> {
        Ok(format!(
            "s4:{}",
            hash(&format!(
                "{}:{}",
                self.projects[p].structural_epoch,
                self.report_token(p, id)?
            ))
        ))
    }
    pub fn check_edit_token(&self, p: usize, id: u64, token: &str, purpose: &str) -> Result<()> {
        let expected = match purpose {
            "draft" if token.starts_with("d4:") => self.report_token(p, id)?,
            "completion" if token.starts_with("c4:") => self.completion_token(p, id)?,
            "structural" if token.starts_with("s4:") => self.structural_token(p, id)?,
            _ if !token.contains(':') => {
                let content = fs::read_to_string(self.path(p, id)?).map_err(|e| e.to_string())?;
                hash(&format!("{}\n{content}", self.projects[p].revision))
            }
            _ => {
                return Err(format!(
                    "token_purpose: {purpose} requires its own token from a fresh node show"
                ));
            }
        };
        if token != expected {
            return Err(format!(
                "record_conflict: node {id} or its relevant {purpose} inputs changed; reload and review"
            ));
        }
        Ok(())
    }
}

fn completion(index: &catalog_index::Index<'_>, dir: &Path, id: u64, own: &str) -> Result<String> {
    let mut queue = vec![id];
    let mut seen = HashSet::new();
    while let Some(id) = queue.pop() {
        if !seen.insert(id) {
            continue;
        }
        if let Some(kids) = index.children.get(&id) {
            queue.extend(kids.iter().filter(|k| !index.nodes[k].context_only));
        }
        if let Some(deps) = index.requires.get(&id) {
            queue.extend(deps);
        }
    }
    let states = index.states_for(dir, seen.iter().copied());
    let mut inputs = vec![];
    for id in seen {
        let n = index.nodes[&id];
        let actual = if n.status == Status::Complete {
            Some(hash(
                &fs::read_to_string(dir.join(&index.paths[&id])).map_err(|e| e.to_string())?,
            ))
        } else {
            None
        };
        inputs.push((
            id,
            n.revision,
            n.status,
            states[&id],
            n.accepted_hash.clone(),
            actual,
        ));
    }
    inputs.sort_by_key(|i| i.0);
    Ok(format!(
        "c4:{}",
        hash(&serde_json::to_string(&(own, inputs)).map_err(|e| e.to_string())?)
    ))
}

#[derive(Serialize)]
pub struct RecordRead {
    pub project: String,
    pub node: Node,
    pub effective_status: Status,
    pub report: PathBuf,
    pub data: Option<PathBuf>,
    pub body: String,
    pub evidence: String,
    pub token: String,
    pub completion_token: String,
    pub structural_token: String,
    pub criteria: &'static str,
    pub research_scope: String,
    pub children: Vec<u64>,
    pub requires: Vec<u64>,
    pub required_by: Vec<u64>,
    pub binary: String,
    pub revision: u64,
    pub dependency_rvas: String,
}
impl Library {
    pub fn record(&self, p: usize, id: u64) -> Result<RecordRead> {
        let project = &self.projects[p];
        let index = catalog_index::Index::new(project);
        let node = index.nodes.get(&id).ok_or("Unknown node")?;
        let report = self.dir(p).join(&index.paths[&id]);
        layout::inside(&self.dir(p), &report)?;
        let text = fs::read_to_string(&report).map_err(|e| e.to_string())?;
        let (body, evidence) = parts(&text)?;
        let token = draft(project, &index, id, &text)?;
        let completion_token = completion(&index, &self.dir(p), id, &token)?;
        let structural_token = format!(
            "s4:{}",
            hash(&format!("{}:{}", project.structural_epoch, token))
        );
        let effective_status = index.states_for(&self.dir(p), [id])[&id];
        let requires = index.requires.get(&id).cloned().unwrap_or_default();
        let dependency_rvas = requires
            .iter()
            .filter_map(|id| index.nodes[id].rva)
            .map(|r| format!("0x{r:08X}"))
            .collect::<Vec<_>>()
            .join(", ");
        Ok(RecordRead {
            project: project.id.clone(),
            node: (**node).clone(),
            effective_status,
            data: (node.kind == Kind::Function).then(|| report.parent().unwrap().join("data")),
            report,
            body: body.into(),
            evidence: evidence.into(),
            token,
            completion_token,
            structural_token,
            criteria: node.kind.completion_criteria(),
            research_scope: project.research_scope.clone(),
            children: index.children.get(&id).cloned().unwrap_or_default(),
            requires,
            required_by: project
                .dependencies
                .iter()
                .filter(|e| e.prerequisite == id)
                .map(|e| e.dependent)
                .collect(),
            binary: project.binary.clone(),
            revision: project.revision,
            dependency_rvas,
        })
    }
}
