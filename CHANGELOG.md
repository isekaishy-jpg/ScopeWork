# Changelog

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
