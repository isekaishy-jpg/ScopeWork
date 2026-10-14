# Research Task Manager

Rust desktop research dashboard with the approved classic Task Manager layout, a modern dark palette, two meters per row, and separate chunk/function counts.

## Research records

Open **View > Research records...**. Stock and Forever start with separate app reports. Create future projects in the same window.

Each project owns a complete hierarchy:

```
data/projects/<project-id>/
  project.json                 # IDs, hierarchy, status, revisions; no report prose
  app.md                       # one complete-app research report
  architectures/000001.md      # one canonical report per category
  chunks/000002.md              # one canonical report per chunk
  functions/00001234.md         # one canonical file per function RVA
  evidence/                    # this project's supporting artifacts
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
