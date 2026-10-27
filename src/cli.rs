//! The `research` command line: argument parsing and command execution, shared by the
//! executable, `research session` and the coordinator.
use crate::{
    paths,
    research::{Batch, Kind, Library, Status, imports, valid_id},
};
use clap::{Parser, Subcommand, ValueEnum};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

#[derive(Parser)]
#[command(
    version,
    about = "Scopework: manage bounded research projects. Successful commands emit JSON; errors use stderr and exit 1 (usage errors: 2)."
)]
pub(crate) struct Args {
    /// Absolute research home. Overrides RESEARCH_DASHBOARD_HOME and LocalAppData.
    #[arg(long, global = true)]
    home: Option<PathBuf>,
    /// Seconds to wait for another process's project lock before reporting `busy`
    /// (default: SCOPEWORK_LOCK_WAIT_SECS, else 120). 0 answers `busy` immediately.
    #[arg(long, global = true)]
    wait: Option<u64>,
    #[command(subcommand)]
    pub(crate) command: Command,
}
#[derive(Subcommand)]
pub(crate) enum Command {
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
    /// Record who is working on an architecture or chunk (advisory, optionally leased).
    #[command(subcommand)]
    Claim(ClaimCommand),
    /// Completed records reopened by structural changes, waiting for their owner's review.
    ReviewQueue { project: String },
    /// Read the project history (catalog receipts and tooling entries) or append a tooling entry.
    #[command(subcommand)]
    History(HistoryCommand),
    /// Group related projects into one dashboard tab (presentation only; no project changes).
    #[command(subcommand)]
    Group(GroupCommand),
    /// Track the recreation: a free-form tree of its organisation, the research functions placed
    /// where they are implemented, implementation-only items and their completion.
    #[command(subcommand)]
    Impl(ImplCommand),
    /// Who holds a project's lock and which processes are waiting for it (takes no lock).
    Locks { project: String },
    /// Block until a condition holds instead of polling from outside.
    Wait {
        project: String,
        /// Until no writer holds the project lock.
        #[arg(long)]
        idle: bool,
        /// Until the catalog revision is above this value.
        #[arg(long)]
        revision_above: Option<u64>,
        /// Until this node's effective status is Complete.
        #[arg(long)]
        complete: Option<u64>,
        /// Give up after this many seconds (exit 1, kind `timeout`).
        #[arg(long, default_value_t = 600)]
        timeout: u64,
    },
    /// Run commands from stdin, one JSON array of arguments per line, answering one JSON line each.
    /// Keeps one process (and its caches) for many commands.
    Session,
    /// Inspect completed journal storage, or compact full history to change receipts.
    Journal {
        project: String,
        /// Apply compaction; without this flag only report usage and reclaimable bytes.
        #[arg(long)]
        compact: bool,
        /// Also replace sidecar texts of 1 MB or more in completed receipts (such as annotation
        /// ledger copies) by their changed JSON values and hashes. Irreversible for those exact
        /// historical bytes; pending journals are never touched.
        #[arg(long)]
        compact_sidecars: bool,
    },
    /// Effective progress and records ready for review (not automatic completion).
    Status {
        project: String,
        #[command(flatten)]
        filter: RecordFilter,
        /// Only records ready for review: written research (not Not Started), incomplete, unflagged, nothing blocking.
        #[arg(long)]
        ready: bool,
        /// Only parents ready for synthesis: every required child complete, own report not started.
        #[arg(long, conflicts_with = "ready")]
        ready_for_synthesis: bool,
        /// Counts by kind and status instead of per-record output.
        #[arg(long)]
        summary: bool,
    },
    /// Running and waiting operations on a project (from the coordinator, or the lock queue).
    Queue { project: String },
    /// Inspect or stop this research home's coordinator.
    #[command(subcommand)]
    Coordinator(CoordinatorCommand),
    /// Tool work under leases on a project's named tool resources.
    #[command(subcommand)]
    Job(JobCommand),
    /// A project's tool declarations and usage.
    #[command(subcommand)]
    Tools(ToolsCommand),
    /// Run the coordinator for this research home (started automatically by clients).
    #[command(hide = true)]
    Serve,
}
#[derive(Subcommand)]
pub(crate) enum JobCommand {
    /// Run COMMAND while holding a lease. Requests with the same key that wait together share
    /// one run: COMMAND then receives every payload and reports a result for each.
    Run {
        project: String,
        /// A kind declared in tools/coordinator.toml, or a new name used with --resource.
        #[arg(long)]
        kind: Option<String>,
        /// A resource to lease (repeatable); overrides the kind's resources.
        #[arg(long = "resource")]
        resources: Vec<String>,
        /// Requests with this key and the same resources join one lease.
        #[arg(long)]
        key: Option<String>,
        /// normal, bulk or job.
        #[arg(long)]
        class: Option<String>,
        /// This request's payload: a JSON file of at most 1 MB.
        #[arg(long)]
        payload_file: Option<PathBuf>,
        /// The command and its arguments, after --.
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
    /// Held and waiting leases (from the coordinator, or the resource lock files).
    Status { project: String },
}
#[derive(Subcommand)]
pub(crate) enum ToolsCommand {
    /// Advisory: what the project's tool sources appear to use, against tools/coordinator.toml.
    Audit { project: String },
}
#[derive(Subcommand)]
pub(crate) enum CoordinatorCommand {
    /// Version, uptime, open projects, queue lengths and read counts.
    Status,
    /// Finish queued work, then exit; clients use direct mode until the next one starts.
    Stop,
}
// Narrows `status` and `node list` output; effective status is still computed for the whole catalog.
#[derive(clap::Args, Default)]
pub(crate) struct RecordFilter {
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
pub(crate) enum StatusFilter {
    NotStarted,
    InProgress,
    Complete,
}
impl RecordFilter {
    fn keeps(&self, n: &crate::research::Node, status: Status) -> bool {
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
pub(crate) enum Scope {
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
pub(crate) enum ImportCommand {
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
pub(crate) enum ImplCommand {
    /// The tree with counters per node and totals; --node adds one node's placements and items.
    Show {
        project: String,
        #[arg(long)]
        node: Option<u64>,
        /// List the function records not placed anywhere yet.
        #[arg(long)]
        unplaced: bool,
    },
    /// Where a research record is implemented.
    Where {
        project: String,
        record: u64,
    },
    /// Set the implementation repository's local path and optional remote.
    Repo {
        project: String,
        #[arg(long)]
        path: String,
        #[arg(long, default_value = "")]
        remote: String,
    },
    /// Add a node (crate, module, file or any grouping) under --parent, or at the top level.
    Add {
        project: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        parent: Option<u64>,
    },
    Rename {
        project: String,
        node: u64,
        #[arg(long)]
        name: String,
    },
    /// Move a node under --parent, or to the top level without it.
    Move {
        project: String,
        node: u64,
        #[arg(long)]
        parent: Option<u64>,
    },
    /// Remove a node that holds no child nodes, placements or items.
    Remove {
        project: String,
        node: u64,
    },
    /// Place research function records on a node (each record on at most one node).
    Place {
        project: String,
        node: u64,
        #[arg(required = true)]
        records: Vec<u64>,
    },
    Unplace {
        project: String,
        #[arg(required = true)]
        records: Vec<u64>,
    },
    /// Add an implementation-only item (needed to make the recreation match) to a node.
    ItemAdd {
        project: String,
        node: u64,
        #[arg(long)]
        name: String,
    },
    ItemRemove {
        project: String,
        item: u64,
    },
    /// Mark placed records and implementation-only items complete.
    Complete {
        project: String,
        #[arg(long = "record")]
        records: Vec<u64>,
        #[arg(long = "item")]
        items: Vec<u64>,
    },
    /// Clear the completion of placed records and implementation-only items.
    Reopen {
        project: String,
        #[arg(long = "record")]
        records: Vec<u64>,
        #[arg(long = "item")]
        items: Vec<u64>,
    },
}
impl ImplCommand {
    pub(crate) fn project(&self) -> &str {
        match self {
            Self::Show { project, .. }
            | Self::Where { project, .. }
            | Self::Repo { project, .. }
            | Self::Add { project, .. }
            | Self::Rename { project, .. }
            | Self::Move { project, .. }
            | Self::Remove { project, .. }
            | Self::Place { project, .. }
            | Self::Unplace { project, .. }
            | Self::ItemAdd { project, .. }
            | Self::ItemRemove { project, .. }
            | Self::Complete { project, .. }
            | Self::Reopen { project, .. } => project,
        }
    }
}
#[derive(Subcommand)]
pub(crate) enum GroupCommand {
    /// Groups with their projects, then the ungrouped projects.
    List,
    /// Put a project in a group (created if new), moving it out of any other group.
    Set { project: String, group: String },
    /// Make a project ungrouped.
    Clear { project: String },
}
#[derive(Subcommand)]
pub(crate) enum ProjectCommand {
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
    /// Set build/hash/image base. Once functions exist, only an amendment that keeps every
    /// SHA-256 and 0x value is accepted, and it keeps all completion.
    SetBinary {
        project: String,
        #[arg(long)]
        binary: String,
        /// Revision from project show; rejects concurrent project edits (required for amendments).
        #[arg(long)]
        expect_revision: Option<u64>,
    },
    /// Report files that hold nothing in existing record folders: generated `dep.toml` and
    /// `prereq.toml` views, markers of committed imports and empty `data/` folders. --apply
    /// removes them; nothing else is touched.
    Tidy {
        project: String,
        #[arg(long)]
        apply: bool,
    },
    /// Set whether completing a function needs a proof (its recreation and tests), and the
    /// repository proof paths are checked against (an empty value clears it).
    SetProof {
        project: String,
        #[arg(long, action = clap::ArgAction::Set)]
        required: bool,
        #[arg(long)]
        repository: Option<String>,
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
pub(crate) enum NodeCommand {
    /// Preview explicit branch deletion from JSON; --expect applies the reviewed token.
    Delete {
        project: String,
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        expect: Option<String>,
        /// The human's instruction, required to remove quarantined records (overrides the
        /// request's `directed_by`).
        #[arg(long)]
        directed_by: Option<String>,
    },
    /// Preview empty architecture/chunk pruning; authored scopes are retained by default.
    Prune {
        project: String,
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        expect: Option<String>,
        /// The human's instruction; without it, quarantined records are never pruned.
        #[arg(long)]
        directed_by: Option<String>,
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
    /// Map report or data paths (old or current) to their record and current location.
    Resolve {
        project: String,
        #[arg(required = true)]
        paths: Vec<String>,
    },
    /// Transactionally create, move, rename, classify membership and reorder nodes from JSON.
    Batch {
        project: String,
        #[arg(long)]
        file: PathBuf,
        /// The human's instruction, required when the batch touches quarantined records
        /// (overrides the request's `directed_by`).
        #[arg(long)]
        directed_by: Option<String>,
    },
    /// Records with their effective status, optionally narrowed by parent, kind or status.
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
        /// The human's instruction, required to create a record in a quarantined chunk.
        #[arg(long)]
        directed_by: Option<String>,
    },
    /// Read the report, scope criteria, paths and current optimistic edit token.
    Show { project: String, node: u64 },
    /// Save UTF-8 Markdown inputs. Never completes; a changed report is left open for review, an unchanged one as it is.
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
    /// Quarantine a chunk or function: it counts as complete, never appears as open work and
    /// is sealed against structural changes (a chunk covers its functions).
    Quarantine {
        project: String,
        node: u64,
        /// Why: the human asked for it, or Claude issued a cyber (safety) stop.
        #[arg(long, value_parser = ["human", "cyber-stop"])]
        origin: String,
        #[arg(long)]
        reason: String,
        /// Who records it (an agent name or the human).
        #[arg(long, default_value = "")]
        by: String,
    },
    /// Lift a quarantine; the record returns to its own status.
    Unquarantine {
        project: String,
        node: u64,
        #[arg(long)]
        reason: String,
        #[arg(long, default_value = "")]
        by: String,
    },
    /// List quarantined records.
    Quarantined { project: String },
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
    /// Record the recreation and tests that prove a function, or clear them (--clear).
    Proof {
        project: String,
        node: u64,
        /// The recreation: a path in the repository, optionally followed by ::item.
        #[arg(long, required_unless_present = "clear")]
        recreation: Option<String>,
        /// A test that checks it; repeat for several.
        #[arg(long = "test", required_unless_present = "clear")]
        tests: Vec<String>,
        #[arg(long, conflicts_with_all = ["recreation", "tests"])]
        clear: bool,
    },
    /// Record many proofs in one transaction: {"proofs":[{"id":N,"recreation":"...","tests":["..."]}]}.
    ProofBatch {
        project: String,
        #[arg(long)]
        file: PathBuf,
    },
    /// List functions without a proof (--complete: only completed ones).
    Unproven {
        project: String,
        #[arg(long)]
        complete: bool,
    },
}
#[derive(Subcommand)]
pub(crate) enum HistoryCommand {
    /// Entries newest first: catalog transaction receipts and project tooling entries.
    Show {
        project: String,
        /// Only entries touching this node.
        #[arg(long)]
        node: Option<u64>,
        /// Lowest catalog revision to include.
        #[arg(long)]
        since: Option<u64>,
        /// Highest catalog revision to include.
        #[arg(long)]
        until: Option<u64>,
        /// Maximum entries (default 50).
        #[arg(long, default_value_t = 50)]
        limit: usize,
        /// Only catalog receipts or only tooling entries.
        #[arg(long, value_enum)]
        only: Option<HistorySource>,
        /// Only tooling entries of this kind (or `catalog`).
        #[arg(long)]
        kind: Option<String>,
        /// Include changed records, not only a summary.
        #[arg(long)]
        detail: bool,
        /// Only entries about the function with this RVA (hex, `0x` optional).
        #[arg(long)]
        rva: Option<String>,
        /// Only entries on or after this UTC day (YYYY-MM-DD).
        #[arg(long)]
        from: Option<String>,
        /// Only entries on or before this UTC day (YYYY-MM-DD).
        #[arg(long)]
        to: Option<String>,
    },
    /// The tooling kinds the app knows, with what an entry of each must name.
    Kinds,
    /// Append a tooling entry (for example ledger-change, ghidra-apply, generated-artifact).
    Append {
        project: String,
        #[arg(long)]
        kind: String,
        #[arg(long)]
        summary: String,
        /// Nodes the entry concerns (repeatable).
        #[arg(long = "node")]
        nodes: Vec<u64>,
        /// Paths or identifiers the entry refers to (repeatable).
        #[arg(long = "ref")]
        refs: Vec<String>,
        /// JSON file with the diff (what changed, not a whole-file copy).
        #[arg(long)]
        diff_file: Option<PathBuf>,
    },
}
#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum HistorySource {
    Catalog,
    Tooling,
}
#[derive(Subcommand)]
pub(crate) enum ClaimCommand {
    /// Claim a branch; another owner's unexpired claim is refused unless --force.
    Set {
        project: String,
        node: u64,
        #[arg(long)]
        owner: String,
        #[arg(long, default_value = "")]
        note: String,
        /// Expire the claim after this many hours.
        #[arg(long)]
        lease_hours: Option<u64>,
        #[arg(long)]
        force: bool,
    },
    /// Release a claim (another owner's only with --force).
    Release {
        project: String,
        node: u64,
        #[arg(long)]
        owner: String,
        #[arg(long)]
        force: bool,
    },
    /// Claims currently in force.
    List { project: String },
}
#[derive(Subcommand)]
pub(crate) enum DepsCommand {
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
        /// The human's instruction, required when the node or a prerequisite is quarantined.
        #[arg(long)]
        directed_by: Option<String>,
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

/// `node delete` and `node prune`: the request file, with `--directed-by` taking precedence.
fn remove(
    lib: &mut Library,
    id: &str,
    file: &PathBuf,
    expect: Option<&str>,
    directed_by: Option<String>,
    prune: bool,
) -> Result<Value, String> {
    let p = project(lib, id)?;
    let mut request: crate::research::Removal =
        serde_json::from_slice(&fs::read(file).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if directed_by.is_some() {
        request.directed_by = directed_by;
    }
    lib.remove_nodes(p, &request, prune, expect)
}
fn expect(lib: &Library, p: usize, id: u64, token: &str) -> Result<(), String> {
    lib.check_edit_token(p, id, token, "structural")
}
/// The project a command names, if any: every such command answers "Unknown project" for a
/// project without a catalog before it runs (creation aside).
fn project_of(command: &Command) -> Option<&str> {
    match command {
        Command::Project(
            ProjectCommand::Show { project }
            | ProjectCommand::Revision { project }
            | ProjectCommand::SetBinary { project, .. }
            | ProjectCommand::SetScope { project, .. }
            | ProjectCommand::SetProof { project, .. }
            | ProjectCommand::Tidy { project, .. },
        ) => Some(project.as_str()),
        Command::Node(
            NodeCommand::List { project, .. }
            | NodeCommand::Delete { project, .. }
            | NodeCommand::Prune { project, .. }
            | NodeCommand::ReadBatch { project, .. }
            | NodeCommand::SaveBatch { project, .. }
            | NodeCommand::Batch { project, .. }
            | NodeCommand::Resolve { project, .. }
            | NodeCommand::RepairLinks { project, .. }
            | NodeCommand::Create { project, .. }
            | NodeCommand::Show { project, .. }
            | NodeCommand::Save { project, .. }
            | NodeCommand::ResetStatus { project, .. }
            | NodeCommand::Flag { project, .. }
            | NodeCommand::Unflag { project, .. }
            | NodeCommand::Flagged { project, .. }
            | NodeCommand::Quarantine { project, .. }
            | NodeCommand::Unquarantine { project, .. }
            | NodeCommand::Quarantined { project, .. }
            | NodeCommand::Complete { project, .. }
            | NodeCommand::Reopen { project, .. }
            | NodeCommand::Proof { project, .. }
            | NodeCommand::ProofBatch { project, .. }
            | NodeCommand::Unproven { project, .. },
        ) => Some(project.as_str()),
        Command::Claim(
            ClaimCommand::Set { project, .. }
            | ClaimCommand::Release { project, .. }
            | ClaimCommand::List { project },
        )
        | Command::ReviewQueue { project }
        | Command::History(
            HistoryCommand::Show { project, .. } | HistoryCommand::Append { project, .. },
        ) => Some(project.as_str()),
        Command::Deps(DepsCommand::Get { project, .. } | DepsCommand::Set { project, .. })
        | Command::Journal { project, .. }
        | Command::Status { project, .. } => Some(project.as_str()),
        Command::Impl(command) => Some(command.project()),
        Command::Wait { project, .. }
        | Command::Locks { project }
        | Command::Queue { project }
        | Command::Job(JobCommand::Run { project, .. } | JobCommand::Status { project })
        | Command::Tools(ToolsCommand::Audit { project })
        | Command::Import(
            ImportCommand::Plan { project, .. }
            | ImportCommand::Start { project, .. }
            | ImportCommand::Status { project, .. }
            | ImportCommand::Pause { project, .. }
            | ImportCommand::Resume { project, .. }
            | ImportCommand::Cancel { project, .. },
        ) => Some(project.as_str()),
        _ => None,
    }
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
    crate::research::set_lock_wait(std::time::Duration::from_secs(wait));
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
    // A command on a project that does not exist answers before it runs: a writable open would
    // take the missing project's lock (that is how creation works) and leave its file, and the
    // lock and queue views would describe a project that is not there.
    if let Some(id) = project_of(&args.command)
        && valid_id(id)
        && !root.join(id).join("project.json").is_file()
    {
        return Err(format!("Unknown project: {id}"));
    }
    if let Command::Group(command) = &args.command {
        return groups(&home, command);
    }
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
    if let Command::Locks { project } = &args.command {
        return crate::research::lock_status(&root, project);
    }
    match &args.command {
        Command::Serve => return crate::coordinator::serve(&home),
        Command::Coordinator(CoordinatorCommand::Status) => {
            return crate::coordinator::client::status(&home);
        }
        Command::Coordinator(CoordinatorCommand::Stop) => {
            return crate::coordinator::client::stop(&home);
        }
        // Reached only in direct mode: the lock queue stands in for the coordinator's.
        Command::Queue { project } => return crate::coordinator::lock_queue(&root, project),
        Command::Job(JobCommand::Run {
            project,
            kind,
            resources,
            key,
            class,
            payload_file,
            command,
        }) => {
            if !valid_id(project) {
                return Err("Invalid project ID".into());
            }
            let manifest = crate::coordinator::manifest::load(&root.join(project))?;
            let spec =
                manifest.resolve(kind.as_deref(), resources, key.as_deref(), class.as_deref())?;
            let payload = match payload_file {
                None => None,
                Some(path) => {
                    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
                    if bytes.len() > 1 << 20 {
                        return Err("lease_refused: a payload may not exceed 1 MB".into());
                    }
                    Some(
                        serde_json::from_slice::<Value>(&bytes)
                            .map_err(|e| format!("{}: {e}", path.display()))?,
                    )
                }
            };
            // The answer carries the command's `exit`; `main` exits with it, and a session
            // answers it like any other command.
            return crate::coordinator::job::run(&home, project, &spec, payload, command);
        }
        Command::Job(JobCommand::Status { project }) => {
            return crate::coordinator::job::status(&home, project);
        }
        Command::Tools(ToolsCommand::Audit { project }) => {
            if !valid_id(project) {
                return Err("Invalid project ID".into());
            }
            return crate::coordinator::audit::run(&root.join(project));
        }
        _ => {}
    }
    if let Command::Wait {
        project: id,
        idle,
        revision_above,
        complete,
        timeout,
    } = &args.command
    {
        if !idle && revision_above.is_none() && complete.is_none() {
            return Err(
                "Give at least one condition: --idle, --revision-above or --complete.".into(),
            );
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(*timeout);
        let started = std::time::Instant::now();
        // The catalog is parsed only when it changes; completion is evaluated only after a
        // change, and at most every two seconds otherwise (reports can change on their own).
        #[derive(serde::Deserialize)]
        struct Revision {
            revision: u64,
        }
        let catalog = root.join(id).join("project.json");
        let mut seen: Option<(u64, Option<std::time::SystemTime>)> = None;
        let mut revision = 0;
        let mut complete_ok = complete.is_none();
        let mut evaluated: Option<std::time::Instant> = None;
        // With a coordinator, writes also wait in its queue while the lock is free.
        let mut coordinator: Option<crate::coordinator::client::Connection> = None;
        loop {
            // Only the writer matters here: no probe for readers, which would take the lock
            // exclusively for a moment four times a second.
            let status = crate::research::lock_status_probing(&root, id, false)?;
            let queue_idle = !idle || {
                if coordinator.is_none() {
                    coordinator = crate::coordinator::client::connect(&home, "cli", false)
                        .ok()
                        .flatten();
                }
                match coordinator
                    .as_mut()
                    .map(|c| c.request(&["queue".into(), id.clone()], None))
                {
                    Some(Ok(answer)) => answer.stdout.as_ref().is_some_and(|queue| {
                        queue["running"].as_array().is_some_and(Vec::is_empty)
                            && queue["waiting"].as_array().is_some_and(Vec::is_empty)
                    }),
                    // Gone: the lock alone decides, as without a coordinator.
                    Some(Err(_)) => {
                        coordinator = None;
                        true
                    }
                    None => true,
                }
            };
            let idle_ok = !idle || (status["writer_active"] == false && queue_idle);
            let stamp = fs::metadata(&catalog)
                .ok()
                .map(|m| (m.len(), m.modified().ok()));
            let changed = stamp != seen;
            if changed
                && let Some(parsed) = fs::read(&catalog)
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Revision>(&b).ok())
            {
                revision = parsed.revision;
                seen = stamp;
            }
            let revision_ok = revision_above.is_none_or(|r| revision > r);
            if let Some(node) = complete
                && (changed || evaluated.is_none_or(|t| t.elapsed().as_secs() >= 2))
            {
                complete_ok = match Library::read_project(root.clone(), id) {
                    Ok(lib) => lib
                        .projects
                        .first()
                        .is_some_and(|_| lib.effective(0, *node) == Status::Complete),
                    Err(e) if e.contains("is busy") => false,
                    Err(e) => return Err(e),
                };
                evaluated = Some(std::time::Instant::now());
            }
            if idle_ok && revision_ok && complete_ok {
                return Ok(
                    json!({"project": id, "revision": revision, "waited_ms": started.elapsed().as_millis() as u64}),
                );
            }
            if std::time::Instant::now() >= deadline {
                return Err(format!("timeout: condition not met within {timeout} s"));
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    }
    if let Command::History(HistoryCommand::Kinds) = &args.command {
        return Ok(crate::research::history::kinds_value());
    }
    if let Command::Journal {
        project,
        compact,
        compact_sidecars,
    } = &args.command
    {
        return Library::maintain_journals(
            root,
            project,
            *compact || *compact_sidecars,
            *compact_sidecars,
        );
    }
    if let Command::Node(NodeCommand::Batch {
        project,
        file,
        directed_by,
    }) = &args.command
    {
        let mut batch: Batch = serde_json::from_slice(&fs::read(file).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        if directed_by.is_some() {
            batch.directed_by = directed_by.clone();
        }
        if batch.is_import() {
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

    let selected = project_of(&args.command);
    let read_only = matches!(
        &args.command,
        Command::Project(
            ProjectCommand::Show { .. }
                | ProjectCommand::Revision { .. }
                | ProjectCommand::Tidy { apply: false, .. }
        ) | Command::Node(
            NodeCommand::Show { .. }
                | NodeCommand::List { .. }
                | NodeCommand::ReadBatch { .. }
                | NodeCommand::Resolve { .. }
                | NodeCommand::Flagged { .. }
                | NodeCommand::Quarantined { .. }
                | NodeCommand::Unproven { .. }
                | NodeCommand::Delete { expect: None, .. }
                | NodeCommand::Prune { expect: None, .. }
        ) | Command::Deps(DepsCommand::Get { .. })
            | Command::Claim(ClaimCommand::List { .. })
            | Command::ReviewQueue { .. }
            | Command::History(HistoryCommand::Show { .. })
            | Command::Status { .. }
            | Command::Journal { compact: false, .. }
            | Command::Impl(ImplCommand::Show { .. } | ImplCommand::Where { .. })
    );
    let creating = matches!(
        args.command,
        Command::Init | Command::Project(ProjectCommand::Create { .. })
    );
    let mut lib = if read_only {
        Library::read_project(home.join("projects"), selected.unwrap())?
    } else if creating {
        // Creating one project needs none of the others opened, or recovered.
        Library::empty(home.join("projects"))?
    } else {
        Library::open_project(home.join("projects"), selected)?
    };
    if let Some(reason) = &lib.recovery_blocked {
        eprintln!(
            "{}",
            json!({"warning": "recovery_blocked", "detail": reason, "answer": "from the committed catalog"})
        );
    }
    if matches!(args.command, Command::Init) {
        return Ok(locations);
    }
    execute(args.command, &mut lib)
}
/// The project IDs in `root`, sorted: folders with a catalog.
/// `research group`: reads and writes the research home's settings, never a project.
fn groups(home: &std::path::Path, command: &GroupCommand) -> Result<Value, String> {
    let existing = crate::settings::existing_projects(home);
    let known = |project: &str| {
        if existing.iter().any(|id| id == project) {
            Ok(())
        } else {
            Err(format!("Unknown project: {project}"))
        }
    };
    let settings = match command {
        GroupCommand::List => crate::settings::load(home)?,
        GroupCommand::Set { project, group } => {
            known(project)?;
            crate::settings::update(home, |s| {
                s.retain_projects(&existing);
                s.assign(project, Some(group))?;
                Ok(s.clone())
            })?
        }
        GroupCommand::Clear { project } => {
            known(project)?;
            crate::settings::update(home, |s| {
                s.retain_projects(&existing);
                s.assign(project, None)?;
                Ok(s.clone())
            })?
        }
    };
    let mut shown = settings.clone();
    shown.retain_projects(&existing);
    let ungrouped: Vec<&String> = existing
        .iter()
        .filter(|id| shown.group_of(id).is_none())
        .collect();
    Ok(json!({"groups": shown.groups, "ungrouped": ungrouped}))
}
/// Runs a project command against an already opened library: a direct-mode library, a
/// coordinator snapshot, or the coordinator writer's library (possibly staging).
pub(crate) fn execute(command: Command, lib: &mut Library) -> Result<Value, String> {
    match command {
        Command::Journal { .. } => {
            unreachable!("journal commands are handled before opening a library")
        }
        Command::Import(_) => unreachable!("import commands handled before opening a library"),
        Command::Impl(command) => {
            let p = project(lib, command.project())?;
            match command {
                ImplCommand::Show { node, unplaced, .. } => lib.impl_view(p, node, unplaced),
                ImplCommand::Where { record, .. } => lib.impl_where(p, record),
                ImplCommand::Repo { path, remote, .. } => {
                    lib.impl_set_repository(p, &path, &remote)?;
                    Ok(json!({"repository": path.trim(), "remote": remote.trim()}))
                }
                ImplCommand::Add { name, parent, .. } => {
                    let id = lib.impl_add(p, parent, &name)?;
                    Ok(json!({"node": id}))
                }
                ImplCommand::Rename { node, name, .. } => {
                    lib.impl_rename(p, node, &name)?;
                    Ok(json!({"node": node, "name": name.trim()}))
                }
                ImplCommand::Move { node, parent, .. } => {
                    lib.impl_move(p, node, parent)?;
                    Ok(json!({"node": node, "parent": parent}))
                }
                ImplCommand::Remove { node, .. } => {
                    lib.impl_remove(p, node)?;
                    Ok(json!({"removed": node}))
                }
                ImplCommand::Place { node, records, .. } => lib.impl_place(p, node, &records),
                ImplCommand::Unplace { records, .. } => lib.impl_unplace(p, &records),
                ImplCommand::ItemAdd { node, name, .. } => {
                    let id = lib.impl_add_item(p, node, &name)?;
                    Ok(json!({"node": node, "item": id}))
                }
                ImplCommand::ItemRemove { item, .. } => {
                    lib.impl_remove_item(p, item)?;
                    Ok(json!({"removed_item": item}))
                }
                ImplCommand::Complete { records, items, .. } => {
                    lib.impl_set_complete(p, &records, &items, true)
                }
                ImplCommand::Reopen { records, items, .. } => {
                    lib.impl_set_complete(p, &records, &items, false)
                }
            }
        }
        Command::Project(command) => match command {
            ProjectCommand::List => {
                for (project, error) in &lib.unreadable {
                    eprintln!(
                        "{}",
                        json!({"warning": format!("Project {project} could not be read and is not listed: {error}"), "kind": "unreadable_project", "project": project})
                    );
                }
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
                    let p = project(lib, &id)?;
                    lib.set_scope(p, &scope, 0)?;
                }
                Ok(json!({"created":id,"app_node":0}))
            }
            ProjectCommand::Revision { project: id } => {
                let p = project(lib, &id)?;
                let pr = &lib.projects[p];
                Ok(
                    json!({"project":pr.id,"binary":pr.binary,"revision":pr.revision,"next_id":pr.next_id,"structural_epoch":pr.structural_epoch,"layout_version":pr.layout_version,"active_import":pr.active_import}),
                )
            }
            ProjectCommand::Show { project: id } => {
                let p = project(lib, &id)?;
                let proofs = lib.proofs(p)?;
                let mut out = json!(&lib.projects[p]);
                out["proof"] = json!({"required":proofs.required,"repository":proofs.repository,"recorded":proofs.proofs.len()});
                Ok(out)
            }
            ProjectCommand::SetProof {
                project: id,
                required,
                repository,
            } => {
                let p = project(lib, &id)?;
                lib.set_proof_policy(p, required, repository.as_deref())?;
                let proofs = lib.proofs(p)?;
                Ok(
                    json!({"project":id,"required":proofs.required,"repository":proofs.repository,"revision":lib.projects[p].revision}),
                )
            }
            ProjectCommand::SetBinary {
                project: id,
                binary,
                expect_revision,
            } => {
                let p = project(lib, &id)?;
                lib.set_binary(p, &binary, expect_revision)?;
                Ok(
                    json!({"project":id,"binary":lib.projects[p].binary,"revision":lib.projects[p].revision}),
                )
            }
            ProjectCommand::Tidy { project: id, apply } => {
                let p = project(lib, &id)?;
                lib.tidy(p, apply)
            }
            ProjectCommand::SetScope {
                project: id,
                scope,
                expect_revision,
            } => {
                let p = project(lib, &id)?;
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
                directed_by,
            } => remove(lib, &id, &file, expect.as_deref(), directed_by, false),
            NodeCommand::Prune {
                project: id,
                file,
                expect,
                directed_by,
            } => remove(lib, &id, &file, expect.as_deref(), directed_by, true),
            NodeCommand::ReadBatch { project: id, nodes } => {
                lib.read_reports(project(lib, &id)?, &nodes)
            }
            NodeCommand::SaveBatch { project: id, file } => {
                let p = project(lib, &id)?;
                let batch = serde_json::from_slice(&fs::read(file).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
                let saved = lib.save_reports(p, &batch)?;
                Ok(json!({"project":id,"revision":lib.projects[p].revision,"saved":saved}))
            }
            NodeCommand::RepairLinks {
                project: id,
                expect_revision,
            } => {
                let p = project(lib, &id)?;
                let batch = serde_json::from_value(json!({"schema":1,"binary":lib.projects[p].binary,"expect_revision":expect_revision,"repair_legacy_links":true})).map_err(|e|e.to_string())?;
                lib.batch(p, &batch)?;
                Ok(json!({"project":id,"revision":lib.projects[p].revision}))
            }
            NodeCommand::Batch {
                project: id,
                file,
                directed_by,
            } => {
                let p = project(lib, &id)?;
                let mut batch: Batch =
                    serde_json::from_slice(&fs::read(file).map_err(|e| e.to_string())?)
                        .map_err(|e| e.to_string())?;
                if directed_by.is_some() {
                    batch.directed_by = directed_by;
                }
                let outcome = lib.apply_batch(p, &batch)?;
                let revision = lib.projects[p].revision;
                // Report what this batch reopened and whose claimed work that touches.
                let states = lib.effective_all(p);
                let claims = lib.claims(p)?;
                let covered = lib.claim_index(p, &claims);
                let reopened: Vec<_> = lib
                    .review_queue(p, &states)?
                    .into_iter()
                    .filter(|(_, entries)| entries.iter().any(|e| e.revision == revision))
                    .map(|(node, _)| {
                        json!({"node": node, "claimed_by": covered.get(&node).map(|(at, owner)| json!({"node": at, "owner": owner}))})
                    })
                    .collect();
                let moved: Vec<_> = outcome
                    .moved
                    .iter()
                    .map(|(node, from, to)| json!({"node": node, "from": from, "to": to}))
                    .collect();
                let mut answer = json!({"project": id, "revision":revision, "created":outcome.created, "reopened": reopened, "moved": moved, "ledger_rows_repathed": outcome.ledger_rows_repathed});
                // Renames: the citations that followed, the mentions left for review, and the
                // records that stayed complete because only names changed in their reports.
                if !outcome.citations.is_empty() {
                    answer["citations"] = json!(outcome.citations);
                }
                if !outcome.unrewritten.is_empty() {
                    answer["unrewritten"] = json!(outcome.unrewritten);
                }
                if !outcome.kept_complete.is_empty() {
                    answer["kept_complete"] = json!(outcome.kept_complete);
                }
                Ok(answer)
            }
            NodeCommand::Resolve { project: id, paths } => {
                let p = project(lib, &id)?;
                Ok(lib.resolve_paths(p, &paths))
            }
            NodeCommand::List {
                project: id,
                filter,
            } => {
                let p = project(lib, &id)?;
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
                directed_by,
            } => {
                let p = project(lib, &id)?;
                let parent_kind = lib.projects[p]
                    .nodes
                    .iter()
                    .find(|n| n.id == parent)
                    .ok_or("Unknown parent")?
                    .kind;
                if (parent_kind == Kind::Chunk) != rva.is_some() {
                    return Err("Supply --rva only when creating a function under a chunk.".into());
                }
                let node = lib.add_directed(
                    p,
                    parent,
                    &name,
                    rva.as_deref().unwrap_or(""),
                    directed_by.as_deref(),
                )?;
                node_view(lib, p, node)
            }
            NodeCommand::Show { project: id, node } => {
                let p = project(lib, &id)?;
                node_view(lib, p, node)
            }
            NodeCommand::Save {
                project: id,
                node,
                body_file,
                evidence_file,
                expect: token,
                evidence_only,
            } => {
                let p = project(lib, &id)?;
                let body = fs::read_to_string(body_file).map_err(|e| e.to_string())?;
                let evidence = fs::read_to_string(evidence_file).map_err(|e| e.to_string())?;
                if evidence_only {
                    lib.save_evidence(p, node, &body, &evidence, &token)?;
                } else {
                    lib.save_report(p, node, &body, &evidence, false, &token)?;
                }
                node_view(lib, p, node)
            }
            NodeCommand::Flag {
                project: id,
                node,
                reason,
            } => {
                let p = project(lib, &id)?;
                lib.set_review_flag(p, node, Some(&reason))?;
                node_view(lib, p, node)
            }
            NodeCommand::Unflag { project: id, node } => {
                let p = project(lib, &id)?;
                lib.set_review_flag(p, node, None)?;
                node_view(lib, p, node)
            }
            NodeCommand::Quarantine {
                project: id,
                node,
                origin,
                reason,
                by,
            } => {
                let p = project(lib, &id)?;
                lib.set_quarantine(p, node, Some(&origin), &reason, &by)?;
                node_view(lib, p, node)
            }
            NodeCommand::Unquarantine {
                project: id,
                node,
                reason,
                by,
            } => {
                let p = project(lib, &id)?;
                lib.set_quarantine(p, node, None, &reason, &by)?;
                node_view(lib, p, node)
            }
            NodeCommand::Quarantined { project: id } => {
                let p = project(lib, &id)?;
                let records = lib.quarantines(p)?;
                let covered = lib.quarantined(p);
                let rows: Vec<_> = records.iter().map(|(n, q)| {
                    let node = lib.projects[p].nodes.iter().find(|x| x.id == *n);
                    let functions = lib.projects[p].nodes.iter().filter(|x| x.parent == Some(*n)).count();
                    json!({"id":n,"name":node.map(|x| x.name.as_str()),"kind":node.map(|x| x.kind),"origin":q.origin,"reason":q.reason,"at_revision":q.at_revision,"by":q.by,"covered_children":functions})
                }).collect();
                Ok(json!({"project":id,"quarantined":rows,"records_covered":covered.len()}))
            }
            NodeCommand::Proof {
                project: id,
                node,
                recreation,
                tests,
                clear,
            } => {
                let p = project(lib, &id)?;
                let input = (!clear).then(|| crate::research::proofs::ProofInput {
                    id: node,
                    recreation: recreation.unwrap_or_default(),
                    tests,
                });
                lib.set_proofs(p, &[(node, input)])?;
                node_view(lib, p, node)
            }
            NodeCommand::ProofBatch { project: id, file } => {
                let p = project(lib, &id)?;
                #[derive(serde::Deserialize)]
                struct Request {
                    proofs: Vec<crate::research::proofs::ProofInput>,
                }
                let request: Request =
                    serde_json::from_slice(&fs::read(file).map_err(|e| e.to_string())?)
                        .map_err(|e| e.to_string())?;
                let entries: Vec<_> = request
                    .proofs
                    .into_iter()
                    .map(|input| (input.id, Some(input)))
                    .collect();
                lib.set_proofs(p, &entries)?;
                Ok(
                    json!({"project":id,"recorded":entries.len(),"revision":lib.projects[p].revision}),
                )
            }
            NodeCommand::Unproven {
                project: id,
                complete,
            } => {
                let p = project(lib, &id)?;
                let proofs = lib.proofs(p)?;
                let states = lib.effective_all(p);
                let rows: Vec<_> = lib.projects[p]
                    .nodes
                    .iter()
                    .filter(|n| n.kind == Kind::Function && !proofs.proofs.contains_key(&n.id))
                    .filter(|n| !complete || states[&n.id] == Status::Complete)
                    .map(|n| json!({"id":n.id,"name":n.name,"rva":n.rva.map(|r| format!("0x{r:08X}")),"effective_status":states[&n.id],"context_only":n.context_only}))
                    .collect();
                Ok(json!({"project":id,"required":proofs.required,"unproven":rows}))
            }
            NodeCommand::Flagged { project: id } => {
                let p = project(lib, &id)?;
                let flags = lib.review_flags(p)?;
                let rows: Vec<_> = flags.iter().map(|(n, f)| {
                    let node = lib.projects[p].nodes.iter().find(|x| x.id == *n);
                    json!({"id":n,"name":node.map(|x| x.name.as_str()),"kind":node.map(|x| x.kind),"reason":f.reason,"flagged_at_revision":f.flagged_at_revision})
                }).collect();
                Ok(json!({"project":id,"flagged":rows}))
            }
            NodeCommand::ResetStatus { project: id, file } => {
                let p = project(lib, &id)?;
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
                let p = project(lib, &id)?;
                let (body, evidence) = lib.read(p, node)?;
                lib.save_report(p, node, &body, &evidence, true, &token)?;
                node_view(lib, p, node)
            }
            NodeCommand::Reopen {
                project: id,
                node,
                expect: token,
            } => {
                let p = project(lib, &id)?;
                lib.reopen(p, node, &token)?;
                node_view(lib, p, node)
            }
        },
        Command::Deps(command) => match command {
            DepsCommand::Get { project: id, node } => {
                let p = project(lib, &id)?;
                node_view(lib, p, node)
            }
            DepsCommand::Set {
                project: id,
                node,
                expect: token,
                rvas,
                directed_by,
            } => {
                let p = project(lib, &id)?;
                expect(lib, p, node, &token)?;
                lib.set_dependencies_directed(p, node, &rvas.join(","), directed_by.as_deref())?;
                node_view(lib, p, node)
            }
        },
        Command::Status {
            project: id,
            filter,
            ready,
            ready_for_synthesis: synthesis_only,
            summary,
        } => {
            let p = project(lib, &id)?;
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
            let claims = lib.claims(p)?;
            let covered = lib.claim_index(p, &claims);
            let queue = lib.review_queue(p, &states)?;
            let quarantined = lib.quarantined(p);
            let proofs = lib.proofs(p)?;
            let mut counts: std::collections::BTreeMap<
                String,
                std::collections::BTreeMap<String, usize>,
            > = Default::default();
            let mut ready_count = 0usize;
            let mut synthesis_count = 0usize;
            let records:Vec<_>=lib.projects[p].nodes.iter().filter(|n| filter.keeps(n, states[&n.id])).filter_map(|n|{
                let children = kids.get(&n.id).cloned().unwrap_or_default();
                let blocked_children:Vec<_>=children.iter().copied().filter(|id|states[id]!=Status::Complete).collect();
                let blocked_prerequisites:Vec<_>=deps.get(&n.id).into_iter().flatten().copied().filter(|id|states[id]!=Status::Complete).collect();
                // Review needs written research: the record's own report has been started. A parent whose
                // children are all complete but whose own report is empty is ready for synthesis instead.
                let unproven = proofs.required&&n.kind==Kind::Function&&!proofs.proofs.contains_key(&n.id);
                let unblocked = states[&n.id]!=Status::Complete&&!flags.contains_key(&n.id)&&!unproven&&blocked_children.is_empty()&&blocked_prerequisites.is_empty()&&(n.kind==Kind::Function||!children.is_empty());
                let ready_for_review = unblocked&&n.status!=Status::NotStarted;
                let ready_for_synthesis = unblocked&&n.status==Status::NotStarted&&n.kind!=Kind::Function;
                if ready && !ready_for_review { return None; }
                if synthesis_only && !ready_for_synthesis { return None; }
                ready_count += usize::from(ready_for_review);
                synthesis_count += usize::from(ready_for_synthesis);
                *counts.entry(format!("{:?}", n.kind)).or_default().entry(format!("{:?}", states[&n.id])).or_default() += 1;
                (!summary).then(|| json!({"id":n.id,"name":n.name,"kind":n.kind,"status":states[&n.id],"required_research":required.contains(&n.id),"blocked_children":blocked_children,"blocked_prerequisites":blocked_prerequisites,
                    "ready_for_review":ready_for_review,"ready_for_synthesis":ready_for_synthesis,"review_flag":flags.get(&n.id),"quarantined":quarantined.contains(&n.id),
                    "claimed_by":covered.get(&n.id).map(|(at, owner)| json!({"node": at, "owner": owner})),
                    "review_queue":queue.get(&n.id)}))
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
            let mut out = json!({"project":id,"app_status":states[&0],"review_flags":flags.len(),"claims":claims.len(),"review_queue":queue.len(),"research_scope":lib.projects[p].research_scope,"inventory_functions":inventory,"required_functions":required_functions,"context_functions":inventory-required_functions});
            if summary {
                out["counts"] = json!(counts);
                out["ready_for_review"] = json!(ready_count);
                out["ready_for_synthesis"] = json!(synthesis_count);
                out["quarantined"] = json!(quarantined.len());
                out["unproven_complete"] = json!(
                    lib.projects[p]
                        .nodes
                        .iter()
                        .filter(|n| n.kind == Kind::Function
                            && states.get(&n.id) == Some(&Status::Complete)
                            && !proofs.proofs.contains_key(&n.id))
                        .count()
                );
            } else {
                out["records"] = json!(records);
            }
            Ok(out)
        }
        Command::Claim(command) => match command {
            ClaimCommand::Set {
                project: id,
                node,
                owner,
                note,
                lease_hours,
                force,
            } => {
                let p = project(lib, &id)?;
                let claim = lib.claim(p, node, &owner, &note, lease_hours, force)?;
                Ok(
                    json!({"project": id, "node": node, "claim": claim, "revision": lib.projects[p].revision}),
                )
            }
            ClaimCommand::Release {
                project: id,
                node,
                owner,
                force,
            } => {
                let p = project(lib, &id)?;
                let released = lib.release_claim(p, node, &owner, force)?;
                Ok(json!({"project": id, "node": node, "released": released}))
            }
            ClaimCommand::List { project: id } => {
                let p = project(lib, &id)?;
                let names: std::collections::HashMap<u64, &str> = lib.projects[p]
                    .nodes
                    .iter()
                    .map(|n| (n.id, n.name.as_str()))
                    .collect();
                let claims: Vec<_> = lib
                    .claims(p)?
                    .into_iter()
                    .map(|(node, c)| json!({"node": node, "name": names.get(&node), "claim": c}))
                    .collect();
                Ok(json!({"project": id, "claims": claims}))
            }
        },
        Command::History(HistoryCommand::Show {
            project: id,
            node,
            since,
            until,
            limit,
            only,
            kind,
            detail,
            rva,
            from,
            to,
        }) => {
            let p = project(lib, &id)?;
            use crate::research::history::day_start;
            let rva = rva
                .map(|r| {
                    let digits = r.trim().trim_start_matches("0x").trim_start_matches("0X");
                    u32::from_str_radix(digits, 16)
                        .map_err(|_| format!("RVAs are hexadecimal: {r}"))
                })
                .transpose()?;
            let from = from.as_deref().map(day_start).transpose()?;
            let to = to
                .as_deref()
                .map(|d| day_start(d).map(|t| t + 86_400))
                .transpose()?;
            lib.history(
                p,
                &crate::research::history::HistoryFilter {
                    node,
                    since,
                    until,
                    limit,
                    catalog: !matches!(only, Some(HistorySource::Tooling)),
                    tooling: !matches!(only, Some(HistorySource::Catalog)),
                    detail,
                    kind,
                    rva,
                    from,
                    to,
                },
            )
        }
        Command::History(HistoryCommand::Kinds) => Ok(crate::research::history::kinds_value()),
        Command::History(HistoryCommand::Append {
            project: id,
            kind,
            summary,
            nodes,
            refs,
            diff_file,
        }) => {
            let p = project(lib, &id)?;
            let diff = diff_file
                .map(|f| -> Result<Value, String> {
                    serde_json::from_slice(&fs::read(f).map_err(|e| e.to_string())?)
                        .map_err(|e| e.to_string())
                })
                .transpose()?;
            lib.append_history(p, &kind, &summary, &nodes, &refs, diff)
        }
        Command::ReviewQueue { project: id } => {
            let p = project(lib, &id)?;
            let states = lib.effective_all(p);
            let claims = lib.claims(p)?;
            let covered = lib.claim_index(p, &claims);
            let names: std::collections::HashMap<u64, &crate::research::Node> =
                lib.projects[p].nodes.iter().map(|n| (n.id, n)).collect();
            let entries: Vec<_> = lib.review_queue(p, &states)?.into_iter().map(|(node, reasons)| {
                json!({"node": node, "name": names.get(&node).map(|n| n.name.as_str()), "kind": names.get(&node).map(|n| n.kind), "status": states.get(&node), "reasons": reasons,
                    "claimed_by": covered.get(&node).map(|(at, owner)| json!({"node": at, "owner": owner}))})
            }).collect();
            Ok(json!({"project": id, "count": entries.len(), "entries": entries}))
        }
        Command::Home
        | Command::Init
        | Command::Gui
        | Command::Group(_)
        | Command::Criteria { .. }
        | Command::Locks { .. }
        | Command::Wait { .. }
        | Command::Queue { .. }
        | Command::Job(_)
        | Command::Tools(_)
        | Command::Coordinator(_)
        | Command::Serve
        | Command::Session => unreachable!(),
    }
}
/// JSON-lines command loop: each stdin line is a JSON array of arguments (without `research`).
fn session(home: Option<PathBuf>, wait: Option<u64>) {
    use std::io::{BufRead, Write};
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let answer = match serde_json::from_str::<Vec<String>>(&line) {
            Err(e) => {
                json!({"error": format!("Each line must be a JSON array of arguments: {e}"), "kind": "usage"})
            }
            Ok(words) => {
                let mut argv = vec!["research".to_string()];
                if let Some(home) = &home {
                    argv.extend(["--home".into(), home.to_string_lossy().into_owned()]);
                }
                if let Some(wait) = wait {
                    argv.extend(["--wait".into(), wait.to_string()]);
                }
                argv.extend(words);
                match Args::try_parse_from(&argv) {
                    Err(e) => json!({"error": e.to_string(), "kind": "usage"}),
                    Ok(args)
                        if matches!(
                            args.command,
                            Command::Session | Command::Gui | Command::Serve
                        ) =>
                    {
                        json!({"error": "session, gui and serve cannot run inside a session", "kind": "usage"})
                    }
                    Ok(args) => match run(args) {
                        Ok(value) => json!({"ok": value}),
                        Err(error) => json!({"error": error, "kind": error_kind(&error)}),
                    },
                }
            }
        };
        if writeln!(stdout, "{answer}").is_err() || stdout.flush().is_err() {
            break;
        }
    }
}
pub(crate) fn error_kind(error: &str) -> &str {
    error
        .split_once(':')
        .map(|(prefix, _)| prefix)
        .filter(|prefix| {
            !prefix.is_empty() && prefix.bytes().all(|c| c.is_ascii_lowercase() || c == b'_')
        })
        .unwrap_or(if error.contains("is busy") {
            "busy"
        } else {
            "operation_failed"
        })
}
/// Writes a command's answer to stdout. A reader that has gone (`research ... | head`) ends the
/// output quietly: the command's work is done, and `println!` would panic instead.
fn print_answer(text: &str) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{text}").and_then(|()| out.flush());
}
/// Entry point of the `research` executable.
pub fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    // Identity first: these answer without a research home or a subcommand.
    match raw.as_slice() {
        [flag] if flag == "--version" || flag == "-V" => {
            println!("{}", crate::build_info::DISPLAY);
            return;
        }
        [flag] if flag == "--build-info" => {
            println!(
                "{}",
                serde_json::to_string_pretty(&crate::build_info::json()).unwrap_or_default()
            );
            return;
        }
        _ => {}
    }
    let args = Args::parse();
    if matches!(args.command, Command::Session) {
        session(args.home, args.wait);
        return;
    }
    if let Some(exit) = through_coordinator(&raw, &args) {
        std::process::exit(exit);
    }
    let job = matches!(args.command, Command::Job(JobCommand::Run { .. }));
    match run(args) {
        Ok(value) => {
            print_answer(&serde_json::to_string_pretty(&value).unwrap());
            // `job run` exits with its own result: the command's failure, or an error result.
            if job && let Some(exit) = value["exit"].as_i64().filter(|e| *e != 0) {
                std::process::exit(i32::try_from(exit).unwrap_or(1));
            }
        }
        Err(error) => {
            eprintln!("{}", json!({"error":error,"kind":error_kind(&error)}));
            std::process::exit(1);
        }
    }
}

/// Sends a project command to the research home's coordinator and prints its answer exactly as
/// direct mode would. `None` means run it in this process instead (direct mode).
fn through_coordinator(raw: &[String], args: &Args) -> Option<i32> {
    let route = route(&args.command);
    if matches!(route, Route::Direct) {
        return None;
    }
    let write = matches!(route, Route::Write(_));
    let home = paths::home(args.home.clone()).ok()?;
    let raw = absolute_file_arguments(raw);
    let raw = raw.as_slice();
    let mut connection = match crate::coordinator::client::connect(&home, "cli", true) {
        Ok(Some(connection)) => connection,
        Ok(None) => return None,
        Err(error) => {
            eprintln!("{}", json!({"error": error, "kind": error_kind(&error)}));
            return Some(1);
        }
    };
    match connection.request(raw, None) {
        // A stopping coordinator: this command runs in direct mode instead.
        Ok(answer) if answer.exit == 75 => None,
        Ok(answer) => {
            if let Some(stdout) = &answer.stdout {
                print_answer(&serde_json::to_string_pretty(stdout).unwrap_or_default());
            }
            for line in &answer.stderr {
                eprintln!("{line}");
            }
            Some(answer.exit)
        }
        // A read can simply be repeated directly; a write may already have been applied.
        Err(_) if !write => None,
        Err(error) => {
            let error = format!(
                "coordinator_lost: the connection failed after the request was sent ({error}); the operation may or may not have been applied. Check the record before retrying."
            );
            eprintln!("{}", json!({"error": error, "kind": "coordinator_lost"}));
            Some(1)
        }
    }
}

/// Input-file arguments made absolute in this process. The coordinator opens them from its
/// own process, whose working directories (including Windows' per-drive ones, for paths such
/// as `D:body.md`) are not the caller's, so a relative path would name another file there (or
/// none). Both `--flag value` and `--flag=value` forms are handled.
pub(crate) fn absolute_file_arguments(raw: &[String]) -> Vec<String> {
    const FILE_FLAGS: [&str; 4] = ["--file", "--body-file", "--evidence-file", "--diff-file"];
    let absolute = |value: &str| -> String {
        std::path::absolute(value)
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|_| value.to_string())
    };
    let mut out = Vec::with_capacity(raw.len());
    let mut arguments = raw.iter();
    while let Some(argument) = arguments.next() {
        if FILE_FLAGS.contains(&argument.as_str()) {
            out.push(argument.clone());
            if let Some(value) = arguments.next() {
                out.push(absolute(value));
            }
        } else if let Some((flag, value)) = argument.split_once('=')
            && FILE_FLAGS.contains(&flag)
        {
            out.push(format!("{flag}={}", absolute(value)));
        } else {
            out.push(argument.clone());
        }
    }
    out
}

/// How a command travels when a coordinator serves the research home.
pub(crate) enum Route {
    /// Runs in this process: setup, imports, maintenance, waiting.
    Direct,
    /// Answered from the project's snapshot, never waiting for writes.
    Read {
        project: String,
        /// The records the answer is about; `None` means the whole project.
        nodes: Option<Vec<u64>>,
    },
    /// Queued for the project's writer.
    Write(WriteRoute),
    /// Answered by the coordinator from its queue.
    Queue { project: String },
}
/// A queued write: what it touches, and whether it may share a commit.
pub(crate) struct WriteRoute {
    pub project: String,
    pub kind: &'static str,
    pub nodes: Vec<u64>,
    pub affects: &'static [&'static str],
    /// Record operations merge; structural ones commit alone.
    pub merge: bool,
    /// Defaults to the bulk class when large.
    pub bulk: bool,
    /// Report text the operation brings, for the merge bound.
    pub bytes: usize,
}
fn file_bytes(paths: &[&PathBuf]) -> usize {
    paths
        .iter()
        .filter_map(|p| fs::metadata(p).ok())
        .map(|m| m.len() as usize)
        .sum()
}
/// Record IDs named in a JSON request file under `field` (as `id` members or plain numbers).
fn ids_in(file: &PathBuf, field: &str) -> Vec<u64> {
    let Some(value) = fs::read(file)
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
    else {
        return vec![];
    };
    value[field]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v["id"].as_u64().or_else(|| v.as_u64()))
        .collect()
}
fn read_route(project: &str, nodes: Option<Vec<u64>>) -> Route {
    Route::Read {
        project: project.to_string(),
        nodes,
    }
}
fn write_route(
    project: &str,
    kind: &'static str,
    nodes: Vec<u64>,
    affects: &'static [&'static str],
    merge: bool,
    bytes: usize,
) -> Route {
    Route::Write(WriteRoute {
        project: project.to_string(),
        kind,
        bulk: nodes.len() > 32,
        nodes,
        affects,
        merge,
        bytes,
    })
}
/// Whether the batch in `file` runs as a background import (`Batch::is_import`).
fn import_batch(file: &PathBuf) -> bool {
    fs::read(file)
        .ok()
        .and_then(|b| serde_json::from_slice::<Batch>(&b).ok())
        .is_some_and(|batch| batch.is_import())
}
pub(crate) fn route(command: &Command) -> Route {
    match command {
        Command::Status { project, .. } | Command::ReviewQueue { project } => {
            read_route(project, None)
        }
        Command::Project(
            ProjectCommand::Show { project } | ProjectCommand::Revision { project },
        ) => read_route(project, None),
        Command::Project(ProjectCommand::SetBinary { project, .. }) => {
            write_route(project, "set-binary", vec![0], &["structure"], false, 0)
        }
        Command::Project(ProjectCommand::SetScope { project, .. }) => {
            write_route(project, "set-scope", vec![], &["scope"], false, 0)
        }
        // Proof writes touch only their sidecar: record operations that may share a commit.
        Command::Project(ProjectCommand::SetProof { project, .. }) => {
            write_route(project, "proof", vec![], &["proof"], true, 0)
        }
        Command::Project(
            ProjectCommand::List | ProjectCommand::Create { .. } | ProjectCommand::Tidy { .. },
        ) => Route::Direct,
        Command::Node(command) => match command {
            NodeCommand::Show { project, node } => read_route(project, Some(vec![*node])),
            NodeCommand::ReadBatch { project, nodes } => read_route(project, Some(nodes.clone())),
            NodeCommand::List { project, .. }
            | NodeCommand::Flagged { project }
            | NodeCommand::Quarantined { project }
            | NodeCommand::Unproven { project, .. }
            | NodeCommand::Resolve { project, .. } => read_route(project, None),
            NodeCommand::Proof { project, node, .. } => {
                write_route(project, "proof", vec![*node], &["proof"], true, 0)
            }
            NodeCommand::ProofBatch { project, file } => write_route(
                project,
                "proof",
                ids_in(file, "proofs"),
                &["proof"],
                true,
                0,
            ),
            NodeCommand::Delete {
                project,
                expect: None,
                ..
            }
            | NodeCommand::Prune {
                project,
                expect: None,
                ..
            } => read_route(project, None),
            NodeCommand::Delete { project, file, .. }
            | NodeCommand::Prune { project, file, .. } => {
                write_route(project, "delete", ids_in(file, "ids"), &["place"], false, 0)
            }
            NodeCommand::Save {
                project,
                node,
                body_file,
                evidence_file,
                ..
            } => write_route(
                project,
                "save",
                vec![*node],
                &["text", "status"],
                true,
                file_bytes(&[body_file, evidence_file]),
            ),
            NodeCommand::SaveBatch { project, file } => write_route(
                project,
                "save-batch",
                ids_in(file, "reports"),
                &["text", "status"],
                true,
                file_bytes(&[file]),
            ),
            NodeCommand::Complete { project, node, .. } => {
                write_route(project, "complete", vec![*node], &["status"], true, 0)
            }
            NodeCommand::Reopen { project, node, .. } => {
                write_route(project, "reopen", vec![*node], &["status"], true, 0)
            }
            NodeCommand::Flag { project, node, .. } | NodeCommand::Unflag { project, node } => {
                write_route(project, "flag", vec![*node], &["flag"], true, 0)
            }
            // Quarantine changes the effective status of a whole chunk and clears flags and
            // claims: it commits alone.
            NodeCommand::Quarantine { project, node, .. }
            | NodeCommand::Unquarantine { project, node, .. } => write_route(
                project,
                "quarantine",
                vec![*node],
                &["status", "flag", "claim"],
                false,
                0,
            ),
            NodeCommand::ResetStatus { project, file } => write_route(
                project,
                "reset-status",
                ids_in(file, "ids"),
                &["status"],
                true,
                0,
            ),
            NodeCommand::RepairLinks { project, .. } => {
                write_route(project, "repair-links", vec![], &["text"], false, 0)
            }
            NodeCommand::Create {
                project, parent, ..
            } => write_route(project, "create", vec![*parent], &["membership"], false, 0),
            NodeCommand::Batch { project, file, .. } => {
                if import_batch(file) {
                    return Route::Direct;
                }
                let mut nodes = ids_in(file, "update");
                if let Some(request) = fs::read(file)
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                {
                    nodes.extend(
                        request["reorder"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|o| o["parent"].as_u64()),
                    );
                }
                write_route(project, "batch", nodes, &["place", "membership"], false, 0)
            }
        },
        Command::Deps(DepsCommand::Get { project, node }) => read_route(project, Some(vec![*node])),
        Command::Deps(DepsCommand::Set { project, node, .. }) => write_route(
            project,
            "prerequisites",
            vec![*node],
            &["prerequisites"],
            false,
            0,
        ),
        Command::Claim(ClaimCommand::List { project }) => read_route(project, None),
        Command::Impl(command @ (ImplCommand::Show { .. } | ImplCommand::Where { .. })) => {
            read_route(command.project(), None)
        }
        // Tracker changes touch only its sidecars: record operations that may share a commit.
        Command::Impl(command) => write_route(
            command.project(),
            "implementation",
            vec![],
            &["implementation"],
            true,
            0,
        ),
        Command::Claim(
            ClaimCommand::Set { project, node, .. } | ClaimCommand::Release { project, node, .. },
        ) => write_route(project, "claim", vec![*node], &["claim"], true, 0),
        Command::History(HistoryCommand::Show { project, .. }) => read_route(project, None),
        Command::History(HistoryCommand::Kinds) => Route::Direct,
        Command::History(HistoryCommand::Append { project, nodes, .. }) => {
            write_route(project, "history", nodes.clone(), &[], false, 0)
        }
        Command::Queue { project } => Route::Queue {
            project: project.clone(),
        },
        Command::Home
        | Command::Init
        | Command::Gui
        | Command::Group(_)
        | Command::Criteria { .. }
        | Command::Import(_)
        | Command::Locks { .. }
        | Command::Wait { .. }
        | Command::Session
        | Command::Journal { .. }
        | Command::Coordinator(_)
        | Command::Job(_)
        | Command::Tools(_)
        | Command::Serve => Route::Direct,
    }
}
/// Parses arguments as the `research` executable would (without the program name).
pub(crate) fn parse(arguments: &[String]) -> Result<Args, String> {
    Args::try_parse_from(std::iter::once("research".to_string()).chain(arguments.iter().cloned()))
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod import_routing_tests {
    use super::import_batch;

    #[test]
    fn only_a_creation_only_batch_without_link_repair_is_a_background_import() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-workspaces")
            .join(format!("routing-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("batch.json");
        let create = r#"[{"parent":2,"name":"F","rva":4096}]"#;
        for (repair, import) in [(false, true), (true, false)] {
            std::fs::write(
                &file,
                format!(
                    r#"{{"schema":1,"binary":"b","expect_revision":1,"create":{create},"repair_legacy_links":{repair}}}"#
                ),
            )
            .unwrap();
            assert_eq!(import_batch(&file), import, "repair_legacy_links {repair}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

#[cfg(test)]
mod file_argument_tests {
    use super::absolute_file_arguments;

    #[test]
    fn every_relative_file_argument_becomes_absolute() {
        let raw: Vec<String> = [
            "node",
            "save",
            "p",
            "3",
            "--body-file",
            "body.md",
            "--evidence-file=sub/e.md",
            "--expect",
            "d4:x",
            "--file",
            r"\rooted.json",
            "--diff-file",
            "C:drive-relative.json",
        ]
        .map(String::from)
        .to_vec();
        let out = absolute_file_arguments(&raw);
        let value = |flag: &str| {
            out.iter()
                .position(|a| a == flag)
                .map(|i| out[i + 1].clone())
                .or_else(|| {
                    out.iter()
                        .find_map(|a| a.strip_prefix(&format!("{flag}=")).map(String::from))
                })
                .unwrap()
        };
        for flag in ["--body-file", "--evidence-file", "--file", "--diff-file"] {
            assert!(
                std::path::Path::new(&value(flag)).is_absolute(),
                "{flag}: {out:?}"
            );
        }
        // Everything else is untouched, and absolute paths stay as written.
        assert_eq!(value("--expect"), "d4:x");
        assert_eq!(&out[..4], &raw[..4]);
        let absolute = std::env::temp_dir()
            .join("x.md")
            .to_string_lossy()
            .into_owned();
        let kept = absolute_file_arguments(&["--file".into(), absolute.clone()]);
        assert_eq!(kept[1], absolute);
    }
}
