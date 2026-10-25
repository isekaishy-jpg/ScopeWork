# Research: storage backend architecture (end to end)

Status: research, 2026-10-22. TODO item "Storage backend". This is the code-level companion to [research-storage-backend.md](research-storage-backend.md): it traces how Scopework stores, reads and commits today, module by module and operation by operation, and checks the history already on disk. The aim is to find where a backend interface can sit and where a side-by-side backend can attach. It decides nothing; the design step does.

Sources: the source tree at `f633ae6` (19,405 lines in `src/`), the projects in the research home, and the coordinator's own commit measurements in `3-3-5a`'s tooling log.

## 1. Layers as they are

| Layer | Modules | Role |
|---|---|---|
| Frontends | `cli.rs` (parse, `route`, `execute`); `coordinator/` (named-pipe server, queue, snapshots, leases, drift); GUI: `main.rs`, `records_ui.rs`, `background.rs`, `history_ui.rs` | Accept requests and present answers |
| Domain | `Library` methods in `research.rs` (`save_report`, `set_dependencies`, `set_binary`, `completion_blockers`), `catalog_batch.rs` (`save_reports`, `apply_batch`), `catalog_remove.rs`, `quarantine.rs`, `coordination.rs` (claims, review queue), `reopen.rs`, `edit_tokens.rs`, `catalog_index.rs` (status, report rendering) | Validate and build the next state |
| Commit | `operations.rs` (`commit_or_stage`, staging), `catalog_batch.rs` (`commit`, `finish`, `resume`), `journal_history.rs` (delta, fingerprints, receipt maintenance) | Turn a before/after pair into files, atomically |
| Other writers | `imports.rs` (reservation, publication), `layout.rs` (migration), `Library::persist` (`set_scope`, import reservation), `Library::create`, `history.rs` (`append_history`), `ledger.rs` (re-path and re-pin inside `finish`) | Write outside the commit function |
| Read helpers | `operations.rs` (`text`, `text_opt`, overlay), `report_hashes.rs` (hash and reference caches), `history.rs` (receipt scan) | Read files, through an overlay where needed |
| Locks | `research.rs` (`acquire_locks`, `.project-locks`, `.library.lock`, holder and waiter files) | One writer per project; readers share |

`Library` holds the domain and the storage together:

```text
Library { root, projects: Vec<Project>, busy_projects, recovery_blocked,
          overlay: Option<Overlay>, staging: Option<Staged>, _lock: Option<OperationLocks> }
```

Every domain method works on the in-memory `Project`, reads files through `self.text`, and ends by building a `Journal` (catalog before and after, plus file edits) that goes to `commit_or_stage`. The domain therefore already hands storage one value per change. Storage is not behind an interface, though: methods also derive paths, read reports and stat files themselves.

## 2. Core types and what is implicit in them

**Catalog** (`project.json`, serialised with `serde_json::to_vec`):

```text
Project { layout_version, id, name, binary, research_scope, next_id, revision,
          structural_epoch, active_import, last_import, nodes: Vec<Node>, dependencies: Vec<Dependency> }
Node    { context_only, id, parent, kind, name, status, rva, revision,
          accepted_hash, accepted_prerequisites: BTreeMap<u64, "revision:sha256"> }
Dependency { dependent, prerequisite }
```

Facts the types do not state:

- **Sibling order is the position in `nodes`.** No field holds it. The receipt diff (`record_changes`) treats a record as changed when the common records are reordered. A backend needs an explicit order key.
- **Report paths are derived, not stored.** `App` → `app.md`, an architecture → `architectures/<id:06>/research.md`, a chunk → its parent's folder plus `<id:06>`, a function → its parent's folder plus `<RVA:08X>`. The rule exists twice: `research.rs::relative` and `catalog_index::Index::new`. Moving a record moves its folder and everything under it.
- **Effective status is computed, not stored.** `catalog_index::state` combines the stored status, the children, the prerequisites, quarantine and **the SHA-256 of each report file on disk**. A complete record stays complete only while its report's hash equals `accepted_hash`, and its prerequisite stamps (`revision:sha256`) still match. An edit outside Scopework therefore reopens records. That is intended, and it depends on reading the files.
- **Report hashes are cached.** `report_hashes.rs` keeps an in-memory table per thread (Solcache) and a persisted copy (`scopework-cache/report-hashes.json`, 480 KB in `3-3-5a`). A hash is reused while size and modification time are unchanged, using git's racy-timestamp rule (2 s). Report references (`report-references-v2.json`, 5.5 MB) use the same rule and find the reports a move must rewrite.
- **A report file is a generated header plus research text.** `Index::render` writes the title, project name and id, the binary identity text, kind, id, RVA, and a "Source reports" list linking every child. The research body and evidence follow after `<!-- research-content -->` and `<!-- research-evidence -->`. **`accepted_hash` covers the whole file, header included.** So:
  - Renaming or moving a child rewrites its parent's report. Batches carry acceptance across such "mechanical" rewrites by re-pinning `accepted_hash` and prerequisite stamps from the old hash to the new one (`catalog_batch.rs` around line 1270).
  - `project set-binary` re-renders **every** report and re-pins every hash in one transaction, whose journal holds every report's text before and after.
  - The annotation ledger pins rows to the same whole-file hash (`report_sha256`), so mechanical rewrites also re-pin ledger rows (`repin_ledger`).

**Transaction** (`catalog_batch::Journal`): `before`, `after` (whole catalogs in memory), `edits` (report path old and new, text before and after), `moves`, `dependencies` (records whose views to regenerate), `sidecars` (path, text before and after), `ledger` / `repin_ledger` / `repin_ids`, `directed_by`.

**What is written for a transaction** (`journal_history::delta_with_after`): the header fields before and after (everything but the arrays), the changed nodes and edges as whole records before and after, `before_hash` and `after_hash` (SHA-256 of the full serialised catalog), and the edits, moves, sidecars and ledger flags as given. The pending journal and the completed receipt have this shape. **The receipt is already a hash-chained change record**; section 5 checks the chain on disk.

## 3. Operations end to end

### 3.1 Draft save through the coordinator (the common case)

1. **Client:** `research node save` connects to the pipe and sends its arguments. `cli::route` classifies it: `Write`, kind `save`, nodes `[id]`, `merge: true`, bytes = size of the body and evidence files.
2. **Queue:** the operation waits in `ProjectState::queue`. `select` takes the most urgent operation, plus queued mergeable ones up to 256 operations or 8 MB.
3. **Writer** (`Server::apply`):
   - Reuses the `Resident` library if it is under 60 s old and the `project.json` stamp is unchanged. A stamp younger than 2 s is confirmed by comparing bytes. Otherwise it reopens with `Library::open_project` (exclusive lock, recovery).
   - Raises the `writing` flag.
   - Publishes any other process's commit first.
   - Runs the batch inside `with_commit_observer`. A single unmergeable operation runs alone; several run in staging mode (`begin_staging`, `stage_one`, `commit_staged`).
4. **Domain** (`Library::save_report`):
   - Checks the edit token, which names the report text the agent last read.
   - Builds a `ReportBatch` and calls `save_reports`, which renders the report and builds a `Journal`, then `commit_or_stage`.
5. **Commit** (`catalog_batch::commit`):
   - (a) `pending_delta` serialises the whole after-catalog and hashes it. The before-catalog's hash is reused from the previous commit when that catalog is equal.
   - (b) The delta is written atomically to `catalog-transactions-v4/pending/journal.json`. **This is the commit point.**
   - (c) `operations::before_commit` hands the commit observer the previous text of every file about to change. The coordinator inserts them into every live snapshot's overlay (`record_previous`).
   - (d) `finish` writes each report and sidecar atomically after checking it still equals its before or after text. It moves folders, re-paths and re-pins the ledger, regenerates dependency views, writes `project.json` atomically, renames `pending` to `revision-N`, and rewrites the journal as the compact receipt with `at_unix`.
   - (e) If an apply step fails, it is retried for up to 20 s before answering `commit_pending`.
6. **After:** the coordinator publishes a new snapshot, keeps the library as `Resident`, and replies.

**Measured** at `3-3-5a`'s size (6.4 MB catalog), from the coordinator's `coordinator-commits` entries on two days (513 and 112 commits, every batch one operation, the resident library reused 99% of the time):

| Stage | Median | p90 |
|---|---|---|
| Open or resume the library | 16–17 ms | 51–86 ms |
| Build the delta (serialise and hash the whole catalog, diff 31,606 records) | 53–57 ms | 77–87 ms |
| Write the pending journal | 4.3–4.7 ms | 6.4–8.8 ms |
| Apply (report, receipt, whole `project.json`, flushes) | 39–87 ms | 56–117 ms |

About half of every commit's time goes to building the delta, which scales with the whole catalog rather than with the change. At `forever`'s size (40 MB, 150,818 records) the delta and the `project.json` rewrite grow about 6.5 times.

### 3.2 Completion

Same path. `completion_states` evaluates effective status over the record's closure, hashing every completed report it reaches. The new `accepted_hash` is the hash of the rendered report, the prerequisite stamps record each prerequisite's `revision:sha256`, and `reopen_affected` walks the records that depend on this one.

### 3.3 Reads

- **Through the coordinator** (`Server::read`): take the current snapshot. If `project.json`'s stamp changed, another process committed, so load a fresh one. If no other process is mid-commit (`external_activity`: a pending journal, an import publication in flight, or a changed stamp), answer from `Library::snapshot_view(project, overlay)` without any lock. Then check again: if outside activity overlapped the read, answer again under the shared lock. Answers carry the snapshot's `revision` and pending operations.
- **Direct mode** (`Library::read_project`): takes the project's **shared lock for the whole life of the library**, so a direct reader holds back writers until it finishes. A reader that finds a pending journal rolls it forward first, or answers from the committed catalog when recovery is blocked.
- **Project tools:** read `project.json` directly (14 tools, `load_project`), without a lock.

### 3.4 Structural batches, deletes, binary amendment

`node batch`, `node delete`/`prune`, `deps set`, `quarantine` and `set-binary` never merge (`merge: false`, enforced again in `commit_or_stage`). They read report texts straight from disk, which is safe because they commit alone. A batch rewrites:

- the reports of changed records;
- reports that mention a moved path (found through the references cache);
- reports that cite a renamed record.

It moves folders, re-paths the ledger, and carries acceptance over for mechanical rewrites. `apply_batch` and `catalog_remove` call `catalog_batch::commit` directly; everything else goes through `commit_or_stage`. **Every production catalog transaction ends in `catalog_batch::commit`.** There are eight `Journal` builders: `catalog_batch.rs` lines 546, 585, 702, 820 and 1342, `catalog_remove.rs` 398, `quarantine.rs` 289, and the staged merge in `operations.rs`.

### 3.5 Writes outside that function

| Writer | What it writes | Receipt in `catalog-transactions-v4` |
|---|---|---|
| `Library::set_scope` | `project.json` via `persist` (revision + 1) | **None.** The change is in no history |
| Import reservation (`imports.rs` line 257) | `project.json` via `persist` (`next_id`, `active_import`, revision + 1) | None |
| Import publication (`imports.rs` around line 700) | Parent reports and `project.json` from its own `publication/pending.json` (with before/after hashes); renamed to `committed.json`; `receipt.json` in the job folder | No; the job folder keeps it |
| Layout migration (`layout.rs`) | Moves, views and `project.json`, with its own journal and backups | None |
| `Library::create` | A staged project folder, renamed into place | None |
| `append_history` | One line appended to `tools/workspaces/history/tooling.jsonl`, then `sync_all` | n/a (separate log) |
| Project tools | `tools/profiles/annotations.json` (50 MB), whole-file rewrites, under a coordinator lease | n/a; drift detection notices outside writes |
| Caches | `scopework-cache/*.json` (best effort) | n/a |

### 3.6 The GUI

- Every 2 s, `background::snapshot` builds a fingerprint by statting every `project.json` **and every complete record's report in every project**. If it changed, it re-reads every project's catalog under the shared lock, releases the lock, computes the meters (hashing reports through the cache), and then reads the flags, review-queue and quarantine sidecars. The sidecars are read after the lock is released, so the GUI can pair a catalog at one revision with sidecars from a later one. This affects only what is displayed.
- Record loads use `read_project`. Saves use `open_project` with a 60 s lock wait, in their own thread, with no coordinator.

### 3.7 History queries

`Library::history`:

- lists every `revision-*` folder in `catalog-transactions-v4` and the older `catalog-transactions`, statting each journal;
- sorts them newest first, filters by revision range and **by the journal file's modification time** (not the `at_unix` inside it), then parses receipts until the limit is reached (texts are skipped while parsing);
- reads the tooling log whole.

A filter on a single record can parse every receipt (76,390 in the old project).

## 4. Locks and snapshots

- Files: `.library.lock` (legacy gate, shared by current builds, exclusive for older writers and migration); `.project-locks/<id>.lock` (exclusive for a writer, shared for readers); `<id>.holder` (who writes); waiter files for queue display.
- The coordinator's readers take no lock. Consistency rests on three things:
  - the overlay, which holds the previous texts recorded before each commit changes a file;
  - the `project.json` stamp;
  - the `external` epoch counter.

  Snapshots that readers still hold stay reachable through `live`, so a long read keeps seeing the texts it started with.
- Anything outside the coordinator (GUI, direct CLI, older builds) is detected only through stamps and pending journals, and answered under the lock.

For a backend: if commits are appended to a log, a reader's snapshot is simply "the log up to the last complete commit record". Readers then need no lock and no overlay for the catalog. Report texts stay files, though. A snapshot that must answer with an older report text still needs that text from somewhere: the overlay today, or the backend's text store by hash.

## 5. History on disk, checked

Each receipt's `before.revision` and `before_hash` were compared with the previous receipt's `after` values, across every project:

| Project | Receipts | Range | Revision chain | Hash chain | Breaks explained |
|---|---|---|---|---|---|
| `3-3-5a` | 3,381 | 7–3,387 | Unbroken | Unbroken | Receipt 7 links by hash to the import published at revision 6. Revisions 1–6 are three imports, each a reservation plus a publication |
| `forever` | 418 (+1 old) | 4,555–4,973 | 1 break | 1 break | 4,557 was a `set_scope` (scope "Rendering, GPU, Asset Platform" to "Whole executable"). Revisions before 4,555 have no receipts except 4,388 |
| `populous-amiga` | 542 | 2–547 | 2 breaks | 2 breaks | Both are imports (revisions 4 and 11); each links by hash to its publication |
| `populous-snes` | 40 | 4–45 | 1 break | 1 break | Import at revision 6, linked by hash |
| `wow-335a-12340` | 76,390 | 1,210–85,061 | Unbroken | 1 break | 1,491 → 1,492 is the layout v4 → v5 migration: the catalog bytes changed while the revision did not, and no receipt was left |

Findings:

- **A receipt is one commit, not one revision.** Every operation bumps the revision, so a merged commit advances it by its operation count. The old project has 493 merged commits of up to 24 operations; for example, receipt 50,488 runs from 50,472 to 50,488. `3-3-5a` has none, since its commits have all been single operations.
- **The receipts, the import publications and the before/after hashes form a verifiable chain.** It breaks only where a writer leaves no receipt: `set_scope`, import reservations (the next receipt still links by hash), and layout migrations.
- **Times:** receipts written since `at_unix` was added carry it, and in `3-3-5a` it agrees with the file's modification time within 1 s on a sample of 68. The old project's early receipts have no `at_unix`, so dated queries there rely on modification times alone. Receipt 1,491's modification time is later than 1,492's, so at least one receipt was rewritten later. A backend should carry the time inside each record.

## 6. The annotation ledger in detail (`3-3-5a`)

| Measure | Value |
|---|---|
| File | 49.9 MB; zlib to 3.2 MB (6.4%) |
| Rows | 71,252, unique by `(rva, kind)`: 17,441 function signatures, 16,803 function names, 16,201 data definitions, 12,964 plate comments, 6,894 labels, 920 data types, 29 no-return marks |
| Types | 992 (956 structures, 36 enumerations) |
| Bytes by field | `reason` 39%, `value` 23%, `report_sha256` 13%, `report` 10%, `catalog_move` 4%, `kind` 3%, `classification` 3%, `rva` 2% |
| Reasons | 10,703 distinct; the most repeated one appears 21,515 times (the carry-over note) |
| Links | 42,548 distinct addresses; 47,003 rows at function entries; 23,509 distinct reports referenced |
| History | Effectively none. The tooling log holds one `ledger-change` and one `drift` entry. Every tool write replaces the 50 MB file |

`(item, kind)` is therefore a sound key for item annotations. Each row's link to a report pins the whole-file hash, which is why mechanical report rewrites must re-pin ledger rows.

## 7. Reads that bypass the overlay, verified

Earlier notes flagged two direct reads as possible stale reads inside merged commits. Checked:

- `coordination::open_queue` and `quarantine::read_records` are called only from `background::snapshot` (the GUI), never inside a commit. Not a bug; the GUI's torn read (section 3.6) is the only effect.
- `catalog_batch.rs` 404 and 1121, `catalog_remove.rs` 267 and 386, and `research.rs` 836 run in `finish` or in operations that never merge. `imports.rs` 650 and 695 run under the import's own lock. All are safe.

The rule that merged operations must read through `Library::text` holds today, but nothing enforces it. A storage interface would make it structural.

## 8. Where an interface can sit

What the code already offers:

1. **One value per change:** `Journal`. Every domain operation ends in one.
2. **One commit function for catalog transactions:** `catalog_batch::commit`. Five writers (section 3.5) sit outside it and would have to be brought under it or given their own record types.
3. **One read funnel for texts:** `Library::text` / `text_opt`, with an overlay for snapshots and staging.
4. **One hook before files change:** `operations::before_commit` / `with_commit_observer`.
5. **A change format with a hash chain:** the receipt (section 5).

Interface candidates derived from the operations traced above:

| Operation | Today | Callers |
|---|---|---|
| Open a snapshot at the latest revision (or a given one) | `read_project`, `snapshot_view`, `Server::snapshot` | Reads, GUI, coordinator |
| Open for writing (exclusive) with recovery | `open_project`, `Resident::resume` | Writers |
| Commit a transaction, with a definite outcome (`committed`, `commit_pending`, failed) | `catalog_batch::commit` via `commit_or_stage` | All catalog writers |
| Stage several transactions into one commit | `begin_staging`, `stage_one`, `commit_staged` | Coordinator |
| Text by path at the snapshot's revision | `text_opt` and the overlay | Domain |
| Report hash by path, cached | `report_hashes::hashes` | Status, meters |
| History by revision range, time, record or kind | `Library::history` (receipt scan) | CLI, GUI |
| Append a tooling entry | `append_history` | Coordinator, CLI, tools |
| Text by hash (history, older snapshots) | Report texts inside receipts | History, snapshots (new) |
| Item annotations by item and kind | The tools' ledger file | Tools; Scopework re-path/re-pin |

Report files stay files (decided). Effective status keeps reading their hashes from disk, so outside edits keep reopening records. The backend stores accepted hashes and history, not the current hash.

## 9. Where a side-by-side backend can attach

| Option | How | Covers | Misses or risks |
|---|---|---|---|
| A. In-process write at commit | After `finish` succeeds in `catalog_batch::commit`, append the same delta to the new backend. Add calls in import publication, `set_scope`, import reservation, layout migration, `create` and `append_history` | Every write by this build, at the moment it happens | Older Scopework builds, which keep writing files only. A crash between `finish` and the append leaves the new backend one commit behind, so it needs a catch-up anyway |
| B. Follower of the receipt chain | The new backend tails `catalog-transactions-v4` (and import `committed.json` files), applies each receipt whose `before_hash` equals its own last `after_hash`, and tails `tooling.jsonl` | Every writer that leaves a receipt, including older builds, direct mode and the GUI. It never touches the authoritative commit path | Receipt-less writes (`set_scope`, reservations, migrations) show up as hash breaks. It needs a re-sync from `project.json` there, and the history it records then has a gap. The ledger needs the drift watch or a lease hook |
| C. Both | A writes; B checks the chain and catches up | Fresh writes plus self-healing | Two paths to maintain |

Either way:

- **The comparison** is cheap and exact at the catalog level: the new backend's catalog at a revision, serialised as today, must hash to that receipt's `after_hash`.
- **Failure isolation:** the new backend's errors and panics must not reach the commit. The coordinator already isolates writer panics with `catch_unwind`. Errors become complaints.
- **The receipt-less writes** are worth fixing whatever the design: they are also missing from `history show` today.

## 10. Costs the current layout imposes (measured or counted)

| Cost | Where | Scale |
|---|---|---|
| Serialise and hash the whole catalog per commit | `delta_with_after` | 53–57 ms median at 6.4 MB; grows with catalog size, not change size |
| Rewrite the whole `project.json` per commit | `finish` | 6.4 MB (`3-3-5a`), 40 MB (`forever`) |
| Re-read every catalog when anything changes | GUI snapshot | Every 2 s poll that sees a change; `forever` alone is 40 MB to parse |
| Stat every complete report in every project | GUI fingerprint | Every 2 s |
| Scan every receipt for a filtered history query | `Library::history` | Up to 76,390 files |
| Rewrite the 50 MB ledger per annotation write | Project tools | Each tool write; no history kept |
| Full report texts before and after in each receipt | Receipts | 70% of `3-3-5a`'s 30 MB of history; half duplicates |

## 11. Found along the way (worth fixing regardless)

1. `set_scope` and import reservations change the catalog without a receipt, so history never shows scope changes. `forever` revision 4,557 is one.
2. Layout migration changes the catalog without a receipt or a revision bump (old project, 1,491 → 1,492).
3. History date filters use the journal file's modification time instead of the receipt's `at_unix`.
4. The report-path rule is written twice (`research.rs::relative`, `catalog_index::Index::new`).
5. The GUI reads sidecars after releasing the lock, so it can show sidecars from a later revision than its catalog.

## 12. Questions this adds for the design step

Answered and sorted on 2026-10-25 in [the backend doc's §10](research-storage-backend.md#answers-and-triage-2026-10-25): item 3 is still the user's to decide; the others are research questions. Item 5 is mostly done: scope changes and import reservations now leave receipts, and only layout migration still does not.

1. **The interface's cut:** a store trait under `Library` (snapshot, transaction, texts, history, tooling log), or a narrower one at `catalog_batch::commit` first.
2. **Side by side:** in-process write, receipt follower, or both (section 9).
3. **Acceptance hash scope:** keep hashing the whole rendered report, or hash only the research body and evidence. The second would end re-pinning after renames, moves and binary amendments, for the catalog and the ledger alike. It changes what "accepted" means, and every existing hash would need converting once.
4. **Order:** an explicit sibling order key in place of vector position.
5. **Receipt-less writers:** fold `set_scope`, reservations and migrations into ordinary transactions before side by side starts, so the chain is complete.
6. **Readers without locks:** once the catalog is a log, whether direct-mode readers stop taking the shared lock. Today they hold back writers while they read.
7. **GUI change detection:** watch the log's end instead of statting every complete report every 2 s.
