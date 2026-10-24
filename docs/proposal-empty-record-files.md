# Proposal: stop writing files that hold nothing

Status: proposed 2026-10-24. Research: [research-empty-record-files.md](research-empty-record-files.md).

## Problem

Besides its report, each function record carries three entries that almost never hold anything: the `dep.toml` and `prereq.toml` prerequisite views, an import marker `.scopework-import-<job>`, and an empty `data/` folder. In `3-3-5a`, 34 MB of data occupy about 494 MB in 125,934 files and 62,397 folders. The markers also stop `node prune` from removing empty organization nodes: it treats them as supporting files (2026-10-23, 92 empty nodes).

## Design

1. **Prerequisite views are no longer written.** Nothing reads them: `research deps get`, `node show` and the GUI answer from the catalog. The import stops rendering them. Commits that change prerequisites and layout migration stop regenerating them. Journals keep their `dependencies` list, which marks a prerequisite change as structural (it never merges), and pending journals from older builds stay readable.
2. **Import markers are removed when the import commits.** They prove which folders an import owns only while it can still be resumed or cancelled. After the publication commits, the job's markers are removed from every folder it installed. A failure to remove one is a warning, not an error: the import has committed.
3. **`data/` folders are created only when used.** The import and report saves no longer create them. **Open data folder** in the GUI creates the folder first. `node show` still returns the path, which may not exist yet. Project tools already create the subfolders they write.
4. **`research project tidy PROJECT [--apply]` cleans existing projects.** Without `--apply` it only reports. With it, under the project's write lock, it removes:
   - `dep.toml` and `prereq.toml` files that begin with Scopework's generated header;
   - import markers of jobs other than the project's active import;
   - `data/` folders that are empty.

   It removes nothing else. A file without the generated header, a non-empty `data/`, and the active import's markers are kept and counted. `fs::remove_dir` fails on a folder that is no longer empty, so a tool that writes into `data/` at the same moment keeps its file. The result counts what was removed, kept and failed. `--apply` appends a `project-tidy` entry to the tooling history.
5. **Docs** (`research-model.md`, `cli.md`, `AGENTS.md`) stop promising per-function views and always-present `data/` folders.

Not included: a project-level prerequisite view file, which would be a new feature. The catalog already answers both directions through `deps get`.

## Compatibility

- Older builds still write views and folders for the records they touch, which is harmless.
- An older GUI's **Open data folder** fails for a folder that `tidy` removed. Run `tidy` on a project after its users have this build installed.

## Acceptance

After an import in this build, no record folder holds views, markers or an empty `data/`. A report save creates no `data/`. A prerequisite change writes no views. `project tidy --apply` on a project with generated views, committed markers, empty and non-empty `data/` folders, a hand-written `dep.toml` and the active import's marker removes exactly the generated, committed and empty ones.

## Tests

Integration tests on small fixture projects:
- an import leaves no views, markers or empty `data/`;
- save and deps set write none;
- `tidy` preview and apply against a fixture with every case above, including the result counts and the history entry;
- `prune` selects an empty chunk once `tidy` has removed its marker.
