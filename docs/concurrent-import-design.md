# Concurrent imports and responsive project work

Status: implementation contract for the 0.5.0 upgrade. See the CLI guide for implemented commands and release qualification notes for measurements. Baseline: Scopework 0.4.5, commit `56c2b7d`. Date: 2026-10-14.

## Outcome

An agent can import a whole binary while other agents read and edit existing reports in that same project. The GUI continues to respond and shows the last committed inventory plus explicit import progress. Independent report edits do not invalidate one another. An interrupted import resumes without losing committed research, duplicating functions, or importing completion claims.

Preserve the existing dashboard design, canonical hierarchy, nested report/data paths, exact binary isolation, explicit context membership, prerequisite rules, and manual completion attestations. Keep the application in Rust. No changes to supplied executables, archives, or research evidence are required.

## Evidence and limits

Forever's existing `tools/logs/full-enrollment-cli.json` records 471.344 seconds for revision 4388. Its retained transaction journal contains 149,357 new records, 149,463 report edits, 149,354 dependency-file pairs, zero moves, and zero sidecars. The journal occupies 107,845,785 bytes. New or changed report/dependency outputs account for approximately 448,171 file writes, excluding temporary-file renames and catalog/journal publication.

In the baseline, `Library::open_project` holds an exclusive project lock throughout the CLI operation. `Library::batch` builds catalogs and report text, writes the journal, and invokes recovery/publication while retaining that lock. `resume` reads the journal back, checks paths and report contents, writes reports and dependency views, then commits the catalog. GUI polling also rebuilds paths and loads changed catalogs on the UI thread. Report tokens include the global project revision.

These are source and receipt observations, not phase timing measurements. Neither a CPU bottleneck nor a particular parallel speedup has been established. Measure lock occupancy, UI stalls and edit latency separately from total import time.

## Decisions

1. Implement a specialized additive import pipeline before general concurrent reorganization.
2. Use Solworker for bounded CPU preparation, background indexing, task dependencies and owner publication. The application supplies filesystem services, durable jobs, cross-process locks and conflict semantics.
3. Permit one active structural job per project. Existing report reads, draft saves, reopening and valid completion attestations remain available during additive preparation. Structural mutations wait or return an explicit structural-job conflict.
4. Keep canonical files at their existing paths. Prepare new, unpublished records at their reserved final paths under a durable ownership manifest; they remain absent from the committed catalog until publication.
5. Serialize commits, not entire jobs. Use shared read leases and short exclusive publication windows.
6. Replace project-wide draft conflicts with record-specific validation. Completion has additional input validation.
7. Use immutable, retained GUI snapshots and generation-checked publication. Adopt the relevant Stack Threads lifetime/freshness principles without adding its Python prototype or a second scheduler in this upgrade.
8. Do not introduce a permanently running service. The GUI and CLI use the same library and disk coordination protocol. A job runner owns its runtime and can be restarted.

## Storage and identity

Introduce catalog format 4. Retain the monotonically increasing project revision for change notification and audit. Add a structural epoch and explicit node revisions for authored content, effective research state, and structural context. The exact field decomposition should avoid redundant counters; the contracts below are mandatory.

Node IDs and function RVAs retain their existing meaning. An import reserves a contiguous node-ID range by advancing `next_id` under an exclusive lock. Aborted ranges are never reused. A reservation changes the project revision, but does not create visible nodes or alter completion. It must not invalidate unrelated format-4 draft tokens.

Store job state under:

```text
tools/workspaces/import-jobs/<job-id>/
  request.json              # immutable normalized request and source digest
  reservation.json          # exact binary, ID range, base structural epoch
  manifest.json             # durable state and prepared-segment references
  segments/<sequence>.json  # owned output paths, expected hashes and sizes
  progress.json             # replaceable diagnostic snapshot
  publication/              # commit inputs, before-images and recovery record
  receipt.json              # final committed IDs, revision and counts
```

Job IDs are opaque random identifiers, never inferred from a PID. These files are operational records, not parallel research reports. Supporting evidence remains in each function's canonical `data/` directory. The request cannot supply acceptance hashes, completed statuses, or authored research bodies.

New files use exclusive creation and job-specific temporary names. Refuse pre-existing target files or unexpected directory contents; never adopt or overwrite them automatically. Canonical folder roots and filesystem containment are validated before work and again at publication, retaining protection against reparse points and path escapes. Cache validated path prefixes only within a lease that makes the assumptions valid.

A fresh catalog cannot reference prepared files until their segment is recorded as complete. Raw directory enumeration may see uncommitted folders; supported app/CLI inventory reads use only the committed catalog. Documentation must distinguish these states.

## Job state machine

```text
Planned -> Reserved -> Preparing -> Ready -> Publishing -> Committed
                         |           |
                         +-> Paused <-+
                         +-> Conflict
                         +-> Failed
Reserved / Preparing / Ready / Paused -> Cancelling -> Cancelled
Publishing -> RecoveryRequired -> Committed or Conflict
```

`Planned` is side-effect-free validation. `Reserved` persists identities and the structural reservation. `Preparing` creates unpublished output in bounded segments. `Ready` means all expected outputs are prepared and validated; it does not mean publication is complete. Only the catalog commit makes new inventory visible.

The OS job lock is the live ownership authority. Timestamps and progress heartbeats are informational and cannot justify taking ownership from a slow process. On runner death, the lock becomes available; a new runner revalidates the request, reservation and completed segments before resuming. Only one runner may prepare or publish a given job.

Pause stops admission and drains current writes before recording a checkpoint. Cancellation before publication removes only manifest-owned files whose bytes still match their expected hashes, then removes empty owned folders. Preserve and report unknown or modified files, including manually added data. Release the structural reservation but never reclaim IDs. Once publication begins, finish/recover the commit rather than attempting ad hoc deletion.

## Additive import protocol

### 1. Inspect and reserve

Read a coherent catalog snapshot. Validate exact project/binary identity, hierarchy, unique RVAs, requested membership, node counts, and admission limits. Estimate disk and memory requirements; estimates do not replace handling disk-full failures.

Acquire the project write lock briefly, reload current structural state, reject another structural job, allocate IDs, and durably publish the reservation. The binary identity and hierarchy/dependency graph remain fixed for this job's preparation. Report content and research state may change.

Version one accepts only creation. Reorder, move, rename, membership changes, prerequisite edits, binary changes, and link repair are structural operations and cannot run alongside a reserved import. A combined legacy batch containing those fields follows the existing exclusive transaction path with an explicit blocking status. Never silently discard fields or relax requested atomicity.

### 2. Prepare without the project write lock

Build the new-node index and partition work by bounded chunks of records, not one scheduler task per tiny file. CPU jobs render report headers and dependency views. A bounded file service writes new canonical reports, the two generated dependency views, and empty data directories. No existing canonical report is rewritten in this phase.

Persist completion manifests in segments so recovery need not deserialize a second full before/after catalog and every report body. Hash prepared output as it is produced. Files from a segment interrupted before its completion record must be checked and adopted only when their job ownership and exact expected contents can be established.

New nodes start NotStarted. Prepared progress is shown separately from committed inventory and research completion. New hierarchy relationships and source links do not appear in committed reports yet.

### 3. Build a publication candidate

Take a fresh coherent snapshot. Combine reserved new nodes with the latest existing nodes, authored reports, acceptance state and revisions. Never replace current nodes with stale copies from the original import request.

Render affected existing parent headers using their current authored bodies and evidence. Calculate ancestor and prerequisite-consumer invalidation against the current graph. Adding required children invalidates parent synthesis, even if a parent was completed while preparation ran. Preserve valid independent function completion; context membership follows existing rules.

Prepare catalog serialization and changed parent outputs outside the write lock, recording the versions and content hashes actually read. If reports change before commit, regenerate only the affected outputs and update the catalog candidate. Bound retries and expose contention; never spin indefinitely or overwrite the winning edit. A busy project may need a brief queued publication turn after bounded optimistic retries.

### 4. Publish and acknowledge

Acquire the exclusive project lock. Revalidate binary identity, structural reservation/epoch, reserved paths, candidate input versions and relevant report hashes. If they differ, release the lock and rebase or report a specific conflict. Do not repeat all preparation.

Write a recovery record containing the current catalog identity, affected existing report before-images, prepared output references and intended after-state. Replace affected existing reports and publish the catalog last. Retain recovery material, mark the job committed, and release the structural reservation. Emit a committed revision notification.

All supported readers hold shared leases while obtaining a coherent catalog/report response, so they cannot observe the partial publication window. A reader that encounters pending publication must release its shared lease before requesting exclusive recovery; never upgrade a held shared lock in place. Preparation state is distinct from pending publication and must never trigger automatic publication on an ordinary read.

If the process dies after catalog replacement but before the receipt, recovery recognizes the committed transaction ID and finishes bookkeeping without incrementing the revision twice. If an external edit changes a file expected by recovery, stop with a conflict and preserve both versions and the journal.

The final lock hold still includes affected existing reports and catalog replacement. It is not a constant-time pointer swap. Measure it; if catalog serialization/revalidation remains too large, introduce segmented committed metadata in a separate storage design rather than claiming short commits prematurely.

## Concurrency and conflict contracts

| Concurrent activity during additive preparation | Policy |
| --- | --- |
| Read existing reports, search, browse | Allowed from committed state |
| Save unrelated existing function drafts | Allowed; neither token becomes stale solely because the other saved |
| Edit the same report | First valid commit wins; second receives a record conflict |
| Complete or reopen an existing record | Allowed under ordinary input checks; import may subsequently invalidate affected synthesis |
| Save affected parent text | Allowed; publication preserves the latest authored text while updating generated links |
| Change dependencies, ownership, names, membership, ordering, binary identity | Structural-job conflict until the job commits/cancels |
| Start another structural import | Queued explicitly or rejected as already active; no competing reservation |
| Read prepared new nodes through ordinary inventory APIs | Not visible until commit |
| Write supporting data for existing records | Existing ownership rules apply; importer never rewrites that data |

A draft token binds exact project/binary identity, stable node identity, authored body/evidence, and that node's relevant structural context. Generated child links and unrelated project revision increments are not draft conflicts. Draft saves reload current metadata, preserve generated sections, and leave acceptance open. Raw external canonical-file changes require validation of the generated envelope and authored sections; malformed headers are reported, not silently repaired during a save.

A completion token additionally binds effective required-child state, applicable prerequisites, their evidence/acceptance revisions, and the node's own current report. Dependency or child changes require renewed review. Merely comparing the draft token is insufficient for completion. Ancestor/consumer invalidations merge under the commit lock and cannot restore stale acceptance.

Provide versioned opaque tokens with explicit purposes. A draft token cannot authorize completion or a structural change. Legacy project-revision tokens retain their conservative semantics during transition; do not reinterpret them as narrower tokens. Batch draft saves validate their full read/write set before any mutation and retain all-or-nothing publication. A project revision can remain an optional deliberately strict precondition; the new concurrent API must not require it for unrelated drafts.

## Locks, readers and cross-process coordination

Keep the legacy library gate for compatibility. Define lock ordering as library gate -> project lease; runners also own a per-job lock and never wait for it while holding a project lease. Structural reservation is durable metadata, not a project lock held for the whole job. Release project leases before CPU work, filesystem preparation, provider waits or UI callbacks.

Normal reads take a shared project lease only long enough to capture a coherent response. Saves and publication take an exclusive lease. Catalog migration and interrupted publication recovery require exclusive access. Never run migration as a side effect of an unguarded snapshot read.

Use bounded retry/backoff and cancellation for contention, with structured busy reasons. Fairness across independent OS processes is not guaranteed by a try-lock loop; measure writer starvation. If bounded retries cannot provide acceptable progress, add a project coordinator/queue as a deliberate follow-up rather than asserting fairness. A running CLI command must not indefinitely starve the GUI or vice versa.

Each active runner owns its Solworker runtime. Initial per-runner CPU and I/O limits must account for multiple projects running on the same host; defaults should leave resources for interactive use. Do not claim a per-process cap is a global machine budget.

## Solworker integration

Review and pin the current Solworker implementation at `273b45b04f8c30d5fcc6c017bdaad4eab9ff8cde`, using `pubdocs/` and the public Rust API rather than the outdated root README. Its package requires Rust 1.96; its development checkout pins a newer toolchain. Scopework's current CI uses 1.95. Update and qualify the toolchain and dependency distribution together. The crate has a local `vendor/micropool` dependency: resolve reproducible CI/MSI source acquisition explicitly, without relying on an absolute developer-machine path or silently copying implementation files.

| Responsibility | Integration |
| --- | --- |
| Render records, build indexes and compute derived progress | Bounded owned CPU jobs/groups on configured lanes |
| Bound queued jobs and retained buffers | Solworker admission/capacity plus explicit application byte estimates |
| Read/write files | Application-owned bounded file service; bridge completion through external outcomes where useful |
| Deliver a ready GUI snapshot | Owner-thread delivery in a small permitted publication phase |
| Display job activity | Solworker diagnostic progress combined with durable application stage/count state |
| Cancel and shut down | Stop admission, settle providers, service owner cleanup, drain according to lifecycle contracts |
| Decide commit validity and persist changes | Scopework coordinator and filesystem protocol |

The GUI must not call blocking group joins or passive waits that depend on its own owner pump. It must not help long import jobs on the input thread. Pump only bounded publication callbacks and return to egui. Filesystem waits must not occupy caller-helpable CPU work. Reserve or retain continuation admission so a full queue cannot strand a required write completion.

Task completion is not proof that file access ended or publication committed. Keep buffers and paths owned until the file provider has finished accessing them. Persist domain progress only after the associated operation is settled. Handle admission failure, cancellation, worker panic, file errors and shutdown explicitly.

## GUI snapshots and Stack Threads

Build an immutable `ProjectSnapshot` off-thread with catalog revision, indexed hierarchy/paths, searchable rows, progress totals and effective states. Share it with readers through retained ownership. The UI displays one coherent snapshot until a replacement is ready; it never incrementally mutates a displayed index while workers read it.

Tag every rebuild/search request with project identity, committed revision and a view generation. Deliver results only if they still match the requested generation; otherwise release them. Coalesce obsolete rebuilds and retain at most a bounded number of generations. Preserve unsaved editor text and validate it at save time independently of view replacement.

This applies Stack Threads' retained-input and validated-publication principles. The existing reference prototype uses process-local identities and does not implement durable transactions. No full frame/strand engine is required for the first upgrade. Reconsider it only if multiple dependent preview stages justify its scheduling/lifetime model. Import publication is mandatory work, not a freshness-discardable preview.

Replace repeated UI-thread full-catalog path construction with cached snapshot indexes. Poll only cheap change/job markers, or use a qualified notification adapter with a fallback poll. Show committed inventory counts, import stage, prepared counts, publication status, pause/resume and actionable conflicts without changing meter completion semantics.

## CLI surface

The upgrade implements these commands:

```text
research import plan PROJECT --file REQUEST.json
research import start PROJECT --file REQUEST.json
research import status PROJECT JOB
research import pause PROJECT JOB
research import resume PROJECT JOB
research import cancel PROJECT JOB
```

`plan` returns eligibility, estimated counts, touched parents, structural requirements and fallback reasons without modifying research. `start` persists a job and runs it in the foreground by default; an explicit detached mode, if implemented, launches a hidden runner and returns its durable job ID. GUI starts run off the UI thread. `status` reports durable state plus diagnostic progress; it never infers success from a dead PID. Pause/cancel requests are durable and acknowledged after active providers settle.

Existing `node batch` remains compatible. Pure creation may route to the new additive path while preserving the submitted revision precondition at admission and returning the usual final receipt. Mixed structural batches keep their documented exclusive semantics. A proposed version-2 draft-save batch uses record tokens without a mandatory global-revision precondition. Document token purposes and machine-readable conflict kinds.

## Migration and compatibility

Before enabling the new protocol, acquire exclusive library compatibility access, ensure no older operation is active, finish or resolve legacy pending transactions, then migrate under the project write lock. Format-4 jobs use a new directory/schema that legacy transaction recovery will not recognize or attempt to replay. Publish the format gate before reserving or preparing concurrent work.

Older versions must refuse format 4 before writing anything. Audit this order explicitly: the baseline attempts legacy recovery before checking layout version. Test old 0.4.3–0.4.5 executables against a migrated fixture, including pending new-style jobs. Preserve pre-migration metadata and a manifest of changes. Binary downgrade requires explicit recovery/export; never make the catalog look like format 3 while active jobs exist.

Deploy GUI and CLI together. Do not rewrite user-authored content, import acceptance, change research scope or move evidence during migration. Installed upgrades must preserve the research home. No new service registration is required.

## Recovery, durability and observability

Use unique temporary files, flushed publication records and an explicitly qualified Windows replacement/write-order protocol. The current write-then-rename helper is not sufficient evidence for a power-loss durability guarantee. Distinguish process-crash recovery from storage/power-failure guarantees in release notes and tests.

Record durations for admission/reservation, CPU preparation, file preparation, journal/checkpoint writes, publication candidate construction, lock wait, lock hold, parent updates and catalog commit. Record prepared/committed counts, bounded queue occupancy and retained-byte estimates. Keep structured logs in the job workspace; never log credentials or copy evidence payloads into diagnostics unnecessarily.

Measure UI frame stalls and foreground read/save latency during imports. Total throughput alone is not success. A faster import that freezes editing or loses conflict protection fails the design.

## Implementation sequence and acceptance

1. **Measurement and storage contracts.** Add phase timings to an isolated representative import; implement format-4 gating and record/draft/completion token contracts. Tests prove unrelated saves coexist, same-record conflicts are rejected, and changed completion inputs require review.
2. **Coherent background snapshots.** Introduce retained indexed snapshots, off-thread construction and generation-checked UI delivery. Tests cover stale result rejection, project switching, unsaved drafts, shutdown and bounded retained generations.
3. **Durable additive jobs.** Implement reservations, owned-output manifests, preparation checkpoints, cancellation and resume. Test duplicate RVAs, ID exhaustion, competing jobs, path collisions, disk full, provider failure, and process termination between checkpoints.
4. **Concurrent preparation and publication.** Integrate bounded Solworker CPU work and file service. Test reading and editing existing records throughout a large import, editing an affected parent, completing a record during preparation, structural conflicts, and failure at every publication boundary.
5. **UI/CLI delivery and packaging.** Add status/progress controls, compatibility routing, pinned dependency/toolchain setup and installer qualification. Test old-version refusal and research-home preservation.

Use isolated fixtures at small size for deterministic fault cases and one representative approximately 150,000-function workload for scale qualification. Compare against the baseline with identical storage, inputs and useful output counts. Capture a single baseline and candidate run set sufficient to explain variance; repeat only for changed code, failure or unresolved measurement uncertainty.

Correctness gates: no lost committed draft, no duplicate node/RVA, no premature inventory visibility, no imported acceptance, preserved function data bytes, exact recovery/idempotency, and no silent same-record overwrite. Availability gate: an existing-record read and save succeeds during paused or deliberately slow preparation without waiting for the import to finish. Responsiveness gate: no project load/index/import join on the UI thread; proposed targets are p95 existing-record read/save below 250 ms during preparation, p95 GUI update work below 16 ms, and normal publication lock holds below 500 ms on the documented reference machine. These are targets to qualify, not measured promises; report misses with phase evidence and revise the implementation or explicitly bound the supported workload before release.

Run focused regression tests, formatting, warning-denying Clippy, release builds and MSI install/upgrade checks. Add dedicated multi-process tests for lock/recovery semantics; in-process tests alone are insufficient. Keep performance checks separate from deterministic CI assertions. Do not rerun research-content verification as part of software qualification.

General concurrent move/link-repair transactions, globally fair cross-process scheduling, a persistent coordinator, and a full Stack Threads integration remain outside this first upgrade. They require separate evidence and design rather than weakening the additive path's guarantees.
