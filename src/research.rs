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
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub binary: String,
    pub next_id: u64,
    pub revision: u64,
    pub nodes: Vec<Node>,
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
        Kind::Architecture => format!("architectures/{:06}.md", n.id),
        Kind::Chunk => format!("chunks/{:06}.md", n.id),
        Kind::Function => format!("functions/{:08X}.md", n.rva.unwrap()),
    }
}

// One project-local destination for each kind of reversing material.
const PROJECT_FOLDERS: &[&str] = &[
    "functions",
    "chunks",
    "architectures",
    "evidence",
    "tools/profiles",
    "tools/scripts",
    "tools/instrumentation",
    "tools/bin",
    "data/analysis",
    "data/captures",
    "data/exports",
    "data/logs",
    "data/scratch",
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
            f.write_all(b"# Project reversing tools\n\nThis tools directory belongs exclusively to the project containing it.\n\n- profiles/: compiler, ABI, type-layout, analysis and decompiler profiles.\n- scripts/: analysis drivers, exporters and processing utilities.\n- instrumentation/: separately maintained observational hooks and capture utilities.\n- bin/: project-local tool executables or launchers, with version/provenance notes here.\n\nWrite generated material under the same project's data directory:\n\n- ../data/analysis/: Ghidra projects, analysis databases and other tool working data.\n- ../data/captures/: runtime captures and traces.\n- ../data/exports/: disassembly, decompilation, extracted tables and structured exports.\n- ../data/logs/: tool execution logs.\n- ../data/scratch/: temporary and reproducible intermediate files.\n\nKeep retained evidence in ../evidence/ and link it from canonical function reports. Keep canonical research in ../functions/, ../chunks/, ../architectures/ and ../app.md. Do not create additional research reports under tools or data. Source installations remain read-only. Give each run a named subfolder and record its exact input binary hash, tool/profile versions, command and output paths in its log. Never borrow another project's profile or evidence silently.\n").map_err(|e|e.to_string())?;
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
        Ok(lib)
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
                format!("../{}", relative(child))
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
        atomic(&self.path(p, id)?, md.as_bytes())
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
        n.revision += 1;
        self.invalidate(p, id);
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
                    for (label,folder) in [("Tools","tools"),("Profiles","tools/profiles"),("Analysis data","data/analysis"),("Captures","data/captures"),("Exports","data/exports"),("Evidence","evidence")] {
                        if ui.button(label).clicked(){if let Err(e)=open_folder(&project_dir.join(folder)){self.message=e;}}
                    }
                });
                ui.label("Tools and profiles stay in this project. Generated data goes to data/analysis, captures, exports, logs, or scratch. Canonical reports stay in their research folders.");
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
                if let Some(kind)=n.kind.child() {
                    ui.label(format!("Add {} here",kind.label()));ui.text_edit_singleline(&mut self.new_name);
                    if kind==Kind::Function {ui.horizontal(|ui|{ui.label("RVA (hex)");ui.text_edit_singleline(&mut self.rva);});}
                    if ui.add_enabled(!self.dirty,egui::Button::new(format!("Create {} file",kind.label()))).clicked(){match self.library.as_mut().unwrap().add(self.project,self.node,&self.new_name,&self.rva){Ok(id)=>{self.new_name.clear();self.rva.clear();self.changed=true;nav=Some((self.project,id));},Err(e)=>self.message=e}}
                }
                ui.label("Save or discard edits before changing records. Parent reports link their direct source reports; evidence remains in the original function files.");
            });
            ui.separator();ui.label("Research and understanding — Markdown");
            if ui.add(egui::TextEdit::multiline(&mut self.body).desired_width(f32::INFINITY).desired_rows(12).code_editor()).changed(){self.dirty=true;}
            ui.label("Evidence — cite source/build, VA/RVA, artifact or capture; label observed / specification-defined / inferred");
            if ui.add(egui::TextEdit::multiline(&mut self.evidence).desired_width(f32::INFINITY).desired_rows(4).code_editor()).changed(){self.dirty=true;}
            if ui.checkbox(&mut self.complete,"Mark this report complete (I have reviewed its understanding and evidence)").changed(){self.dirty=true;}
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
