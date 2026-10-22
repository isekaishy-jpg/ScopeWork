# Research: files that hold nothing

Status: research, 2026-10-22. TODO item "Stop writing files that hold nothing".

## Measured

In `3-3-5a` the record folders hold 125,934 files in 62,397 folders: 34 MB of data occupying about 494 MB of 4 KB clusters. Per function there are four entries besides the report, and only the report holds research. Of 30,682 `data/` folders, 75 hold anything (110 files); `populous-snes` uses 2 of 390 and `populous-amiga` none of 445.

## Each entry: who writes it, who reads it

### `dep.toml` and `prereq.toml` (prerequisite views)

- **What:** generated views of the prerequisite graph in `project.json`, one per direction. Each repeats the project id, the full binary identity text and the function RVA, then lists linked functions (empty for nearly every function: 1,297 edges across 30,682 functions).
- **Written by:**
  - the import, empty, for every function (`imports.rs`, `render_files`);
  - `sync_dependency_subset` for the records a commit's dependency change touches (`catalog_batch.rs`, applying a journal);
  - `sync_dependency_files`, all functions, during layout migration (`layout.rs`).
- **Read by:** nothing in Scopework and nothing in the project tools. `research deps get` and the GUI's prerequisite fields answer from the catalog. `docs/research-model.md` and `docs/cli.md` describe the files as generated views for people and say never to edit them.
- **Stale by design today:** a binary-identity amendment (`project set-binary --expect-revision`) re-renders reports but does not regenerate these files, so their copied identity text goes stale.

### `.scopework-import-<job>` (import ownership marker)

- **What:** a 36-byte file holding the import job id, written into each folder an import creates.
- **Purpose:** while an import runs, it proves which folders the job owns, so resuming after a crash accepts its own folders and a cancel or rollback removes only files the job created (`install_folder`, the cleanup at the end of `imports.rs`).
- **Removed by:** only the cancel and rollback path. After an import commits, nothing removes the markers, and nothing reads them again: a committed job cannot be resumed or cancelled.

### `data/` (record data folder)

- **Created by:**
  - the import, for every function (`install_folder`);
  - every journal application that edits a function's report (`catalog_batch.rs`, line 402), which recreates the folder on each save.
- **Used by:** people and tools that store captures and exports there. `node show` returns the path. The project tools create subfolders themselves when they store something (`route_batch_artifacts`).
- **GUI:** "Open data folder" passes the path straight to Explorer (`records_ui.rs`, `open_folder`), so it relies on the folder existing.

## Options for the design step

1. **Prerequisite views.** (a) Stop writing them; `deps get` and the GUI already serve the information. (b) Write them only for functions that have prerequisites in either direction (about 2,600 files instead of 61,364) and remove empty ones. (c) Replace them with one project-level view file. Each option needs `docs/research-model.md` and `docs/cli.md` updated, since both promise the per-function files. Whichever remains should also be regenerated when the binary identity is amended.
2. **Import markers.** Remove a job's markers when the job commits; a cleanup removes the ones left by past imports.
3. **`data/` folders.** Create the folder only when something is stored: stop creating it at import and on report saves, and have "Open data folder" create it before opening. Remove empty folders from existing projects.

## Compatibility and migration

- Existing projects: a one-time cleanup removes empty `data/` folders, committed-import markers and, depending on option 1, the views. It must not touch a folder that holds anything, nor the markers of a job still running.
- Older Scopework versions do not read these files. An older build that applies a dependency change or a report save would recreate them for the records it touches; that is harmless.
- The project tools are unaffected: none reads the views or markers, and they create `data/` subfolders as needed.
- Expected result for `3-3-5a`: from 125,934 files and 62,397 folders to about 31,600 reports, plus a few thousand views if option 1(b) is chosen, and about 31,500 record folders.
