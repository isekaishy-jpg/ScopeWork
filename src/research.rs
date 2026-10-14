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
    fn completion_criteria(self) -> &'static str {
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
                "Synthesize the completed architecture reports into an explanation of the complete application and how its subsystems work together. Every required architecture must be complete."
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
    pub id: String,
    pub name: String,
    pub binary: String,
    pub next_id: u64,
    pub revision: u64,
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
}
// Reports contain their editable source in Markdown; the catalog stores no duplicate report prose.
fn hash(text: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(text.as_bytes()))
}
fn atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let temp = path.with_extension("pending");
    fs::write(&temp, bytes).map_err(|e| e.to_string())?;
    fs::rename(&temp, path).map_err(|e| format!("Cannot replace {}: {e}", path.display()))
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !["con", "prn", "aux", "nul", "com1", "lpt1"].contains(&id)
}
fn relative(n: &Node) -> String {
    match n.kind {
        Kind::App => "app.md".into(),
        Kind::Architecture => format!("architectures/{:06}/research.md", n.id),
        Kind::Chunk => format!("chunks/{:06}/research.md", n.id),
        Kind::Function => format!("functions/{:08X}/research.md", n.rva.unwrap()),
    }
}

// One project-local destination for each kind of reversing material.
const PROJECT_FOLDERS: &[&str] = &[
    "functions",
    "chunks",
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
            f.write_all(b"# Project reversing tools\n\nThis directory belongs only to its enclosing research project.\n\nprofiles/: compiler, ABI, type and analysis profiles.\nscripts/: analysis drivers and exporters.\ninstrumentation/: observational hooks and capture utilities.\nbin/: tool binaries or launchers.\nworkspaces/: shared Ghidra projects and whole-binary analysis databases.\nlogs/: shared tool run logs and provenance.\nscratch/: shared temporary working files.\n\nFunction-specific captures, exports and evidence belong in ../functions/<RVA>/data/, beside that function's research.md. Link to them from research.md using data/<filename>. Do not create project-level evidence or data dumps. Chunks and architecture reports reference the canonical reports below them. Source installations remain read-only. Record input binary hashes and tool/profile versions for tool runs.\n").map_err(|e|e.to_string())?;
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

fn rebase_links(text: &str) -> String {
    let mut output = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("](") {
        output.push_str(&rest[..start + 2]);
        rest = &rest[start + 2..];
        let Some(end) = rest.find(')') else {
            output.push_str(rest);
            return output;
        };
        let target = &rest[..end];
        if !target.is_empty()
            && !target.starts_with('#')
            && !target.starts_with('/')
            && !target.starts_with('\\')
            && !target.contains(':')
        {
            if target.starts_with('<') {
                output.push_str("<../");
                output.push_str(&target[1..]);
            } else {
                output.push_str("../");
                output.push_str(target);
            }
        } else {
            output.push_str(target);
        }
        output.push(')');
        rest = &rest[end + 1..];
    }
    output.push_str(rest);
    output
}

pub struct Library {
    pub root: PathBuf,
    pub projects: Vec<Project>,
    _lock: std::fs::File,
}
impl Library {
    pub fn open(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join(".library.lock"))
            .map_err(|e| e.to_string())?;
        lock.try_lock().map_err(|_| {
            "This research library is already open in another app instance.".to_string()
        })?;
        let mut lib = Self {
            root,
            projects: vec![],
            _lock: lock,
        };
        for e in fs::read_dir(&lib.root).map_err(|e| e.to_string())? {
            let e = e.map_err(|e| e.to_string())?;
            if e.file_type().map_err(|e| e.to_string())?.is_symlink() {
                continue;
            }
            let path = e.path().join("project.json");
            if path.exists() {
                let p: Project =
                    serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
                        .map_err(|e| e.to_string())?;
                if !valid_id(&p.id) || e.file_name().to_string_lossy() != p.id {
                    return Err("Invalid project identity".into());
                }
                let mut ids = std::collections::HashSet::new();
                let mut rvas = std::collections::HashSet::new();
                for n in &p.nodes {
                    if !ids.insert(n.id) {
                        return Err("Duplicate node identity".into());
                    }
                    if n.kind == Kind::Function && (n.rva.is_none() || !rvas.insert(n.rva)) {
                        return Err("Invalid or duplicate function RVA".into());
                    }
                    if n.kind != Kind::App {
                        let parent = p
                            .nodes
                            .iter()
                            .find(|v| Some(v.id) == n.parent)
                            .ok_or("Missing report parent")?;
                        if parent.kind.child() != Some(n.kind) {
                            return Err("Invalid research hierarchy".into());
                        }
                    } else if n.parent.is_some() {
                        return Err("App cannot have a parent".into());
                    }
                }
                if p.nodes.iter().filter(|n| n.kind == Kind::App).count() != 1 {
                    return Err("Project must contain exactly one app report".into());
                }
                ensure_folders(&lib.root.join(&p.id))?;
                lib.projects.push(p);
            }
        }
        lib.projects.sort_by_key(|p| p.name.clone());
        if !lib.projects.iter().any(|p| p.id == "stock") {
            lib.create("stock","Stock","WoW 3.3.5a build 12340; SHA-256 aa63a5750d60ef16746c686b3d5e26876d98953eab08b1c026cd0faf78e88cb8; image base 0x00400000")?;
        }
        if !lib.projects.iter().any(|p| p.id == "forever") {
            lib.create("forever", "Forever", "")?;
        }
        lib.projects.sort_by_key(|p| {
            (
                match p.id.as_str() {
                    "stock" => 0,
                    "forever" => 1,
                    _ => 2,
                },
                p.name.clone(),
            )
        });
        for p in 0..lib.projects.len() {
            lib.validate_dependencies(p, &lib.projects[p].dependencies)?;
            lib.migrate_layout(p)?;
            lib.sync_dependency_files(p)?;
        }
        Ok(lib)
    }
    fn dir(&self, p: usize) -> PathBuf {
        self.root.join(&self.projects[p].id)
    }
    fn persist(&self, p: usize) -> Result<()> {
        atomic(
            &self.dir(p).join("project.json"),
            &serde_json::to_vec_pretty(&self.projects[p]).map_err(|e| e.to_string())?,
        )?;
        self.sync_dependency_files(p)
    }
    fn migrate_layout(&mut self, p: usize) -> Result<()> {
        let dir = self.dir(p);
        let nodes = self.projects[p].nodes.clone();
        let mut moved = false;
        for n in &nodes {
            let legacy = match n.kind {
                Kind::App => continue,
                Kind::Architecture => dir.join(format!("architectures/{:06}.md", n.id)),
                Kind::Chunk => dir.join(format!("chunks/{:06}.md", n.id)),
                Kind::Function => dir.join(format!("functions/{:08X}.md", n.rva.unwrap())),
            };
            let target = self.path(p, n.id)?;
            if legacy.exists() {
                if target.exists() {
                    return Err(format!(
                        "Both legacy and canonical reports exist: {}. Neither was overwritten.",
                        target.display()
                    ));
                }
                fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
                // Both paths are constructed only from the validated project and node IDs.
                let root = dir.canonicalize().map_err(|e| e.to_string())?;
                if !legacy
                    .canonicalize()
                    .map_err(|e| e.to_string())?
                    .starts_with(&root)
                    || !target
                        .parent()
                        .unwrap()
                        .canonicalize()
                        .map_err(|e| e.to_string())?
                        .starts_with(&root)
                {
                    return Err("Report migration leaves project directory".into());
                }
                fs::rename(&legacy, &target).map_err(|e| e.to_string())?;
                let (body, evidence) = self.read(p, n.id)?;
                self.write_report(p, n.id, &rebase_links(&body), &rebase_links(&evidence))?;
                let item = self.projects[p]
                    .nodes
                    .iter_mut()
                    .find(|v| v.id == n.id)
                    .unwrap();
                if item.status == Status::Complete {
                    item.status = Status::InProgress;
                }
                item.accepted_hash = None;
                item.revision += 1;
                moved = true;
            }
            if n.kind == Kind::Function {
                fs::create_dir_all(target.parent().unwrap().join("data"))
                    .map_err(|e| e.to_string())?;
            }
        }
        // Rebuild generated source-report links after changing canonical paths.
        if moved {
            for n in nodes.iter().filter(|n| n.kind != Kind::Function) {
                let (body, evidence) = self.read(p, n.id)?;
                self.write_report(p, n.id, &body, &evidence)?;
                let item = self.projects[p]
                    .nodes
                    .iter_mut()
                    .find(|v| v.id == n.id)
                    .unwrap();
                if item.status == Status::Complete {
                    item.status = Status::InProgress;
                }
                item.accepted_hash = None;
            }
            self.projects[p].revision += 1;
            self.persist(p)?;
        }
        // Shared tool databases have an unambiguous home. Unowned historical data
        // is retained for assignment rather than attributed to an invented function.
        for (from, to) in [
            ("data/analysis", "tools/workspaces/analysis"),
            ("data/logs", "tools/logs/legacy"),
            ("data/scratch", "tools/scratch/legacy"),
            ("data/captures", "tools/unassigned/captures"),
            ("data/exports", "tools/unassigned/exports"),
            ("evidence", "tools/unassigned/evidence"),
        ] {
            let source = dir.join(from);
            let target = dir.join(to);
            if source.is_dir() {
                if fs::read_dir(&source)
                    .map_err(|e| e.to_string())?
                    .next()
                    .is_none()
                {
                    fs::remove_dir(&source).map_err(|e| e.to_string())?;
                    continue;
                }
                if target.exists() {
                    return Err(format!(
                        "Migration destination already exists: {}. Existing data was preserved.",
                        target.display()
                    ));
                }
                fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
                let root = dir.canonicalize().map_err(|e| e.to_string())?;
                if !source
                    .canonicalize()
                    .map_err(|e| e.to_string())?
                    .starts_with(&root)
                    || !target
                        .parent()
                        .unwrap()
                        .canonicalize()
                        .map_err(|e| e.to_string())?
                        .starts_with(&root)
                {
                    return Err("Data migration leaves project directory".into());
                }
                fs::rename(source, target).map_err(|e| e.to_string())?;
            }
        }
        let old = dir.join("data");
        if old.is_dir()
            && fs::read_dir(&old)
                .map_err(|e| e.to_string())?
                .next()
                .is_none()
        {
            fs::remove_dir(old).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    pub fn create(&mut self, id: &str, name: &str, binary: &str) -> Result<()> {
        if !valid_id(id) || name.trim().is_empty() {
            return Err("Use a project ID with lowercase letters, digits, and hyphens, and a nonempty name.".into());
        }
        if self.root.join(id).exists() {
            return Err("That project already exists.".into());
        }
        let dir = self.root.join(id);
        fs::create_dir(&dir).map_err(|e| e.to_string())?;
        ensure_folders(&dir)?;
        self.projects.push(Project {
            id: id.into(),
            name: name.into(),
            binary: binary.into(),
            dependencies: vec![],
            next_id: 1,
            revision: 0,
            nodes: vec![Node {
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
        });
        let p = self.projects.len() - 1;
        self.write_report(p, 0, "", "")?;
        self.persist(p)
    }
    pub fn path(&self, p: usize, id: u64) -> Result<PathBuf> {
        let n = self.projects[p]
            .nodes
            .iter()
            .find(|n| n.id == id)
            .ok_or("Unknown report")?;
        Ok(self.dir(p).join(relative(n)))
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
    fn write_report(&self, p: usize, id: u64, body: &str, evidence: &str) -> Result<()> {
        if body.contains("<!-- research-") || evidence.contains("<!-- research-") {
            return Err("Reserved report markers cannot appear in report content.".into());
        }
        let project = &self.projects[p];
        let n = project
            .nodes
            .iter()
            .find(|n| n.id == id)
            .ok_or("Unknown report")?;
        let mut md = format!(
            "# {}\n\nProject: {} (`{}`)\n\nBinary: {}\n\nKind: {} | ID: {}\n",
            n.name,
            project.name,
            project.id,
            project.binary,
            n.kind.label(),
            id
        );
        if let Some(rva) = n.rva {
            md += &format!("\nRVA: `0x{rva:08X}`\n");
        }
        md += "\n## Source reports\n";
        for child in project.nodes.iter().filter(|c| c.parent == Some(id)) {
            let rel = if n.kind == Kind::App {
                relative(child)
            } else {
                format!("../../{}", relative(child))
            };
            md += &format!(
                "- [{}]({})\n",
                child.name.replace('[', "(").replace(']', ")"),
                rel
            );
        }
        md += "\n## Research and understanding\n<!-- research-content -->\n";
        md += body;
        md += "\n<!-- research-evidence -->\n";
        md += evidence;
        let path = self.path(p, id)?;
        let folder = path.parent().ok_or("Missing report folder")?;
        fs::create_dir_all(folder).map_err(|e| e.to_string())?;
        if n.kind == Kind::Function {
            fs::create_dir_all(folder.join("data")).map_err(|e| e.to_string())?;
        }
        atomic(&path, md.as_bytes())
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
    pub fn effective(&self, p: usize, id: u64) -> Status {
        let Some(n) = self.projects[p].nodes.iter().find(|n| n.id == id) else {
            return Status::NotStarted;
        };
        if n.status != Status::Complete {
            if n.status == Status::NotStarted
                && self.projects[p]
                    .nodes
                    .iter()
                    .filter(|c| c.parent == Some(id))
                    .any(|c| self.effective(p, c.id) != Status::NotStarted)
            {
                return Status::InProgress;
            }
            return n.status;
        }
        let valid = fs::read_to_string(self.path(p, id).unwrap())
            .ok()
            .map(|s| Some(hash(&s)) == n.accepted_hash)
            .unwrap_or(false);
        if !valid {
            return Status::InProgress;
        }
        if self.projects[p]
            .dependencies
            .iter()
            .filter(|e| e.dependent == id)
            .any(|e| self.effective(p, e.prerequisite) != Status::Complete)
            || n.accepted_prerequisites != self.dependency_stamps(p, id)
        {
            return Status::InProgress;
        }
        let kids: Vec<_> = self.projects[p]
            .nodes
            .iter()
            .filter(|k| k.parent == Some(id))
            .collect();
        if n.kind != Kind::Function
            && (kids.is_empty()
                || kids
                    .iter()
                    .any(|k| self.effective(p, k.id) != Status::Complete))
        {
            return Status::InProgress;
        }
        Status::Complete
    }
    pub fn add(&mut self, p: usize, parent: u64, name: &str, rva: &str) -> Result<u64> {
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
            let x = u32::from_str_radix(rva.trim().trim_start_matches("0x"), 16)
                .map_err(|_| "Enter an RVA in hexadecimal, not a VA.")?;
            if self.projects[p].nodes.iter().any(|n| n.rva == Some(x)) {
                return Err("That function already has a canonical file in this project.".into());
            }
            Some(x)
        } else {
            None
        };
        let id = self.projects[p].next_id;
        self.projects[p].next_id += 1;
        let relnode = Node {
            id,
            parent: Some(parent),
            kind,
            name: name.trim().into(),
            rva: address,
            status: Status::NotStarted,
            revision: 0,
            accepted_hash: None,
            accepted_prerequisites: Default::default(),
        };
        let path = self.dir(p).join(relative(&relnode));
        if path.exists() {
            return Err("Canonical file already exists; refusing to replace it.".into());
        }
        let (body, evidence) = self.read(p, parent)?;
        self.projects[p].nodes.push(relnode);
        self.write_report(p, id, "", "")?;
        self.invalidate(p, id);
        self.write_report(p, parent, &body, &evidence)?;
        self.projects[p].revision += 1;
        self.persist(p)?;
        Ok(id)
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
        let current = fs::read_to_string(self.path(p, id)?).map_err(|e| e.to_string())?;
        if hash(&current) != expected {
            return Err("This report changed outside the editor. Reload it before saving; your text has not been overwritten.".into());
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
                .filter(|k| k.parent == Some(id))
                .collect();
            if n.kind != Kind::Function
                && (kids.is_empty()
                    || kids
                        .iter()
                        .any(|k| self.effective(p, k.id) != Status::Complete))
            {
                return Err("Every required child must be complete, and the parent needs its own compiled report.".into());
            }
        }
        if complete
            && self.projects[p]
                .dependencies
                .iter()
                .filter(|e| e.dependent == id)
                .any(|e| self.effective(p, e.prerequisite) != Status::Complete)
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
        self.write_report(p, id, body, evidence)?;
        let report = fs::read_to_string(self.path(p, id)?).map_err(|e| e.to_string())?;
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
        self.persist(p)
    }
}

pub struct ResearchUi {
    pub open: bool,
    pub library: Option<Library>,
    pub changed: bool,
    project: usize,
    node: u64,
    body: String,
    evidence: String,
    loaded_hash: String,
    dirty: bool,
    message: String,
    new_name: String,
    rva: String,
    dependency_rvas: String,
    new_project_name: String,
    new_project_id: String,
    binary: String,
    complete: bool,
}
impl ResearchUi {
    pub fn new() -> Self {
        let root = std::env::var_os("RESEARCH_DASHBOARD_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("data/projects"));
        let (library, message) = match Library::open(root) {
            Ok(l) => (Some(l), String::new()),
            Err(e) => (None, e),
        };
        let mut s = Self {
            open: false,
            library,
            changed: true,
            project: 0,
            node: 0,
            body: String::new(),
            evidence: String::new(),
            loaded_hash: String::new(),
            dirty: false,
            message,
            new_name: String::new(),
            rva: String::new(),
            dependency_rvas: String::new(),
            new_project_name: String::new(),
            new_project_id: String::new(),
            binary: String::new(),
            complete: false,
        };
        if s.library.is_some() {
            s.load(0, 0);
        }
        s
    }
    fn load(&mut self, p: usize, id: u64) {
        let Some(lib) = &self.library else {
            return;
        };
        match lib.read(p, id) {
            Ok((body, evidence)) => {
                self.project = p;
                self.node = id;
                self.body = body;
                self.evidence = evidence;
                self.loaded_hash =
                    hash(&fs::read_to_string(lib.path(p, id).unwrap()).unwrap_or_default());
                self.complete = lib.effective(p, id) == Status::Complete;
                self.binary = lib.projects[p].binary.clone();
                self.dependency_rvas = lib.projects[p]
                    .dependencies
                    .iter()
                    .filter(|e| e.dependent == id)
                    .filter_map(|e| {
                        lib.projects[p]
                            .nodes
                            .iter()
                            .find(|n| n.id == e.prerequisite)
                            .and_then(|n| n.rva)
                    })
                    .map(|r| format!("0x{r:08X}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                self.dirty = false;
                self.message.clear();
            }
            Err(e) => self.message = e,
        }
    }
    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        let mut open = self.open;
        let mut nav = None;
        egui::Window::new("Research records").open(&mut open).default_size([950.0,700.0]).vscroll(true).show(ctx,|ui| {
            if !self.message.is_empty(){ui.colored_label(egui::Color32::from_rgb(230,170,80),&self.message);}
            let Some(lib)=&self.library else{return;};
            let projects=lib.projects.clone();let project=&projects[self.project];
            ui.horizontal_wrapped(|ui|{for (i,p) in projects.iter().enumerate(){if ui.add_enabled(!self.dirty,egui::Button::new(&p.name).selected(i==self.project)).clicked(){nav=Some((i,0));}}});
            ui.label(format!("Isolated project: {}",lib.dir(self.project).display()));
            let project_dir=lib.dir(self.project);
            egui::CollapsingHeader::new("Project tools and data folders").show(ui,|ui|{
                ui.horizontal_wrapped(|ui|{
                    for (label,folder) in [("Tools","tools"),("Profiles","tools/profiles"),("Shared analysis","tools/workspaces"),("Tool logs","tools/logs")] {
                        if ui.button(label).clicked(){if let Err(e)=open_folder(&project_dir.join(folder)){self.message=e;}}
                    }
                });
                ui.label("Each function owns research.md and data/ in its function folder. Only shared tool workspaces, logs and scratch stay under tools/.");
            });
            egui::CollapsingHeader::new("New project").show(ui,|ui|{
                ui.horizontal(|ui|{ui.label("ID");ui.text_edit_singleline(&mut self.new_project_id);ui.label("Name");ui.text_edit_singleline(&mut self.new_project_name);});
                if ui.add_enabled(!self.dirty,egui::Button::new("Create isolated project")).clicked(){match self.library.as_mut().unwrap().create(&self.new_project_id,&self.new_project_name,""){Ok(())=>{nav=Some((self.library.as_ref().unwrap().projects.len()-1,0));self.changed=true;},Err(e)=>self.message=e}}
            });
            egui::CollapsingHeader::new("Binary identity").show(ui,|ui|{
                ui.label("Build, SHA-256, image base. Use another project for a different binary.");
                let locked=project.nodes.iter().any(|n|n.kind==Kind::Function);
                ui.add_enabled(!locked,egui::TextEdit::singleline(&mut self.binary).desired_width(f32::INFINITY));
                if ui.add_enabled(!locked&&!self.dirty,egui::Button::new("Save binary identity")).clicked(){
                    let lib=self.library.as_mut().unwrap();lib.projects[self.project].binary=self.binary.clone();
                    let ids:Vec<_>=lib.projects[self.project].nodes.iter().map(|n|n.id).collect();
                    let result=(||->Result<()>{for id in ids {let(b,e)=lib.read(self.project,id)?;lib.write_report(self.project,id,&b,&e)?;let n=lib.projects[self.project].nodes.iter_mut().find(|n|n.id==id).unwrap();n.accepted_hash=None;if n.status==Status::Complete{n.status=Status::InProgress;}}lib.persist(self.project)})();
                    match result{Ok(())=>nav=Some((self.project,self.node)),Err(e)=>self.message=e}
                }
            });
            ui.separator();
            ui.columns(2,|columns| {
                egui::ScrollArea::vertical().id_salt("recordtree").max_height(240.0).show(&mut columns[0],|ui|{
                    fn tree(ui:&mut egui::Ui,lib:&Library,p:usize,id:u64,selected:u64,dirty:bool,nav:&mut Option<(usize,u64)>) {
                        let n=lib.projects[p].nodes.iter().find(|n|n.id==id).unwrap();
                        if ui.add_enabled(!dirty,egui::Button::new(format!("{}: {} [{:?}]",n.kind.label(),n.name,lib.effective(p,id))).selected(id==selected)).clicked(){*nav=Some((p,id));}
                        ui.indent(id,|ui|{for child in lib.projects[p].nodes.iter().filter(|n|n.parent==Some(id)){tree(ui,lib,p,child.id,selected,dirty,nav);}});
                    }
                    tree(ui,self.library.as_ref().unwrap(),self.project,0,self.node,self.dirty,&mut nav);
                });
                let n=project.nodes.iter().find(|n|n.id==self.node).unwrap();
                let ui=&mut columns[1];ui.strong(format!("{} — {}",n.kind.label(),n.name));
                ui.label(self.library.as_ref().unwrap().path(self.project,self.node).unwrap().display().to_string());
                if n.kind==Kind::Function && ui.button("Open this function's data folder").clicked(){
                    let path=self.library.as_ref().unwrap().path(self.project,self.node).unwrap().parent().unwrap().join("data");
                    if let Err(e)=open_folder(&path){self.message=e;}
                }

                if let Some(kind)=n.kind.child() {
                    ui.label(format!("Add {} here",kind.label()));ui.text_edit_singleline(&mut self.new_name);
                    if kind==Kind::Function {ui.horizontal(|ui|{ui.label("RVA (hex)");ui.text_edit_singleline(&mut self.rva);});}
                    if ui.add_enabled(!self.dirty,egui::Button::new(format!("Create {} file",kind.label()))).clicked(){match self.library.as_mut().unwrap().add(self.project,self.node,&self.new_name,&self.rva){Ok(id)=>{self.new_name.clear();self.rva.clear();self.changed=true;nav=Some((self.project,id));},Err(e)=>self.message=e}}
                }
                if n.kind==Kind::Function {
                    ui.separator();ui.label("Prerequisites (function RVAs, comma separated)");
                    ui.text_edit_singleline(&mut self.dependency_rvas);
                    if ui.add_enabled(!self.dirty,egui::Button::new("Save prerequisite tags")).clicked(){
                        match self.library.as_mut().unwrap().set_dependencies(self.project,self.node,&self.dependency_rvas){Ok(())=>{self.changed=true;nav=Some((self.project,self.node));},Err(e)=>self.message=e}
                    }
                    let inbound:Vec<_>=project.dependencies.iter().filter(|e|e.prerequisite==self.node).filter_map(|e|project.nodes.iter().find(|n|n.id==e.dependent)).map(|n|format!("{} (0x{:08X})",n.name,n.rva.unwrap())).collect();
                    ui.label(format!("Required by: {}",if inbound.is_empty(){"none".into()}else{inbound.join(", ")}));
                    ui.label("dep.toml lists prerequisites; prereq.toml lists dependents. Managed together by the app.");
                }
                ui.label("Save or discard edits before changing records. Parent reports link their direct source reports; evidence remains in the original function files.");
            });
            ui.separator();ui.label("Research and understanding — Markdown");
            if ui.add(egui::TextEdit::multiline(&mut self.body).desired_width(f32::INFINITY).desired_rows(12).code_editor()).changed(){self.dirty=true;}
            ui.label("Evidence — cite source/build, VA/RVA, artifact or capture; label observed / specification-defined / inferred");
            if ui.add(egui::TextEdit::multiline(&mut self.evidence).desired_width(f32::INFINITY).desired_rows(4).code_editor()).changed(){self.dirty=true;}
            let kind=project.nodes.iter().find(|n|n.id==self.node).unwrap().kind;
            egui::CollapsingHeader::new("Completion criteria for this scope").show(ui,|ui|{
                ui.label(kind.completion_criteria());
                ui.label("Completed children make a parent ready for synthesis; the parent's own understanding must be reviewed separately.");
                ui.label("Keep this assignment's scope fixed. Record missing knowledge outside it as a separate task; tag it as a prerequisite only when needed to establish this scope's behavior. A connection alone does not expand the assignment.");
            });
            if ui.checkbox(&mut self.complete,"Mark complete (I reviewed the scope criteria, understanding and evidence)").changed(){self.dirty=true;}
            ui.horizontal(|ui|{
                if ui.button("Save canonical report").clicked(){match self.library.as_mut().unwrap().save_report(self.project,self.node,&self.body,&self.evidence,self.complete,&self.loaded_hash){Ok(())=>{self.changed=true;nav=Some((self.project,self.node));},Err(e)=>self.message=e}}
                if ui.button("Discard / reload").clicked(){nav=Some((self.project,self.node));}
                if self.dirty{ui.label("Unsaved edits");}
            });
        });
        if let Some((p, id)) = nav {
            self.load(p, id);
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
    pub app_status: Status,
    pub id: String,
    pub name: String,
    pub meters: Vec<Meter>,
}
impl ResearchUi {
    pub fn snapshot(&self) -> Vec<ProjectView> {
        let Some(lib) = &self.library else {
            return vec![];
        };
        lib.projects
            .iter()
            .enumerate()
            .map(|(p, project)| {
                let meters = project
                    .nodes
                    .iter()
                    .filter(|n| n.kind == Kind::Architecture)
                    .map(|a| {
                        let chunks: Vec<_> = project
                            .nodes
                            .iter()
                            .filter(|c| c.parent == Some(a.id))
                            .collect();
                        let states = chunks
                            .iter()
                            .map(|c| match lib.effective(p, c.id) {
                                Status::Complete => 0,
                                Status::InProgress => 1,
                                Status::NotStarted => 2,
                            })
                            .collect();
                        let functions: Vec<_> = project
                            .nodes
                            .iter()
                            .filter(|f| {
                                f.kind == Kind::Function
                                    && chunks.iter().any(|c| f.parent == Some(c.id))
                            })
                            .collect();
                        Meter {
                            id: a.id,
                            name: a.name.clone(),
                            functions: functions.len(),
                            completed_functions: functions
                                .iter()
                                .filter(|f| lib.effective(p, f.id) == Status::Complete)
                                .count(),
                            states,
                        }
                    })
                    .collect();
                ProjectView {
                    app_status: lib.effective(p, 0),
                    id: project.id.clone(),
                    name: project.name.clone(),
                    meters,
                }
            })
            .collect()
    }
    pub fn open_record(&mut self, project_id: &str, id: u64) {
        self.open = true;
        if self.dirty {
            self.message =
                "Save or discard the current edits before opening another report.".into();
            return;
        }
        if let Some(p) = self
            .library
            .as_ref()
            .and_then(|l| l.projects.iter().position(|p| p.id == project_id))
        {
            self.load(p, id);
        }
    }
}

impl ResearchUi {
    pub fn has_unsaved(&self) -> bool {
        self.dirty
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
        let project = &self.projects[p];
        for node in project.nodes.iter().filter(|n| n.kind == Kind::Function) {
            let tag = |id| {
                let n = project.nodes.iter().find(|n| n.id == id).unwrap();
                FunctionTag {
                    node_id: id,
                    rva: format!("0x{:08X}", n.rva.unwrap()),
                    report: format!("../{:08X}/research.md", n.rva.unwrap()),
                }
            };
            let deps = DependencyFile {
                schema: 1,
                project: project.id.clone(),
                binary: project.binary.clone(),
                function_rva: format!("0x{:08X}", node.rva.unwrap()),
                requires: project
                    .dependencies
                    .iter()
                    .filter(|e| e.dependent == node.id)
                    .map(|e| tag(e.prerequisite))
                    .collect(),
            };
            let prereq = PrerequisiteFile {
                schema: 1,
                project: project.id.clone(),
                binary: project.binary.clone(),
                function_rva: format!("0x{:08X}", node.rva.unwrap()),
                required_by: project
                    .dependencies
                    .iter()
                    .filter(|e| e.prerequisite == node.id)
                    .map(|e| tag(e.dependent))
                    .collect(),
            };
            let folder = self.path(p, node.id)?.parent().unwrap().to_owned();
            fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
            for (name, body) in [
                ("dep.toml", toml::to_string_pretty(&deps)),
                ("prereq.toml", toml::to_string_pretty(&prereq)),
            ] {
                let content = format!(
                    "# Managed by Research Task Manager. Edit through the app or --set-prerequisites.\n{}",
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
        self.persist(p)
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
