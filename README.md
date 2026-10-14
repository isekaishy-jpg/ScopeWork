# Research Task Manager

Rust desktop research dashboard with the approved classic Task Manager layout, a modern dark palette, two meters per row, and separate chunk/function counts.

## Research records

Open **View > Research records...**. Stock and Forever start with separate app reports. Create future projects in the same window.

Each project owns a complete hierarchy:

```
data/projects/<project-id>/
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

### Completion

Use the completion checkbox only after reviewing the report. Completion requires a written report, function evidence where applicable, and every required child complete. Empty parent reports and parents without children cannot be completed. Changing a child or adding a child reopens completed ancestors. SHA-256 checks detect externally changed completed files, and the editor rejects saves over a concurrently changed file. Use View > Refresh research progress after external edits. Completion is a human attestation supported by these structural checks, not automated judgment of research quality.

Save or discard edits before navigating. Unsaved report edits prevent closing the app. One application instance can write the research library at a time.

When a project has architecture records, its dashboard uses them: one meter per architecture, one existing-size segment per chunk (green complete, yellow in progress, dim red not started), and separate function counts. Double-click a meter to open its architecture report. The App report's completion status is separate from function coverage. The existing previews remain explicitly labeled for projects without architecture records; no historical completion claims are imported.

All research data is local and excluded from Git. Default root is `data/projects` in this checkout; set `RESEARCH_DASHBOARD_HOME` to choose another library root before launching. Projects have separate IDs, trees, metadata, evidence directories, and document membership. Do not copy research between projects without an explicit evidence reference and scope review.

## Run

```powershell
cargo run --release
```

Windows executable: `target/release/research-dashboard.exe`.

Options > Dark mode switches palettes and saves the choice. Other menus provide Exit, Always on top, research records, progress refresh, Clear chunk selection, and About. The custom title bar supports dragging, minimizing, maximizing, and closing; the bottom-right grip resizes the window.

## Preview mode

The local ignored `data/stock-preview.json` file supplies the Stock assumptions preview. Its function counts came from the existing index; its chunk counts are hypothetical. Forever's preview uses illustrative data. Preview meters allow segment counts to be changed through their context menu. Canonical meters derive segment counts from their chunk records.

Every function has its own folder: `research.md` plus `data/`. Use relative `data/<file>` links from that report. There is no new project-level evidence or data directory. Shared tool state lives under `tools/workspaces`, `tools/logs`, and `tools/scratch`. The research editor has an **Open this function's data folder** button.

Existing flat canonical reports migrate to the folder layout when the library opens. Generated source-report links are rebuilt; ordinary relative inline Markdown links are rebased. Affected completion records reopen for review. The app refuses to overwrite a conflicting destination. Empty obsolete folders are removed; any historical unowned captures/exports are preserved under `tools/unassigned` for deliberate assignment to functions, not attributed automatically. Shared old analysis data moves to `tools/workspaces/analysis`. Use `research-dashboard.exe --migrate-library` to apply folder migration without opening the GUI.

Meter pairs use padded inset panels sized to their own meter and text widths. The second panel follows the first with a small gap rather than a fixed half-window offset. Large meters grow their own row and remain scrollable. The total-progress meter occupies a separate full-width panel below the meter bank.

## Function prerequisite tags

Each function folder also contains `dep.toml` (functions this report requires) and `prereq.toml` (functions that require this one). Both include project/binary identity, function RVAs, and relative canonical-report links. Tags stay within one project, do not duplicate research files, and do not add functions to coverage totals.

The app maintains both directions from the project's dependency graph in `project.json`; these two TOML files are generated views. In a function's research editor, enter prerequisite RVAs separated by commas and use **Save prerequisite tags**. The reverse list is generated automatically. Self-dependencies, missing functions, duplicates and prerequisite cycles are rejected. These are understanding prerequisites, not a complete machine-code call graph.

A dependent function cannot be marked complete until its prerequisites are complete. Its completion records which prerequisite revisions it used. Changes to a shared function reopen transitive dependents and their owning chunk/architecture/app reports. External changes are detected by the report hash checks. Merely referencing an already understood function does not reopen that shared function. Re-saving unchanged research does not invalidate dependents.

Agents can update tags without hand-editing two files. Close the GUI first to release the library write lock:

```powershell
research-dashboard.exe --set-prerequisites stock 0x00123456 0x000ABCDE 0x00012345
```

The first RVA is the dependent function; subsequent RVAs replace its prerequisite list. Omit subsequent RVAs to clear the list. All function records must already exist in that project. Use `RESEARCH_DASHBOARD_HOME` for a non-default library. Do not edit the generated TOML files directly; use this command or the app so both directions remain consistent.
