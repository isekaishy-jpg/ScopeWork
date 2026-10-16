# Changelog

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

## 0.4.0 â€” 2026-10-13

- Add an optional desktop shortcut to the installer, enabled by default and remembered on upgrades.
- Replace the original dimensional icon with a simpler focus mark and a dedicated small title-bar asset.
- Add the MIT license, contribution guidance, issue templates, and a Windows build workflow.
- Reorganize user documentation into installation, research-model, CLI, and build guides.

## 0.3.0 â€” 2026-10-13

- Introduce the Scopework name and Windows application resources.
- Add a setup wizard with installation-folder, research-home, and PATH selections.
- Share the installed research-home setting between the desktop app and agent CLI.

## 0.2.1 â€” 2026-10-13

- Display a short project research-scope label above the meters.

## 0.2.0 â€” 2026-10-13

- Add project research-scope metadata and version-aligned MSI packaging.

## 0.1.0 â€” 2026-10-13

- Establish the Rust dashboard, isolated research hierarchy, canonical reports and prerequisite tags.
- Add the agent CLI and per-user Windows MSI packaging.

These entries describe implemented changes, not a record of published or runtime-qualified releases.
