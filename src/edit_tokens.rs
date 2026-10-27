//! Purpose-bound record edits. Project revisions remain strict legacy preconditions.
use super::*;
use std::collections::HashSet;

/// A report's body and evidence, split at the markers every rendered report carries.
pub(super) fn parts(text: &str) -> Result<(&str, &str)> {
    use catalog_index::{CONTENT_MARKER, EVIDENCE_MARKER};
    text.split_once(&format!("\n{CONTENT_MARKER}\n"))
        .ok_or("Report marker is missing; restore the canonical report header before editing.")?
        .1
        .split_once(&format!("\n{EVIDENCE_MARKER}\n"))
        .ok_or_else(|| {
            "Evidence marker is missing; restore the canonical report layout before editing.".into()
        })
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

/// The prerequisite-list token: purpose, project, binary, record and its prerequisites.
fn prerequisites(project: &Project, index: &catalog_index::Index<'_>, id: u64) -> Result<String> {
    if !index.nodes.contains_key(&id) {
        return Err("Unknown report".into());
    }
    let mut dependencies = index.requires.get(&id).cloned().unwrap_or_default();
    dependencies.sort_unstable();
    Ok(format!(
        "s5:{}",
        hash(
            &serde_json::to_string(&(
                "prerequisites",
                project.id.as_str(),
                project.binary.as_str(),
                id,
                dependencies
            ))
            .map_err(|e| e.to_string())?
        )
    ))
}

impl Library {
    pub fn report_token(&self, p: usize, id: u64) -> Result<String> {
        let index = catalog_index::Index::new(&self.projects[p]);
        let path = index.paths.get(&id).ok_or("Unknown report")?;
        draft(&self.projects[p], &index, id, &self.text(p, path)?)
    }
    pub fn completion_token(&self, p: usize, id: u64) -> Result<String> {
        let project = &self.projects[p];
        let index = catalog_index::Index::new(project);
        let path = index.paths.get(&id).ok_or("Unknown report")?;
        let own = draft(project, &index, id, &self.text(p, path)?)?;
        self.completion(p, &index, id, &own)
    }
    /// The token `deps set` expects: the record's own prerequisite list, so only a change to
    /// that list (or to the binary its RVAs belong to) conflicts.
    pub fn structural_token(&self, p: usize, id: u64) -> Result<String> {
        prerequisites(
            &self.projects[p],
            &catalog_index::Index::new(&self.projects[p]),
            id,
        )
    }
    /// The `s4:` token issued before 0.15: any structural change in the project invalidates it.
    fn legacy_structural_token(&self, p: usize, id: u64) -> Result<String> {
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
            "structural" if token.starts_with("s5:") => self.structural_token(p, id)?,
            "structural" if token.starts_with("s4:") => self.legacy_structural_token(p, id)?,
            _ if !token.contains(':') => {
                let content = self.text(p, &self.report_rel(p, id)?)?;
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

impl Library {
    /// The completion token: the draft token plus the state of everything completion checks.
    fn completion(
        &self,
        p: usize,
        index: &catalog_index::Index<'_>,
        id: u64,
        own: &str,
    ) -> Result<String> {
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
        let states = index.states_with(&self.dir(p), seen.iter().copied(), &|| {
            self.hash_overrides()
        });
        let mut inputs = vec![];
        for id in seen {
            let n = index.nodes[&id];
            let actual = if n.status == Status::Complete {
                Some(hash(&self.text(p, &index.paths[&id])?))
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
    pub review_flag: Option<super::ReviewFlag>,
    /// The pool covering this record (`node` is the record that carries it), with its members.
    pub pool: Option<serde_json::Value>,
    /// Whether the caller may change this record (false only for a pool it is not in).
    pub may_write: bool,
    /// The recreation and tests that prove this function, if recorded.
    pub proof: Option<super::proofs::Proof>,
    /// A function without a proof in a project that requires one.
    pub unproven: bool,
}
impl Library {
    pub fn record(&self, p: usize, id: u64) -> Result<RecordRead> {
        let project = &self.projects[p];
        let index = catalog_index::Index::new(project);
        let node = index.nodes.get(&id).ok_or("Unknown node")?;
        let report = self.dir(p).join(&index.paths[&id]);
        layout::inside(&self.dir(p), &report)?;
        let text = self.text(p, &index.paths[&id])?;
        let (body, evidence) = parts(&text)?;
        let token = draft(project, &index, id, &text)?;
        let completion_token = self.completion(p, &index, id, &token)?;
        let structural_token = prerequisites(project, &index, id)?;
        let effective_status =
            index.states_with(&self.dir(p), [id], &|| self.hash_overrides())[&id];
        let proofs = self.proofs(p);
        let (pool, may_write) = self.pool_access(p, id);
        let unproven = proofs.required && self.unproven(p, id, &proofs);
        let proof = proofs.proofs.get(&id).cloned();
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
            review_flag: self.review_flags(p)?.remove(&id),
            pool: pool.map(|(carrier, r, members)| {
                serde_json::json!({"pool": r.pool, "node": carrier, "origin": r.origin,
                    "reason": r.reason, "at_revision": r.at_revision, "by": r.by,
                    "members": members})
            }),
            may_write,
            proof,
            unproven,
        })
    }
}
