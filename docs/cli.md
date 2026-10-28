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
research node create example --parent 2 --name "Archive table" --rva 00140000 --length 96
research node list example
research node show example 3
research criteria function
research criteria data
research criteria chunk
research criteria architecture
research criteria app
research status example
```

Use the actual IDs returned by creation; the numbers above illustrate a fresh project. App is always node 0. A new project appears as a dashboard tab after **View > Refresh research progress**. Set binary identity before adding functions; it becomes locked once functions exist. After that, `project set-binary` accepts only an amendment, with `--expect-revision` from `project show`: every SHA-256 and every `0x` value of the current identity must remain, and new wording or mappings may be added. An amendment re-renders the report headers without reopening anything: completed records stay complete, and annotation-ledger rows pinned to those reports are re-pinned. `--expect-revision` is also accepted before enrolment, to reject concurrent identity edits. Setting the identity it already has commits nothing. The project name and the binary identity are rendered into every report's header, so each must be one line without report section markers (`<!-- research-`). A command that names a project that does not exist answers `Unknown project` without creating anything. A function has one canonical owner and one unique RVA per project. RVAs are hexadecimal; node IDs are decimal.

## Project research scope

Research Scope is a few words naming the research topic, such as Rendering or Asset platform. Set it during project creation with `--scope`, or update it directly:

```powershell
$project = research project show example | ConvertFrom-Json
research project set-scope example --scope "Asset platform" --expect-revision $project.revision
```

The label appears above the project's meters and in `project show/list`, `node show`, and `status`. It identifies what agents are researching. Changing it does not reopen completed reports or change assignments. Pass an empty label to clear it.

## Dashboard groups

```powershell
research group set 3-3-5a WoW
research group set wow-335a-12340 WoW
research group list
research group clear 3-3-5a
```

Related projects can share one dashboard tab. A group tab reads `Group: Project`; clicking it shows that project, and clicking it again (or its arrow) opens a menu of the group's projects. Groups are one level, each project is in at most one, and a group left empty is removed. They are kept in the research home's `settings.json`, not in any project, and change no project data or history. Research records > Project > **Dashboard group** sets the same thing from the GUI. Group names are one line of at most 40 characters.

## Implementation tracker

```powershell
research impl repo example --path C:/src/example-rs --remote https://github.com/me/example-rs.git
research impl add example --name example_rs                 # returns {"node": 1}
research impl add example --name src/heap.rs --parent 1
research impl place example 2 1234 1235                     # research function records
research impl item-add example 2 --name "Host heap shim"   # implementation-only
research impl complete example --record 1234 --item 3
research impl show example --node 2
research impl show example --unplaced
research impl where example 1235
```

The tracker records the recreation: a free-form tree of its own organisation (crates, modules, files, or any grouping, decided by whoever builds it), the research function records placed where they are implemented, implementation-only items needed to make the recreation match, and completion marks with who set them and when (`SCOPEWORK_SUBMITTER`, else the Windows user name). A record is placed on at most one node; placing it elsewhere is refused until it is unplaced. `show` returns every node with its own and subtree counters (`research` and `implementation_only`, each `done` of `total`), the project totals and the number of functions not placed; `--node` adds a node's placements and items and `--unplaced` lists the functions not placed. `rename`, `move` (no `--parent`: top level) and `remove` (only an empty node) reshape the tree; `unplace`, `item-remove` and `reopen` undo.

Completion is a tracker, not a proof: it is the builder's discipline, and the running product is the proof. The tracker lives in the project's journaled sidecars `implementation/tree.json` and `implementation/nodes/<id>.json`, so every change has a receipt and history. Through the coordinator its changes are record operations that may share a commit. The dashboard's **Implementation** tab (at the right end of the project tabs) shows the tree with its counters, the selected node's placements and items, and the totals; clicking a placement opens the record. Research records > Organize shows where a function is implemented.

## Edit and review

Read the node before editing. Its draft `token` binds that record, its authored content and relevant structural context. Unrelated draft saves, import ID reservations and scope-label edits do not invalidate it. Write UTF-8 Markdown body and evidence inputs, then save through the CLI. These are editing inputs; the canonical report remains the file returned by `node show`. Saving leaves the record open for review.

```powershell
$record = research node show example 3 | ConvertFrom-Json
research node save example 3 --body-file C:\work\body.md --evidence-file C:\work\evidence.md --expect $record.token
$record = research node show example 3 | ConvertFrom-Json
# Review the actual report, evidence and scope criteria before attesting.
research node complete example 3 --expect $record.completion_token --reviewed
$record = research node show example 3 | ConvertFrom-Json
research node reopen example 3 --expect $record.token
```

After a record conflict, read again and review rather than blindly retrying. Completion requires its own `completion_token`, binding applicable required children, prerequisites and acceptance state. Dependency changes require `structural_token`, which binds only that function's current prerequisite list and the binary identity: moves, renames, report saves and other functions' prerequisite changes leave it valid, so `deps set` conflicts only when another writer changed the same function's prerequisites. Take it when you review the prerequisites, not just before applying, and treat a conflict as a reason to review again. `s4:` tokens from earlier versions are still accepted with their old meaning (any structural change in the project invalidates them). These opaque purposes are not interchangeable. Old unprefixed tokens retain their conservative project-revision semantics. Evidence must cite exact source/build, addresses or captures and classify claims as observed, specification-defined or inferred. Completion remains an attestation of understanding, not an automated semantic judgment. Completed children enable parent synthesis; they do not complete it. Each assignment stays within its own scope.

## Prerequisites

```powershell
research deps get example 3
$record = research node show example 3 | ConvertFrom-Json
research deps set example 3 --expect $record.structural_token 000ABCDE 00012345
```

Prerequisite RVAs must already have canonical function records in the same project. Omit the RVAs to clear the list. `deps get`, `node show` (`requires` and `required_by`) and the GUI answer both directions from the catalog. Earlier versions also wrote `dep.toml` and `prereq.toml` views into each function folder; nothing reads them, they are no longer written, and `project tidy` removes them. Only necessary understanding prerequisites belong here, not every call edge.

The dashboard can stay open during CLI work. Writes reload the latest catalog under a project-specific exclusive lock. Reads share a coherent project lease. Writes serialize; contention may return a busy error and can be retried. Additive import preparation releases the project lease, allowing existing records to remain usable. Different projects proceed independently. Conflicting report edits require review. Background polling updates meters and catalog snapshots automatically; **View > Refresh research progress** requests an immediate refresh. Unsaved editor text is retained and stale saves are rejected.

## Evidence-only saves and status reset (0.7.0)

Generated evidence (inventories, census or decoding sections) should not count as research started. Add `--evidence-only` to `node save`, or `"evidence_only": true` to a `save-batch` request. The report is saved with the usual token checks and journaling, but a Not Started record stays Not Started. A completed record still reopens when its text changes, because its accepted text changed. Saving a report unchanged (with or without `--evidence-only`) changes nothing; `node reopen` is how a completed record is reopened without editing it.

To return records that were marked In Progress only by generated evidence:

```powershell
$project = research project show example | ConvertFrom-Json
# request.json: {"schema":1,"binary":"<exact binary>","expect_revision":<revision>,"all_in_progress":true}
#           or: {"schema":1,"binary":"<exact binary>","expect_revision":<revision>,"ids":[12,13]}
research node reset-status example --file request.json
```

Complete records are never changed. Report text and evidence are untouched.

## Review flags (0.7.0)

```powershell
research node flag example 12 --reason "Check the error branch"
research node flagged example
research node unflag example 12
```

A flag marks a record for review with a short reason. It does not change status, but it blocks completion until cleared. `node show` returns `review_flag`; `status` reports the flag count and a per-record `review_flag`, and flagged records are not `ready_for_review`. Flags are stored in `review-flags.json` beside `project.json` and change through the catalog journal. Deleting a node removes its flag. In the GUI, flagged records show ⚑ in the browser, the "Flagged for review" filter lists them, and the editor has flag and clear controls.

## Proofs

```powershell
research project set-proof example --required true --repository C:/src/example-rs
research node proof example 12 --recreation src/heap.rs::alloc --test tests/heap.rs
research node proof example 12 --clear
research node proof-batch example --file proofs.json   # {"proofs":[{"id":12,"recreation":"...","tests":["..."]}]}
research node unproven example --complete
```

A proof names the recreation of a function (a path, optionally `::item`) and one or more tests that check it. When a project requires proof (`set-proof --required true`), a function cannot be completed without one: `node complete` fails with `proof_required`, and an unproven function is not `ready_for_review`. Saving drafts stays free, and chunks, architectures and the app take no proof. With `--repository` (an absolute path; `--repository ""` clears it), every recreation and test file must exist under it when the proof is recorded (`proof_path_missing` otherwise); whether the named item is still defined is the project's own check. `proof-batch` records many proofs in one transaction and refuses the whole batch on any invalid entry.

Functions completed before the requirement stay complete; they show as unproven. `node show` returns `proof` and `unproven`, `node unproven` lists functions without a proof (`--complete`: only completed ones), `status --summary` counts `unproven_complete`, `project show` gives the policy under `proof`, and the GUI's Files group shows the proof or **Unproven**. Proofs are stored in `proofs.json` beside `project.json`, change through the catalog journal and merge in the coordinator like review flags. Deleting a node removes its proof. If `proofs.json` exists but cannot be read, reads treat it as holding no proofs and no requirement, while proof writes and function completion are refused with `proofs_unreadable` until it is repaired.

## Research pools

```powershell
research agent register                       # registers SCOPEWORK_SUBMITTER (or: register NAME)
research agent list
research pool add example voice 152 --origin safety-stop --reason "Voice chat stops agents"
research pool join example voice --agent astra   # the human directs who joins
research pool leave example voice --agent astra
research pool list example
research pool remove example 152 --reason "The human reopened it to everyone"   # members, or --directed-by
research pool migrate-quarantine example
```

A research pool holds chunks and functions that only its members may change. An agent that hits a safety stop on a record puts it in a pool (`--origin safety-stop`) and leaves it; the human can pool records too (`--origin human`, or **Research pool** in the record panel). A pooled chunk covers its functions. Pooling a record that is in no pool is open to everyone; taking a record out of its pool, moving it to another pool or changing its entry (`pool add` again with a new reason or origin) is a change to a pooled record, for members or `--directed-by`. The log records a move as `remove` and `add`, and a changed entry as `update`.

- **Members work it.** Report saves (including batches and evidence-only saves), completion, reopening, status resets, review flags, claims, proofs and changes to its own prerequisites need the caller to be a member of the record's pool. So do creating records in it, moving records into or out of it, renaming, reordering or changing its membership, and removing or pruning it. Anyone else gets `pooled`, naming the pool. An import is checked when it is reserved and again when it publishes, with the `directed_by` its request carried, so a chunk pooled in between is respected. `--directed-by` with the human's instruction overrides, and history records it. The dashboard acts for the human and is never refused.
- **Everyone reads and builds on it.** Reading a pooled record and naming it as a prerequisite of other work are free. Mechanical rewrites (a rename's citations, link repairs) still apply.
- **Its status is its own.** A pooled record is open work until it is researched: meters, parent completion and the reopen walk treat it like any record. A completed pooled record stays in its pool, so non-members still never write to it.
- **Open work for members only.** `status` gives each record's `pool` and offers a pooled record as `ready_for_review` / `ready_for_synthesis` only to members of its pool; `status --summary` counts `pooled`. `node show` gives `pool` (with the carrying `node` and the `members`) and `may_write` for the caller. `read-batch` gives `pool` per record. The GUI marks pooled records with ⊘ and filters them with **In a pool**.
- **Who is asking.** Agents identify themselves with `SCOPEWORK_SUBMITTER`, through the coordinator too. They register once in the research home (`agents.json`); only registered agents can join a pool. Registration needs no approval: the human directs which agents join which pool.
- **Storage.** Pools, their members and pooled records live in the journaled `pools.json` sidecar beside `project.json`, with a log of every addition, removal, join and leave. Deleting a node drops its record. If `pools.json` exists but cannot be read, every write is refused with `pools_unreadable` until it is repaired; reads treat it as holding no pools.
- **Quarantine is gone.** 0.14's `node quarantine`, `unquarantine` and `quarantined` are replaced by pools. `pool migrate-quarantine` moves every quarantine record into the pool `quarantine` (`cyber-stop` becomes `safety-stop`) and empties `quarantine.json` in one transaction. Until then, a project with quarantine records refuses writes with `quarantine_unmigrated`. Migrated records show their own status: one that was complete only through quarantine is open work again.

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

`update` moves or renames nodes without changing their IDs or RVAs. Omitted fields are preserved.

A rename reaches every report that cites the function exactly as `` `OldName` (`VA`) `` (VA as eight hex digits, any case, optionally `0x`-prefixed; VA = RVA + the image base in the binary identity): function reports, chunk and architecture syntheses and the app report, body and evidence alike. These edits are mechanical: a report changed only by renames (its own `# Name` header, a child's name in its source list, rewritten citations) keeps its completion. Its accepted hash and the prerequisite stamps on it follow the new text, and its annotation-ledger rows are re-pinned in the same transaction. Anything a structural change in the same batch touches (moves, creations, context or order changes, dependency resynchronisation, legacy link repair) is reviewed as before. The result adds `citations` (each rewritten report with its count), `unrewritten` (reports that still mention an old name outside the exact form, with counts, for manual follow-up) and `kept_complete` (the records that kept their completion). Bare mentions are never rewritten. `reorder` must contain every direct child exactly once; it controls browser, meter and generated source-link order. Arrays may be omitted. Existing research bodies, evidence and data stay with their nodes. New records start unresearched; the batch cannot import completion claims or fabricate report content.

Mixed batches containing updates/reordering/link repair hold the project write lock, record a recoverable catalog delta and affected reports under `tools/workspaces/catalog-transactions-v4/pending`, stages moved folders, updates reports, and commits the catalog last. The next CLI/GUI open resumes interrupted work after checking recorded inputs. Completed journals remain under `revision-N`; preserve them if recovery reports a conflict. Do not manually edit files during a pending transaction. The journal does not back up arbitrary supporting data; maintain ordinary backups separately.

Ownership, membership, name, order and report changes reopen affected accepted reports and their consumers/ancestors for review. A reference in a report is rebased only when it must change: the project path it names lies under a moved folder, or the report holding a relative reference moved and the named project file exists. Every other text keeps its exact bytes, including quoted source strings such as `.\Movement_C.cpp` or `..\src\file.cpp`, so moves never reopen unrelated reports. Besides the moved records, a move reads only the reports that mention a moved folder name; a persisted index (`tools/workspaces/scopework-cache/report-references.json`, rebuilt on demand) tells which, so unchanged reports are only statted.

Annotation-ledger report paths follow moves: when the catalog commits, the `"report"` values in `tools/profiles/annotations.json` (schema 1) that name a canonical report or data path at an outdated location are replaced in the existing text. Formatting, key order and line endings stay as written; original review hashes stay pinned, requiring review of changed evidence. The same pass repairs any row a tool wrote back with an old path. The journal records the move, never ledger copies, so a tool writing the ledger during a move can no longer block the project. Results list `moved` records (`node`, old `from` and new `to` report paths) and `ledger_rows_repathed`.

### Record-level preconditions (schema 2)

Schema-1 batches require the exact project revision, which every save anywhere advances. Schema 2 instead checks each updated record: omit `expect_revision` and give every `update` the record's current `expect_node_revision` (the `node.revision` of `node show`). Unrelated commits no longer conflict; a changed record fails with `record_conflict`. Creations still check their parents and reorders still list every child. Creation-only schema-2 batches run as one transaction rather than as a background import.

```json
{"schema": 2, "binary": "EXACT binary identity", "update": [{"id": 9, "parent": 8, "expect_node_revision": 14}]}
```

### Finding moved reports

```powershell
research node resolve PROJECT architectures/000015/000184/003F4930/research.md
```

Maps report or data paths, old or current, project-relative or absolute, to their record and current location (`node`, `kind`, `name`, `current`, `absolute`, `moved`). Tools that keep report paths can repair them after other sessions' moves.

Current operations use catalog format 4. Upgrade the GUI and all CLI workers together to 0.5.0. First open migrates under exclusive compatibility access, retaining metadata under `tools/workspaces/layout-migration-v4`. Earlier versions refuse the new format; do not manually lower its version to downgrade.

## Data items

A chunk holds data items (globals, tables, strings, vtables) beside its functions. A data item has a start RVA and a length in bytes, and is researched like a function: its own report in `<chunk folder>/D<RVA>/research.md`, a stored status, evidence required for completion, a `data/` folder, review flags, claims and pools. Proofs are for functions only.

- Create one with `node create --rva HEX --length BYTES` under a chunk, or in a batch with `"length"` beside `"rva"` (`{"parent": 2, "name": "Archive table", "rva": 1310720, "length": 96}`). A batch `update` may change a data item's `length`.
- An RVA names at most one record, function or data item. Data items may contain one another; they stay flat in their chunk, and the extents tell which contains which.
- Prerequisites connect functions and data items in any direction (`deps set` takes either kind's RVA).
- `status` counts them in `data_items` (`total`, `required`, `complete`), apart from the function inventory; `node list --kind data` and `status --kind data` list them.
- A catalog takes format 6 when its first data item is created. Earlier Scopework versions cannot open a format 6 catalog; projects without data stay at format 5.

## Artifacts and address spaces

```powershell
research project artifact add PROJECT --name NAME --sha256 HEX [--size BYTES] [--format TEXT] [--inside ID --path PATH] [--description TEXT] --expect-revision N
research project artifact update|remove PROJECT ID ... --expect-revision N
research project space add PROJECT --name NAME --kind image|file|processor [--artifact ID] [--processor TEXT] [--base HEX] [--description TEXT] --expect-revision N
research project space update|remove PROJECT ID ... --expect-revision N
research node create PROJECT --parent CHUNK --name NAME --rva HEX [--length BYTES] --space ID
```

A project names its input files once ([design](design-storage-promotion.md)), and the binary identity text stays as their description. An **artifact** has a name and SHA-256 (both unique in the project), optionally a size, a format and a description; a file extracted from another artifact names it with `--inside` and its path there with `--path`.

The project's addresses are in its **default space**, as before: the RVA from the image base in the binary identity. Another **space** counts addresses somewhere else: `file` (offsets in one artifact, such as data on a disk with no program of its own; needs `--artifact`), `image` (another loaded image, with its `--base`) or `processor` (another processor's address space, such as a sound CPU).

- A function or data item is placed in a space with `--space ID`, or `"space"` in a batch or import creation. An address names at most one record **per space**, so the same number can be a function in the image and a table in a file.
- Its folder carries the space: `S<n>-<RVA>` for a function, `S<n>-D<RVA>` for a data item. Its report header names the space, as do the record panel and the tree (`S1:0x...`).
- `deps set` names a prerequisite in another space as `S<n>:<RVA>`; a bare RVA is in the default space. Rename citations (`Name` (`VA`)) apply to the default space only, the one whose image base gives a VA.
- An update changes only the fields given; an empty text clears that text. An artifact a space names or another artifact lies inside cannot be removed, nor a space that holds records. Every change is a journaled catalog transaction (it appears in history and the store follows it).
- The first artifact or space raises the catalog to format 7, which earlier versions cannot open; projects without them keep their format and bytes. `project show` lists `artifacts` and `spaces`.

## Required research versus context inventory

Catalog the whole binary even for a smaller research subject. Use coherent semantic chunks for established or provisional ownership. An unresolved queue denotes uncertainty, not subsystem ownership; navigation address pages are not semantic chunks.

Every node defaults to required child membership (`context_only: false`). Set `context_only: true` in a batch or with the **Context only** check box on the Research records **Organize** tab to exclude that branch from its parent's required synthesis. Its report, data, progress and canonical identity remain intact. Explicit function prerequisites remain required even when their canonical owner is contextual. Context records are never automatically completed. Because a parent's completion does not depend on its context-only children, editing or completing a context-only record does not reopen the parent (changing its membership does, since that rewrites the parent's report).

Membership is separate from the short Research Scope label. The label never silently excludes nodes. For a whole-client recreation, leave all inventoried functions required. CLI `status` exposes `inventory_functions`, `required_functions`, `context_functions` and per-record `required_research`. Meters count required work; the status line retains the full inventory count.

The browser starts with collapsed branches, supports case-insensitive name or hexadecimal RVA search, renders only visible rows, and can focus on required research. Clearing the filter restores the inventory.

## Repair previously migrated links

For reports migrated by 0.4.2 whose authored absolute links still use `chunks/<id>` or `functions/<RVA>`, run:

```powershell
$project = research project show example | ConvertFrom-Json
research node repair-links example --expect-revision $project.revision
```

This repairs recognized internal references through the same journaled operation using current canonical identities. A repair rewrites only links, so the records it touches keep their completion and their annotation-ledger pins follow the new text; it does not reinstate stale completion or alter external evidence. The equivalent batch flag is `"repair_legacy_links": true`.

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


## Concurrent additive imports (0.5.0)

Creation-only `node batch` requests now use the durable additive pipeline and retain their exact binary/revision admission precondition. They still run to a final receipt in the foreground. Mixed requests retain their existing atomic, exclusive behavior.

```powershell
research import plan example --file C:\work\inventory.json
research import start example --file C:\work\inventory.json
# To reserve without starting preparation:
research import start example --file C:\work\inventory.json --paused
research import status example JOB-UUID
research import pause example JOB-UUID
research import resume example JOB-UUID
research import cancel example JOB-UUID
```

`plan` does not modify catalog or report data. It reports counts, affected existing parents and estimated metadata bytes. A legacy catalog or pending publication must be opened/recovered before planning. Lock files may be created to coordinate the read.

One active import reserves a range of IDs and fixes structural context. Existing report reads, independent drafts and valid completion remain available during preparation. Structural changes and a second import return a structural-job conflict. Pausing retains the reservation; cancellation releases it. Reserved IDs are never reused.

Prepared files live at reserved canonical paths but remain absent from ordinary inventory until the catalog commit. Raw directory enumeration can see these files; use the CLI/catalog for committed inventory. Import progress is not research completion. New nodes always start NotStarted. Existing parent prose and evidence are preserved when generated source links are published.

Durable jobs live under `tools/workspaces/import-jobs/<UUID>`. `status` reports state, prepared/total counts, errors, preserved-file warnings and phase durations. A live OS runner lock prevents competing runners; a dead PID alone proves nothing. Resume validates the immutable request, reservation and prepared output hashes. Pause/cancel is acknowledged after current file work settles. Once publication starts, recovery finishes the commit.

Cancellation deletes only owned outputs whose bytes still match the manifest. Changed reports and additional data are retained and reported in `warnings`. Preserve job journals and receipts when investigating errors. Publication recovery covers process termination, including catalog replacement before the receipt; power-loss ordering and storage-controller failures are not guaranteed.

In **Research records > Project > Inventory import**, enter the request file path and start a hidden CLI runner. Pause, resume and cancel operate on its durable job. Closing the dashboard leaves that runner independent. If launch fails, the reserved job can be resumed from the CLI.

## Independent batch draft saves

Use schema 2 to omit a global revision precondition while retaining all-or-nothing record-token validation:

```json
{
  "schema": 2,
  "binary": "EXACT binary identity from project show",
  "reports": [
    {"id": 3, "expect": "d4:opaque-token-from-read", "body": "Authored Markdown", "evidence": "Evidence references"}
  ]
}
```

Submit with `research node save-batch PROJECT --file INPUT.json`. An optional `expect_revision` adds a deliberately strict project-wide precondition. Schema 1 still requires it. Every input is checked against the same starting snapshot before any report changes. Draft batches never attest completion.

Errors retain the JSON `error` message on stderr and include a machine-readable `kind` (for example `record_conflict`, `structural_job_conflict`, `busy`, or `operation_failed`). Successful JSON stays on stdout.

Three kinds say what happened to a write. `operation_failed` (and the conflict kinds) means nothing was applied; fix the cause and retry. `commit_pending` means the change is recorded in the transaction journal but a file could not be replaced yet (usually another process held it open past the retries); Scopework applies it before any other operation on the project, so do not repeat the request, and read the record back later. `coordinator_lost` and `writer_failed` are the only unknown outcomes: check the record before retrying. See [0.5.0 qualification](qualification-0.5.0.md) for large-catalog latency limits.

## Node deletion and empty-node pruning (0.6.0)

Both commands preview by default. Save a JSON request with the exact binary and current revision from `project show`:

```json
{"schema":1,"binary":"exact build identity","expect_revision":42,"ids":[17,19]}
```

```powershell
research node delete PROJECT --file remove.json
research node delete PROJECT --file remove.json --expect PREVIEW_TOKEN
```

Deletion includes every descendant of the selected IDs. App 0 is never deletable. The preview lists candidates, archive paths, removed dependency edges and surviving reports requiring review. `--expect` must match the preview token; changes to the catalog or affected report content reject the operation. No node IDs are reused.

Pruning accepts additional optional fields:

```json
{"schema":1,"binary":"exact build identity","expect_revision":42,"ids":[],"keep":[25],"name_prefix":"Retired","context_only":true,"include_authored":false,"recursive":true}
```

```powershell
research node prune PROJECT --file prune.json
research node prune PROJECT --file prune.json --expect PREVIEW_TOKEN
```

An empty organization node is an Architecture or Chunk with no remaining child nodes. Functions are never prune candidates. By default, authored body/evidence or extra supporting files also protect a childless scope. `include_authored:true` explicitly permits their archival. `ids` limits eligible IDs (empty means all); `keep` always protects listed nodes. `name_prefix` and `context_only` further restrict candidates. `recursive:true` considers ancestors that become empty, but **all filters still apply to ancestors**. Without it, only nodes that were already childless are selected. Intentionally open source scopes can be retained with `keep`, selection filters, or the default authored-content protection.

Previews and applied results list `excluded`: for each requested ID (or, when `ids` is empty, up to 50 skipped scopes) the rule that kept it: `kind`, `keep`, `context-filter`, `name-prefix`, `has-children` (with the remaining `children`), `authored-content` (body, evidence or both) or `extra-files` (with up to five `files` and a `count`).

Removed node directories are moved, not destroyed, to `tools/workspaces/deleted-nodes/revision-N/ID/`. Supporting files retain their bytes. Links in canonical reports that point into removed folders, and schema-1 annotation report references, follow the archive (other report text is untouched); original annotation review hashes are retained and require review before reapplication. Completed records the removal reopens enter the review queue; review flags, claims and queue entries of removed records are dropped in the same transaction. Opaque paths in custom scripts or supporting files are not rewritten. Live dependency edges involving deleted nodes are removed, and affected consumers/ancestors are reopened. Active imports block structural removal.

The standard pending journal recovers interrupted moves, report updates and catalog publication. Completed receipts retain before/after metadata, report text and old/new paths; the archive retains supporting data. There is no automatic undelete command. Recovery from a completed deletion requires restoring the intended records and files using its receipt under controlled maintenance, accounting for later edits; do not copy an old catalog over a live one.

## Files that hold nothing: `project tidy`

```powershell
research project tidy example
research project tidy example --apply
```

A record folder holds its `research.md` and whatever research stores beside it. Scopework creates nothing else there: a function's `data/` folder (the `data` path in `node show`) exists once something is stored in it, and the GUI's **Open data folder** creates it. Imports remove their ownership markers (`.scopework-import-<job>`) when they commit.

Folders made by earlier versions still hold generated `dep.toml` and `prereq.toml` views, markers of committed imports and empty `data/` folders. `project tidy` lists them per category with counts, sizes and sample paths; `--apply` removes them under the project's write lock and appends a `project-tidy` history entry. It keeps a `dep.toml` or `prereq.toml` without Scopework's generated header, the active import's markers and any `data/` folder that holds files, and lists what it kept. Removal never touches a folder that gained a file since the scan. Run it once everyone using the project has a version with these changes, because an older GUI's **Open data folder** expects the folder to exist.

## Journal storage and compaction (0.6.0)

```powershell
research journal PROJECT
research journal PROJECT --compact
```

The first command reports completed journal bytes, potential savings, pending journal bytes, bytes held in large sidecar texts (`large_sidecar_bytes`), and per-journal formats/paths. The second atomically converts completed full journals to change receipts. It does not remove history, reports, data, or pending/incomplete transactions. These history commands bypass project migration and recovery, so pending journals remain untouched even when they need conflict resolution. Interrupted compaction is repeatable; already compacted entries are retained. Only recognized `revision-N/journal.json` records in the catalog transaction directories are eligible.

New pending journals and completed receipts use schema-1 deltas: project metadata excluding inventory arrays, changed node/edge records with their array positions, affected reports/sidecars, directory moves, and fingerprints of the complete before/after catalog. Recovery reconstructs and verifies the other catalog state against those fingerprints. Older full pending journals remain readable. A completed receipt cannot be replayed by renaming it to `pending`.

Receipts written by 0.8.0 and earlier copy the whole annotation ledger twice per move; on large projects these copies are most of the transaction history. `research journal PROJECT --compact-sidecars` replaces every sidecar text of 1 MB or more in completed receipts with its changed JSON values (each with its JSON-pointer path), both texts' SHA-256 and sizes. This is irreversible for those exact historical bytes, so it never runs automatically; pending journals are never touched.

Retention policy: retain compact completed change history indefinitely, including removed-node archives; never automatically expire evidence. This removes unchanged whole-inventory duplication per edit, while storage still grows with actual changed content. Whole-catalog reorder operations can have large deltas. The format-5 migration keeps one full pre-upgrade catalog. Import publication history and other tool workspaces are separate from catalog-journal compaction and are not included in its byte totals.

## The project store (side by side)

```powershell
research store init PROJECT
research store sync PROJECT
research store status PROJECT
research store verify PROJECT
research store history PROJECT NODE
research store show PROJECT NODE --revision N
research store catalog PROJECT --revision N [--out FILE]
research store annotations PROJECT ADDRESS [--kind KIND]
research store conflicts PROJECT
research store compact PROJECT
```

The store keeps a project's whole catalog history in `<project>/store/`, beside the files, which stay authoritative ([design](design-storage-backend.md)). It follows the change receipts every writer leaves, so it never takes part in a commit: a store problem can never fail or slow a commit, or change a file.

- **`store init`** is the manual migration step: it creates the store from as much receipt history as verifies (each checkpoint is checked against the fingerprint the receipts recorded for it) and starts following. A project without a store is unaffected.
- **`store sync`** brings the store to the current revision: it reads the catalog under the project's shared lock, walks the receipts back to the store's head and appends what is new. A change that leaves no receipt (an import publication, a layout migration) is recorded as a gap, and the store re-anchors on the current catalog; later commits connect again. A frame cut short by an interrupted sync is removed; damage anywhere else is refused (`store_damaged`) and reported by `verify`.
- **`store status`** gives the head, the catalog revision and how far behind the store is, its segments, gaps, text count and sizes.
- **`store verify`** checks every frame's checksum, every checkpoint's fingerprint, each segment's commit chain by replaying it, every referenced text against its hash, and the head against the catalog.

- **`store history`** lists the revisions that changed a record or its report, newest first: time, direction, the record's changed fields (name, status, parent and length with both values), creation or removal, the report's path and text hashes before and after, moves and prerequisite changes. Gaps are listed, so an incomplete history says so.
- **`store show`** gives a record and its report text as they were at revision N. When the report did not change in the stored history from N on, `report.source` is `current_file` and the text is the file's.
- **`store catalog`** rebuilds the whole catalog at revision N from the nearest checkpoint and checks it against the fingerprint recorded for N; `--out` writes it to a new file (never over an existing one).
- **The coordinator keeps stores current:** when a project's writer has been idle for a few seconds after a new revision, it syncs that project's store, if it has one. `coordinator status` shows each project's `store.followed_revision` and the last sync `error`. Direct-mode and GUI commits are picked up by the next sync.

**The annotation ledger** ([design](design-storage-promotion.md)). The project tools' ledger (`tools/profiles/annotations.json`) stays authoritative; the store follows it as well as the catalog.

- **`store sync`** also records the ledger's change since the last sync (`ledger` in its answer): the rows and types added, removed and changed, keyed by address and kind and by type name. The first time, and for a change those keys cannot describe (two rows with one key), the whole ledger is stored once as a text. A rewrite in another order changes nothing (`reordered`). A ledger that does not parse, such as one being written, is skipped (`skipped`) and recorded at a later sync. Several tool writes between syncs become one entry.
- **The coordinator** follows the ledger when its stamp changes, on a thread of its own (comparing a 50 MB ledger takes a few seconds, and no commit waits for it); `coordinator status` shows `store.ledger_following` and `store.ledger_error`.
- **`store annotations`** gives the rows at an address now and every change to them through the store's history, newest first (`--kind` narrows to one kind).
- **`store conflicts`** lists where the catalog and the ledger disagree now: `name` (a `function-name` row unlike its function's name, or a `label` unlike its data item's), `kind` (function rows at a data item, data definitions or types at a function), `report` (a row pinned to another report than the record's at its address) and `unknown` (a row kind this build does not know). Rows at addresses without a record are counted as `unplaced_rows`, not conflicts. Conflicts are resolved before the store is adopted; the store only reports them.
- **`store status`** shows the ledger's entries, rows, types and last change, and `store verify` replays the ledger's chain.

**Compaction.** Every sync that adds texts writes one small pack frame, and every reader indexes all of them. **`store compact`** merges the texts into frames of about 4 MB (pack files up to 64 MB) and compresses each closed segment's log to `segment-N.logz` (the same frames as one zlib stream; the active log stays plain). Every read answers exactly as before. A sync compacts by itself once more than 64 small frames have built up (`compacted` in its answer); `store status` shows `small_pack_frames`.

Every answer names its source (`"backend":"store"`). Readers take no lock. Files: `store.json`; `segment-N.zlib` (the catalog where segment N starts) and `segment-N.log` (checksummed JSON entries: a start entry, then one commit entry per revision, with report and sidecar texts replaced by their SHA-256); `texts-N.pack` (texts compressed in groups); `ledger.log` (the ledger's entries) and `ledger-head.zlib` (the ledger as of the last entry, so a sync need not replay). A segment closes after 4 MB of log.

## Waiting, small answers and coordination (0.8.0)

### Lock waiting

Every command waits for another process's project lock instead of failing at once. `--wait SECONDS` (global, or per command) sets the limit; `SCOPEWORK_LOCK_WAIT_SECS` sets the default, otherwise 120 seconds. `--wait 0` restores the immediate `busy` answer. GUI actions wait up to 60 seconds. The lock holder writes `.project-locks/<project>.holder` (process, command, start time).

Waiting processes form a queue under `.project-locks/<project>.waiters/`, ordered by arrival: a writer goes when everyone ahead of it has gone; readers go together when no writer is ahead of them, so a waiting writer is never overtaken by later readers. A waiter that died is detected and skipped. `research locks` lists the queue with each waiter's `position`. Scopework 0.8.0 and earlier do not queue but still exclude correctly.

```powershell
research locks example                       # holder and waiters; takes no lock
research wait example --idle                 # until no writer holds the lock
research wait example --revision-above 812   # until another write has landed
research wait example --complete 42 --timeout 300
```

`wait` exits 1 with error kind `timeout` when its limit passes. Use it instead of polling loops. It re-reads the catalog only when it changes, and evaluates `--complete` after each change and at most every two seconds otherwise.

### Blocked recovery

If an interrupted transaction cannot resume (a report or sidecar it changes was edited outside the app), read-only commands still answer from the committed catalog and print a `recovery_blocked` warning on stderr. Writes fail with a message naming the file, the pending journal and the way out: restore that file to the text the journal records, then run any command.

On Windows, a file replacement that collides with another process briefly holding the target open (an indexer, antivirus or a tool) is retried for a few seconds instead of failing with `os error 32`.

### Small answers

```powershell
research project revision example                 # binary, revision, next_id; no catalog
research status example --summary                 # counts by kind and status
research status example --ready                   # only records ready for review (written, unblocked)
research status example --ready-for-synthesis     # parents whose children are complete and own report is empty
research status example --parent 12 --kind function --status in-progress
research node list example --parent 12
```

Filters narrow the output only; effective status is still evaluated for the whole catalog. `project revision` returns what a `node batch` request needs in a few hundred bytes instead of the whole `project.json`. Catalogs are now written as compact JSON, and orphaned `project.*.pending` temporaries from interrupted writes are removed when a project is opened for writing.

### Long-lived session

`research session` reads one JSON array of arguments per line from stdin and answers one JSON line per command (`{"ok": RESULT}` on success, or `{"kind": ..., "error": ...}` on failure). One process serves many commands and keeps its report-hash cache warm:

```text
["status","example","--summary"]
["node","show","example","42"]
```

### Report hash cache

`status`, completion checks and the GUI compare each accepted report's SHA-256 with its accepted hash. A hash is reused while the file's size and modification time are unchanged and that time is at least two seconds older than the moment it was hashed, so a change within the same instant is always re-read. Large evaluations stat and hash in parallel on Solworker. The GUI keeps the cache in memory (Solcache); one-shot CLI calls load and refresh `tools/workspaces/scopework-cache/report-hashes.json`. Deleting that file only costs one full rehash.

### Work claims

```powershell
research claim set example 12 --owner arch-6 --note "chunk research" --lease-hours 8
research claim list example
research claim release example 12 --owner arch-6
```

Claims name who is working on an architecture or chunk; functions are covered by their chunk's claim. Another owner's unexpired claim is refused (`claim_conflict`) unless `--force`. Claims are advisory: they do not block writes, but `status` shows `claimed_by` on every covered record and batch results report which claimed work a change reopened. They live in the journaled `claims.json` sidecar.

### Review queue

When a batch move, membership or order change reopens a completed record, that record enters the review queue with the reason and revision. So does a completed record reopened by an import's publication. An entry stays open until its record is complete again, so completing it needs no extra step.

```powershell
research review-queue example
research status example --summary    # includes the review_queue count
```

`node batch` results list the `reopened` records with any `claimed_by` owner. The GUI marks queued records with ↻, offers a "Review queue" browser filter and shows the reason in the editor. The queue lives in the journaled `review-queue.json` sidecar.

### Project history

```powershell
research history show example --limit 20
research history show example --node 42 --detail
research history show example --since 800 --until 812 --only catalog
research history show example --rva 3FF480 --from 2026-10-14 --to 2026-10-21
research history kinds
research history append example --kind ledger-change --summary "Retyped 14 parameters" --node 42 --ref tools/annotations/ledger.json --diff-file diff.json
```

`history show` lists entries newest first. Catalog entries summarise each transaction receipt (`revision-N/journal.json`): created and removed records, status, parent, name and membership changes, reopened completions, edited reports, moves and sidecars, and changes to the project's own fields (`project`: research scope, name, binary identity, import reservations); `--detail` adds the changed records. Every catalog change has a receipt, including scope changes and import reservations; an import's publication keeps its own receipt in its job folder, and `history show` lists it as `import <job> published` at its revision, with the range of records it created. Tooling entries are appended by project tools to `tools/workspaces/history/tooling.jsonl` with a lowercase hyphenated kind, the nodes and paths concerned, and a diff of what changed rather than a copy of whole files. The `catalog` kind is reserved. Restoring from history remains a manual, reviewed operation.

Every entry carries `at_unix`. Catalog receipts record their commit time, and `--from`/`--to` filter by it, so copying or restoring receipts does not move them in time; older receipts without one are dated by their `journal.json` modification time, which `journal --compact` keeps (it also writes the time into each receipt it rewrites). `--rva HEX` selects the record with that RVA (an unknown RVA is an error) and combines with `--node`; `--from` and `--to` take UTC days (`YYYY-MM-DD`, both inclusive).

`history kinds` lists the tooling kinds the app knows. `history append` refuses a known kind without what it must name:

| Kind | Must name |
|---|---|
| `reviewed-type` | `--ref` (the type) |
| `typed-signature` | `--node` |
| `behaviour-artifact-review` | `--node` and `--ref` (the artifact) |
| `dependency-list` | `--ref` (the list file) |

`ledger-change`, `ghidra-apply`, `generated-artifact` and `tooling-upgrade` need nothing more; `coordinator-commits`, `coordinator-reads`, `coordinator-leases` and `drift` are written by the app. Other lowercase hyphenated kinds are still accepted.

In the GUI, **View > Project history...** shows the same history for the dashboard's project: filter by kind, node, RVA and UTC day, select an entry to see its details, and **Open record** (or double-click) opens the record it concerns in Research records.

## Coordinator

Several agents on one research home share one coordinator process. It starts on the first command that needs it, serves every project in the home, and exits after 10 idle minutes. Nothing changes in the commands themselves: the same arguments give the same answers.

- **Reads never wait for writers.** They answer from the last committed revision held in memory. Object answers carry `revision` and `pending`: the queued or running operations that concern the records read (`kind`, `class`, `position`, `submitter`, `nodes`, `waited_ms`). A non-empty `pending` means the record is about to change. Array answers such as `node list` keep their shape.
- **Writes queue.** Record operations (report saves, review flags, sidecars, metadata) that wait together are merged into one commit and accept or reject exactly what they would one at a time in queue order; expected tokens still guard every save. Structural operations (moves, dependency changes, deletions, ledger changes) commit alone. The GUI's requests go first, operations on more than 32 records run as `bulk`, and anything that has waited 5 seconds moves up a class.
- **Direct mode stays correct.** Another process (an older Scopework, or `SCOPEWORK_COORDINATOR=off`) can still commit; the coordinator notices and reads the new revision. `research session` runs in its own process, as before.
- **Relative file paths mean the caller's directory.** The command makes `--file`, `--body-file`, `--evidence-file` and `--diff-file` absolute before sending them, so a coordinator started elsewhere opens the same files direct mode would.

```powershell
research queue example            # running and waiting operations; the lock queue without a coordinator
research coordinator status       # version, uptime, per-project queue and read counts
research coordinator stop         # stop admitting, finish queued work, exit
```

`SCOPEWORK_COORDINATOR` selects the behaviour: `auto` (default) uses or starts a coordinator and falls back to direct mode when none can be used; `off` always runs in-process; `required` fails with `coordinator_unavailable` instead of falling back. `SCOPEWORK_SUBMITTER` names the caller in queue entries (default: process ID and command).

If the connection fails after a write was sent, the command fails with `coordinator_lost`: the operation may or may not have been applied, so check the record (its token or revision) before retrying. If the coordinator's writer itself fails while applying a batch, its operations fail with `writer_failed`, with the same advice; the writer keeps serving later operations. Reads and writes that could not be sent fall back or fail as above and are safe to retry.

Reads answer from the coordinator's snapshot unless another process (direct mode, an older version, an import publication) is changing the project at the same time; such a read is answered again under the project lock, the way direct mode answers it, so it never mixes two revisions. `research wait --idle` waits until no writer holds the lock and the coordinator's queue is empty.

The pipe accepts only the same Windows user on the same machine. A coordinator from an older version retires when a newer client connects; a newer coordinator turns older clients away and they run in direct mode. Windows shutdown and installer upgrades close it gracefully after queued work finishes.

Reads are counted per record class with how many carried a pending entry. `coordinator status` shows the counts, and each day's counts are appended to the tooling history as one `coordinator-reads` entry.

The dashboard footer and the Research records status bar show `Queue: n running, m waiting` for their project; click it for the list.

### Commit measurements

`coordinator status` reports per project how many batches the writer committed, the operations per batch, and how often it reused its in-memory catalog. It also gives the median and p90 times of opening, building the journal (delta and fingerprints), writing it, and applying it, plus queue wait per class. Each day's figures are appended to the tooling history as one `coordinator-commits` entry.

## Tool leases

Tools that share project resources, such as the annotation ledger or the Ghidra project, can run their work under a **lease** instead of a private lock file:
- **Ordering:** leases queue in order with the same classes and ageing as catalog writes.
- **Visibility:** they show in `research queue` and in the GUI queue panes.
- **Joining:** work of one kind can be collected and done once.

```powershell
research job run example --kind ledger-put --payload-file row.json -- python tools/scripts/annotations.py apply-queued
research job run example --resource ghidra --class job -- python tools/scripts/fresh_re.py analyze
research job status example
research tools audit example
```

`job run` holds the lease exactly while COMMAND runs, and passes COMMAND's exit code through. COMMAND's own output goes to stderr. Stdout carries the job's JSON answer: `ran` (whether this request's command ran), `result`, `lease`, and `source`:
- `coordinator`: it ran under a coordinator lease;
- `done`: another holder ran it and returned this payload's result;
- `covered`: another holder's run covered this payload-less request;
- `locks`: it ran under lock files, without a coordinator.

**Joining.** Requests with the same key and the same resources that are waiting when a lease is granted join it, up to 256 payloads or 8 MB.
- The holder's command receives every payload, and must give each the result it would have had alone, in order.
- Payload-less requests are *covered* by the run's overall result.
- A request only joins work that started after it was queued.

**The command's contract.**
- `SCOPEWORK_JOB_PAYLOADS` names a JSON array of `{"payload_id", "submitter", "kind", "payload"}`, this request's own first.
- Write `{"results": [{"payload_id": n, "ok": ...} or {"payload_id": n, "error": "..."}], "overall": ...}` to the path in `SCOPEWORK_JOB_RESULTS`. A payload without a result is answered `no result`.
- A non-zero exit fails every payload with `job_failed`.
- Each request's own result decides its exit code, whether it ran the command, joined another's run or ran without a coordinator: the command's non-zero exit, otherwise 1 when its result is an `error` (including `no result`), otherwise 0.
- `SCOPEWORK_JOB_LEASE` holds the lease number.

**Leases end with the holder.** A lease lasts as long as its holder's connection, so a holder that dies releases it at once. Requests that had joined it and were not yet answered wait again at their original place, and may run themselves next.

**Several resources** are granted together or not at all, and a waiting request reserves its resources against later ones. Nobody holds a resource while waiting, and requests cannot starve.

**Exclusion is the same in every mode.** A `job run` always takes exclusive lock files per resource, in name order, under `.project-locks/` (`research locks` shows them under `resources`). With a coordinator it takes them once its lease is granted, so the lease decides order and joining and the files exclude jobs started with `SCOPEWORK_COORDINATOR=off`. Without a coordinator (`off`, or none can start), the files alone order the requests. Every request then runs alone with its own payload, so results are the same, only without joining.

**Pipe clients.** A tool that keeps a connection, such as a Python module, can lease directly. Once granted, it must also hold the resource lock files described above while it works, as `job run` does, or direct-mode jobs could run beside it. After the hello (role `tool`), it sends:
- `{"id": n, "lease": {"project", "resources": [...], "key", "class", "kind", "payload"}}`, and waits for one of:
  - `{"granted": {"lease", "payloads"}}`;
  - `{"done": {"by", "result"}}`;
  - `{"covered": {"by", "result"}}`.
- When granted, `{"id": n, "results": [...], "overall": ...}`, then `{"id": n, "release": lease}`, or it closes the connection.

### Shared resources

A resource that belongs to no project, such as a verification worktree every session commits through, is leased home-wide:

```powershell
research --wait 3600 job run --shared --resource verify-worktree --class job -- bash mycommit.sh
research queue --shared
research job status --shared
```

`--shared` replaces the project and needs `--resource` (shared resources have no declarations). Everything else is as for a project lease: ordering, classes, joining with `--key`, payloads, results and exit codes, and release when the holder dies. The command runs in the caller's current folder, and its payload and result files go in the research home's `tools/scratch`. Without a coordinator, exclusive lock files named `_shared.res-NAME` under `projects/.project-locks/` order the requests with fair tickets; they wait at most the lock wait, so give a long `--wait` for work that takes minutes. With a coordinator, a request waits until it is granted. `queue --shared` and `job status --shared` show held and waiting shared leases, every project's `queue` carries them as `shared_leases`, and the lock files held by jobs run without a coordinator as `resources` and `shared_resources` (in both modes, since those exclude leased work too), and the dashboard's queue panes list them marked `shared`. This replaces hand-written lock directories and ticket scripts kept outside Scopework.

### Declarations: `tools/coordinator.toml`

```toml
version = 1

[resources.ledger]
files = ["tools/profiles/annotations.json"]   # watched for drift

[resources.ghidra]                            # exclusive; no files watched

[kinds.ledger-put]
resources = ["ledger"]
key = "ledger-put"
class = "normal"                              # normal, bulk or job
```

Flags override every field (`--resource`, `--key`, `--class`), and an undeclared kind can lease with `--resource`. Names are lowercase words joined by hyphens. Unknown fields and other mistakes are refused with their line.

### Drift

While a coordinator runs, it polls each project's shared files every 2 s:
- **Declared files:** a change is drift when no lease on their resource was held, granted or released since the previous poll.
- **Catalog and sidecars:** a change to `project.json`, or to the review-flag, claim or review-queue sidecars, is drift unless the catalog revision (or layout version) also changed. Every Scopework write raises the revision.
- **Ledger repaths:** the annotation ledger's in-place repath by a catalog transaction is recognised from its receipt.

Drift is recorded as a history entry of kind `drift`, naming the file, the resource and both hashes, and is counted in `coordinator status`. It is never blocked or reverted. `research history show example --kind drift` lists the entries.

`research tools audit` is advisory: a textual scan of `tools/scripts` and `tools/instrumentation`. It reports:
- the project's own modules that tools import;
- the `research` subcommands and subprocess executables tools use;
- declared kinds against the kinds referenced or leased: declared but unused, and used but undeclared;
- drift entries per resource.

It cannot tell which files a tool writes; drift entries record that at runtime.
