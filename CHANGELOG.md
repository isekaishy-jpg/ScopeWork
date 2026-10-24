# Changelog

## Unreleased

Maintenance from the 2026-10-21 TODO pass; each item has its proposal in `docs/`.

- **Dashboard memory no longer grows with every commit** ([proposal](docs/proposal-report-hash-memory.md)). Every refresh ran on a new thread whose cached report hashes were never freed, and re-cached hashes piled up on a cleanup queue that was never drained. The dashboard reached 50 GB on 2026-10-21. Refreshes now run on one long-lived thread that reuses its hashes between refreshes, other threads keep no hashes in memory, and replaced entries are freed at once. The coordinator's per-connection threads stop leaking the same way.
- The Write queue window is no longer see-through below its content: windows that are not resizable draw their frame around what they fill ([proposal](docs/proposal-window-frame.md)).

- **Quarantine** ([proposal](docs/proposal-quarantine.md)): `node quarantine` / `unquarantine` / `quarantined` mark a chunk or function as complete and sealed. Moves, renames, reorders, membership changes and new prerequisites touching it need `--directed-by` the human's instruction, which history records. Prerequisites on it count as satisfied; report text stays editable. The GUI marks it ⊘ with a **Quarantined** filter.
- **Renames reach every citation** ([proposal](docs/proposal-rename-citations.md)): a `node batch` rename rewrites every exact `` `OldName` (`VA`) `` citation in all reports, syntheses included. Reports changed only by renames keep their completion and their annotation-ledger pins follow. The result lists `citations`, `unrewritten` bare mentions and `kept_complete`. Structural changes in the same batch still reopen as before.
- **History** ([proposal](docs/proposal-history-view.md)): catalog receipts record their commit time (`at_unix`; older receipts use their journal's modification time, which `journal --compact` keeps). `history show --rva` and `--from`/`--to` (UTC days) filter; `history kinds` lists the known tooling kinds, and `history append` refuses a known kind without its required `--node` or `--ref`. New **View > Project history...** window.
- **Definite commit outcomes on Windows** ([proposal](docs/proposal-commit-outcomes.md)): a commit whose apply step keeps meeting sharing violations retries for 20 s and then fails with `commit_pending` instead of an ambiguous error; renames retry everywhere.
- **CLI** ([proposal](docs/proposal-cli-papercuts.md)): `node prune` explains skipped records; `project set-binary --expect-revision` amends the binary identity and keeps accepted hashes; `ready_for_review` excludes Not Started records and `status --ready-for-synthesis` lists chunks and architectures ready for their synthesis.
- **Crash log** ([proposal](docs/proposal-gui-crash-log.md)): the dashboard writes `logs/gui.log` in the research home (start line, panics with backtrace, abnormal exits; rotated at 1 MiB).
- The coordinator creates its shutdown window before accepting clients, so an installer or Windows shutdown during startup cannot miss it.
- Completion throughput was measured: a member completion costs about 0.4 s, and the time went to project tools, not the app ([findings](docs/proposal-completion-throughput.md)).

## 0.13.1a - 2026-10-18

Release notes, validation and the commit ledger: [docs/releases/0.13.1a.md](docs/releases/0.13.1a.md). Fixes from a code review of 0.13.0a, whose draft release was not published:

- Relative `--file`, `--body-file`, `--evidence-file` and `--diff-file` paths sent through the coordinator resolve in the caller's directory, not the coordinator's. Previously, a save could read another directory's file of the same name.
- A coordinated `job run` also takes the resource lock files that direct mode uses, so coordinated and direct-mode jobs on the same resource can no longer run at once.
- Reads no longer skip their checks for other processes' commits while the coordinator's writer is still waiting for the project lock; the check is suspended only while the writer holds the lock.
- A `job run` exits non-zero when its own payload's result is an error, whether it ran the command, joined another's run or ran without a coordinator; a missing result is `no result`.
- The dashboard counts functions in progress and not started by each function's own status, instead of by its chunk's colour.
- The coordinator is started without inheriting any of its caller's handles, so a script or test reading a command's output is never kept waiting by it.
- If a coordinator cannot be started, commands under `SCOPEWORK_COORDINATOR=auto` fall back to direct mode instead of failing.

## 0.13.0a - 2026-10-18

Release notes, validation and the full commit ledger: [docs/releases/0.13.0a.md](docs/releases/0.13.0a.md). Versions now follow the Solarity convention: a product version with a stage suffix, plus a numbered build. The number jumps from 0.8.0 because the version path was recalculated from 0.1.0; see the release notes. Measured causes and acceptance criteria for the multi-agent work are in [the proposal](docs/proposal-2026-10-18.md).

- Project tabs on the dashboard widen to fit long project names instead of being overdrawn by the next tab.
- Product versions and numbered builds ([proposal](docs/proposal-versioning.md)):
  - `Cargo.toml` `0.13.0-alpha` is product `0.13.0a`;
  - `installer/build.ps1` reserves a never-reused build number (`BUILD_NUMBER`) per package;
  - `research --version` and `--build-info`, the About dialog and the executables' version resources show `Scopework 0.13.0a (Build 000001)`;
  - the MSI version is `major.minor.build`, so every package upgrades the previous one;
  - release tags are `v<product version>`;
  - coordinators and clients order themselves by build number, or by version and stage.

- Moves and deletions no longer rewrite unrelated reports. A reference changes only when it names a moved folder, or when its report moved and the named project file exists; quoted source strings such as `.\Movement_C.cpp` and `..\src\x.cpp` keep their bytes, so other sessions' completed work is not reopened. Moves read only the reports that mention a moved folder, through a persisted reference index, instead of every report under the lock.
- The annotation ledger is repathed in place when the catalog commits: only outdated `"report"` values change; formatting and line endings stay. Journals no longer copy the ledger, a tool's concurrent write can no longer block a project, older journals with ledger copies resume the same way, and rows written back with an old path are repaired by the next move. Ledger rows no longer get a `catalog_move` note.
- Read-only commands answer from the committed catalog while a pending transaction is blocked (`recovery_blocked` warning on stderr); write and recovery errors name the file and the journal. File and folder renames retry Windows sharing violations.
- Project locks queue fairly in arrival order; later readers no longer overtake a waiting writer, dead waiters are skipped and `research locks` shows queue positions. GUI actions wait up to 60 s for the lock; the GUI refresh releases its lock before building meters. `research wait` re-reads the catalog only when it changes.
- Batch schema 2 checks each updated record's `expect_node_revision` instead of the project revision. `node batch` reports `moved` report paths and `ledger_rows_repathed`. New `node resolve` maps old report or data paths to current ones.
- `research journal PROJECT --compact-sidecars` replaces large sidecar copies in completed receipts with their changed JSON values and hashes (opt-in, irreversible for those bytes); `journal` reports `large_sidecar_bytes`.
- Deletion queues the completed records it reopens and drops claims and queue entries of deleted records; previews write nothing. Completion evaluates only its own children and prerequisites; history summaries skip report and sidecar text.
- Secondary windows, dialogs and menus use the dashboard's Windows 95 Task Manager style (`src/classic.rs`). Research records follows the dashboard's project tab, with a records list, Report / Organize / Project tabs and a status bar. **Mark complete...** is a separate, confirmed action that says what blocks it. New project moves to **File > New project...**.
- A coordinator process per research home serves the CLI over a same-user named pipe ([proposal](docs/proposal-coordinator.md), [CLI guide](docs/cli.md#coordinator)). Reads answer from the last committed revision without waiting for writers and carry `revision` and `pending`; queued record operations merge into one commit with the same results as running them one by one; structural operations commit alone. New `research queue`, `coordinator status` and `coordinator stop`; `SCOPEWORK_COORDINATOR=off|auto|required` and `SCOPEWORK_SUBMITTER`. Direct mode and older versions stay correct beside it. It shuts down gracefully for Windows shutdown and installer upgrades.
- The dashboard footer and the Research records status bar show each project's write queue; click it for the list.
- Reads through the coordinator are counted per record class, shown by `coordinator status` and appended daily to the tooling history as `coordinator-reads`.
- Tool leases ([phase 2](docs/proposal-coordinator-phase2.md), [CLI guide](docs/cli.md#tool-leases)): `research job run` holds a lease on named project resources while a command runs. Waiting requests with the same key join one run, each getting its own result; payload-less ones are covered. Leases end with their holder, several resources are granted together, and without a coordinator lock files stand in. A project's `tools/coordinator.toml` declares resources, files and kinds. Changes to declared files outside a lease, and to the catalog or sidecars outside Scopework, are recorded as `drift`. New `research job status` and `research tools audit` (advisory); leases show in `research queue` and the GUI queue panes.
- The coordinator's writer keeps its catalog in memory between batches and reuses each commit's catalog fingerprint ([phase 3](docs/proposal-coordinator-phase3.md)): about 131 ms less fixed work per batch on the live catalog. `coordinator status` reports commit measurements, also written daily as `coordinator-commits`.
- Whole-app review fixes: merged claim operations no longer drop earlier claims or accept a conflicting one; a failed merged commit no longer leaves uncommitted state for readers; a failing coordinator writer answers `writer_failed` instead of hanging later writes or sending clients to direct mode; reads that overlap another process's commit are answered again under the lock; `research wait --idle` honours the coordinator's queue; drift detection ignores sidecar creation and same-tick rewrites; direct-mode resource locks remove their holder records; path prefixes with non-ASCII case folding can no longer panic.
- A completion change reopens only the records whose completion depends on it (ancestors and prerequisite consumers) instead of every App/architecture/chunk revision. A commit serialises its catalog once (about 200 ms to 112 ms per commit on the measured catalog). One Solworker runtime serves each process.
- CONTRIBUTING.md states the testing rule: research does not get tests; this software does.

### Project tooling: action needed

These are changes to each research project's own tools (`tools/scripts`), made by that project's sessions. Scopework cannot make them. Do them in this order.

1. **Fix shared scratch files for report saves.** A tool that writes report bodies or evidence to a fixed file before calling `node save` can submit another agent's text when two saves overlap; the expected token does not catch it, because the record itself did not change. Write each save's files to a unique temporary name. Example: `catalog.save` in `wow-335a-12340` uses `tools/scratch/canonical-body-input.md` and `canonical-evidence-input.md` for every call.
2. **Name the caller.** Set `SCOPEWORK_SUBMITTER` (for example the session and tool name) so queue entries, `research queue` and the GUI queue panes show who is waiting.
3. **Take leases instead of private lock files** ([CLI guide](docs/cli.md#tool-leases)).
   - Declare the shared resources and kinds of work in `tools/coordinator.toml`, for example `ledger` (with the ledger file, so drift is detected) and `ghidra`.
   - Replace the project-wide tool lock with a lease on the resources the work needs. During the change-over, take the lease first and then the old lock file, so tools not yet updated still exclude correctly. Remove the old lock once none can remain.
4. **Let queued work of one kind run once.**
   - Ledger edits become payloads of one keyed kind (for example `ledger-put`). The holder reads the ledger once, applies the payloads in order (a later row for the same kind and RVA replaces an earlier one), validates each row on its own, and writes once.
   - A whole-ledger apply needs no payload: duplicates waiting together are covered by one run.
   - Read-only export batches with the same options can merge their function lists into one headless run.
   - Each holder must give every payload the result it would have had alone.
5. **Optional, for less overhead:**
   - talk to the coordinator's pipe from long-lived tool modules instead of starting `research.exe` per call (two process starts per save today);
   - move batch requests from schema 1 (project-wide revision) to schema 2 (record-level preconditions), so unrelated commits stop rejecting them.

After installing, use the new build normally for about a week. `coordinator status` and the daily `coordinator-reads` and `coordinator-commits` tooling-history entries gather the measurements that decide the two remaining storage changes ([phase 3](docs/proposal-coordinator-phase3.md), 3.3 and 3.4).

## 0.8.0 - 2026-10-17

See [qualification and limits](docs/qualification-0.8.0.md).

- Wait for another process's project lock instead of failing `busy`: global and per-command `--wait SECONDS` (default `SCOPEWORK_LOCK_WAIT_SECS`, else 120; `0` keeps the immediate answer). Lock holders and waiting processes are recorded under `.project-locks/`; `research locks` shows them without taking the lock. `research wait --idle | --revision-above N | --complete ID` blocks until a condition holds.
- Write catalogs as compact JSON and remove orphaned `project.*.pending` temporaries when a project is opened for writing. Compute transaction deltas and pending-journal recovery on typed records (263 ms to 86 ms per write on a 30,682-function catalog).
- Reuse report hashes for effective status: size and modification time with git's racy-timestamp rule, parallel stat and hashing on Solworker, an in-memory Solcache table in the GUI and a persisted copy for one-shot CLI calls. Warm `status` on the same catalog went from 1.94 s to about 0.43 s with identical results.
- Add small answers: `project revision`; `--parent`, `--kind` and `--status` filters for `status` and `node list`; `status --ready` and `--summary`. Add `research session` to run many commands in one process.
- Add advisory work claims on architectures and chunks (`claim set|release|list`, optional leases, `claimed_by` in `status`), and a review queue of completed records reopened by moves, membership, order or name changes (`review-queue`, a `review_queue` count in `status`, `reopened` with claim owners in `node batch` results). The GUI marks queued records with ↻, adds a "Review queue" filter and shows the reason in the editor. Both use journaled sidecars that older versions leave intact.
- Add `research history show` (catalog transaction receipts summarised newest first, filtered by node, revision and kind) and `history append` for project tooling diffs in `tools/workspaces/history/tooling.jsonl`.
- Add a tag-triggered release workflow that checks the tag against `Cargo.toml`, runs the CI checks, builds the MSI and creates a draft GitHub release with the checksum and changelog notes. Pin Solcache by Git revision.

## 0.7.0 - 2026-10-16

- Add evidence-only report saves (`node save --evidence-only`, `"evidence_only": true` in `save-batch`). Generated evidence no longer marks research started, so Not Started records keep their dim red meter segments. Add `node reset-status` to return In Progress records to Not Started by ID or for every In Progress record. Meter colours are unchanged: dim red Not Started, yellow In Progress, green Complete.
- Add review flags: `node flag --reason`, `node unflag` and `node flagged`. Flags appear in `node show`, `status` (count and per record) and the GUI (⚑ marker, "Flagged for review" filter, flag and clear controls in the editor). Flags block completion until cleared, never change status, are dropped with deleted nodes, and live in a journaled `review-flags.json` sidecar that older versions leave intact.
- Style secondary windows, menus, popups and fields in the dashboard's Windows 95 Task Manager look (square corners, no shadows, grey faces, thin borders, navy selection) from one shared definition used for light and dark modes. Group project settings, tools and import under one collapsed header so the record browser and editor show first.
- Set `MSIFASTINSTALL=1` so setup skips the system restore point and costs less before progress starts. Document closing Scopework before upgrades and the end-of-install PATH broadcast.
- Document that moving a function between chunks reopens both chunks, their ancestors and the moved function, while keeping its report and `data/`. Add regression coverage for moves, evidence-only saves, status reset and review flags. Recovery tests that rely on debug-only fault injection now skip explicitly in release test runs.

## 0.6.0 - 2026-10-16

See [qualification and limits](docs/qualification-0.6.0.md).

- Store pending and completed catalog transactions as changed-record deltas with whole-catalog conflict fingerprints. Preserve recovery of older full journals; add `research journal PROJECT [--compact]` to inspect storage and compact completed history without deleting it.
- Add preview/token-checked `node delete` and `node prune` commands. Removed reports and supporting files are archived; recognized report links and annotation provenance follow the archive. Dependencies are removed and affected consumers/ancestors require review.
- Introduce catalog format 5 to exclude older writers that cannot recover delta journals. Finish or cancel active imports with the previous version before migrating.
- Clean package-build scratch directories in `finally`, suppress packaging debug sidecars, and remove empty application folders during uninstall/upgrade. Preserve research and Windows Installer's own repair cache.
- Replace the 150,000-function qualification fixture with a self-cleaning 16-function smoke run, capped at 100. Concurrency tests also use small fixtures.

## 0.5.0 - 2026-10-14

See [qualification and performance limits](docs/qualification-0.5.0.md).

- Add durable, resumable additive imports with bounded CPU/file preparation, ID reservations, pause/cancel controls and recoverable catalog-last publication. Existing reports remain editable during preparation.
- Introduce format-4 compatibility gating and purpose-bound draft, completion and structural tokens. Schema-2 batch draft saves can omit a global revision precondition.
- Move project refresh, meters, report loading/saving and browser search off the input thread; publish retained snapshots through bounded owner callbacks and preserve unsaved drafts.
- Pin Rust 1.98.0 and the Solworker Git revision for reproducible GUI/CLI packaging.
- Add multi-process recovery, concurrency, conflict, cancellation, provider-failure and UI freshness regression coverage.


## 0.4.5 - 2026-10-14

- Fit extra meters per row within the viewport; wrap before overflow, retaining horizontal scrolling for oversized baseline pairs.
- Isolate operations with project locks. GUI refresh retains busy project snapshots while loading other projects. A shared compatibility gate prevents unsafe overlap with older executables.
- Add `node read-batch` and token-checked, journaled `node save-batch` to amortize large-catalog report editing. Saves remain unaccepted and invalidate affected consumers and ancestors.
- Add regression coverage for lock isolation, legacy compatibility, stale/malformed batches, interrupted publication, completion invalidation, evidence preservation and meter wrapping.

## 0.4.4 - 2026-10-14

- Load only the targeted project for project/node/dependency CLI commands.
- Read unstarted reports on demand instead of scanning every canonical file on every command. Accepted reports retain content-hash validation.
- Reuse one effective-state calculation when checking completion of a large parent.

## 0.4.3 - 2026-10-14

- Add revision-checked batch creation, move, rename, membership and sibling ordering with resumable journals.
- Index catalog paths and progress; update only affected generated dependency files.
- Add collapsed, virtualized record browsing with name/RVA lookup and required-research filtering.
- Keep whole-binary context inventory separate from required research; explicit prerequisites remain required.
- Repair recognized absolute and relative internal links during moves, and expose repair for older migrated reports.
- Use catalog format 3 for batch and membership changes; older app versions reject it instead of losing membership.

## 0.4.2 - 2026-10-14

- Fix project/tab selection after external project deletion; target writes by stable project ID.
- Refresh project tabs and progress automatically, preserving unsaved drafts and stale-write protection.
- Nest category, chunk and function folders, with resumable migration for current projects, updated report/dependency links and preserved function data.
- Keep the existing dashboard design.


## 0.4.1 — 2026-10-13

- Start new research homes empty instead of creating development projects.
- Remove sample dashboards, preview-file loading and sample meter preferences.
- Show an empty state with project creation; preserve existing research on upgrade.

## 0.4.0 — 2026-10-13

- Add an optional desktop shortcut to the installer, enabled by default and remembered on upgrades.
- Replace the original dimensional icon with a simpler focus mark and a dedicated small title-bar asset.
- Add the MIT license, contribution guidance, issue templates, and a Windows build workflow.
- Reorganize user documentation into installation, research-model, CLI, and build guides.

## 0.3.0 — 2026-10-13

- Introduce the Scopework name and Windows application resources.
- Add a setup wizard with installation-folder, research-home, and PATH selections.
- Share the installed research-home setting between the desktop app and agent CLI.

## 0.2.1 — 2026-10-13

- Display a short project research-scope label above the meters.

## 0.2.0 — 2026-10-13

- Add project research-scope metadata and version-aligned MSI packaging.

## 0.1.0 — 2026-10-13

- Establish the Rust dashboard, isolated research hierarchy, canonical reports and prerequisite tags.
- Add the agent CLI and per-user Windows MSI packaging.

These entries describe implemented changes, not a record of published or runtime-qualified releases.
