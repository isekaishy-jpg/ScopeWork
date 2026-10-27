//! Durable additive imports. Unpublished files never replace committed research.
use super::*;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::Write,
    time::Instant,
};
const SEGMENT: usize = 256;
const WORKERS: usize = 2;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum JobState {
    Reserved,
    Preparing,
    Verifying,
    Paused,
    Ready,
    Publishing,
    Committed,
    Cancelling,
    Cancelled,
    Failed,
    Conflict,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Job {
    #[serde(default)]
    pub warnings: Vec<String>,
    pub schema: u32,
    pub id: String,
    pub project: String,
    pub state: JobState,
    pub first_id: u64,
    pub end_id: u64,
    pub prepared: usize,
    #[serde(default)]
    pub verified: usize,
    pub total: usize,
    pub error: Option<String>,
    pub revision: Option<u64>,
    pub timings_ms: BTreeMap<String, u64>,
}
#[derive(Serialize, Deserialize)]
struct Reservation {
    binary: String,
    structural_epoch: u64,
    request_hash: String,
}
#[derive(Clone, Serialize, Deserialize)]
struct Output {
    path: String,
    hash: String,
    bytes: usize,
}
#[derive(Serialize, Deserialize)]
struct Segment {
    sequence: usize,
    ids: Vec<u64>,
    outputs: Vec<Output>,
}
#[derive(Clone)]
struct Unit {
    node: Node,
    path: String,
    report: String,
}
#[derive(Clone)]
struct Prepared {
    id: u64,
    folder: String,
    files: Vec<(Output, Vec<u8>)>,
}
#[derive(Serialize, Deserialize)]
struct ParentEdit {
    path: String,
    before: String,
    after: String,
}
#[derive(Serialize, Deserialize)]
struct Publication {
    before_hash: String,
    after_hash: String,
    catalog: String,
    edits: Vec<ParentEdit>,
}
fn json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    atomic(path, &serde_json::to_vec(value).map_err(|e| e.to_string())?)
}
fn job_path(root: &Path, id: &str) -> Result<PathBuf> {
    uuid::Uuid::parse_str(id).map_err(|_| "Invalid import job ID")?;
    Ok(root.join("tools/workspaces/import-jobs").join(id))
}
fn library_project(root: &Path, project: &str, write: bool) -> Result<Library> {
    let until = Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let result = if write {
            Library::open_project(root.into(), Some(project))
        } else {
            Library::read_project(root.into(), project)
        };
        match result {
            Err(e) if e.contains("is busy") && Instant::now() < until => {
                std::thread::sleep(std::time::Duration::from_millis(20))
            }
            Ok(lib) if lib.projects.is_empty() => return Err("Unknown project".into()),
            other => return other,
        }
    }
}
fn job_lock(folder: &Path) -> Result<fs::File> {
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(folder.join("runner.lock"))
        .map_err(|e| e.to_string())?;
    file.try_lock()
        .map_err(|_| "import_busy: this job already has a runner".to_string())?;
    Ok(file)
}
fn checkpoint(folder: &Path, job: &Job) -> Result<()> {
    json(&folder.join("manifest.json"), job)
}
fn elapsed(start: Instant) -> u64 {
    start.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}
fn addition(project: &Project, batch: &Batch, first: u64) -> Result<Vec<Node>> {
    if batch.schema != 1
        || batch.binary != project.binary
        || batch.create.is_empty()
        || !batch.update.is_empty()
        || !batch.reorder.is_empty()
        || batch.repair_legacy_links
    {
        return Err("additive_only: use a creation-only schema-1 batch with exact binary identity; mixed batches retain exclusive publication".into());
    }
    let mut kinds: HashMap<_, _> = project.nodes.iter().map(|n| (n.id, n.kind)).collect();
    let mut rvas: HashSet<_> = project.nodes.iter().filter_map(|n| n.rva).collect();
    let mut nodes = Vec::with_capacity(batch.create.len());
    for (offset, c) in batch.create.iter().enumerate() {
        let id = first
            .checked_add(offset as u64)
            .ok_or("Node ID exhausted")?;
        if kinds.contains_key(&id) {
            return Err("Reserved node ID is already visible".into());
        }
        let kind = kinds
            .get(&c.parent)
            .and_then(|k| k.child())
            .ok_or("Unknown/invalid parent; create parents before their children")?;
        if (kind == Kind::Function) != c.rva.is_some() {
            return Err("Invalid node name or RVA scope".into());
        }
        let name = record_name(&c.name)?;
        if let Some(rva) = c.rva
            && (project.binary.trim().is_empty() || !rvas.insert(rva))
        {
            return Err("Duplicate function RVA or empty binary identity".into());
        }
        kinds.insert(id, kind);
        nodes.push(Node {
            id,
            parent: Some(c.parent),
            kind,
            name,
            rva: c.rva,
            context_only: c.context_only,
            status: Status::NotStarted,
            revision: 0,
            accepted_hash: None,
            accepted_prerequisites: Default::default(),
        });
    }
    first
        .checked_add(nodes.len() as u64)
        .ok_or("Node ID exhausted")?;
    Ok(nodes)
}
pub fn plan(root: &Path, project: &str, batch: &Batch) -> Result<serde_json::Value> {
    let lib = Library::open_mode(root.into(), Some(project), false, false)?;
    let p = lib.projects.first().ok_or("Unknown project")?;
    if Some(p.revision) != batch.expect_revision {
        return Err("revision_conflict: reload project show before admission".into());
    }
    lib.ensure_structural(0)?;
    let nodes = addition(p, batch, p.next_id)?;
    let functions = nodes.iter().filter(|n| n.kind == Kind::Function).count();
    let parents: HashSet<_> = nodes
        .iter()
        .filter_map(|n| n.parent)
        .filter(|id| *id < p.next_id)
        .collect();
    Ok(
        serde_json::json!({"project":project,"eligible":true,"nodes":nodes.len(),"functions":functions,"output_files":nodes.len()+functions*2,
        "affected_existing_parents":parents.len(),"first_id":p.next_id,"cpu_workers":WORKERS,"file_workers":WORKERS,"segment_records":SEGMENT,
        "estimated_metadata_bytes":serde_json::to_vec(batch).map_err(|e|e.to_string())?.len()*4}),
    )
}
pub fn reserve(root: &Path, project: &str, batch: &Batch) -> Result<Job> {
    let start = Instant::now();
    let mut lib = library_project(root, project, true)?;
    lib.ensure_structural(0)?;
    lib.check_pool(
        0,
        batch.create.iter().map(|c| c.parent),
        batch.directed_by.as_deref(),
        "Creating a record in",
    )?;
    let before = &lib.projects[0];
    if batch.expect_revision != Some(before.revision) {
        return Err("revision_conflict: reload project show before admission".into());
    }
    let first = before.next_id;
    let nodes = addition(before, batch, first)?;
    let id = uuid::Uuid::new_v4().to_string();
    let folder = job_path(&lib.dir(0), &id)?;
    layout::inside(&lib.dir(0), &folder)?;
    fs::create_dir_all(folder.join("segments")).map_err(|e| e.to_string())?;
    fs::create_dir_all(folder.join("staging")).map_err(|e| e.to_string())?;
    let request = serde_json::to_vec(batch).map_err(|e| e.to_string())?;
    atomic(&folder.join("request.json"), &request)?;
    json(
        &folder.join("reservation.json"),
        &Reservation {
            binary: before.binary.clone(),
            structural_epoch: before.structural_epoch,
            request_hash: sha256_hex(&request),
        },
    )?;
    let mut job = Job {
        warnings: vec![],
        schema: 1,
        id: id.clone(),
        project: project.into(),
        state: JobState::Reserved,
        first_id: first,
        end_id: first + nodes.len() as u64,
        prepared: 0,
        verified: 0,
        total: nodes.len(),
        error: None,
        revision: None,
        timings_ms: BTreeMap::new(),
    };
    checkpoint(&folder, &job)?;
    let catalog = lib.projects[0].clone();
    lib.projects[0].next_id = job.end_id;
    lib.projects[0].active_import = Some(id);
    lib.projects[0].revision += 1;
    lib.commit_catalog(0, catalog)?;
    job.timings_ms
        .insert("reservation_lock".into(), elapsed(start));
    checkpoint(&folder, &job)?;
    Ok(job)
}
pub fn status(root: &Path, project: &str, id: &str) -> Result<Job> {
    if !valid_id(project) {
        return Err("Invalid project ID".into());
    }
    let project_root = root.join(project);
    let folder = job_path(&project_root, id)?;
    layout::inside(&project_root, &folder)?;
    let job: Job = read_json(&folder.join("manifest.json"))?;
    if job.schema != 1 || job.project != project || job.id != id {
        return Err("Invalid import manifest identity".into());
    }
    Ok(job)
}
pub fn request(root: &Path, project: &str, id: &str, cancel: bool) -> Result<Job> {
    let job = status(root, project, id)?;
    if matches!(job.state, JobState::Committed | JobState::Cancelled) {
        return Ok(job);
    }
    let folder = job_path(&root.join(project), id)?;
    atomic(
        &folder.join(if cancel {
            "cancel.request"
        } else {
            "pause.request"
        }),
        b"requested",
    )?;
    match job_lock(&folder) {
        Ok(lease) => {
            drop(lease);
            run(root, project, id)
        }
        Err(e) if e.starts_with("import_busy:") => Ok(job),
        Err(e) => Err(e),
    }
}
pub fn resume_job(root: &Path, project: &str, id: &str) -> Result<Job> {
    status(root, project, id)?;
    let folder = job_path(&root.join(project), id)?;
    let _lease = job_lock(&folder)?;
    if folder.join("pause.request").exists() {
        fs::remove_file(folder.join("pause.request")).map_err(|e| e.to_string())?;
    }
    run_locked(root, project, id, &folder)
}
pub fn run(root: &Path, project: &str, id: &str) -> Result<Job> {
    status(root, project, id)?;
    let folder = job_path(&root.join(project), id)?;
    let _lease = job_lock(&folder)?;
    run_locked(root, project, id, &folder)
}
fn run_locked(root: &Path, project: &str, id: &str, folder: &Path) -> Result<Job> {
    let mut job = status(root, project, id)?;
    if matches!(job.state, JobState::Committed | JobState::Cancelled) {
        return Ok(job);
    }
    if let Err(e) = prepare_and_publish(root, &mut job, folder) {
        job.state = if e.contains("conflict") || e.contains("changed") {
            JobState::Conflict
        } else {
            JobState::Failed
        };
        job.error = Some(e);
        checkpoint(folder, &job)?;
    }
    Ok(job)
}
fn control(root: &Path, job: &mut Job, folder: &Path) -> Result<bool> {
    if folder.join("cancel.request").exists() {
        cancel_preparation(root, job, folder)?;
        return Ok(true);
    }
    if folder.join("pause.request").exists() {
        job.state = JobState::Paused;
        checkpoint(folder, job)?;
        return Ok(true);
    }
    Ok(false)
}
fn prepare_and_publish(root: &Path, job: &mut Job, folder: &Path) -> Result<()> {
    if job.state == JobState::Cancelling {
        return cancel_preparation(root, job, folder);
    }
    let lib = library_project(root, &job.project, false)?;
    if lib.projects[0].last_import.as_deref() == Some(&job.id) {
        *job = status(root, &job.project, &job.id)?;
        return Ok(());
    }
    if lib.projects[0].active_import.as_deref() != Some(&job.id) {
        return Err("reservation_conflict: job is not registered in the catalog".into());
    }
    let reservation: Reservation = read_json(&folder.join("reservation.json"))?;
    let request_bytes = fs::read(folder.join("request.json")).map_err(|e| e.to_string())?;
    if sha256_hex(&request_bytes) != reservation.request_hash {
        return Err("Import request changed".into());
    }
    let batch: Batch = serde_json::from_slice(&request_bytes).map_err(|e| e.to_string())?;
    if lib.projects[0].binary != reservation.binary
        || lib.projects[0].structural_epoch != reservation.structural_epoch
    {
        return Err("structural_conflict: binary or hierarchy changed during preparation".into());
    }
    let mut project = lib.projects[0].clone();
    let new_nodes = addition(&project, &batch, job.first_id)?;
    if new_nodes.len() != job.total || job.end_id != job.first_id + job.total as u64 {
        return Err("Invalid reserved range".into());
    }
    drop(lib);
    if control(root, job, folder)? {
        return Ok(());
    }
    job.error = None;
    job.state = JobState::Preparing;
    checkpoint(folder, job)?;
    let start = Instant::now();
    project.nodes.extend(new_nodes.clone());
    let index = catalog_index::Index::new(&project);
    let mut units: Vec<_> = new_nodes
        .iter()
        .map(|node| Unit {
            node: node.clone(),
            path: index.paths[&node.id].clone(),
            report: index.render(&project, node.id, "", ""),
        })
        .collect();
    units.sort_by_key(|u| {
        (
            match u.node.kind {
                Kind::App => 0,
                Kind::Architecture => 1,
                Kind::Chunk => 2,
                Kind::Function => 3,
            },
            u.node.id,
        )
    });
    let project_root = root.join(&job.project);
    job.timings_ms
        .insert("catalog_preparation".into(), elapsed(start));
    let prepared_result = (|| -> Result<bool> {
        job.prepared = 0;
        for (sequence, chunk) in units.chunks(SEGMENT).enumerate() {
            if control(root, job, folder)? {
                return Ok(false);
            }
            let planned = folder.join("segments").join(format!("{sequence:06}.json"));
            let done = planned.with_extension("done");
            let compute = Instant::now();
            let mut tasks = vec![];
            for part in chunk.chunks(chunk.len().div_ceil(WORKERS)) {
                let part = part.to_vec();
                tasks.push(background::spawn(
                    solworker::SWExecutionClass::Low,
                    move || render_files(part),
                )?);
            }
            let mut prepared = vec![];
            for task in tasks {
                prepared.extend(background::join(task)??);
            }
            *job.timings_ms.entry("cpu_render".into()).or_default() += elapsed(compute);
            let segment = Segment {
                sequence,
                ids: prepared.iter().map(|p| p.id).collect(),
                outputs: prepared
                    .iter()
                    .flat_map(|p| p.files.iter().map(|(o, _)| o.clone()))
                    .collect(),
            };
            if planned.exists() {
                let old: Segment = read_json(&planned)?;
                if serde_json::to_vec(&old).unwrap() != serde_json::to_vec(&segment).unwrap() {
                    return Err("Prepared segment changed".into());
                }
            } else {
                json(&planned, &segment)?;
            }
            if done.exists() {
                let expected = sha256_hex(&fs::read(&planned).map_err(|e| e.to_string())?);
                if fs::read_to_string(&done).map_err(|e| e.to_string())? != expected {
                    return Err("Prepared checkpoint changed".into());
                }
                verify_outputs(&project_root, &segment.outputs)?;
                job.prepared += segment.ids.len();
                continue;
            }
            fault("segment_planned");
            let files = Instant::now();
            for unit in &prepared {
                install_folder(&project_root, folder, job, unit)?;
            }
            std::thread::scope(|scope| -> Result<()> {
                let mut handles = vec![];
                for part in prepared.chunks(prepared.len().div_ceil(WORKERS)) {
                    let project_root = &project_root;
                    let id = &job.id;
                    handles.push(scope.spawn(move || -> Result<()> {
                        for unit in part {
                            for (output, bytes) in &unit.files {
                                write_owned(project_root, id, output, bytes)?;
                            }
                        }
                        Ok(())
                    }));
                }
                for handle in handles {
                    handle.join().map_err(|_| "File provider panicked")??;
                }
                Ok(())
            })?;
            *job.timings_ms.entry("file_preparation".into()).or_default() += elapsed(files);
            atomic(
                &done,
                sha256_hex(&fs::read(&planned).map_err(|e| e.to_string())?).as_bytes(),
            )?;
            job.prepared += segment.ids.len();
            checkpoint(folder, job)?;
            fault("segment_done");
            #[cfg(debug_assertions)]
            if let Ok(ms) = std::env::var("SCOPEWORK_IMPORT_DELAY_MS") {
                std::thread::sleep(std::time::Duration::from_millis(ms.parse().unwrap_or(0)));
            }
        }
        Ok(true)
    })();
    let ready = prepared_result?;
    if !ready {
        return Ok(());
    }
    job.state = JobState::Verifying;
    job.verified = 0;
    checkpoint(folder, job)?;
    let verify_start = Instant::now();
    for sequence in 0..units.len().div_ceil(SEGMENT) {
        if control(root, job, folder)? {
            return Ok(());
        }
        let planned = folder.join("segments").join(format!("{sequence:06}.json"));
        let segment: Segment = read_json(&planned)?;
        verify_outputs(&project_root, &segment.outputs)?;
        job.verified += segment.ids.len();
        checkpoint(folder, job)?;
    }
    job.timings_ms
        .insert("prepared_verification".into(), elapsed(verify_start));
    job.state = JobState::Ready;
    checkpoint(folder, job)?;
    if control(root, job, folder)? {
        return Ok(());
    }
    fault("ready");
    publish(root, job, folder, &reservation, new_nodes)
}
fn render_files(units: Vec<Unit>) -> Result<Vec<Prepared>> {
    units
        .into_iter()
        .map(|u| {
            let folder = u.path.trim_end_matches("/research.md").to_string();
            let bytes = u.report.into_bytes();
            Ok(Prepared {
                id: u.node.id,
                folder,
                files: vec![(
                    Output {
                        path: u.path,
                        hash: sha256_hex(&bytes),
                        bytes: bytes.len(),
                    },
                    bytes,
                )],
            })
        })
        .collect()
}
fn install_folder(root: &Path, workspace: &Path, job: &Job, unit: &Prepared) -> Result<()> {
    let target = root.join(&unit.folder);
    layout::inside(root, &target)?;
    let marker = format!(".scopework-import-{}", job.id);
    if target.exists() {
        if !target.join(&marker).is_file() {
            return Err(format!(
                "path_conflict: {} exists without job ownership",
                unit.folder
            ));
        }
    } else {
        let stage = workspace.join("staging").join(unit.id.to_string());
        fs::create_dir_all(&stage).map_err(|e| e.to_string())?;
        let marker_path = stage.join(&marker);
        if marker_path.exists() {
            if fs::read_to_string(&marker_path).map_err(|e| e.to_string())? != job.id {
                return Err("Staging ownership changed".into());
            }
        } else {
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&marker_path)
                .map_err(|e| e.to_string())?
                .write_all(job.id.as_bytes())
                .map_err(|e| e.to_string())?;
        }
        if !target.parent().ok_or("Missing parent directory")?.is_dir() {
            return Err("Missing committed/prepared parent directory".into());
        }
        rename_retrying(&stage, &target)
            .map_err(|e| format!("Cannot install {}: {e}", target.display()))?;
    }
    Ok(())
}
fn write_owned(root: &Path, job: &str, output: &Output, bytes: &[u8]) -> Result<()> {
    #[cfg(debug_assertions)]
    if std::env::var_os("SCOPEWORK_IMPORT_IO_ERROR").is_some() {
        return Err("Injected file-provider write failure".into());
    }

    let path = root.join(&output.path);
    layout::inside(root, &path)?;
    if path.exists() {
        if sha256_hex(&fs::read(&path).map_err(|e| e.to_string())?) != output.hash {
            return Err(format!("prepared_file_conflict: {}", output.path));
        }
        return Ok(());
    }
    let temporary = path.with_extension(format!("{job}.preparing"));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temporary)
        .map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())?;
    // Closed outputs are recoverable after process failure. Checkpoints are flushed;
    // this protocol does not promise power-loss ordering for prepared data.
    drop(file);
    if path.exists() {
        return Err("prepared_file_conflict: destination appeared during write".into());
    }
    // Install without replacing a destination that appeared after validation.
    fs::hard_link(&temporary, &path).map_err(|e| e.to_string())?;
    fs::remove_file(&temporary).map_err(|e| e.to_string())?;
    Ok(())
}
fn verify_outputs(root: &Path, outputs: &[Output]) -> Result<()> {
    for output in outputs {
        let path = root.join(&output.path);
        layout::inside(root, &path)?;
        let bytes = fs::read(&path).map_err(|e| format!("Prepared file {}: {e}", output.path))?;
        if bytes.len() != output.bytes || sha256_hex(&bytes) != output.hash {
            return Err(format!("prepared_file_conflict: {}", output.path));
        }
    }
    Ok(())
}
fn publish(
    root: &Path,
    job: &mut Job,
    folder: &Path,
    reservation: &Reservation,
    new_nodes: Vec<Node>,
) -> Result<()> {
    let publication = folder.join("publication");
    fs::create_dir_all(&publication).map_err(|e| e.to_string())?;
    let project_root = root.join(&job.project);
    for _attempt in 0..8 {
        let start = Instant::now();
        let lib = library_project(root, &job.project, false)?;
        let before = &lib.projects[0];
        if before.active_import.as_deref() != Some(&job.id)
            || before.structural_epoch != reservation.structural_epoch
            || before.binary != reservation.binary
        {
            return Err("structural_conflict: import reservation no longer matches".into());
        }
        // Pooling a chunk changes no structure, so a pool added since the reservation is
        // checked here, with the direction the request carried.
        let request: Batch = serde_json::from_slice(
            &fs::read(folder.join("request.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let created: HashSet<u64> = new_nodes.iter().map(|n| n.id).collect();
        lib.check_pool(
            0,
            new_nodes
                .iter()
                .filter_map(|n| n.parent)
                .filter(|parent| !created.contains(parent)),
            request.directed_by.as_deref(),
            "Creating a record in",
        )?;
        let before_hash =
            sha256_hex(&fs::read(project_root.join("project.json")).map_err(|e| e.to_string())?);
        let mut after = before.clone();
        after.nodes.extend(new_nodes.clone());
        let index = catalog_index::Index::new(&after);
        let old_index = catalog_index::Index::new(before);
        let parents: HashSet<_> = new_nodes
            .iter()
            .filter_map(|n| n.parent)
            .filter(|id| old_index.nodes.contains_key(id))
            .collect();
        let mut edits = vec![];
        for id in parents.iter().copied() {
            let path = &index.paths[&id];
            let original =
                fs::read_to_string(project_root.join(path)).map_err(|e| e.to_string())?;
            edit_tokens::draft(before, &old_index, id, &original)?;
            let (body, evidence) = edit_tokens::parts(&original)?;
            edits.push(ParentEdit {
                path: path.clone(),
                after: index.render(&after, id, body, evidence),
                before: original,
            });
        }
        drop(index);
        drop(lib);
        // Parents gain source links; complete records they reach need review again, and are
        // queued for it as any other change queues them.
        let mut reopened: Vec<u64> = after
            .nodes
            .iter()
            .filter(|n| parents.contains(&n.id) && n.status == Status::Complete)
            .map(|n| n.id)
            .collect();
        for node in &mut after.nodes {
            if parents.contains(&node.id) {
                reopen::mark_changed(node, None);
            }
        }
        let consumers = reopen::consumers_of(&after.dependencies);
        reopened.extend(reopen::reopen_affected(
            &mut after,
            &parents,
            &HashMap::new(),
            &consumers,
        ));
        after.revision += 1;
        after.structural_epoch += 1;
        after.active_import = None;
        after.last_import = Some(job.id.clone());
        let bytes = catalog_bytes(&after)?;
        let candidate = format!("candidate-{}.json", uuid::Uuid::new_v4());
        atomic(&publication.join(&candidate), &bytes)?;
        let mut journal = Publication {
            before_hash,
            after_hash: sha256_hex(&bytes),
            catalog: candidate,
            edits,
        };
        *job.timings_ms
            .entry("publication_candidate".into())
            .or_default() += elapsed(start);
        let lock_start = Instant::now();
        let _lease = Library::commit_lease(root, &job.project)?;
        *job.timings_ms.entry("publication_wait".into()).or_default() += elapsed(lock_start);
        let hold = Instant::now();
        // A catalog transaction left pending by a process that died since the read above would
        // be applied after this publication and conflict with it: read again, which recovers it.
        let pending = ["catalog-transactions-v4", "catalog-transactions"]
            .iter()
            .any(|folder| {
                project_root
                    .join("tools/workspaces")
                    .join(folder)
                    .join("pending/journal.json")
                    .is_file()
            });
        if pending
            || sha256_hex(&fs::read(project_root.join("project.json")).map_err(|e| e.to_string())?)
                != journal.before_hash
        {
            // This attempt's candidate is never published: do not leave it behind.
            let _ = fs::remove_file(publication.join(&journal.catalog));
            continue;
        }
        for edit in &journal.edits {
            if fs::read_to_string(project_root.join(&edit.path)).map_err(|e| e.to_string())?
                != edit.before
            {
                return Err(
                    "report_conflict: parent changed outside the app during publication".into(),
                );
            }
        }
        if !reopened.is_empty() {
            // Read under the commit lease, so the queue cannot change before recovery applies it.
            let original =
                coordination::ensure(&project_root, coordination::REVIEW_QUEUE, "entries")?;
            let updated = coordination::queue_reopened(
                &original,
                &after,
                &reopened,
                &format!("Reopened by import {} adding records", job.id),
            )?;
            if updated != original {
                journal.edits.push(ParentEdit {
                    path: coordination::REVIEW_QUEUE.into(),
                    before: original,
                    after: updated,
                });
            }
        }
        job.state = JobState::Publishing;
        checkpoint(folder, job)?;
        json(&publication.join("pending.json"), &journal)?;
        fault("publication_journal");
        let mut current = after;
        recover(&project_root, &mut current)?;
        *job = status(root, &job.project, &job.id)?;
        job.timings_ms
            .insert("publication_lock".into(), elapsed(hold));
        checkpoint(folder, job)?;
        return Ok(());
    }
    Err("publication_contention: preparation retained after eight changed snapshots; resume when writes settle".into())
}
pub(crate) fn needs_recovery(root: &Path, project: &Project) -> bool {
    project
        .active_import
        .as_ref()
        .or(project.last_import.as_ref())
        .and_then(|id| job_path(root, id).ok())
        .is_some_and(|folder| folder.join("publication/pending.json").is_file())
}
pub(super) fn recover(root: &Path, project: &mut Project) -> Result<()> {
    let Some(id) = project
        .active_import
        .as_ref()
        .or(project.last_import.as_ref())
        .cloned()
    else {
        return Ok(());
    };
    let folder = job_path(root, &id)?;
    let pending = folder.join("publication/pending.json");
    if !pending.is_file() {
        return Ok(());
    }
    layout::inside(root, &pending)?;
    let journal: Publication = read_json(&pending)?;
    let catalog = folder.join("publication").join(&journal.catalog);
    layout::inside(root, &catalog)?;
    let bytes = fs::read(&catalog).map_err(|e| e.to_string())?;
    if sha256_hex(&bytes) != journal.after_hash {
        return Err("Recovery catalog changed".into());
    }
    let current = sha256_hex(&fs::read(root.join("project.json")).map_err(|e| e.to_string())?);
    if current != journal.before_hash && current != journal.after_hash {
        return Err("recovery_conflict: catalog differs from publication inputs".into());
    }
    for edit in &journal.edits {
        let path = root.join(&edit.path);
        layout::inside(root, &path)?;
        let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        if text != edit.before && text != edit.after {
            return Err(format!("recovery_conflict: {} changed", edit.path));
        }
    }
    for edit in &journal.edits {
        atomic(&root.join(&edit.path), edit.after.as_bytes())?;
    }
    fault("parents_published");
    if current != journal.after_hash {
        atomic(&root.join("project.json"), &bytes)?;
    }
    fault("catalog_published");
    if project.last_import.as_deref() != Some(&id) || project.active_import.is_some() {
        *project = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    }
    let mut job: Job = read_json(&folder.join("manifest.json"))?;
    // The catalog owns the installed folders now; ownership markers only served a resume or a
    // cancel, which a published job no longer allows.
    let leftover = remove_markers(root, &folder, &id);
    job.warnings.extend(leftover);
    job.state = JobState::Committed;
    job.prepared = job.total;
    job.revision = Some(project.revision);
    job.error = None;
    checkpoint(&folder, &job)?;
    json(&folder.join("receipt.json"), &job)?;
    let committed = folder.join("publication/committed.json");
    rename_retrying(&pending, &committed)
        .map_err(|e| format!("Cannot replace {}: {e}", committed.display()))?;
    Ok(())
}
/// Removes job `id`'s ownership markers from every folder its segments installed. Returns a
/// warning for each marker that could not be removed; `project tidy` removes them later.
fn remove_markers(project_root: &Path, folder: &Path, id: &str) -> Vec<String> {
    let marker = format!(".scopework-import-{id}");
    let mut folders = HashSet::new();
    if let Ok(entries) = fs::read_dir(folder.join("segments")) {
        for path in entries.flatten().map(|e| e.path()) {
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            if let Ok(segment) = read_json::<Segment>(&path) {
                for output in segment.outputs {
                    if let Some(parent) = Path::new(&output.path).parent() {
                        folders.insert(parent.to_path_buf());
                    }
                }
            }
        }
    }
    let mut warnings = vec![];
    for relative in folders {
        let path = project_root.join(&relative).join(&marker);
        if path.is_file()
            && let Err(e) = fs::remove_file(&path)
        {
            warnings.push(format!("Kept import marker {}: {e}", path.display()));
        }
    }
    warnings.sort();
    warnings
}
fn cancel_preparation(root: &Path, job: &mut Job, folder: &Path) -> Result<()> {
    job.state = JobState::Cancelling;
    checkpoint(folder, job)?;
    let project_root = root.join(&job.project);
    let mut folders = HashSet::new();
    for entry in fs::read_dir(folder.join("segments")).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let segment: Segment = read_json(&path)?;
        for output in &segment.outputs {
            let target = project_root.join(&output.path);
            layout::inside(&project_root, &target)?;
            if !target
                .parent()
                .unwrap()
                .join(format!(".scopework-import-{}", job.id))
                .is_file()
            {
                if target.exists() {
                    job.warnings
                        .push(format!("Preserved unowned {}", output.path));
                }
                continue;
            }
            if target.exists() {
                if sha256_hex(&fs::read(&target).map_err(|e| e.to_string())?) != output.hash {
                    job.warnings
                        .push(format!("Preserved changed {}", output.path));
                    folders.insert(target.parent().unwrap().to_path_buf());
                    continue;
                }
                fs::remove_file(&target).map_err(|e| e.to_string())?;
            }
            let temp = target.with_extension(format!("{}.preparing", job.id));
            if temp.exists() {
                fs::remove_file(temp).map_err(|e| e.to_string())?;
            }
            folders.insert(target.parent().unwrap().to_path_buf());
        }
    }
    let mut folders: Vec<_> = folders.into_iter().collect();
    folders.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    for path in folders {
        let marker = path.join(format!(".scopework-import-{}", job.id));
        if marker.exists() {
            fs::remove_file(marker).map_err(|e| e.to_string())?;
        }
        let _ = fs::remove_dir(path.join("data"));
        if fs::remove_dir(&path).is_err() {
            job.warnings
                .push(format!("Preserved remaining files in {}", path.display()));
        }
    }
    checkpoint(folder, job)?;
    let mut lib = library_project(root, &job.project, true)?;
    if lib.projects[0].active_import.as_deref() == Some(&job.id) {
        json(
            &folder.join("cancel-release.json"),
            &serde_json::json!({"revision":lib.projects[0].revision+1,"end_id":job.end_id}),
        )?;
        let catalog = lib.projects[0].clone();
        lib.projects[0].active_import = None;
        lib.projects[0].revision += 1;
        lib.commit_catalog(0, catalog)?;
        fault("cancel_released");
    } else {
        let release: serde_json::Value = read_json(&folder.join("cancel-release.json"))?;
        if lib.projects[0].revision
            < release["revision"]
                .as_u64()
                .ok_or("Invalid cancellation receipt")?
            || lib.projects[0].next_id < job.end_id
        {
            return Err("reservation_conflict during cancellation".into());
        }
    }
    job.state = JobState::Cancelled;
    job.error = None;
    job.warnings.sort();
    job.warnings.dedup();
    checkpoint(folder, job)?;
    json(&folder.join("receipt.json"), job)
}
fn fault(_point: &str) {
    #[cfg(debug_assertions)]
    if std::env::var("SCOPEWORK_IMPORT_FAIL_AT").as_deref() == Ok(_point) {
        std::process::exit(99);
    }
}
