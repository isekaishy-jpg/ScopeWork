//! Proof records (docs/proposal-proof-records.md): which recreation and tests prove a function,
//! and whether the project requires a proof before a function is completed.
//!
//! Stored in a journaled sidecar beside the catalog, like review flags, so older Scopework
//! versions leave it intact instead of dropping an unknown catalog field.
use super::catalog_batch::{Journal, Sidecar};
use super::*;
use std::collections::{BTreeMap, HashSet};

pub const PROOFS: &str = "proofs.json";

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub struct Proof {
    /// The recreation: a path in the repository, optionally followed by `::item`.
    pub recreation: String,
    /// The tests that check it: one or more paths.
    pub tests: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub by: String,
    pub at_revision: u64,
}
/// One entry of a proof request: a proof for `id`, or `None` to clear it.
#[derive(Clone, Deserialize)]
pub struct ProofInput {
    pub id: u64,
    pub recreation: String,
    pub tests: Vec<String>,
}
#[derive(Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ProofFile {
    pub schema: u32,
    /// Whether completing a function needs a proof.
    #[serde(default)]
    pub required: bool,
    /// The recreation's repository; when set, proof paths must exist under it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub repository: String,
    #[serde(default)]
    pub proofs: BTreeMap<u64, Proof>,
}

fn parse(text: &str) -> Result<ProofFile> {
    if text.trim().is_empty() {
        return Ok(ProofFile {
            schema: 1,
            ..Default::default()
        });
    }
    let file: ProofFile = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if file.schema != 1 {
        return Err("Unsupported proof file".into());
    }
    Ok(file)
}
fn render(file: &ProofFile) -> Result<String> {
    serde_json::to_string_pretty(file).map_err(|e| e.to_string())
}
/// The proof file without proofs of `removed` nodes, or `None` when nothing changes.
pub(crate) fn without_removed(text: &str, removed: &HashSet<u64>) -> Result<Option<String>> {
    let mut file = parse(text)?;
    let count = file.proofs.len();
    file.proofs.retain(|id, _| !removed.contains(id));
    if file.proofs.len() == count {
        return Ok(None);
    }
    render(&file).map(Some)
}
/// A proof path must be one line; under a repository its file part (before `::`) must exist.
fn check_path(repository: &str, label: &str, path: &str) -> Result<String> {
    let path = path.trim();
    if path.is_empty() || path.chars().any(char::is_control) {
        return Err(format!("A proof's {label} needs a single-line path."));
    }
    if !repository.is_empty() {
        let file = path.split_once("::").map_or(path, |(file, _)| file);
        if !Path::new(repository).join(file).is_file() {
            return Err(format!(
                "proof_path_missing: the {label} {file} does not exist under {repository}."
            ));
        }
    }
    Ok(path.to_string())
}

impl Library {
    /// The project's proof requirement and recorded proofs (none when the file is missing).
    pub fn proofs(&self, p: usize) -> Result<ProofFile> {
        match self.text_opt(p, PROOFS)? {
            Some(text) => parse(&text),
            None => parse(""),
        }
    }
    /// Whether `id` is a function without a proof (whatever the requirement).
    pub fn unproven(&self, p: usize, id: u64, file: &ProofFile) -> bool {
        self.projects[p]
            .nodes
            .iter()
            .any(|n| n.id == id && n.kind == Kind::Function)
            && !file.proofs.contains_key(&id)
    }
    /// Refuses completing `id` when the project requires a proof and it has none.
    pub(super) fn check_proof(&self, p: usize, id: u64) -> Result<()> {
        let file = self.proofs(p)?;
        if file.required && self.unproven(p, id, &file) {
            return Err(format!(
                "proof_required: this project requires a proof before a function is completed. Record its recreation and tests with research node proof {} {id} --recreation PATH --test PATH.",
                self.projects[p].id
            ));
        }
        Ok(())
    }
    /// Sets whether completion needs a proof and, optionally, the repository proofs are checked
    /// against (an empty string clears it).
    pub fn set_proof_policy(
        &mut self,
        p: usize,
        required: bool,
        repository: Option<&str>,
    ) -> Result<()> {
        if !self.writable(p) {
            return self.write_project(p, |l, p| l.set_proof_policy(p, required, repository));
        }
        let mut file = self.proofs(p)?;
        file.required = required;
        if let Some(repository) = repository.map(str::trim) {
            // The coordinator runs in another folder, so a relative path would mean something else.
            if !repository.is_empty() && !Path::new(repository).is_absolute() {
                return Err("Give the proof repository as an absolute path.".into());
            }
            if !repository.is_empty() && !Path::new(repository).is_dir() {
                return Err(format!("The repository {repository} is not a folder."));
            }
            file.repository = repository.into();
        }
        self.write_proofs(p, file)
    }
    /// Records (`Some`) or clears (`None`) proofs in one transaction; any invalid entry refuses
    /// the whole request.
    pub fn set_proofs(&mut self, p: usize, entries: &[(u64, Option<ProofInput>)]) -> Result<()> {
        if !self.writable(p) {
            let entries = entries.to_vec();
            return self.write_project(p, move |l, p| l.set_proofs(p, &entries));
        }
        if entries.is_empty() {
            return Err("The proof request is empty.".into());
        }
        self.check_pool(
            p,
            entries.iter().map(|(id, _)| *id),
            None,
            "Recording a proof of",
        )?;
        let mut file = self.proofs(p)?;
        let mut seen = HashSet::new();
        let at_revision = self.projects[p].revision + 1;
        let by = caller();
        for (id, input) in entries {
            if !seen.insert(*id) {
                return Err(format!("Node {id} appears twice in the proof request."));
            }
            let node = self.projects[p]
                .nodes
                .iter()
                .find(|n| n.id == *id)
                .ok_or_else(|| format!("Unknown node {id}"))?;
            if node.kind != Kind::Function {
                return Err(format!(
                    "Node {id} is not a function: only functions take a proof."
                ));
            }
            let Some(input) = input else {
                file.proofs.remove(id);
                continue;
            };
            let recreation = check_path(&file.repository, "recreation", &input.recreation)?;
            if input.tests.is_empty() {
                return Err(format!("The proof of node {id} needs at least one test."));
            }
            let tests = input
                .tests
                .iter()
                .map(|t| check_path(&file.repository, "test", t))
                .collect::<Result<Vec<_>>>()?;
            let proof = Proof {
                recreation,
                tests,
                by: by.clone(),
                at_revision,
            };
            // Restating a recorded proof changes nothing.
            if file
                .proofs
                .get(id)
                .is_none_or(|old| old.recreation != proof.recreation || old.tests != proof.tests)
            {
                file.proofs.insert(*id, proof);
            }
        }
        self.write_proofs(p, file)
    }
    fn write_proofs(&mut self, p: usize, file: ProofFile) -> Result<()> {
        let before_text = match self.text_opt(p, PROOFS)? {
            Some(text) => text,
            None => {
                // An empty file first, so the transaction has a `before` to recover to.
                let empty = render(&parse("")?)?;
                atomic(&self.dir(p).join(PROOFS), empty.as_bytes())?;
                empty
            }
        };
        if parse(&before_text)? == file {
            return Ok(());
        }
        let before = self.projects[p].clone();
        let mut after = before.clone();
        after.revision += 1;
        let journal = Journal {
            before,
            after,
            edits: vec![],
            moves: vec![],
            dependencies: vec![],
            sidecars: vec![Sidecar {
                path: PROOFS.into(),
                before: before_text,
                after: render(&file)?,
            }],
            ledger: false,
            repin_ledger: false,
            directed_by: None,
            repin_ids: vec![],
        };
        self.commit_or_stage(p, journal).map(|_| ())
    }
}
