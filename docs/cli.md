# Agent CLI

Install the MSI, open a new terminal, and use `research --help`. The GUI executable is `research-dashboard.exe`; the console executable is `research.exe`. Both use the same backend and completion rules. All successful CLI commands print JSON. Operation errors print JSON to stderr and exit 1; argument errors exit 2.

## Research home

The default is `%LOCALAPPDATA%\ResearchDashboard`, independent of the installation folder or current directory. `RESEARCH_DASHBOARD_HOME` selects another absolute home; CLI `--home PATH` overrides it for one invocation. It contains `projects/` and one shared `complaints/` folder. `research home` prints these paths without creating anything. `research init` creates fresh empty Stock/Forever projects; ordinary research commands and GUI startup also initialize the home when needed.

Nothing is imported or migrated. Carry-over is manual. Uninstall removes the installed application, shortcut and its PATH entry, and leaves the research home intact.

## Commands

```powershell
research home
research init
research gui
research project list
research project create example --name "Example" --scope "Archive lookup only; exclude rendering and networking." --binary "Build ...; SHA-256 ...; image base ..."
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

Use `project create --scope "..."` or set a longer UTF-8 Markdown scope from a file:

```powershell
$project = research project show example | ConvertFrom-Json
research project set-scope example --scope-file C:\work\scope.md --expect-revision $project.revision
```

State the objective, included behavior, exclusions and boundaries. The scope may cover a whole binary or a selected part. It is required before completing records; `node show`, `project show/list`, and `status` include it. The app report represents completion of this declared scope, not automatically the entire executable. Agents must review that the hierarchy covers the scope; the app cannot infer coverage from prose. Changing scope reopens app synthesis for review and rejects stale project revisions. Update affected child assignments explicitly.

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

The dashboard can stay open during CLI work. Writes reload the latest catalog under a short exclusive lock. Concurrent operations receive a busy error and may retry; conflicting report edits require review. Use **View > Refresh research progress** to display CLI changes. Unsaved editor text is retained and stale saves are rejected.

## Complaints

Put one narrow Markdown report per app issue in the `complaints` path printed by `research home`. Filing the report is the whole process.
