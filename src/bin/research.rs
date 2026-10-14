use clap::{Parser, Subcommand, ValueEnum};
use research_dashboard::{
    paths,
    research::{Kind, Library, Status},
};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

#[derive(Parser)]
#[command(
    version,
    about = "Scopework: manage bounded research projects. Successful commands emit JSON; errors use stderr and exit 1 (usage errors: 2)."
)]
struct Args {
    /// Absolute research home. Overrides RESEARCH_DASHBOARD_HOME and LocalAppData.
    #[arg(long, global = true)]
    home: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Print resolved home, project and complaints paths without creating anything.
    Home,
    /// Create an empty research home. Projects are created explicitly; no data is imported.
    Init,
    /// Open the desktop dashboard using this same research home.
    Gui,
    /// Print scope-specific completion rules.
    Criteria {
        #[arg(value_enum)]
        scope: Scope,
    },
    #[command(subcommand)]
    Project(ProjectCommand),
    #[command(subcommand)]
    Node(NodeCommand),
    #[command(subcommand)]
    Deps(DepsCommand),
    /// Effective progress and records ready for review (not automatic completion).
    Status { project: String },
}
#[derive(Clone, Copy, ValueEnum)]
enum Scope {
    App,
    Architecture,
    Chunk,
    Function,
}
impl Scope {
    fn kind(self) -> Kind {
        match self {
            Self::App => Kind::App,
            Self::Architecture => Kind::Architecture,
            Self::Chunk => Kind::Chunk,
            Self::Function => Kind::Function,
        }
    }
}
#[derive(Subcommand)]
enum ProjectCommand {
    List,
    Create {
        id: String,
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "")]
        binary: String,
        /// Short research topic, such as Rendering or Asset platform.
        #[arg(long)]
        scope: Option<String>,
    },
    Show {
        project: String,
    },
    /// Set build/hash/image base before creating any functions.
    SetBinary {
        project: String,
        #[arg(long)]
        binary: String,
    },
    /// Set the short research-topic label displayed above the meters.
    SetScope {
        project: String,
        #[arg(long)]
        scope: String,
        /// Revision from project show; rejects concurrent project edits.
        #[arg(long)]
        expect_revision: u64,
    },
}
#[derive(Subcommand)]
enum NodeCommand {
    List {
        project: String,
    },
    /// Kind is inferred from the parent: app 0 -> architecture -> chunk -> function.
    Create {
        project: String,
        #[arg(long)]
        parent: u64,
        #[arg(long)]
        name: String,
        #[arg(long)]
        rva: Option<String>,
    },
    /// Read the report, scope criteria, paths and current optimistic edit token.
    Show {
        project: String,
        node: u64,
    },
    /// Save UTF-8 Markdown inputs. Always leaves the report open for review.
    Save {
        project: String,
        node: u64,
        #[arg(long)]
        body_file: PathBuf,
        #[arg(long)]
        evidence_file: PathBuf,
        #[arg(long)]
        expect: String,
    },
    /// Attest reviewed understanding after reading the scope criteria and report.
    Complete {
        project: String,
        node: u64,
        #[arg(long)]
        expect: String,
        #[arg(long, required = true)]
        reviewed: bool,
    },
    /// Reopen a report and invalidate its consumers and ancestors.
    Reopen {
        project: String,
        node: u64,
        #[arg(long)]
        expect: String,
    },
}
#[derive(Subcommand)]
enum DepsCommand {
    Get {
        project: String,
        node: u64,
    },
    /// Replace prerequisites with existing hexadecimal function RVAs; no RVAs clears them.
    Set {
        project: String,
        node: u64,
        #[arg(long)]
        expect: String,
        rvas: Vec<String>,
    },
}

fn project(lib: &Library, id: &str) -> Result<usize, String> {
    lib.projects
        .iter()
        .position(|p| p.id == id)
        .ok_or_else(|| format!("Unknown project: {id}"))
}
fn node_view(lib: &Library, p: usize, id: u64) -> Result<Value, String> {
    let node = lib.projects[p]
        .nodes
        .iter()
        .find(|n| n.id == id)
        .ok_or("Unknown node")?;
    let (body, evidence) = lib.read(p, id)?;
    let path = lib.path(p, id)?;
    let children: Vec<_> = lib.projects[p]
        .nodes
        .iter()
        .filter(|n| n.parent == Some(id))
        .map(|n| n.id)
        .collect();
    let requires: Vec<_> = lib.projects[p]
        .dependencies
        .iter()
        .filter(|e| e.dependent == id)
        .map(|e| e.prerequisite)
        .collect();
    let required_by: Vec<_> = lib.projects[p]
        .dependencies
        .iter()
        .filter(|e| e.prerequisite == id)
        .map(|e| e.dependent)
        .collect();
    Ok(
        json!({"project": lib.projects[p].id, "node": node, "effective_status": lib.effective(p,id), "report":path,
        "data": if node.kind == Kind::Function {Some(path.parent().unwrap().join("data"))} else {None},
        "body":body, "evidence":evidence, "token":lib.report_token(p,id)?, "criteria":node.kind.completion_criteria(), "research_scope":lib.projects[p].research_scope,
        "children":children, "requires":requires, "required_by":required_by}),
    )
}
fn expect(lib: &Library, p: usize, id: u64, token: &str) -> Result<(), String> {
    if lib.report_token(p, id)? != token {
        return Err(
            "Research changed since it was read. Read node show again and review before retrying."
                .into(),
        );
    }
    Ok(())
}
fn run(args: Args) -> Result<Value, String> {
    if let Command::Criteria { scope } = &args.command {
        return Ok(
            json!({"scope":scope.kind(), "criteria":scope.kind().completion_criteria(),
            "rule":"Keep the assignment's scope fixed. Completed children enable parent synthesis; they never automatically complete it. Record missing outside knowledge as a separate task; tag a prerequisite only when necessary to establish this scope's behavior."}),
        );
    }
    let home = paths::home(args.home)?;
    let locations =
        json!({"home":home,"projects":home.join("projects"),"complaints":home.join("complaints")});
    if matches!(args.command, Command::Home) {
        return Ok(locations);
    }
    if matches!(args.command, Command::Gui) {
        let executable = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .with_file_name("research-dashboard.exe");
        std::process::Command::new(executable)
            .env("RESEARCH_DASHBOARD_HOME", &home)
            .spawn()
            .map_err(|e| e.to_string())?;
        return Ok(json!({"opened":true,"home":home}));
    }
    paths::initialize(&home)?;
    let mut lib = Library::open(home.join("projects"))?;
    match args.command {
        Command::Init => Ok(locations),
        Command::Project(command) => match command {
            ProjectCommand::List => Ok(json!(
                lib.projects
                    .iter()
                    .map(|p| json!({"id":p.id,"name":p.name,"binary":p.binary,"research_scope":p.research_scope}))
                    .collect::<Vec<_>>()
            )),
            ProjectCommand::Create { id, name, binary, scope } => {
                if scope.as_ref().is_some_and(|s| s.trim().chars().any(char::is_control)) {
                    return Err("Scope must be a single-line topic label.".into());
                }
                lib.create(&id, &name, &binary)?;
                if let Some(scope) = scope { let p=project(&lib,&id)?; lib.set_scope(p,&scope,0)?; }
                Ok(json!({"created":id,"app_node":0}))
            }
            ProjectCommand::Show { project: id } => {
                let p = project(&lib, &id)?;
                Ok(json!(&lib.projects[p]))
            }
            ProjectCommand::SetBinary {
                project: id,
                binary,
            } => {
                let p = project(&lib, &id)?;
                lib.set_binary(p, &binary)?;
                Ok(json!({"project":id,"binary":binary}))
            }
            ProjectCommand::SetScope { project:id, scope, expect_revision } => {
                let p=project(&lib,&id)?;
                lib.set_scope(p,&scope,expect_revision)?;
                Ok(json!({"project":id,"research_scope":lib.projects[p].research_scope,"revision":lib.projects[p].revision}))
            }
        },
        Command::Node(command) => match command {
            NodeCommand::List { project: id } => {
                let p = project(&lib, &id)?;
                Ok(json!(lib.projects[p].nodes.iter().map(|n| json!({"node":n,"effective_status":lib.effective(p,n.id),"report":lib.path(p,n.id).ok()})).collect::<Vec<_>>()))
            }
            NodeCommand::Create {
                project: id,
                parent,
                name,
                rva,
            } => {
                let p = project(&lib, &id)?;
                let parent_kind = lib.projects[p]
                    .nodes
                    .iter()
                    .find(|n| n.id == parent)
                    .ok_or("Unknown parent")?
                    .kind;
                if (parent_kind == Kind::Chunk) != rva.is_some() {
                    return Err("Supply --rva only when creating a function under a chunk.".into());
                }
                let node = lib.add(p, parent, &name, rva.as_deref().unwrap_or(""))?;
                node_view(&lib, p, node)
            }
            NodeCommand::Show { project: id, node } => {
                let p = project(&lib, &id)?;
                node_view(&lib, p, node)
            }
            NodeCommand::Save {
                project: id,
                node,
                body_file,
                evidence_file,
                expect: token,
            } => {
                let p = project(&lib, &id)?;
                let body = fs::read_to_string(body_file).map_err(|e| e.to_string())?;
                let evidence = fs::read_to_string(evidence_file).map_err(|e| e.to_string())?;
                lib.save_report(p, node, &body, &evidence, false, &token)?;
                node_view(&lib, p, node)
            }
            NodeCommand::Complete {
                project: id,
                node,
                expect: token,
                reviewed: _,
            } => {
                let p = project(&lib, &id)?;
                let (body, evidence) = lib.read(p, node)?;
                lib.save_report(p, node, &body, &evidence, true, &token)?;
                node_view(&lib, p, node)
            }
            NodeCommand::Reopen {
                project: id,
                node,
                expect: token,
            } => {
                let p = project(&lib, &id)?;
                let (body, evidence) = lib.read(p, node)?;
                lib.save_report(p, node, &body, &evidence, false, &token)?;
                node_view(&lib, p, node)
            }
        },
        Command::Deps(command) => match command {
            DepsCommand::Get { project: id, node } => {
                let p = project(&lib, &id)?;
                node_view(&lib, p, node)
            }
            DepsCommand::Set {
                project: id,
                node,
                expect: token,
                rvas,
            } => {
                let p = project(&lib, &id)?;
                expect(&lib, p, node, &token)?;
                lib.set_dependencies(p, node, &rvas.join(","))?;
                node_view(&lib, p, node)
            }
        },
        Command::Status { project: id } => {
            let p = project(&lib, &id)?;
            let records:Vec<_>=lib.projects[p].nodes.iter().map(|n|{
                let children:Vec<_>=lib.projects[p].nodes.iter().filter(|c|c.parent==Some(n.id)).collect();
                let blocked_children:Vec<_>=children.iter().filter(|c|lib.effective(p,c.id)!=Status::Complete).map(|c|c.id).collect();
                let blocked_prerequisites:Vec<_>=lib.projects[p].dependencies.iter().filter(|e|e.dependent==n.id&&lib.effective(p,e.prerequisite)!=Status::Complete).map(|e|e.prerequisite).collect();
                json!({"id":n.id,"name":n.name,"kind":n.kind,"status":lib.effective(p,n.id),"blocked_children":blocked_children,"blocked_prerequisites":blocked_prerequisites,
                    "ready_for_review":lib.effective(p,n.id)!=Status::Complete&&blocked_children.is_empty()&&blocked_prerequisites.is_empty()&&(n.kind==Kind::Function||!children.is_empty())})
            }).collect();
            Ok(json!({"project":id,"app_status":lib.effective(p,0),"research_scope":lib.projects[p].research_scope,"records":records}))
        }
        Command::Home | Command::Gui | Command::Criteria { .. } => unreachable!(),
    }
}
fn main() {
    match run(Args::parse()) {
        Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
        Err(error) => {
            eprintln!("{}", json!({"error":error}));
            std::process::exit(1);
        }
    }
}
