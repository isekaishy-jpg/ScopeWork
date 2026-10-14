# Scopework agent CLI

Install the MSI with PATH enabled, open a new terminal, and use `research --help`. If PATH is disabled, invoke `research.exe` from the application folder. The GUI executable is `research-dashboard.exe`; the console executable is `research.exe`. Both use the same backend and completion rules. All successful CLI commands print JSON. Operation errors print JSON to stderr and exit 1; argument errors exit 2.

## Research home

The installer-selected research home is shared by the GUI and CLI. New installations default to `%LOCALAPPDATA%\Scopework`, independent of the installation folder or current directory. The existing earlier default home is retained when present. `RESEARCH_DASHBOARD_HOME` overrides the installed home with another absolute home; CLI `--home PATH` overrides it for one invocation. It contains `projects/` and one shared `complaints/` folder. `research home` prints these paths without creating anything. `research init` creates an empty home without any projects; ordinary research commands and GUI startup also initialize the home when needed.

Historical research is never imported automatically. Current catalogs receive versioned layout upgrades; carry-over from other research is manual. Uninstall removes the installed application, shortcut and its PATH entry, and leaves the research home intact.

## Commands

```powershell
research home
research init
research gui
research project list
research project create example --name "Example" --scope "Asset platform" --binary "Build ...; SHA-256 ...; image base ..."
research project show example
research project set-binary example --binary "Build ...; SHA-256 ...; image base ..."
research node create example --parent 0 --name "Asset loading"
research node create example --parent 1 --name "Archive lookup"
research node create example --parent 2 --name "Lookup entry" --rva 00123456
research node list example
research node show example 3
research criteria function
research criteria chunk
research criteria architecture
research criteria app
research status example
```

Use the actual IDs returned by creation; the numbers above illustrate a fresh project. App is always node 0. A new project appears as a dashboard tab after **View > Refresh research progress**. Set binary identity before adding functions; it becomes locked once functions exist. A function has one canonical owner and one unique RVA per project. RVAs are hexadecimal; node IDs are decimal.

## Project research scope

Research Scope is a few words naming the research topic, such as Rendering or Asset platform. Set it during project creation with `--scope`, or update it directly:

```powershell
$project = research project show example | ConvertFrom-Json
research project set-scope example --scope "Asset platform" --expect-revision $project.revision
```

The label appears above the project's meters and in `project show/list`, `node show`, and `status`. It identifies what agents are researching. Changing it does not reopen completed reports or change assignments. Pass an empty label to clear it.

## Edit and review

Read the node before editing. Its `token` detects stale report or project metadata. Write UTF-8 Markdown body and evidence inputs, then save through the CLI. These are editing inputs; the canonical report remains the file returned by `node show`. Saving leaves the record open for review.

```powershell
$record = research node show example 3 | ConvertFrom-Json
research node save example 3 --body-file C:\work\body.md --evidence-file C:\work\evidence.md --expect $record.token
$record = research node show example 3 | ConvertFrom-Json
# Review the actual report, evidence and scope criteria before attesting.
research node complete example 3 --expect $record.token --reviewed
$record = research node show example 3 | ConvertFrom-Json
research node reopen example 3 --expect $record.token
```

After another project edit, read again and review rather than blindly retrying a stale write. Evidence must cite exact source/build, addresses or captures and classify claims as observed, specification-defined or inferred. Completion remains an attestation of understanding, not an automated semantic judgment. Completed children enable parent synthesis; they do not complete it. Each assignment stays within its own scope.

## Prerequisites

```powershell
research deps get example 3
$record = research node show example 3 | ConvertFrom-Json
research deps set example 3 --expect $record.token 000ABCDE 00012345
```

Prerequisite RVAs must already have canonical function records in the same project. Omit the RVAs to clear the list. The app maintains `dep.toml` and `prereq.toml` together; never edit these generated views directly. Only necessary understanding prerequisites belong here, not every call edge.

The dashboard can stay open during CLI work. Writes reload the latest catalog under a project-specific exclusive lock. Concurrent operations on the same project receive a busy error and may retry; different projects proceed independently. Conflicting report edits require review. Use **View > Refresh research progress** to display CLI changes. Unsaved editor text is retained and stale saves are rejected.

## Complaints

Put one narrow Markdown report per app issue in the `complaints` path printed by `research home`. Filing the report is the whole process.

## Whole-binary inventory and batch catalog changes

Use `research node batch PROJECT --file C:\work\catalog.json` to enroll a complete function inventory or reorganize existing ownership in one operation. Read `project show` first. Copy its exact `binary`, `revision`, and `next_id`; do not edit `project.json` directly.

```json
{
  "schema": 1,
  "binary": "EXACT binary identity from project show",
  "expect_revision": 42,
  "create": [
    {"parent": 2, "name": "Archive lookup", "rva": 1193046},
    {"parent": 7, "name": "Context helper", "rva": 1193060, "context_only": true}
  ],
  "update": [
    {"id": 9, "parent": 8, "name": "Decoded lookup", "context_only": false}
  ],
  "reorder": [
    {"parent": 0, "children": [1, 6]}
  ]
}
```

These IDs are illustrative; referenced nodes must exist. JSON RVAs are unsigned 32-bit **decimal byte offsets from the image base**, unlike hexadecimal command-line RVAs. IDs are allocated in `create` order starting at `next_id`; later entries may reference parents created earlier in that array. Architecture/chunk creation omits `rva`. Each function has one unique RVA across the project. Unknown fields, stale revisions, mismatched binary identities, duplicate RVAs, invalid parent kinds, and incomplete sibling order lists are rejected before canonical writes.

`update` moves or renames nodes without changing their IDs or RVAs. Omitted fields are preserved. `reorder` must contain every direct child exactly once; it controls browser, meter and generated source-link order. Arrays may be omitted. Existing research bodies, evidence and data stay with their nodes. New records start unresearched; the batch cannot import completion claims or fabricate report content.

The operation holds the project write lock, records the original catalog and affected reports under `tools/workspaces/catalog-transactions/pending`, stages moved folders, updates reports and affected dependency views, and commits the catalog last. The next CLI/GUI open resumes interrupted work after checking recorded inputs. Completed journals remain under `revision-N`; preserve them if recovery reports a conflict. Do not manually edit files during a pending transaction. The journal does not back up arbitrary supporting data; maintain ordinary backups separately.

Ownership, membership, name, order and report changes reopen affected accepted reports and their consumers/ancestors for review. Recognized internal Markdown and prose paths are rebased; external URLs and paths outside the project are retained. Schema-1 annotation-ledger report paths follow moves while original review hashes remain pinned, requiring review of changed evidence.

Batch changes use catalog format 3. Upgrade the GUI and all CLI workers to 0.4.3 or later before using them. Earlier versions refuse these catalogs rather than losing membership information.

## Required research versus context inventory

Catalog the whole binary even for a smaller research subject. Use coherent semantic chunks for established or provisional ownership. An unresolved queue denotes uncertainty, not subsystem ownership; navigation address pages are not semantic chunks.

Every node defaults to required child membership (`context_only: false`). Set `context_only: true` in a batch or with the editor's **Context-only child membership** checkbox to exclude that branch from its parent's required synthesis. Its report, data, progress and canonical identity remain intact. Explicit function prerequisites remain required even when their canonical owner is contextual. Context records are never automatically completed.

Membership is separate from the short Research Scope label. The label never silently excludes nodes. For a whole-client recreation, leave all inventoried functions required. CLI `status` exposes `inventory_functions`, `required_functions`, `context_functions` and per-record `required_research`. Meters count required work; the status line retains the full inventory count.

The browser starts with collapsed branches, supports case-insensitive name or hexadecimal RVA search, renders only visible rows, and can focus on required research. Clearing the filter restores the inventory.

## Repair previously migrated links

For reports migrated by 0.4.2 whose authored absolute links still use `chunks/<id>` or `functions/<RVA>`, run:

```powershell
$project = research project show example | ConvertFrom-Json
research node repair-links example --expect-revision $project.revision
```

This repairs recognized internal references through the same journaled operation using current canonical identities. Changed reports require review; it does not reinstate stale completion or alter external evidence. The equivalent batch flag is `"repair_legacy_links": true`.

## Large-library reads

From 0.4.4, commands naming a project load that project only. Unstarted report files are read on demand; accepted reports remain hash-checked when evaluating progress. A missing report produces an error when opened or saved, and cannot retain accepted completion. Ordinary CLI reads are not an implicit audit of every unstarted file in every project. From 0.4.5, operations on different projects use separate locks. The GUI retains its last catalog snapshot for a busy project, refreshes available projects, and retries busy projects on its next refresh. `project list` reports busy project names rather than returning an incomplete list. Upgrade all GUI and CLI workers to 0.4.5 for cross-project concurrency: a shared compatibility gate deliberately still excludes older executables that require the legacy global lock. Mixed versions remain safe, but an active older worker can still block the library.


## Batch report editing

Read multiple reports from one catalog snapshot:

```powershell
research node read-batch example 0 1 2
```

The JSON contains `project`, exact `binary`, `revision`, and `reports` with each report's `id`, canonical path, body, evidence, criteria, and edit `token`. Prepare a UTF-8 JSON input with edited body/evidence text:

```json
{
  "schema": 1,
  "binary": "EXACT binary identity from read-batch",
  "expect_revision": 42,
  "reports": [
    {"id": 1, "expect": "TOKEN from read-batch", "body": "Updated understanding", "evidence": "Source references"},
    {"id": 2, "expect": "TOKEN from read-batch", "body": "Updated synthesis", "evidence": "Source references"}
  ]
}
```

Save with `research node save-batch example --file C:\work\reports.json`. IDs and tokens above are illustrative. Each `expect` is validated independently against the same starting revision, before any report is written. Duplicate/unknown IDs, reserved report markers, unknown JSON fields, wrong binary identity, and stale tokens or revision reject the batch. Read again and review conflicts before retrying.

The catalog is loaded/indexed once, changed reports are journaled together, and the catalog revision advances once. Exact unchanged open reports are skipped. Saving an accepted report reopens it even when its text is unchanged. Changed reports and affected consumers/ancestors lose acceptance; no batch field can attest completion. Supporting data is retained. Successful JSON returns `saved` node IDs and the resulting `revision`.

The durable `catalog-transactions` journal used for catalog edits makes interrupted report publication resumable on the next open. Preserve journals and do not edit canonical files during pending publication. Completion remains a separate, individually reviewed operation.
