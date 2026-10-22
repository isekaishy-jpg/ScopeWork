# Research: project groups

Status: research, 2026-10-22. TODO item "Project groups".

## Request

Group related project tabs on the dashboard: several WoW builds (including two projects for the same build), and several Populous projects that tie back to one main project. Mostly a UI change.

## Current behaviour

- The dashboard draws one row of project tabs, left to right, with no overflow handling (`main.rs`). A tab is the name's width plus 24 pixels, at least 61.
- Projects are sorted by display name (`research.rs`, opening the library).
- The window opens 1,100 pixels wide, minimum 1,060.
- The GUI persists one setting, dark mode, in egui's own per-user storage. Nothing else is a setting: the research home has no settings file, and the registry holds only the install location and the chosen home (`paths.rs`).

## Measured

| Project | Name | Tab width (approx.) |
|---|---|---|
| `3-3-5a` | 3.3.5a | 61 |
| `forever` | Forever | 63 |
| `populous-amiga` | Populous (Amiga) | 114 |
| `populous-snes` | Populous (SNES) | 108 |
| `populous-editor-amiga` | Populous World Editor (Amiga) | 186 |
| `populous-final-frontier-amiga` | Populous: The Final Frontier (Amiga) | 226 |
| `populous-promised-lands-amiga` | Populous: The Promised Lands (Amiga) | 226 |
| `wow-335a-12340` | WoW 3.3.5a | 80 |

The eight tabs need about 1,060 pixels: the minimum window width. **One more project overflows the row**, and tabs past the edge cannot be reached. Name order also separates related projects: `3.3.5a` sorts first and `WoW 3.3.5a` last, with the Populous projects between them.

## Requirements

- Group related projects; each project in at most one group; ungrouped projects remain.
- Groups are a presentation layer: they change neither project data nor history. (Real links between projects, such as data disks loaded by the base game, are a separate gap in the storage research.)
- Assignable from the GUI and from the CLI (agents set up projects too).
- The tab row must stay usable as projects are added.

## Where groups could be stored

| Place | Seen by | Notes |
|---|---|---|
| egui's per-user storage (like dark mode) | The GUI only | The CLI cannot read or set it |
| A settings file in the research home (new) | GUI and CLI, every machine that uses the home | Travels with the projects; first home-level settings file |
| A field in each project's catalog | Everything | Puts a display preference into project data and history, against the requirement |
| The registry | GUI and CLI on this machine | Separate from the home; does not travel with it |

A home-level settings file fits the requirements. It is also where the storage backend's home-level data (groups, cross-project references) will live.

## Display options for the design step

1. **Group row, then project row:** one row of group tabs (plus "Ungrouped") above the selected group's project tabs. Short rows; scales to many groups.
2. **Group tabs with a drop-down** listing the group's projects. One row; the active project's name must be visible on the tab.
3. **Grouped single row** with group captions. Still overflows as projects grow.

Each needs a rule for order within a group (name order, or an explicit order set in the settings) and for which group and project the dashboard opens on (the last used, which the GUI already remembers for the project).
