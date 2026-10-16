# Install and update Scopework

Scopework currently targets 64-bit Windows. Published builds appear on the [GitHub releases page](https://github.com/isekaishy-jpg/ScopeWork/releases). You can also [build the MSI locally](building.md).

## Setup choices

The wizard shows Welcome, Options, Summary, Progress and Finish pages. Before installation you can choose:

| Choice | Default for a new installation |
| --- | --- |
| Application folder | `%LOCALAPPDATA%\Programs\Scopework` |
| Research home | `%LOCALAPPDATA%\Scopework` |
| Agent CLI on user PATH | Enabled |
| Desktop shortcut | Enabled |

A Start menu shortcut is always installed. The desktop shortcut and PATH entry are optional. The Summary page displays your choices before applying them. Scopework installs for your Windows account.

Open Scopework from Start or the desktop shortcut. If PATH was enabled, open a **new terminal** and run `research --help`. Otherwise, run `research.exe` from the selected application folder.

The MSI is currently unsigned. Installation and upgrade validation status belongs in the notes for each release.

## Your research home

The GUI and CLI use the same home, containing `projects/` and one shared `complaints/` folder. Run `research home` to see its location. New homes start without projects; choose **Create project** to begin.

Version 0.4.1 removes the development defaults and sample meters. Existing project folders are preserved, including any Stock or Forever projects created by earlier versions. Their dashboards now show only canonical records.

Selection precedence is:

1. CLI `--home` for that command.
2. `RESEARCH_DASHBOARD_HOME` environment variable.
3. The home saved by the installer.
4. The default local home. An existing earlier `ResearchDashboard` home is retained when present.

Changing the selected home opens another library. Scopework never automatically imports historical research or moves projects between homes.

## Upgrades and removal

Close Scopework and stop agent CLI/tool jobs before installing a newer-version MSI. Setup reuses saved location and option choices. The app has no automatic update checker or downloader.

For local development, you can run the rebuilt `target/release/research-dashboard.exe` directly without reinstalling. It uses the same research-home selection. Installed shortcuts continue to launch the installed copy until you upgrade it.

Re-running the same-version MSI offers Repair or Uninstall. Uninstall removes application files, shortcuts, and its PATH entry. Research files and the saved research-home setting are preserved.

Setup uses Windows Installer's embedded cabinet rather than a separate app-owned extraction/updater cache. Windows manages its installation scratch and repair cache; Scopework does not delete that cache or your downloaded MSI. Empty application, documentation, asset and shortcut directories are removed on uninstall/upgrade. Unknown files in those directories are preserved. Package-build temporary files are cleaned on success or failure.

## Version 0.6.0 journal upgrade

Finish or cancel active imports with the previous version, then close all old GUI/CLI processes. The first project open upgrades its compatibility marker to format 5 under an exclusive library gate, retaining one pre-upgrade catalog in `tools/workspaces/layout-migration-v5`. Reports and supporting evidence do not change. Older executables reject format 5 because they cannot recover the new delta journals.

New edits retain only changed catalog records plus affected report and sidecar contents. To inspect existing completed history, run `research journal PROJECT`; to compact it without deleting history, run `research journal PROJECT --compact`. Each completed journal is replaced atomically. An interrupted compaction can be run again. History inspection and compaction bypass project migration and pending recovery; pending journals remain untouched. See the [CLI guide](cli.md) for retention and archive semantics.

## Version 0.4.2 folder upgrade

On first GUI or CLI library open, existing current projects migrate to `architectures/<category-id>/<chunk-id>/<RVA>/`. Each level owns its report; functions retain their complete data folders. New projects use this layout immediately. Keep old app/CLI copies closed after upgrading.

A retained journal in each project at `tools/workspaces/layout-migration-v2/journal.json` records original catalog/report text and old/new paths. Interrupted upgrades resume; conflicting files stop migration without overwriting them. Standard report references, generated dependency files and schema-1 annotation report references/hash pins are updated. Custom scripts must resolve canonical paths from the CLI instead of assuming flat folders.

Project tabs and progress refresh automatically about every two seconds. Removing a project folder removes its tab; surviving tabs retain their identity. Unsaved edits for a removed report remain available to copy or discard and cannot be saved into a different project.
