# Research Task Manager

Rust desktop research dashboard with the approved classic Task Manager layout, a modern dark palette, two meters per row, and separate chunk/function counts.

## Research records

Open **View > Research records...**. Stock and Forever start with separate app reports. Create future projects in the same window.

Each project owns a complete hierarchy:

```
%LOCALAPPDATA%/ResearchDashboard/projects/<project-id>/
  project.json
  app.md
  architectures/<id>/research.md
  chunks/<id>/research.md
  functions/<RVA>/
    research.md
    data/                       # this function's captures, exports and raw evidence
  tools/
    README.md
    profiles/
    scripts/
    instrumentation/
    bin/
    workspaces/                 # shared Ghidra / whole-binary analysis databases
    logs/                       # shared tool run provenance
    scratch/                    # shared temporary working data
```

Select a parent and create its child: App -> Architecture -> Chunk -> Function. Architectures are categories such as Asset Loading or Models. Enter the exact binary build/hash and image base before adding functions. Function identity is an RVA within that project's binary. A function cannot be duplicated elsewhere in the same project. Different binaries require separate projects.

The editor updates each canonical Markdown file in place. Parent reports link their direct source reports: architecture to chunks, chunks to function files. Function evidence remains in the function file instead of being copied into parallel reports. Parent synthesis is authored in the parent's own report; linking children alone does not complete it.

### Project research scope

Research Scope is a short topic label: for example, **Rendering** or **Asset platform**. Set it in **View > Research records > Research scope**. It appears in the header above the selected project's meters, and in CLI output so agents know what is being researched.

The label can describe a selected part of a binary. Editing it does not change completion states or assignments. Detailed understanding and completion criteria remain in the research reports and hierarchy.

### Completion

Research uses fixed assignments within expanding levels of understanding:

- **Function:** Establish its behavior, inputs, outputs, side effects, boundaries and connections with evidence.
- **Chunk:** Synthesize completed function reports to explain the chunk's behavior, control flow, data flow and external interactions.
- **Architecture/category:** Synthesize completed chunk reports to explain subsystem behavior and interfaces.
- **App:** Synthesize completed architecture reports to fulfill the project's declared research scope and explain relevant subsystem interactions.

Completed children make a parent ready for synthesis; they do not automatically complete it. Each parent needs a separate review of its own understanding. The research editor shows completion criteria for the selected scope beside the completion control.

Keep each assignment's scope fixed. Record missing knowledge outside it as a separate research task, and make it a prerequisite only when necessary to establish the current scope's behavior. A connection or call edge alone does not expand the assignment into the entire reachable graph. Known shared behavior is referenced through its canonical function report.

Use the completion checkbox only after reviewing the report. Completion requires a written report, function evidence where applicable, and every required child complete. Empty parent reports and parents without children cannot be completed. Changing a child or adding a child reopens completed ancestors. SHA-256 checks detect externally changed completed files, and the editor rejects saves over a concurrently changed file. Use View > Refresh research progress after external edits. Completion is a human attestation supported by these structural checks, not automated judgment of research quality.

Save or discard edits before navigating. Unsaved report edits prevent closing the app. GUI and CLI share short exclusive write operations. The GUI can stay open while agents work; use View > Refresh research progress to display their changes. Stale report or project revisions require reloading and review before saving.

When a project has architecture records, its dashboard uses them: one meter per architecture, one existing-size segment per chunk (green complete, yellow in progress, dim red not started), and separate function counts. Double-click a meter to open its architecture report. The App report's completion status is separate from function coverage. The existing previews remain explicitly labeled for projects without architecture records; no historical completion claims are imported.

Research lives outside the installation and checkout. The default home is `%LOCALAPPDATA%\ResearchDashboard`, containing `projects/` and a shared `complaints/` folder. Set `RESEARCH_DASHBOARD_HOME` to another absolute home before launching, or use CLI `--home PATH`. Projects have separate IDs, trees, metadata, evidence directories, and document membership. Do not copy research between projects without an explicit evidence reference and scope review.

## Run

Agents file narrow app complaints in the single `complaints/` folder under the research home, one Markdown report per issue. `research home` prints its path. Filing the report is the entire process. The [folder guide](complaints/README.md) is bundled into new homes.

```powershell
cargo run --release
```

Windows executable: `target/release/research-dashboard.exe`.

Options > Dark mode switches palettes and saves the choice. Other menus provide Exit, Always on top, research records, progress refresh, Clear chunk selection, and About. The custom title bar supports dragging, minimizing, maximizing, and closing; the bottom-right grip resizes the window.

## Preview mode

The optional `stock-preview.json` file in the research home supplies the Stock assumptions preview. Its function counts came from the existing index; its chunk counts are hypothetical. Forever's preview uses illustrative data. Preview meters allow segment counts to be changed through their context menu. Canonical meters derive segment counts from their chunk records.

Every function has its own folder: `research.md` plus `data/`. Use relative `data/<file>` links from that report. There is no new project-level evidence or data directory. Shared tool state lives under `tools/workspaces`, `tools/logs`, and `tools/scratch`. The research editor has an **Open this function's data folder** button.

There is no migration or import workflow. Existing material is carried over manually; the app never moves historical research into a new home.

Meter pairs use padded inset panels sized to their own meter and text widths. The second panel follows the first with a small gap rather than a fixed half-window offset. Large meters grow their own row and remain scrollable. The total-progress meter occupies a separate full-width panel below the meter bank.

## Function prerequisite tags

Each function folder also contains `dep.toml` (functions this report requires) and `prereq.toml` (functions that require this one). Both include project/binary identity, function RVAs, and relative canonical-report links. Tags stay within one project, do not duplicate research files, and do not add functions to coverage totals.

The app maintains both directions from the project's dependency graph in `project.json`; these two TOML files are generated views. In a function's research editor, enter prerequisite RVAs separated by commas and use **Save prerequisite tags**. The reverse list is generated automatically. Self-dependencies, missing functions, duplicates and prerequisite cycles are rejected. These are understanding prerequisites, not a complete machine-code call graph.

A dependent function cannot be marked complete until its prerequisites are complete. Its completion records which prerequisite revisions it used. Changes to a shared function reopen transitive dependents and their owning chunk/architecture/app reports. External changes are detected by the report hash checks. Merely referencing an already understood function does not reopen that shared function. Re-saving unchanged research does not invalidate dependents.

Agents update tags with `research deps set`; see the [CLI guide](docs/cli.md). The dashboard may remain open. Both TOML files are maintained by the app; do not edit them directly.

## Windows installation and CLI

Build the installer with `powershell -NoProfile -ExecutionPolicy Bypass -File installer/build.ps1` (Rust and .NET SDK required). This permits the build script for that process only. The script installs a pinned local WiX build tool if needed and produces `dist/ResearchDashboard-0.2.1-x64.msi`.

The per-user MSI installs into `%LOCALAPPDATA%\Programs\Research Dashboard`, adds that directory to the user PATH, and creates a Start menu shortcut. Open a new terminal after installation. Use `research --help` for the console CLI, `research gui` to open the dashboard, and `research home` to locate research. The research home is initialized on first use. Uninstall removes the program and its PATH entry while preserving research. The MSI is unsigned; no signing certificate is configured.

The [CLI guide](docs/cli.md) covers project/tab creation, hierarchy creation, report reading/writing, reviewed completion/reopening, prerequisite tags and progress. Successful commands emit JSON. CLI and GUI enforce the same hierarchy, dependency and completion rules.

## Updating an installed app

Install a newer-version MSI over the existing per-user installation. The stable MSI UpgradeCode and MajorUpgrade rule replace the older program; research remains in its separate home. Close the dashboard first. There is no automatic update checker or downloader. Releases must increment the three-part version in Cargo.toml; the MSI build script reads that version so CLI and installer versions stay aligned. Older-version MSI installation is blocked.
