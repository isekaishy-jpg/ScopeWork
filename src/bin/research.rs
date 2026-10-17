use clap::{Parser, Subcommand, ValueEnum};
use research_dashboard::{
    paths,
    research::{Batch, Kind, Library, Status, imports},
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
    /// Seconds to wait for another process's project lock before reporting `busy`
    /// (default: SCOPEWORK_LOCK_WAIT_SECS, else 120). 0 answers `busy` immediately.
    #[arg(long, global = true)]
    wait: Option<u64>,
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
    Import(ImportCommand),
    #[command(subcommand)]
    Project(ProjectCommand),
    #[command(subcommand)]
    Node(NodeCommand),
    #[command(subcommand)]
    Deps(DepsCommand),
    /// Inspect completed journal storage, or compact full history to change receipts.
    Journal {
        project: String,
        /// Apply compaction; without this flag only report usage and reclaimable bytes.
        #[arg(long)]
        compact: bool,
    },
    /// Effective progress and records ready for review (not automatic completion).
    Status {
        project: String,
        #[command(flatten)]
        filter: RecordFilter,
        /// Only records ready for review (incomplete, unflagged, nothing blocking).
        #[arg(long)]
        ready: bool,
        /// Counts by kind and status instead of per-record output.
        #[arg(long)]
        summary: bool,
    },
}
/// Narrows `status` and `node list` output; effective status is still computed for the whole catalog.
#[derive(clap::Args, Default)]
struct RecordFilter {
    /// Only direct children of this node.
    #[arg(long)]
    parent: Option<u64>,
    /// Only records of this kind.
    #[arg(long, value_enum)]
    kind: Option<Scope>,
    /// Only records with this effective status.
    #[arg(long, value_enum)]
    status: Option<StatusFilter>,
}
#[derive(Clone, Copy, ValueEnum)]
enum StatusFilter {
    NotStarted,
    InProgress,
    Complete,
}
impl RecordFilter {
    fn keeps(&self, n: &research_dashboard::research::Node, status: Status) -> bool {
        self.parent.is_none_or(|p| n.parent == Some(p))
            && self.kind.is_none_or(|k| n.kind == k.kind())
            && self.status.is_none_or(|s| {
                status
                    == match s {
                        StatusFilter::NotStarted => Status::NotStarted,
                        StatusFilter::InProgress => Status::InProgress,
                        StatusFilter::Complete => Status::Complete,
                    }
            })
    }
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
enum ImportCommand {
    /// Validate an additive request and estimate output counts.
    Plan {
        project: String,
        #[arg(long)]
        file: PathBuf,
    },
    /// Reserve identities and prepare/publish without blocking existing research.
    Start {
        project: String,
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        paused: bool,
    },
    /// Read durable job progress without waiting for the runner.
    Status { project: String, job: String },
    /// Request a pause after in-flight file writes settle.
    Pause { project: String, job: String },
    /// Continue an interrupted/paused job with its reserved identities.
    Resume { project: String, job: String },
    /// Cancel unpublished work, preserving unknown or changed files.
    Cancel { project: String, job: String },
}
fn job_result(job: imports::Job) -> Result<Value, String> {
    if matches!(
        job.state,
        imports::JobState::Failed | imports::JobState::Conflict
    ) {
        return Err(format!(
            "import_failed: {} {:?}: {}",
            job.id,
            job.state,
            job.error.as_deref().unwrap_or("Inspect import status")
        ));
    }
    serde_json::to_value(job).map_err(|e| e.to_string())
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
    /// Identity and revision only (what a `node batch` request needs), without the catalog.
    Revision {
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
    /// Preview explicit branch deletion from JSON; --expect applies the reviewed token.
    Delete {
        project: String,
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        expect: Option<String>,
    },
    /// Preview empty architecture/chunk pruning; authored scopes are retained by default.
    Prune {
        project: String,
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        expect: Option<String>,
    },
    /// Read multiple canonical reports and edit tokens from one project snapshot.
    ReadBatch {
        project: String,
        #[arg(required = true)]
        nodes: Vec<u64>,
    },
    /// Save independently token-checked report bodies in one recoverable transaction.
    SaveBatch {
        project: String,
        #[arg(long)]
        file: PathBuf,
    },
    /// Repair recognized old flat-layout links using the current canonical catalog.
    RepairLinks {
        project: String,
        #[arg(long)]
        expect_revision: u64,
    },
    /// Transactionally create, move, rename, classify membership and reorder nodes from JSON.
    Batch {
        project: String,
        #[arg(long)]
        file: PathBuf,
    },
    List {
        project: String,
        #[command(flatten)]
        filter: RecordFilter,
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
    Show { project: String, node: u64 },
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
        /// Attach generated evidence without marking research started (Not Started stays Not Started).
        #[arg(long)]
        evidence_only: bool,
    },
    /// Flag a record for review with a short reason (blocks completion until cleared).
    Flag {
        project: String,
        node: u64,
        #[arg(long)]
        reason: String,
    },
    /// Clear a record's review flag.
    Unflag { project: String, node: u64 },
    /// List flagged records.
    Flagged { project: String },
    /// Return In Progress records to Not Started from a JSON request (ids or all_in_progress).
    ResetStatus {
        project: String,
        #[arg(long)]
        file: PathBuf,
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
    serde_json::to_value(lib.record(p, id)?).map_err(|e| e.to_string())
}

fn expect(lib: &Library, p: usize, id: u64, token: &str) -> Result<(), String> {
    lib.check_edit_token(p, id, token, "structural")
}
fn run(args: Args) -> Result<Value, String> {
    if let Command::Criteria { scope } = &args.command {
        return Ok(
            json!({"scope":scope.kind(), "criteria":scope.kind().completion_criteria(),
            "rule":"Keep the assignment's scope fixed. Completed children enable parent synthesis; they never automatically complete it. Record missing outside knowledge as a separate task; tag a prerequisite only when necessary to establish this scope's behavior."}),
        );
    }
    let wait = args
        .wait
        .or_else(|| {
            std::env::var("SCOPEWORK_LOCK_WAIT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
        })
        .unwrap_or(120);
    research_dashboard::research::set_lock_wait(std::time::Duration::from_secs(wait));
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
    let root = home.join("projects");
    if let Command::Import(command) = &args.command {
        return match command {
            ImportCommand::Plan { project, file } => {
                let batch = serde_json::from_slice(&fs::read(file).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
                imports::plan(&root, project, &batch)
            }
            ImportCommand::Start {
                project,
                file,
                paused,
            } => {
                let batch = serde_json::from_slice(&fs::read(file).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
                let job = imports::reserve(&root, project, &batch)?;
                job_result(if *paused {
                    imports::request(&root, project, &job.id, false)?
                } else {
                    imports::run(&root, project, &job.id)?
                })
            }
            ImportCommand::Status { project, job } => {
                serde_json::to_value(imports::status(&root, project, job)?)
                    .map_err(|e| e.to_string())
            }
            ImportCommand::Pause { project, job } => {
                job_result(imports::request(&root, project, job, false)?)
            }
            ImportCommand::Resume { project, job } => {
                job_result(imports::resume_job(&root, project, job)?)
            }
            ImportCommand::Cancel { project, job } => {
                job_result(imports::request(&root, project, job, true)?)
            }
        };
    }
    if let Command::Journal { project, compact } = &args.command {
        return Library::inspect_journals(root, project, *compact);
    }
    if let Command::Node(NodeCommand::Batch { project, file }) = &args.command {
        let batch: Batch = serde_json::from_slice(&fs::read(file).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        if !batch.create.is_empty()
            && batch.update.is_empty()
            && batch.reorder.is_empty()
            && !batch.repair_legacy_links
        {
            let job = imports::reserve(&root, project, &batch)?;
            let job = imports::run(&root, project, &job.id)?;
            if job.state != imports::JobState::Committed {
                return job_result(job);
            }
            return Ok(
                json!({"project":project,"revision":job.revision,"created":(job.first_id..job.end_id).collect::<Vec<_>>(),"job":job.id}),
            );
        }
    }

    let selected = match &args.command {
        Command::Project(
            ProjectCommand::Show { project }
            | ProjectCommand::Revision { project }
            | ProjectCommand::SetBinary { project, .. }
            | ProjectCommand::SetScope { project, .. },
        ) => Some(project.as_str()),
        Command::Node(
            NodeCommand::List { project, .. }
            | NodeCommand::Delete { project, .. }
            | NodeCommand::Prune { project, .. }
            | NodeCommand::ReadBatch { project, .. }
            | NodeCommand::SaveBatch { project, .. }
            | NodeCommand::Batch { project, .. }
            | NodeCommand::RepairLinks { project, .. }
            | NodeCommand::Create { project, .. }
            | NodeCommand::Show { project, .. }
            | NodeCommand::Save { project, .. }
            | NodeCommand::ResetStatus { project, .. }
            | NodeCommand::Flag { project, .. }
            | NodeCommand::Unflag { project, .. }
            | NodeCommand::Flagged { project, .. }
            | NodeCommand::Complete { project, .. }
            | NodeCommand::Reopen { project, .. },
        ) => Some(project.as_str()),
        Command::Deps(DepsCommand::Get { project, .. } | DepsCommand::Set { project, .. })
        | Command::Journal { project, .. }
        | Command::Status { project, .. } => Some(project.as_str()),
        _ => None,
    };
    let read_only = matches!(
        &args.command,
        Command::Project(ProjectCommand::Show { .. } | ProjectCommand::Revision { .. })
            | Command::Node(
                NodeCommand::Show { .. }
                    | NodeCommand::List { .. }
                    | NodeCommand::ReadBatch { .. }
                    | NodeCommand::Flagged { .. }
                    | NodeCommand::Delete { expect: None, .. }
                    | NodeCommand::Prune { expect: None, .. }
            )
            | Command::Deps(DepsCommand::Get { .. })
            | Command::Status { .. }
            | Command::Journal { compact: false, .. }
    );
    let mut lib = if read_only {
        Library::read_project(home.join("projects"), selected.unwrap())?
    } else {
        Library::open_project(home.join("projects"), selected)?
    };
    match args.command {
        Command::Journal {
            project: id,
            compact,
        } => {
            let p = project(&lib, &id)?;
            lib.journal_history(p, compact)
        }
        Command::Import(_) => unreachable!("import commands handled before opening a library"),
        Command::Init => Ok(locations),
        Command::Project(command) => match command {
            ProjectCommand::List => {
                if !lib.busy_projects.is_empty() {
                    return Err(format!(
                        "Projects busy: {}. Retry or address a project directly.",
                        lib.busy_projects.join(", ")
                    ));
                }
                Ok(json!(
                lib.projects
                    .iter()
                    .map(|p| json!({"id":p.id,"name":p.name,"binary":p.binary,"research_scope":p.research_scope}))
                    .collect::<Vec<_>>()
            ))
            }
            ProjectCommand::Create {
                id,
                name,
                binary,
                scope,
            } => {
                if scope
                    .as_ref()
                    .is_some_and(|s| s.trim().chars().any(char::is_control))
                {
                    return Err("Scope must be a single-line topic label.".into());
                }
                lib.create(&id, &name, &binary)?;
                if let Some(scope) = scope {
                    let p = project(&lib, &id)?;
                    lib.set_scope(p, &scope, 0)?;
                }
                Ok(json!({"created":id,"app_node":0}))
            }
            ProjectCommand::Revision { project: id } => {
                let p = project(&lib, &id)?;
                let pr = &lib.projects[p];
                Ok(
                    json!({"project":pr.id,"binary":pr.binary,"revision":pr.revision,"next_id":pr.next_id,"structural_epoch":pr.structural_epoch,"layout_version":pr.layout_version,"active_import":pr.active_import}),
                )
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
            ProjectCommand::SetScope {
                project: id,
                scope,
                expect_revision,
            } => {
                let p = project(&lib, &id)?;
                lib.set_scope(p, &scope, expect_revision)?;
                Ok(
                    json!({"project":id,"research_scope":lib.projects[p].research_scope,"revision":lib.projects[p].revision}),
                )
            }
        },
        Command::Node(command) => match command {
            NodeCommand::Delete {
                project: id,
                file,
                expect,
            } => {
                let p = project(&lib, &id)?;
                let request = serde_json::from_slice(&fs::read(file).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
                lib.remove_nodes(p, &request, false, expect.as_deref())
            }
            NodeCommand::Prune {
                project: id,
                file,
                expect,
            } => {
                let p = project(&lib, &id)?;
                let request = serde_json::from_slice(&fs::read(file).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
                lib.remove_nodes(p, &request, true, expect.as_deref())
            }
            NodeCommand::ReadBatch { project: id, nodes } => {
                lib.read_reports(project(&lib, &id)?, &nodes)
            }
            NodeCommand::SaveBatch { project: id, file } => {
                let p = project(&lib, &id)?;
                let batch = serde_json::from_slice(&fs::read(file).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
                let saved = lib.save_reports(p, &batch)?;
                Ok(json!({"project":id,"revision":lib.projects[p].revision,"saved":saved}))
            }
            NodeCommand::RepairLinks {
                project: id,
                expect_revision,
            } => {
                let p = project(&lib, &id)?;
                let batch = serde_json::from_value(json!({"schema":1,"binary":lib.projects[p].binary,"expect_revision":expect_revision,"repair_legacy_links":true})).map_err(|e|e.to_string())?;
                lib.batch(p, &batch)?;
                Ok(json!({"project":id,"revision":lib.projects[p].revision}))
            }
            NodeCommand::Batch { project: id, file } => {
                let p = project(&lib, &id)?;
                let batch = serde_json::from_slice(&fs::read(file).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
                let created = lib.batch(p, &batch)?;
                Ok(json!({"project": id, "revision":lib.projects[p].revision, "created":created}))
            }
            NodeCommand::List {
                project: id,
                filter,
            } => {
                let p = project(&lib, &id)?;
                let states = lib.effective_all(p);
                let paths = lib.report_paths(p);
                let required = lib.required_nodes(p);
                Ok(json!(lib.projects[p].nodes.iter().filter(|n| filter.keeps(n, states[&n.id])).map(|n| json!({"node":n,"effective_status":states[&n.id],"required_research":required.contains(&n.id),"report":paths[&n.id]})).collect::<Vec<_>>()))
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
                evidence_only,
            } => {
                let p = project(&lib, &id)?;
                let body = fs::read_to_string(body_file).map_err(|e| e.to_string())?;
                let evidence = fs::read_to_string(evidence_file).map_err(|e| e.to_string())?;
                if evidence_only {
                    lib.save_evidence(p, node, &body, &evidence, &token)?;
                } else {
                    lib.save_report(p, node, &body, &evidence, false, &token)?;
                }
                node_view(&lib, p, node)
            }
            NodeCommand::Flag {
                project: id,
                node,
                reason,
            } => {
                let p = project(&lib, &id)?;
                lib.set_review_flag(p, node, Some(&reason))?;
                node_view(&lib, p, node)
            }
            NodeCommand::Unflag { project: id, node } => {
                let p = project(&lib, &id)?;
                lib.set_review_flag(p, node, None)?;
                node_view(&lib, p, node)
            }
            NodeCommand::Flagged { project: id } => {
                let p = project(&lib, &id)?;
                let flags = lib.review_flags(p)?;
                let rows: Vec<_> = flags.iter().map(|(n, f)| {
                    let node = lib.projects[p].nodes.iter().find(|x| x.id == *n);
                    json!({"id":n,"name":node.map(|x| x.name.as_str()),"kind":node.map(|x| x.kind),"reason":f.reason,"flagged_at_revision":f.flagged_at_revision})
                }).collect();
                Ok(json!({"project":id,"flagged":rows}))
            }
            NodeCommand::ResetStatus { project: id, file } => {
                let p = project(&lib, &id)?;
                let request = serde_json::from_slice(&fs::read(file).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
                let reset = lib.reset_status(p, &request)?;
                Ok(json!({"project":id,"revision":lib.projects[p].revision,"reset":reset}))
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
        Command::Status {
            project: id,
            filter,
            ready,
            summary,
        } => {
            let p = project(&lib, &id)?;
            let states = lib.effective_all(p);
            let required = lib.required_nodes(p);
            let mut kids: std::collections::HashMap<u64, Vec<u64>> = Default::default();
            let mut deps: std::collections::HashMap<u64, Vec<u64>> = Default::default();
            for n in &lib.projects[p].nodes {
                if !n.context_only
                    && let Some(parent) = n.parent
                {
                    kids.entry(parent).or_default().push(n.id);
                }
            }
            for e in &lib.projects[p].dependencies {
                deps.entry(e.dependent).or_default().push(e.prerequisite);
            }
            let flags = lib.review_flags(p)?;
            let mut counts: std::collections::BTreeMap<
                String,
                std::collections::BTreeMap<String, usize>,
            > = Default::default();
            let mut ready_count = 0usize;
            let records:Vec<_>=lib.projects[p].nodes.iter().filter(|n| filter.keeps(n, states[&n.id])).filter_map(|n|{
                let children = kids.get(&n.id).cloned().unwrap_or_default();
                let blocked_children:Vec<_>=children.iter().copied().filter(|id|states[id]!=Status::Complete).collect();
                let blocked_prerequisites:Vec<_>=deps.get(&n.id).into_iter().flatten().copied().filter(|id|states[id]!=Status::Complete).collect();
                let ready_for_review = states[&n.id]!=Status::Complete&&!flags.contains_key(&n.id)&&blocked_children.is_empty()&&blocked_prerequisites.is_empty()&&(n.kind==Kind::Function||!children.is_empty());
                if ready && !ready_for_review { return None; }
                ready_count += usize::from(ready_for_review);
                *counts.entry(format!("{:?}", n.kind)).or_default().entry(format!("{:?}", states[&n.id])).or_default() += 1;
                (!summary).then(|| json!({"id":n.id,"name":n.name,"kind":n.kind,"status":states[&n.id],"required_research":required.contains(&n.id),"blocked_children":blocked_children,"blocked_prerequisites":blocked_prerequisites,
                    "ready_for_review":ready_for_review,"review_flag":flags.get(&n.id)}))
            }).collect();
            let inventory = lib.projects[p]
                .nodes
                .iter()
                .filter(|n| n.kind == Kind::Function)
                .count();
            let required_functions = lib.projects[p]
                .nodes
                .iter()
                .filter(|n| n.kind == Kind::Function && required.contains(&n.id))
                .count();
            let mut out = json!({"project":id,"app_status":states[&0],"review_flags":flags.len(),"research_scope":lib.projects[p].research_scope,"inventory_functions":inventory,"required_functions":required_functions,"context_functions":inventory-required_functions});
            if summary {
                out["counts"] = json!(counts);
                out["ready_for_review"] = json!(ready_count);
            } else {
                out["records"] = json!(records);
            }
            Ok(out)
        }
        Command::Home | Command::Gui | Command::Criteria { .. } => unreachable!(),
    }
}
fn main() {
    match run(Args::parse()) {
        Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
        Err(error) => {
            let kind = error
                .split_once(':')
                .map(|(prefix, _)| prefix)
                .filter(|prefix| {
                    !prefix.is_empty()
                        && prefix.bytes().all(|c| c.is_ascii_lowercase() || c == b'_')
                })
                .unwrap_or(if error.contains("is busy") {
                    "busy"
                } else {
                    "operation_failed"
                });
            eprintln!("{}", json!({"error":error,"kind":kind}));
            std::process::exit(1);
        }
    }
}
