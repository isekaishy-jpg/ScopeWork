use eframe::egui;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Kind {
    App,
    Architecture,
    Chunk,
    Function,
}
impl Kind {
    pub fn completion_criteria(self) -> &'static str {
        match self {
            Self::Function => {
                "Explain this function's behavior, inputs, outputs, side effects, boundaries and connections, supported by evidence. Complete only the prerequisites necessary to establish this function's behavior."
            }
            Self::Chunk => {
                "Synthesize the completed function reports into an explanation of this chunk's behavior, control flow, data flow and external interactions. Every required function must be complete."
            }
            Self::Architecture => {
                "Synthesize the completed chunk reports into an explanation of this subsystem's behavior and interfaces. Every required chunk must be complete."
            }
            Self::App => {
                "Synthesize the completed architecture reports to explain the project's research subject and relevant subsystem interactions. Every required architecture must be complete. The subject may be a selected part of the binary."
            }
        }
    }
    fn child(self) -> Option<Self> {
        match self {
            Self::App => Some(Self::Architecture),
            Self::Architecture => Some(Self::Chunk),
            Self::Chunk => Some(Self::Function),
            Self::Function => None,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::App => "App",
            Self::Architecture => "Architecture",
            Self::Chunk => "Chunk",
            Self::Function => "Function",
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Status {
    NotStarted,
    InProgress,
    Complete,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Node {
    #[serde(default)]
    pub context_only: bool,
    pub id: u64,
    pub parent: Option<u64>,
    pub kind: Kind,
    pub name: String,
    pub status: Status,
    pub rva: Option<u32>,
    pub revision: u64,
    pub accepted_hash: Option<String>,
    #[serde(default)]
    pub accepted_prerequisites: std::collections::BTreeMap<u64, String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Project {
    #[serde(default = "legacy_layout")]
    pub layout_version: u32,
    pub id: String,
    pub name: String,
    pub binary: String,
    #[serde(default)]
    pub research_scope: String,
    pub next_id: u64,
    pub revision: u64,
    #[serde(default)]
    pub structural_epoch: u64,
    #[serde(default)]
    pub active_import: Option<String>,
    #[serde(default)]
    pub last_import: Option<String>,
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
}
fn legacy_layout() -> u32 {
    1
}
#[path = "background.rs"]
mod background;
#[path = "catalog_batch.rs"]
mod catalog_batch;
#[path = "catalog_browser.rs"]
mod catalog_browser;
#[path = "catalog_index.rs"]
mod catalog_index;
#[path = "edit_tokens.rs"]
mod edit_tokens;
#[path = "imports.rs"]
pub mod imports;
#[path = "layout.rs"]
mod layout;
pub use catalog_batch::Batch;
// Reports contain their editable source in Markdown; the catalog stores no duplicate report prose.
fn hash(text: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(text.as_bytes()))
}
fn atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let temp = path.with_extension(format!("{}.pending", uuid::Uuid::new_v4()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    fs::rename(&temp, path).map_err(|e| format!("Cannot replace {}: {e}", path.display()))
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !["con", "prn", "aux", "nul"].contains(&id)
        && !(id.len() == 4
            && (id.starts_with("com") || id.starts_with("lpt"))
            && matches!(id.as_bytes()[3], b'1'..=b'9'))
}
fn relative(project: &Project, n: &Node) -> String {
    if n.kind == Kind::App {
        return "app.md".into();
    }
    let folder = match n.kind {
        Kind::Function => format!("{:08X}", n.rva.unwrap()),
        _ => format!("{:06}", n.id),
    };
    if n.kind == Kind::Architecture {
        return format!("architectures/{folder}/research.md");
    }
    let parent = project
        .nodes
        .iter()
        .find(|v| Some(v.id) == n.parent)
        .unwrap();
    let parent_path = relative(project, parent);
    format!(
        "{}/{folder}/research.md",
        parent_path.trim_end_matches("/research.md")
    )
}

// One project-local destination for each kind of reversing material.
const PROJECT_FOLDERS: &[&str] = &[
    "architectures",
    "tools/profiles",
    "tools/scripts",
    "tools/instrumentation",
    "tools/bin",
    "tools/workspaces",
    "tools/logs",
    "tools/scratch",
];
fn ensure_folders(dir: &Path) -> Result<()> {
    for folder in PROJECT_FOLDERS {
        fs::create_dir_all(dir.join(folder)).map_err(|e| format!("Cannot create {folder}: {e}"))?;
    }
    // Preserve an existing project tool guide; no tool or evidence files are replaced.
    let guide = dir.join("tools/README.md");
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&guide)
    {
        Ok(mut f) => {
            use std::io::Write;
            f.write_all(b"# Project reversing tools\n\nThis directory belongs only to its enclosing research project.\n\nprofiles/: compiler, ABI, type and analysis profiles.\nscripts/: analysis drivers and exporters.\ninstrumentation/: observational hooks and capture utilities.\nbin/: tool binaries or launchers.\nworkspaces/: shared Ghidra projects and whole-binary analysis databases.\nlogs/: shared tool run logs and provenance.\nscratch/: shared temporary working files.\n\nFunction-specific captures, exports and evidence belong in ../architectures/<category>/<chunk>/<RVA>/data/, beside that function's research.md. Link to them from research.md using data/<filename>. Do not create project-level evidence or data dumps. Chunks and architecture reports reference the canonical reports below them. Source installations remain read-only. Record input binary hashes and tool/profile versions for tool runs.\n").map_err(|e|e.to_string())?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.to_string()),
    }
    Ok(())
}
fn open_folder(path: &Path) -> Result<()> {
    #[cfg(target_os = "windows")]
    let mut command = std::process::Command::new("explorer.exe");
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let mut command = std::process::Command::new("xdg-open");
    command.arg(path).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

pub struct Library {
    pub root: PathBuf,
    pub projects: Vec<Project>,
    pub busy_projects: Vec<String>,
    _lock: Option<OperationLocks>,
}
// Keep the legacy gate shared for the lifetime of every project operation. Older
// executables take it exclusively, so mixed-version processes cannot race writes.
struct OperationLocks {
    project: Option<String>,
    writable: bool,
    _project: Option<fs::File>,
    _gate: fs::File,
}
fn project_lock(root: &Path, id: &str, writable: bool) -> Result<fs::File> {
    let folder = root.join(".project-locks");
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(folder.join(format!("{id}.lock")))
        .map_err(|e| e.to_string())?;
    (if writable {
        file.try_lock()
    } else {
        file.try_lock_shared()
    })
    .map_err(|_| {
        format!("Research project '{id}' is busy with another operation. Retry shortly.")
    })?;
    Ok(file)
}
impl Library {
    fn commit_lease(root: &Path, id: &str) -> Result<OperationLocks> {
        if !valid_id(id) {
            return Err("Invalid project ID".into());
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let result: Result<OperationLocks> = (|| {
                let gate = fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(root.join(".library.lock"))
                    .map_err(|e| e.to_string())?;
                gate.try_lock_shared()
                    .map_err(|_| "Research library is busy".to_string())?;
                Ok(OperationLocks {
                    project: Some(id.into()),
                    writable: true,
                    _project: Some(project_lock(root, id, true)?),
                    _gate: gate,
                })
            })();
            match result {
                Err(e) if e.contains("is busy") && std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(20))
                }
                other => return other,
            }
        }
    }
    pub fn set_scope(&mut self, p: usize, scope: &str, expected_revision: u64) -> Result<()> {
        if !self.writable(p) {
            return self.write_project(p, |l, p| l.set_scope(p, scope, expected_revision));
        }
        if self.projects[p].revision != expected_revision {
            return Err("Project changed. Reload and review its scope before saving.".into());
        }
        let scope = scope.trim();
        if scope.chars().any(char::is_control) {
            return Err(
                "Research scope is a single-line topic label, such as Rendering or Asset platform."
                    .into(),
            );
        }
        if scope == self.projects[p].research_scope {
            return Ok(());
        }
        self.projects[p].research_scope = scope.into();
        self.projects[p].revision += 1;
        self.persist(p)
    }
    pub fn release(&mut self) {
        self._lock = None;
    }
    fn fresh(&self) -> Result<Self> {
        let mut fresh = Self::open(self.root.clone())?;
        for old in &self.projects {
            if fresh.busy_projects.contains(&old.id) {
                fresh.projects.push(old.clone());
            }
        }
        fresh.projects.sort_by_key(|p| {
            self.projects
                .iter()
                .position(|old| old.id == p.id)
                .unwrap_or(usize::MAX)
        });
        Ok(fresh)
    }
    fn write_project<T>(
        &mut self,
        p: usize,
        operation: impl FnOnce(&mut Self, usize) -> Result<T>,
    ) -> Result<T> {
        let id = self
            .projects
            .get(p)
            .ok_or("Project no longer exists")?
            .id
            .clone();
        self.release();
        let mut fresh = Self::open_project(self.root.clone(), Some(&id))?;
        let current = fresh
            .projects
            .iter()
            .position(|v| v.id == id)
            .ok_or("Project was removed. Refresh before continuing.")?;
        let result = operation(&mut fresh, current);
        fresh.release();
        self.projects[p] = fresh.projects.remove(current);
        self.busy_projects.retain(|busy| busy != &id);
        result
    }
    pub fn refresh(&mut self) -> Result<()> {
        let mut fresh = self.fresh()?;
        fresh.release();
        *self = fresh;
        Ok(())
    }
    pub fn set_binary(&mut self, p: usize, binary: &str) -> Result<()> {
        if !self.writable(p) {
            return self.write_project(p, |l, p| l.set_binary(p, binary));
        }
        self.ensure_structural(p)?;
        if binary.trim().is_empty() {
            return Err("Binary identity cannot be empty.".into());
        }
        if self.projects[p]
            .nodes
            .iter()
            .any(|n| n.kind == Kind::Function)
        {
            return Err("Binary identity is locked after adding functions.".into());
        }
        let before = self.projects[p].clone();
        let mut after = before.clone();
        after.structural_epoch += 1;
        after.binary = binary.into();
        after.revision += 1;
        let old_index = catalog_index::Index::new(&before);
        let index = catalog_index::Index::new(&after);
        let mut reports = vec![];
        for node in &before.nodes {
            let original = fs::read_to_string(self.dir(p).join(&old_index.paths[&node.id]))
                .map_err(|e| e.to_string())?;
            edit_tokens::draft(&before, &old_index, node.id, &original)?;
            let (body, evidence) = edit_tokens::parts(&original)?;
            reports.push((
                node.id,
                original.clone(),
                index.render(&after, node.id, body, evidence),
            ));
        }
        for node in &mut after.nodes {
            node.accepted_hash = None;
            node.accepted_prerequisites.clear();
            if node.status == Status::Complete {
                node.status = Status::InProgress;
            }
            node.revision += 1;
        }
        self.commit_metadata(p, before, after, reports, vec![])
    }

    fn writable(&self, p: usize) -> bool {
        self._lock.as_ref().is_some_and(|l| {
            l.writable && l.project.as_deref() == Some(self.projects[p].id.as_str())
        })
    }
    fn ensure_structural(&self, p: usize) -> Result<()> {
        if let Some(job) = &self.projects[p].active_import {
            return Err(format!(
                "structural_job_conflict: import {job} owns the structural reservation; pause does not release it. Commit or cancel the job first."
            ));
        }
        Ok(())
    }
    pub fn open(root: PathBuf) -> Result<Self> {
        Self::open_project(root, None)
    }
    pub fn open_project(root: PathBuf, selected: Option<&str>) -> Result<Self> {
        Self::open_mode(root, selected, true, true)
    }
    pub fn read_project(root: PathBuf, selected: &str) -> Result<Self> {
        Self::open_mode(root, Some(selected), false, true)
    }
    fn open_mode(
        root: PathBuf,
        selected: Option<&str>,
        writable: bool,
        recover: bool,
    ) -> Result<Self> {
        fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let Some(id) = selected else {
            let mut lib = Self {
                root: root.clone(),
                projects: vec![],
                busy_projects: vec![],
                _lock: None,
            };
            for entry in fs::read_dir(&root).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                if entry.file_type().map_err(|e| e.to_string())?.is_symlink()
                    || !entry.path().join("project.json").is_file()
                {
                    continue;
                }
                let id = entry.file_name().to_string_lossy().into_owned();
                match Self::read_project(root.clone(), &id) {
                    Ok(mut project) => lib.projects.append(&mut project.projects),
                    Err(e) if e.contains("is busy") => lib.busy_projects.push(id),
                    Err(e) => return Err(e),
                }
            }
            lib.projects.sort_by_key(|p| p.name.clone());
            return Ok(lib);
        };
        if !valid_id(id) {
            return Err("Invalid project identity".into());
        }
        let acquire = |exclusive_gate: bool, write: bool| -> Result<OperationLocks> {
            let gate = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(root.join(".library.lock"))
                .map_err(|e| e.to_string())?;
            (if exclusive_gate {
                gate.try_lock()
            } else {
                gate.try_lock_shared()
            })
            .map_err(|_| {
                "Research library is busy with another operation. Retry shortly.".to_string()
            })?;
            Ok(OperationLocks {
                project: Some(id.into()),
                writable: write,
                _project: Some(project_lock(&root, id, write)?),
                _gate: gate,
            })
        };
        let mut locks = acquire(false, writable)?;
        let dir = root.join(id);
        if dir.exists() {
            layout::inside(&root, &dir)?;
        }
        let path = dir.join("project.json");
        let mut project: Option<Project> = if path.is_file() {
            Some(
                serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?,
            )
        } else {
            None
        };
        if project.as_ref().is_some_and(|p| p.layout_version < 4) {
            if !recover {
                return Err("migration_required: open this project with the current CLI before planning an import".into());
            }
            // Never upgrade a held shared lease. Exclude legacy writers during migration.
            drop(locks);
            locks = acquire(true, true)?;
            let mut p: Project =
                serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            catalog_batch::resume(&dir, &mut p)?;
            Self::validate_project(&p, id)?;
            layout::migrate(&dir, &mut p)?;
            if p.layout_version < 4 {
                let backup = dir.join("tools/workspaces/layout-migration-v4");
                fs::create_dir_all(&backup).map_err(|e| e.to_string())?;
                if !backup.join("before.json").exists() {
                    atomic(
                        &backup.join("before.json"),
                        &serde_json::to_vec_pretty(&p).map_err(|e| e.to_string())?,
                    )?;
                }
                p.layout_version = 4;
                atomic(
                    &path,
                    &serde_json::to_vec_pretty(&p).map_err(|e| e.to_string())?,
                )?;
                atomic(
                    &backup.join("migration.json"),
                    b"{\"format\":4,\"authored_reports_changed\":false}",
                )?;
            }
            drop(locks);
            locks = acquire(false, writable)?;
            project = Some(
                serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?,
            );
        }
        if let Some(p) = &mut project {
            if p.layout_version != 4 {
                return Err("Unsupported catalog format; upgrade Scopework.".into());
            }
            let pending = dir
                .join("tools/workspaces/catalog-transactions/pending/journal.json")
                .is_file()
                || dir
                    .join("tools/workspaces/catalog-transactions-v4/pending/journal.json")
                    .is_file()
                || imports::needs_recovery(&dir, p);
            if pending && !recover {
                return Err(
                    "recovery_required: resolve pending publication before planning an import"
                        .into(),
                );
            }
            if pending && !locks.writable {
                drop(locks);
                locks = acquire(false, true)?;
                *p = serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            }
            if locks.writable {
                catalog_batch::resume(&dir, p)?;
                imports::recover(&dir, p)?;
            }
            Self::validate_project(p, id)?;
            if !dir.join("app.md").is_file() {
                return Err("Canonical app report missing. Restore it before continuing.".into());
            }
        }
        Ok(Self {
            root,
            projects: project.into_iter().collect(),
            busy_projects: vec![],
            _lock: Some(locks),
        })
    }
    fn validate_project(p: &Project, id: &str) -> Result<()> {
        use std::collections::{HashMap, HashSet};
        if p.id != id || !valid_id(&p.id) {
            return Err("Invalid project identity".into());
        }
        let lookup: HashMap<_, _> = p.nodes.iter().map(|n| (n.id, n)).collect();
        let mut ids = HashSet::new();
        let mut rvas = HashSet::new();
        for n in &p.nodes {
            if !ids.insert(n.id) {
                return Err("Duplicate node identity".into());
            }
            if n.kind == Kind::Function && (n.rva.is_none() || !rvas.insert(n.rva)) {
                return Err("Invalid or duplicate function RVA".into());
            }
            if n.kind != Kind::App {
                let parent = n
                    .parent
                    .and_then(|id| lookup.get(&id))
                    .ok_or("Missing report parent")?;
                if parent.kind.child() != Some(n.kind) {
                    return Err("Invalid research hierarchy".into());
                }
            } else if n.parent.is_some() {
                return Err("App cannot have a parent".into());
            }
        }
        if p.nodes.iter().filter(|n| n.kind == Kind::App).count() != 1
            || !p.nodes.iter().any(|n| n.id == 0 && n.kind == Kind::App)
            || p.nodes.iter().any(|n| n.id >= p.next_id)
        {
            return Err("Invalid app ID or next node ID".into());
        }
        let validation = Self {
            root: PathBuf::new(),
            projects: vec![p.clone()],
            busy_projects: vec![],
            _lock: None,
        };
        validation.validate_dependencies(0, &p.dependencies)
    }
    fn dir(&self, p: usize) -> PathBuf {
        self.root.join(&self.projects[p].id)
    }
    fn persist(&self, p: usize) -> Result<()> {
        atomic(
            &self.dir(p).join("project.json"),
            &serde_json::to_vec_pretty(&self.projects[p]).map_err(|e| e.to_string())?,
        )
    }
    pub fn create(&mut self, id: &str, name: &str, binary: &str) -> Result<()> {
        if self
            ._lock
            .as_ref()
            .is_none_or(|locks| !locks.writable || locks.project.as_deref() != Some(id))
        {
            let mut fresh = Self::open_project(self.root.clone(), Some(id))?;
            fresh.create(id, name, binary)?;
            fresh.release();
            self.projects.extend(fresh.projects);
            return Ok(());
        }
        if !valid_id(id) || name.trim().is_empty() {
            return Err("Use a project ID with lowercase letters, digits, and hyphens, and a nonempty name.".into());
        }
        if self.root.join(id).exists() {
            return Err("That project already exists.".into());
        }
        let dir = self.root.join(id);
        let stage = self
            .root
            .join(".project-staging")
            .join(uuid::Uuid::new_v4().to_string());
        layout::inside(&self.root, &stage)?;
        fs::create_dir_all(&stage).map_err(|e| e.to_string())?;
        ensure_folders(&stage)?;
        let project = Project {
            layout_version: 4,
            structural_epoch: 0,
            active_import: None,
            last_import: None,
            id: id.into(),
            name: name.into(),
            binary: binary.into(),
            research_scope: String::new(),
            dependencies: vec![],
            next_id: 1,
            revision: 0,
            nodes: vec![Node {
                context_only: false,
                id: 0,
                parent: None,
                kind: Kind::App,
                name: format!("{name} research"),
                status: Status::NotStarted,
                rva: None,
                revision: 0,
                accepted_hash: None,
                accepted_prerequisites: Default::default(),
            }],
        };
        let text = catalog_index::Index::new(&project).render(&project, 0, "", "");
        atomic(&stage.join("app.md"), text.as_bytes())?;
        atomic(
            &stage.join("project.json"),
            &serde_json::to_vec_pretty(&project).map_err(|e| e.to_string())?,
        )?;
        fs::rename(&stage, &dir).map_err(|e| e.to_string())?;
        self.projects.push(project);
        Ok(())
    }
    pub fn path(&self, p: usize, id: u64) -> Result<PathBuf> {
        let n = self.projects[p]
            .nodes
            .iter()
            .find(|n| n.id == id)
            .ok_or("Unknown report")?;
        let path = self.dir(p).join(relative(&self.projects[p], n));
        layout::inside(&self.dir(p), &path)?;
        Ok(path)
    }
    pub fn read(&self, p: usize, id: u64) -> Result<(String, String)> {
        let full = fs::read_to_string(self.path(p, id)?).map_err(|e| e.to_string())?;
        let marker = "\n<!-- research-content -->\n";
        let content = full
            .split_once(marker)
            .ok_or("Report marker is missing; restore the canonical report header before editing.")?
            .1;
        let (body, evidence) = content
            .split_once("\n<!-- research-evidence -->\n")
            .ok_or("Evidence marker missing")?;
        Ok((body.to_string(), evidence.to_string()))
    }
    fn invalidate(&mut self, p: usize, id: u64) {
        let mut cursor = self.projects[p]
            .nodes
            .iter()
            .find(|n| n.id == id)
            .and_then(|n| n.parent);
        while let Some(parent) = cursor {
            let n = self.projects[p]
                .nodes
                .iter_mut()
                .find(|n| n.id == parent)
                .unwrap();
            if n.status == Status::Complete {
                n.status = Status::InProgress;
            }
            n.accepted_hash = None;
            n.revision += 1;
            cursor = n.parent;
        }
    }
    pub fn effective_all(&self, p: usize) -> std::collections::HashMap<u64, Status> {
        catalog_index::Index::new(&self.projects[p]).states(&self.dir(p))
    }
    pub fn required_nodes(&self, p: usize) -> std::collections::HashSet<u64> {
        catalog_index::Index::new(&self.projects[p]).required
    }
    pub fn report_paths(&self, p: usize) -> std::collections::HashMap<u64, PathBuf> {
        catalog_index::Index::new(&self.projects[p])
            .paths
            .into_iter()
            .map(|(id, path)| (id, self.dir(p).join(path)))
            .collect()
    }
    pub fn effective(&self, p: usize, id: u64) -> Status {
        let Some(n) = self.projects[p].nodes.iter().find(|n| n.id == id) else {
            return Status::NotStarted;
        };
        if n.kind == Kind::Function && n.status != Status::Complete {
            return n.status;
        }
        self.effective_all(p)
            .get(&id)
            .copied()
            .unwrap_or(Status::NotStarted)
    }
    pub fn add(&mut self, p: usize, parent: u64, name: &str, rva: &str) -> Result<u64> {
        if !self.writable(p) {
            return self.write_project(p, |l, p| l.add(p, parent, name, rva));
        }
        self.ensure_structural(p)?;
        if name.trim().is_empty() {
            return Err("Enter a name.".into());
        }
        let kind = self.projects[p]
            .nodes
            .iter()
            .find(|n| n.id == parent)
            .ok_or("Unknown parent")?
            .kind
            .child()
            .ok_or("Functions cannot have children")?;
        let address = if kind == Kind::Function {
            if self.projects[p].binary.trim().is_empty() {
                return Err("Set the project's binary build/hash before adding functions.".into());
            }
            let x = u32::from_str_radix(
                rva.trim().trim_start_matches("0x").trim_start_matches("0X"),
                16,
            )
            .map_err(|_| "Enter an RVA in hexadecimal, not a VA.")?;
            if self.projects[p].nodes.iter().any(|n| n.rva == Some(x)) {
                return Err("That function already has a canonical file in this project.".into());
            }
            Some(x)
        } else {
            None
        };
        let batch = Batch {
            schema: 1,
            binary: self.projects[p].binary.clone(),
            expect_revision: self.projects[p].revision,
            create: vec![catalog_batch::Create {
                parent,
                name: name.trim().into(),
                rva: address,
                context_only: false,
            }],
            update: vec![],
            reorder: vec![],
            repair_legacy_links: false,
        };
        self.batch(p, &batch)?
            .into_iter()
            .next()
            .ok_or_else(|| "Creation returned no identity".into())
    }
    pub fn save_report(
        &mut self,
        p: usize,
        id: u64,
        body: &str,
        evidence: &str,
        complete: bool,
        expected: &str,
    ) -> Result<()> {
        if !self.writable(p) {
            return self.write_project(p, |l, p| {
                l.save_report(p, id, body, evidence, complete, expected)
            });
        }
        self.check_edit_token(
            p,
            id,
            expected,
            if complete { "completion" } else { "draft" },
        )?;
        if !complete {
            let batch = catalog_batch::ReportBatch {
                schema: 2,
                binary: self.projects[p].binary.clone(),
                expect_revision: None,
                reports: vec![catalog_batch::ReportSave {
                    id,
                    expect: expected.into(),
                    body: body.into(),
                    evidence: evidence.into(),
                }],
            };
            self.save_reports(p, &batch)?;
            return Ok(());
        }
        let n = self.projects[p].nodes.iter().find(|n| n.id == id).unwrap();
        let (old_body, old_evidence) = self.read(p, id)?;
        if old_body == body
            && old_evidence == evidence
            && complete == (n.status == Status::Complete)
            && self.effective(p, id) == n.status
        {
            return Ok(());
        }
        let states = if complete {
            self.effective_all(p)
        } else {
            Default::default()
        };
        if complete {
            if body.trim().is_empty() {
                return Err(
                    "Write the compiled understanding report before marking complete.".into(),
                );
            }
            if n.kind == Kind::Function && evidence.trim().is_empty() {
                return Err("A function needs evidence references and claim classification before completion.".into());
            }
            let kids: Vec<_> = self.projects[p]
                .nodes
                .iter()
                .filter(|k| k.parent == Some(id) && !k.context_only)
                .collect();
            if n.kind != Kind::Function
                && (kids.is_empty() || kids.iter().any(|k| states[&k.id] != Status::Complete))
            {
                return Err("Every required child must be complete, and the parent needs its own compiled report.".into());
            }
        }
        if complete
            && self.projects[p]
                .dependencies
                .iter()
                .filter(|e| e.dependent == id)
                .any(|e| states[&e.prerequisite] != Status::Complete)
        {
            return Err(
                "Every tagged prerequisite must be complete before this function can be complete."
                    .into(),
            );
        }
        let stamps = if complete {
            self.dependency_stamps(p, id)
        } else {
            Default::default()
        };
        if body.contains("<!-- research-") || evidence.contains("<!-- research-") {
            return Err("Reserved report markers cannot appear in report content.".into());
        }
        let before = self.projects[p].clone();
        let original = fs::read_to_string(self.path(p, id)?).map_err(|e| e.to_string())?;
        let report = catalog_index::Index::new(&before).render(&before, id, body, evidence);
        let n = self.projects[p]
            .nodes
            .iter_mut()
            .find(|n| n.id == id)
            .unwrap();
        n.status = if complete {
            Status::Complete
        } else if body.trim().is_empty() && evidence.trim().is_empty() {
            Status::NotStarted
        } else {
            Status::InProgress
        };
        n.accepted_hash = complete.then(|| hash(&report));
        n.accepted_prerequisites = stamps;
        n.revision += 1;
        self.invalidate(p, id);
        self.invalidate_dependents(p, id);
        self.projects[p].revision += 1;
        let after = std::mem::replace(&mut self.projects[p], before.clone());
        self.commit_report(p, before, after, id, original, report)
    }
}

type ActionResult = Result<Option<(String, u64)>>;

pub struct ResearchUi {
    browser: catalog_browser::Browser,
    ui_projects: Vec<std::sync::Arc<Project>>,
    pub open: bool,
    root: Option<PathBuf>,
    pub changed: bool,
    project: usize,
    project_id: String,
    last_poll: std::time::Instant,
    fingerprint: String,
    views: Vec<ProjectView>,
    browser_data: Vec<std::sync::Arc<catalog_browser::BrowserData>>,
    refresh_task: Option<(u64, std::sync::mpsc::Receiver<Result<()>>)>,
    snapshot_inbox: Option<background::SnapshotInbox>,
    load_task: Option<(u64, std::sync::mpsc::Receiver<Result<background::Loaded>>)>,
    action_task: Option<std::sync::mpsc::Receiver<ActionResult>>,
    generation: u64,
    refresh_generation: u64,
    requested: Option<(String, u64)>,
    import_file: String,
    jobs: std::collections::HashMap<String, imports::Job>,
    node: u64,
    body: String,
    evidence: String,
    loaded_hash: String,
    completion_hash: String,
    dirty: bool,
    message: String,
    new_name: String,
    rva: String,
    dependency_rvas: String,
    new_project_name: String,
    new_project_id: String,
    binary: String,
    scope: String,
    scope_dirty: bool,
    scope_revision: u64,
    complete: bool,
}
impl Default for ResearchUi {
    fn default() -> Self {
        Self::new()
    }
}

impl ResearchUi {
    pub fn new() -> Self {
        Self {
            browser: Default::default(),
            ui_projects: vec![],
            open: false,
            root: None,
            changed: true,
            project: 0,
            project_id: String::new(),
            last_poll: std::time::Instant::now() - std::time::Duration::from_secs(3),
            fingerprint: String::new(),
            views: vec![],
            browser_data: vec![],
            refresh_task: None,
            snapshot_inbox: None,
            load_task: None,
            action_task: None,
            refresh_generation: 0,
            generation: 0,
            requested: None,
            import_file: String::new(),
            jobs: Default::default(),
            node: 0,
            body: String::new(),
            evidence: String::new(),
            loaded_hash: String::new(),
            completion_hash: String::new(),
            dirty: false,
            message: String::new(),
            new_name: String::new(),
            rva: String::new(),
            dependency_rvas: String::new(),
            new_project_name: String::new(),
            new_project_id: String::new(),
            binary: String::new(),
            scope: String::new(),
            scope_dirty: false,
            scope_revision: 0,
            complete: false,
        }
    }
    fn load(&mut self, p: usize, id: u64) {
        if let Some(project) = self.ui_projects.get(p) {
            self.generation += 1;
            self.requested = Some((project.id.clone(), id));
            self.dirty = false;
            self.scope_dirty = false;
        }
    }
    fn action(
        &mut self,
        work: impl FnOnce(PathBuf) -> Result<Option<(String, u64)>> + Send + 'static,
    ) {
        if self.action_task.is_some() {
            return;
        }
        let (send, recv) = std::sync::mpsc::sync_channel(1);
        self.action_task = Some(recv);
        std::thread::spawn(move || {
            let result = crate::paths::home(None)
                .map(|h| h.join("projects"))
                .and_then(work);
            let _ = send.send(result);
        });
    }
    fn mutate(
        &mut self,
        work: impl FnOnce(&mut Library, usize, u64) -> Result<u64> + Send + 'static,
    ) {
        let project = self.project_id.clone();
        let id = self.node;
        self.action(move |root| {
            let mut lib = Library::open_project(root, Some(&project))?;
            if lib.projects.is_empty() {
                return Err("Project was removed".into());
            }
            let id = work(&mut lib, 0, id)?;
            Ok(Some((project, id)))
        });
    }
    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        let mut open = self.open;
        let mut nav = None;
        let navigation_ids: Vec<_> = self.ui_projects.iter().map(|p| p.id.clone()).collect();
        egui::Window::new("Research records").open(&mut open).default_size([950.0,700.0]).vscroll(true).show(ctx,|ui| {
            if !self.message.is_empty(){ui.colored_label(egui::Color32::from_rgb(230,170,80),&self.message);}
            if self.root.is_none() {ui.label("Loading research library...");return;}
            let working=self.action_task.is_some() || self.load_task.is_some() || self.requested.is_some();
            if working {ui.label("Working...");ui.disable();}
            let projects=self.ui_projects.clone();
            egui::CollapsingHeader::new("New project").default_open(projects.is_empty()).show(ui,|ui|{
                ui.horizontal(|ui|{ui.label("ID");ui.text_edit_singleline(&mut self.new_project_id);ui.label("Name");ui.text_edit_singleline(&mut self.new_project_name);});
                if ui.add_enabled(!self.has_unsaved(),egui::Button::new("Create isolated project")).clicked(){let id=self.new_project_id.clone();let name=self.new_project_name.clone();self.action(move |root|{let mut lib=Library::open(root)?;lib.create(&id,&name,"")?;Ok(Some((id,0)))})}
            });
            let found = projects.iter().position(|p| p.id == self.project_id);
            if let Some(p) = found { self.project = p; }
            let orphan = found.is_none() || self.browser_data[self.project].node(&projects[self.project],self.node).is_none();
            if orphan && self.has_unsaved() {
                ui.label("The project or report was removed. Your unsaved draft is retained here.");
                if ui.button("Copy draft").clicked() { ui.ctx().copy_text(format!("Scope: {}\n\n{}\n\n{}",self.scope,self.body,self.evidence)); }
                ui.add(egui::TextEdit::multiline(&mut self.body).desired_rows(12));
                ui.add(egui::TextEdit::multiline(&mut self.evidence).desired_rows(4));
                if ui.button("Discard removed report draft").clicked() { self.dirty=false; self.scope_dirty=false; self.project_id.clear(); self.project=0; self.node=0; if !projects.is_empty() { nav=Some((0,0)); } }
                return;
            }
            if orphan && !projects.is_empty() { nav=Some((found.unwrap_or(0),0)); return; }
            let Some(project)=projects.get(self.project) else {
                ui.label("Create a project above to start researching.");
                return;
            };
            let project_dir=self.root.as_ref().unwrap().join(&project.id);
            ui.horizontal_wrapped(|ui|{for (i,p) in projects.iter().enumerate(){if ui.add_enabled(!self.has_unsaved(),egui::Button::new(&p.name).selected(i==self.project)).clicked(){nav=Some((i,0));}}});
            ui.label(format!("Isolated project: {}",project_dir.display()));

            egui::CollapsingHeader::new("Project tools and data folders").show(ui,|ui|{
                ui.horizontal_wrapped(|ui|{
                    for (label,folder) in [("Tools","tools"),("Profiles","tools/profiles"),("Shared analysis","tools/workspaces"),("Tool logs","tools/logs")] {
                        if ui.button(label).clicked()&& let Err(e)=open_folder(&project_dir.join(folder)){self.message=e;}
                    }
                });
                ui.label("Each function owns research.md and data/ in its function folder. Only shared tool workspaces, logs and scratch stay under tools/.");
            });
            egui::CollapsingHeader::new("Inventory import").show(ui,|ui|{
                ui.label("Creation-only JSON batch (exact binary identity and project revision)");
                ui.text_edit_singleline(&mut self.import_file);
                if ui.button("Start background import").clicked() {
                    let file=self.import_file.clone();let project=project.id.clone();
                    self.action(move |root|{let batch:Batch=serde_json::from_slice(&fs::read(file).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;let job=imports::reserve(&root,&project,&batch)?;background::launch_import(&root,&project,&job.id)?;Ok(None)});
                }
                if let Some(job)=self.jobs.get(&project.id).cloned() {
                    ui.label(format!("{:?}: {} / {} prepared; committed inventory remains separate",job.state,job.prepared,job.total));
                    if job.state==imports::JobState::Verifying {ui.label(format!("{} / {} records verified",job.verified,job.total));}
                    if let Some(error)=&job.error {ui.colored_label(egui::Color32::YELLOW,error);}
                    ui.label(format!("Job {}",job.id));
                    if !matches!(job.state,imports::JobState::Committed|imports::JobState::Cancelled) {
                        ui.horizontal(|ui|{
                            if ui.button("Pause").clicked(){let project=project.id.clone();let id=job.id.clone();self.action(move |root|{imports::request(&root,&project,&id,false)?;Ok(None)});}
                            if ui.button("Resume").clicked(){let project=project.id.clone();let id=job.id.clone();self.action(move |root|{background::launch_import(&root,&project,&id)?;Ok(None)});}
                            if ui.button("Cancel import").clicked(){let project=project.id.clone();let id=job.id.clone();self.action(move |root|{imports::request(&root,&project,&id,true)?;Ok(None)});}
                        });
                    }
                }
            });
            egui::CollapsingHeader::new("Research scope").default_open(true).show(ui,|ui|{
                ui.label("A few words describing what is being researched, e.g. Rendering or Asset platform.");
                if ui.add(egui::TextEdit::singleline(&mut self.scope).desired_width(f32::INFINITY)).changed(){self.scope_dirty=true;}
                ui.horizontal(|ui|{
                    if ui.add_enabled(!self.dirty,egui::Button::new("Save research scope")).clicked(){
                        let scope=self.scope.clone();let revision=self.scope_revision;self.mutate(move |lib,p,id|{lib.set_scope(p,&scope,revision)?;Ok(id)});
                    }
                    if ui.button("Discard scope edits").clicked(){self.scope=project.research_scope.clone();self.scope_revision=project.revision;self.scope_dirty=false;}
                });
            });
            egui::CollapsingHeader::new("Binary identity").show(ui,|ui|{
                ui.label("Build, SHA-256, image base. Use another project for a different binary.");
                let locked=project.nodes.iter().any(|n|n.kind==Kind::Function);
                ui.add_enabled(!locked,egui::TextEdit::singleline(&mut self.binary).desired_width(f32::INFINITY));
                if ui.add_enabled(!locked&&!self.has_unsaved(),egui::Button::new("Save binary identity")).clicked(){
                    let binary=self.binary.clone();self.mutate(move |lib,p,id|{lib.set_binary(p,&binary)?;Ok(id)});
                }
            });
            ui.separator();
            ui.columns(2,|columns| {
                let dirty = self.has_unsaved();
                if let Some(id) = self.browser.show(&mut columns[0], &self.browser_data[self.project], self.node, dirty) { nav = Some((self.project, id)); }
                let n=self.browser_data[self.project].node(project,self.node).unwrap();
                let ui=&mut columns[1];ui.strong(format!("{} — {}",n.kind.label(),n.name));
                ui.label(project_dir.join(&self.browser_data[self.project].paths[&self.node]).display().to_string());
                ui.label(if self.browser.required(n.id) { "Required research (including explicit prerequisites)" } else { "Context inventory; preserved outside required progress" });
                let mut context_only = n.context_only;
                if n.kind != Kind::App && ui.add_enabled(!self.has_unsaved(), egui::Checkbox::new(&mut context_only, "Context-only child membership")).changed() {
                    let batch = Batch { schema: 1, repair_legacy_links: false, binary: project.binary.clone(), expect_revision: project.revision, create: vec![], update: vec![catalog_batch::Update { id: n.id, parent: None, name: None, context_only: Some(context_only) }], reorder: vec![] };
                    self.mutate(move |lib,p,id|{lib.batch(p,&batch)?;Ok(id)});
                }
                if n.kind==Kind::Function && ui.button("Open this function's data folder").clicked(){
                    let path=project_dir.join(&self.browser_data[self.project].paths[&self.node]).parent().unwrap().join("data");
                    if let Err(e)=open_folder(&path){self.message=e;}
                }

                if let Some(kind)=n.kind.child() {
                    ui.label(format!("Add {} here",kind.label()));ui.text_edit_singleline(&mut self.new_name);
                    if kind==Kind::Function {ui.horizontal(|ui|{ui.label("RVA (hex)");ui.text_edit_singleline(&mut self.rva);});}
                    if ui.add_enabled(!self.has_unsaved(),egui::Button::new(format!("Create {} file",kind.label()))).clicked(){let name=self.new_name.clone();let rva=self.rva.clone();self.mutate(move |lib,p,id|lib.add(p,id,&name,&rva));}
                }
                if n.kind==Kind::Function {
                    ui.separator();ui.label("Prerequisites (function RVAs, comma separated)");
                    ui.text_edit_singleline(&mut self.dependency_rvas);
                    if ui.add_enabled(!self.has_unsaved(),egui::Button::new("Save prerequisite tags")).clicked(){
                        let rvas=self.dependency_rvas.clone();self.mutate(move |lib,p,id|{lib.set_dependencies(p,id,&rvas)?;Ok(id)});
                    }
                    let inbound:Vec<_>=project.dependencies.iter().filter(|e|e.prerequisite==self.node).filter_map(|e|self.browser_data[self.project].node(project,e.dependent)).map(|n|format!("{} (0x{:08X})",n.name,n.rva.unwrap())).collect();
                    ui.label(format!("Required by: {}",if inbound.is_empty(){"none".into()}else{inbound.join(", ")}));
                    ui.label("dep.toml lists prerequisites; prereq.toml lists dependents. Managed together by the app.");
                }
                ui.label("Save or discard edits before changing records. Parent reports link their direct source reports; evidence remains in the original function files.");
            });
            ui.separator();ui.label("Research and understanding — Markdown");
            if ui.add(egui::TextEdit::multiline(&mut self.body).desired_width(f32::INFINITY).desired_rows(12).code_editor()).changed(){self.dirty=true;}
            ui.label("Evidence — cite source/build, VA/RVA, artifact or capture; label observed / specification-defined / inferred");
            if ui.add(egui::TextEdit::multiline(&mut self.evidence).desired_width(f32::INFINITY).desired_rows(4).code_editor()).changed(){self.dirty=true;}
            let kind=self.browser_data[self.project].node(project,self.node).unwrap().kind;
            egui::CollapsingHeader::new("Completion criteria for this scope").show(ui,|ui|{
                ui.label(kind.completion_criteria());
                ui.label("Completed children make a parent ready for synthesis; the parent's own understanding must be reviewed separately.");
                ui.label("Keep this assignment's scope fixed. Record missing knowledge outside it as a separate task; tag it as a prerequisite only when needed to establish this scope's behavior. A connection alone does not expand the assignment.");
            });
            if ui.checkbox(&mut self.complete,"Mark complete (I reviewed the scope criteria, understanding and evidence)").changed(){self.dirty=true;}
            ui.horizontal(|ui|{
                if ui.add_enabled(!self.scope_dirty,egui::Button::new("Save canonical report")).clicked(){let body=self.body.clone();let evidence=self.evidence.clone();let complete=self.complete;let token=if complete{self.completion_hash.clone()}else{self.loaded_hash.clone()};self.mutate(move |lib,p,id|{lib.save_report(p,id,&body,&evidence,complete,&token)?;Ok(id)});}
                if ui.button("Discard / reload").clicked(){nav=Some((self.project,self.node));}
                if self.has_unsaved(){ui.label("Unsaved edits");}
            });
        });
        if let Some((p, id)) = nav
            && let Some(current) = navigation_ids
                .get(p)
                .and_then(|id| self.ui_projects.iter().position(|v| &v.id == id))
        {
            self.load(current, id);
        }
        self.open = open;
    }
}

#[derive(Clone)]
pub struct Meter {
    pub id: u64,
    pub name: String,
    pub functions: usize,
    pub completed_functions: usize,
    pub states: Vec<usize>,
}
#[derive(Clone)]
pub struct ProjectView {
    pub inventory_functions: usize,
    pub required_functions: usize,
    pub app_status: Status,
    pub id: String,
    pub name: String,
    pub meters: Vec<Meter>,
    pub research_scope: String,
}
impl ResearchUi {
    pub fn poll(&mut self, ctx: &egui::Context) {
        use std::sync::mpsc::TryRecvError;
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        if let Some(task) = &self.action_task {
            match task.try_recv() {
                Ok(result) => {
                    self.action_task = None;
                    match result {
                        Ok(target) => {
                            self.message = "Operation finished. Refreshing...".into();
                            if let Some(target) = target {
                                self.generation += 1;
                                self.requested = Some(target);
                                self.dirty = false;
                                self.scope_dirty = false;
                            }
                            self.refresh_library();
                        }
                        Err(e) => self.message = e,
                    }
                }
                Err(TryRecvError::Disconnected) => {
                    self.action_task = None;
                    self.message =
                        "Background operation failed; reload to inspect committed state.".into();
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        if let Some((generation, task)) = &self.load_task {
            let generation = *generation;
            match task.try_recv() {
                Ok(result) => {
                    self.load_task = None;
                    if generation == self.generation {
                        match result {
                            Ok(v) => {
                                self.project_id = v.project;
                                self.node = v.id;
                                self.body = v.body;
                                self.evidence = v.evidence;
                                self.loaded_hash = v.draft;
                                self.completion_hash = v.completion;
                                self.complete = v.complete;
                                self.binary = v.binary;
                                self.scope = v.scope;
                                self.scope_revision = v.revision;
                                self.dependency_rvas = v.dependencies;
                                self.dirty = false;
                                self.scope_dirty = false;
                                self.message.clear();
                            }
                            Err(e) => self.message = e,
                        }
                    }
                }
                Err(TryRecvError::Disconnected) => {
                    self.load_task = None;
                    self.message = "Report loader failed".into();
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        if self.load_task.is_none()
            && let Some((project, id)) = self.requested.take()
        {
            let (send, recv) = std::sync::mpsc::sync_channel(1);
            self.load_task = Some((self.generation, recv));
            std::thread::spawn(move || {
                let _ = send.send(background::load(&project, id));
            });
        }
        let delivered = self
            .snapshot_inbox
            .as_mut()
            .and_then(|inbox| match inbox.take() {
                Ok(v) => v,
                Err(e) => Some(Err(e)),
            });
        if let Some((generation, task)) = &self.refresh_task {
            let generation = *generation;
            let outcome = match delivered {
                Some(value) => Ok(value),
                None => match task.try_recv() {
                    Ok(Err(e)) => Ok(Err(e)),
                    Ok(Ok(())) | Err(TryRecvError::Empty) => Err(TryRecvError::Empty),
                    Err(TryRecvError::Disconnected) => Err(TryRecvError::Disconnected),
                },
            };
            match outcome {
                Ok(Ok(v)) if generation == self.refresh_generation => {
                    self.refresh_task = None;
                    self.fingerprint = v.fingerprint;
                    self.jobs = v.jobs;
                    if let Some(root) = v.root {
                        self.root = Some(root);
                        let retired = (
                            std::mem::replace(&mut self.ui_projects, v.projects),
                            std::mem::replace(&mut self.browser_data, v.browser),
                            std::mem::replace(&mut self.views, v.views),
                        );
                        std::thread::spawn(move || drop(retired));
                        self.changed = true;
                        if let Some(p) = self
                            .ui_projects
                            .iter()
                            .position(|p| p.id == self.project_id)
                        {
                            self.project = p;
                        }
                        if !self.has_unsaved()
                            && self.load_task.is_none()
                            && self.requested.is_none()
                            && let Some(p) = self.ui_projects.get(self.project)
                        {
                            let id = if self.browser_data[self.project].node(p, self.node).is_some()
                            {
                                self.node
                            } else {
                                0
                            };
                            self.load(self.project, id);
                        }
                    }
                }
                Ok(Ok(stale)) => {
                    std::thread::spawn(move || drop(stale));
                    self.refresh_task = None;
                }
                Ok(Err(e)) => {
                    self.refresh_task = None;
                    self.message = e;
                }
                Err(TryRecvError::Disconnected) => {
                    self.refresh_task = None;
                    self.message = "Snapshot worker failed".into();
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        if self.refresh_task.is_none()
            && self.last_poll.elapsed() >= std::time::Duration::from_secs(2)
        {
            if self.snapshot_inbox.is_none() {
                match background::SnapshotInbox::new() {
                    Ok(inbox) => self.snapshot_inbox = Some(inbox),
                    Err(e) => {
                        self.message = e;
                        return;
                    }
                }
            }
            let owner = self.snapshot_inbox.as_ref().unwrap().sender();
            self.last_poll = std::time::Instant::now();
            let (send, recv) = std::sync::mpsc::sync_channel(1);
            self.refresh_task = Some((self.refresh_generation, recv));
            let fingerprint = self.fingerprint.clone();
            let projects = self.ui_projects.clone();
            let browser = self.browser_data.clone();
            let selected = (self.project_id.clone(), self.node);
            let views = self.views.clone();
            std::thread::spawn(move || {
                let result = background::snapshot(fingerprint, projects, browser, views, selected);
                let posted = owner
                    .try_post(solworker::SWPhase(1), move |slot| *slot = Some(result))
                    .map(|_| ())
                    .map_err(|e| format!("Snapshot delivery rejected: {:?}", e.reason));
                let _ = send.send(posted);
            });
        }
    }
    pub fn refresh_library(&mut self) {
        self.refresh_generation += 1;
        self.fingerprint.clear();
        self.last_poll = std::time::Instant::now() - std::time::Duration::from_secs(3);
    }
    pub fn snapshot(&self) -> Vec<ProjectView> {
        self.views.clone()
    }
    pub fn open_record(&mut self, project_id: &str, id: u64) {
        self.open = true;
        if self.has_unsaved() {
            self.message =
                "Save or discard the current edits before opening another report.".into();
            return;
        }
        if let Some(p) = self.ui_projects.iter().position(|p| p.id == project_id) {
            self.load(p, id);
        }
    }
}

impl ResearchUi {
    pub fn has_unsaved(&self) -> bool {
        self.dirty || self.scope_dirty
    }
    pub fn prevent_close(&mut self) {
        self.open = true;
        self.message = "Save or discard your report edits before closing the app.".into();
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Dependency {
    pub dependent: u64,
    pub prerequisite: u64,
}
#[derive(Serialize)]
struct FunctionTag {
    node_id: u64,
    rva: String,
    report: String,
}
#[derive(Serialize)]
struct DependencyFile {
    schema: u32,
    project: String,
    binary: String,
    function_rva: String,
    requires: Vec<FunctionTag>,
}
#[derive(Serialize)]
struct PrerequisiteFile {
    schema: u32,
    project: String,
    binary: String,
    function_rva: String,
    required_by: Vec<FunctionTag>,
}
impl Library {
    fn validate_dependencies(&self, p: usize, edges: &[Dependency]) -> Result<()> {
        let functions: std::collections::HashSet<_> = self.projects[p]
            .nodes
            .iter()
            .filter(|n| n.kind == Kind::Function)
            .map(|n| n.id)
            .collect();
        let mut pairs = std::collections::HashSet::new();
        for e in edges {
            if e.dependent == e.prerequisite
                || !functions.contains(&e.dependent)
                || !functions.contains(&e.prerequisite)
                || !pairs.insert((e.dependent, e.prerequisite))
            {
                return Err(
                    "Prerequisites must be unique, different functions in this same project."
                        .into(),
                );
            }
        }
        fn visit(
            id: u64,
            edges: &[Dependency],
            active: &mut std::collections::HashSet<u64>,
            done: &mut std::collections::HashSet<u64>,
        ) -> bool {
            if done.contains(&id) {
                return true;
            }
            if !active.insert(id) {
                return false;
            }
            for e in edges.iter().filter(|e| e.dependent == id) {
                if !visit(e.prerequisite, edges, active, done) {
                    return false;
                }
            }
            active.remove(&id);
            done.insert(id);
            true
        }
        let mut done = std::collections::HashSet::new();
        for id in functions {
            if !visit(id, edges, &mut Default::default(), &mut done) {
                return Err("That creates a prerequisite cycle. These tags describe research prerequisites, not every call edge.".into());
            }
        }
        Ok(())
    }
    fn sync_dependency_files(&self, p: usize) -> Result<()> {
        self.sync_dependency_subset(p, None)
    }
    fn sync_dependency_subset(
        &self,
        p: usize,
        only: Option<&std::collections::HashSet<u64>>,
    ) -> Result<()> {
        let project = &self.projects[p];
        let index = catalog_index::Index::new(project);
        let mut incoming: std::collections::HashMap<u64, Vec<u64>> = Default::default();
        for e in &project.dependencies {
            incoming
                .entry(e.prerequisite)
                .or_default()
                .push(e.dependent);
        }
        for node in project
            .nodes
            .iter()
            .filter(|n| n.kind == Kind::Function && only.is_none_or(|ids| ids.contains(&n.id)))
        {
            let tag = |id| {
                let n = index.nodes[&id];
                FunctionTag {
                    node_id: id,
                    rva: format!("0x{:08X}", n.rva.unwrap()),
                    report: layout::link(&index.paths[&node.id], &index.paths[&n.id]),
                }
            };
            let deps = DependencyFile {
                schema: 1,
                project: project.id.clone(),
                binary: project.binary.clone(),
                function_rva: format!("0x{:08X}", node.rva.unwrap()),
                requires: index
                    .requires
                    .get(&node.id)
                    .into_iter()
                    .flatten()
                    .map(|id| tag(*id))
                    .collect(),
            };
            let prereq = PrerequisiteFile {
                schema: 1,
                project: project.id.clone(),
                binary: project.binary.clone(),
                function_rva: format!("0x{:08X}", node.rva.unwrap()),
                required_by: incoming
                    .get(&node.id)
                    .into_iter()
                    .flatten()
                    .map(|id| tag(*id))
                    .collect(),
            };
            let folder = self
                .dir(p)
                .join(&index.paths[&node.id])
                .parent()
                .unwrap()
                .to_owned();
            fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
            for (name, body) in [
                ("dep.toml", toml::to_string_pretty(&deps)),
                ("prereq.toml", toml::to_string_pretty(&prereq)),
            ] {
                let content = format!(
                    "# Managed by Scopework. Edit through the app or research deps set.\n{}",
                    body.map_err(|e| e.to_string())?
                );
                let path = folder.join(name);
                if fs::read_to_string(&path).ok().as_deref() != Some(content.as_str()) {
                    atomic(&path, content.as_bytes())?;
                }
            }
        }
        Ok(())
    }
    fn dependency_stamps(&self, p: usize, id: u64) -> std::collections::BTreeMap<u64, String> {
        self.projects[p]
            .dependencies
            .iter()
            .filter(|e| e.dependent == id)
            .map(|e| {
                let n = self.projects[p]
                    .nodes
                    .iter()
                    .find(|n| n.id == e.prerequisite)
                    .unwrap();
                let content = self
                    .path(p, n.id)
                    .ok()
                    .and_then(|path| fs::read_to_string(path).ok())
                    .unwrap_or_default();
                (n.id, format!("{}:{}", n.revision, hash(&content)))
            })
            .collect()
    }
    fn invalidate_dependents(&mut self, p: usize, id: u64) {
        let mut queue = vec![id];
        let mut seen = std::collections::HashSet::new();
        while let Some(source) = queue.pop() {
            let targets: Vec<_> = self.projects[p]
                .dependencies
                .iter()
                .filter(|e| e.prerequisite == source)
                .map(|e| e.dependent)
                .collect();
            for target in targets {
                if seen.insert(target) {
                    let n = self.projects[p]
                        .nodes
                        .iter_mut()
                        .find(|n| n.id == target)
                        .unwrap();
                    if n.status == Status::Complete {
                        n.status = Status::InProgress;
                    }
                    n.accepted_hash = None;
                    n.accepted_prerequisites.clear();
                    n.revision += 1;
                    self.invalidate(p, target);
                    queue.push(target);
                }
            }
        }
    }
    pub fn set_dependencies(&mut self, p: usize, id: u64, rvas: &str) -> Result<()> {
        if !self.writable(p) {
            return self.write_project(p, |l, p| l.set_dependencies(p, id, rvas));
        }
        self.ensure_structural(p)?;
        if !self.projects[p]
            .nodes
            .iter()
            .any(|n| n.id == id && n.kind == Kind::Function)
        {
            return Err("Select a function report.".into());
        }
        let mut edges: Vec<_> = self.projects[p]
            .dependencies
            .iter()
            .filter(|e| e.dependent != id)
            .cloned()
            .collect();
        for rva in rvas
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|s| !s.is_empty())
        {
            let address =
                u32::from_str_radix(rva.trim_start_matches("0x").trim_start_matches("0X"), 16)
                    .map_err(|_| format!("Invalid hexadecimal RVA: {rva}"))?;
            let n = self.projects[p]
                .nodes
                .iter()
                .find(|n| n.rva == Some(address))
                .ok_or_else(|| format!("RVA {rva} has no function report in this project."))?;
            edges.push(Dependency {
                dependent: id,
                prerequisite: n.id,
            });
        }
        edges.sort_by_key(|e| (e.dependent, e.prerequisite));
        self.validate_dependencies(p, &edges)?;
        let mut previous = self.projects[p].dependencies.clone();
        previous.sort_by_key(|e| (e.dependent, e.prerequisite));
        if edges == previous {
            return Ok(());
        }
        let affected: std::collections::HashSet<_> = previous
            .iter()
            .chain(&edges)
            .filter(|e| e.dependent == id)
            .flat_map(|e| [e.dependent, e.prerequisite])
            .chain([id])
            .collect();
        let before = self.projects[p].clone();
        self.projects[p].structural_epoch += 1;
        self.projects[p].dependencies = edges;
        let n = self.projects[p]
            .nodes
            .iter_mut()
            .find(|n| n.id == id)
            .unwrap();
        if n.status == Status::Complete {
            n.status = Status::InProgress;
        }
        n.accepted_hash = None;
        n.accepted_prerequisites.clear();
        n.revision += 1;
        self.invalidate(p, id);
        self.invalidate_dependents(p, id);
        self.projects[p].revision += 1;
        let after = std::mem::replace(&mut self.projects[p], before.clone());
        self.commit_metadata(p, before, after, vec![], affected.into_iter().collect())
    }
    pub fn set_dependencies_by_rva(
        &mut self,
        project: &str,
        function: &str,
        dependencies: &str,
    ) -> Result<()> {
        let p = self
            .projects
            .iter()
            .position(|p| p.id == project)
            .ok_or("Unknown project ID")?;
        let rva = u32::from_str_radix(
            function.trim_start_matches("0x").trim_start_matches("0X"),
            16,
        )
        .map_err(|_| "Invalid function RVA")?;
        let id = self.projects[p]
            .nodes
            .iter()
            .find(|n| n.rva == Some(rva))
            .ok_or("Unknown function RVA in this project")?
            .id;
        self.set_dependencies(p, id, dependencies)
    }
}

impl Library {
    fn views(&self) -> Vec<ProjectView> {
        let lib = self;
        lib.projects
            .iter()
            .enumerate()
            .map(|(p, project)| {
                let index = catalog_index::Index::new(project);
                let states_all = index.states(&lib.dir(p));
                let mut counts = std::collections::HashMap::<u64, (usize, usize)>::new();
                let mut inventory_functions = 0;
                let mut required_functions = 0;
                for n in &project.nodes {
                    if n.kind == Kind::Function {
                        inventory_functions += 1;
                        if index.required.contains(&n.id) {
                            required_functions += 1;
                            if let Some(arch) = n.parent.and_then(|id| index.nodes[&id].parent) {
                                let count = counts.entry(arch).or_default();
                                count.0 += 1;
                                count.1 += usize::from(states_all[&n.id] == Status::Complete);
                            }
                        }
                    }
                }
                let meters = project
                    .nodes
                    .iter()
                    .filter(|n| n.kind == Kind::Architecture)
                    .map(|a| {
                        let (functions, completed_functions) =
                            counts.get(&a.id).copied().unwrap_or_default();
                        let states = index
                            .children
                            .get(&a.id)
                            .into_iter()
                            .flatten()
                            .filter(|id| index.required.contains(id))
                            .map(|id| match states_all[id] {
                                Status::Complete => 0,
                                Status::InProgress => 1,
                                Status::NotStarted => 2,
                            })
                            .collect();
                        Meter {
                            id: a.id,
                            name: if index.required.contains(&a.id) {
                                a.name.clone()
                            } else {
                                format!("{} [context]", a.name)
                            },
                            functions,
                            completed_functions,
                            states,
                        }
                    })
                    .collect();
                ProjectView {
                    inventory_functions,
                    required_functions,
                    app_status: states_all[&0],
                    id: project.id.clone(),
                    name: project.name.clone(),
                    meters,
                    research_scope: project.research_scope.clone(),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod background_ui_tests {
    use super::*;
    fn isolated_ui() -> ResearchUi {
        let mut ui = ResearchUi::new();
        ui.root = Some(PathBuf::from("C:/unused-background-ui-fixture"));
        ui.last_poll = std::time::Instant::now();
        ui
    }
    fn loaded(project: &str, id: u64) -> background::Loaded {
        background::Loaded {
            project: project.into(),
            id,
            body: "loaded".into(),
            evidence: String::new(),
            draft: "d4:test".into(),
            completion: "c4:test".into(),
            complete: false,
            binary: "fixture".into(),
            scope: String::new(),
            revision: 1,
            dependencies: String::new(),
        }
    }
    #[test]
    fn obsolete_report_load_cannot_replace_current_selection_or_draft() {
        let mut ui = isolated_ui();
        ui.generation = 2;
        ui.project_id = "newer".into();
        ui.body = "unsaved prose".into();
        ui.dirty = true;
        let (send, recv) = std::sync::mpsc::sync_channel(1);
        send.send(Ok(loaded("older", 42))).unwrap();
        ui.load_task = Some((1, recv));
        ui.poll(&egui::Context::default());
        assert_eq!(ui.project_id, "newer");
        assert_eq!(ui.body, "unsaved prose");
        assert!(ui.dirty);
        assert!(ui.load_task.is_none());
    }
    #[test]
    fn newer_meters_publish_without_discarding_unsaved_text() {
        let mut ui = isolated_ui();
        ui.body = "unsaved prose".into();
        ui.dirty = true;
        let inbox = background::SnapshotInbox::new().unwrap();
        let value = background::Snapshot {
            root: Some(ui.root.clone().unwrap()),
            projects: vec![],
            browser: vec![],
            views: vec![ProjectView {
                inventory_functions: 12,
                required_functions: 10,
                app_status: Status::InProgress,
                id: "fixture".into(),
                name: "Fixture".into(),
                meters: vec![],
                research_scope: "Scope".into(),
            }],
            jobs: Default::default(),
            fingerprint: "new".into(),
        };
        assert!(
            inbox
                .sender()
                .try_post(solworker::SWPhase(1), move |slot| *slot = Some(Ok(value)))
                .is_ok()
        );
        let (_send, recv) = std::sync::mpsc::sync_channel(1);
        ui.refresh_task = Some((0, recv));
        ui.snapshot_inbox = Some(inbox);
        ui.poll(&egui::Context::default());
        assert_eq!(ui.views[0].inventory_functions, 12);
        assert_eq!(ui.body, "unsaved prose");
        assert!(ui.dirty);
        assert!(ui.requested.is_none());
    }
    #[test]
    fn obsolete_snapshot_is_rejected_and_owner_shutdown_rejects_late_delivery() {
        let mut ui = isolated_ui();
        ui.refresh_generation = 2;
        ui.fingerprint = "current".into();
        let inbox = background::SnapshotInbox::new().unwrap();
        let sender = inbox.sender();
        let value = background::Snapshot {
            root: None,
            projects: vec![],
            browser: vec![],
            views: vec![],
            jobs: Default::default(),
            fingerprint: "obsolete".into(),
        };
        assert!(
            sender
                .try_post(solworker::SWPhase(1), move |slot| *slot = Some(Ok(value)))
                .is_ok()
        );
        // Capacity is one; providers cannot accumulate snapshot generations.
        assert!(sender.try_post(solworker::SWPhase(1), |_| {}).is_err());
        let (_send, recv) = std::sync::mpsc::sync_channel(1);
        ui.refresh_task = Some((1, recv));
        ui.snapshot_inbox = Some(inbox);
        ui.poll(&egui::Context::default());
        assert_eq!(ui.fingerprint, "current");
        drop(ui);
        assert!(sender.try_post(solworker::SWPhase(1), |_| {}).is_err());
    }
}
