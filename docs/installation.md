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

The GUI and CLI use the same home, containing `projects/` and one shared `complaints/` folder. Run `research home` to see its location.

Selection precedence is:

1. CLI `--home` for that command.
2. `RESEARCH_DASHBOARD_HOME` environment variable.
3. The home saved by the installer.
4. The default local home. An existing earlier `ResearchDashboard` home is retained when present.

Changing the selected home opens another library. Scopework never automatically imports or moves existing research.

## Upgrades and removal

Close Scopework and install a newer-version MSI. Setup reuses saved location and option choices. The app has no automatic update checker or downloader.

Re-running the same-version MSI offers Repair or Uninstall. Uninstall removes application files, shortcuts, and its PATH entry. Research files and the saved research-home setting are preserved.
