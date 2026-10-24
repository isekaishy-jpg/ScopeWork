# Proposal: project groups

Status: implemented 2026-10-24. Research: [research-project-groups.md](research-project-groups.md).

## Problem

The dashboard draws one row of project tabs in name order. Related projects end up apart: `3.3.5a` sorts first and `WoW 3.3.5a` last, with the Populous family in between. The eight tabs already fill the 1,060-pixel minimum window width, so a ninth project overflows the row out of reach.

## Design

1. **Stored in the research home**, in a new `settings.json`: `{"schema": 1, "groups": [{"name": "WoW", "projects": ["3-3-5a", "wow-335a-12340"]}]}`.
   - One level of groups; each project is in at most one group.
   - Groups are presentation only: they change no project data or history.
   - Writes take an exclusive lock on `.settings.lock` and replace the file atomically, so the GUI and the CLI can both write it.
   - Projects that no longer exist are ignored when shown, and dropped when their group is next written.
2. **CLI**:
   - `research group list`: groups with their projects, then the ungrouped projects.
   - `research group set PROJECT GROUP`: puts the project in that group, creating it and moving the project out of any other.
   - `research group clear PROJECT`: makes the project ungrouped.

   A group left with no projects is removed. Group names are one line of at most 40 characters. These commands read and write only the settings file, never a project's catalog.
3. **Dashboard: one row, a tab per group.** The tab row keeps its place and height, so the approved layout does not move. The row holds the ungrouped projects and one tab per group, all in name order.
   - A group tab reads `Group: Project ▾`, naming the group's project currently shown.
   - Clicking a group tab shows that group's last shown project (its first project by name at the start).
   - Clicking the tab again, or its ▾, opens a menu listing the group's projects in the classic menu style.
   - A project chosen from the menu becomes the group's shown project.

   With no groups, the row is exactly as today.
4. **Assignment from the GUI.** Research records > Project gains a **Dashboard group** box: a field holding the project's group, **Set group** and **Ungroup**. The dashboard rereads `settings.json` when its modification time changes, checked at most once a second.

Not included:
- an explicit order for groups or for the projects within them (name order, as the tabs use today);
- nested groups;
- renaming a group in one step (`group set` each project).

## Acceptance

- `group set`, `group clear` and `group list` keep each project in at most one group, remove empty groups, refuse unknown projects and invalid names, and leave project catalogs untouched.
- Concurrent writers do not lose each other's changes.
- On the dashboard, grouped projects share one tab whose menu switches between them. The dashboard is unchanged when there are no groups.

## Tests

- Unit tests for the settings rules: assignment, moving between groups, removal of empty groups, name validation, and ignoring missing projects.
- An integration test of the three CLI commands on a fixture home.
- A unit test of how the dashboard builds its tab list from projects and groups.
- A headless test of the dashboard: in a fixture home, a group tab selects its project, a second click opens its menu, choosing the second project switches to it, and Escape closes the menu.
- A capture of the dashboard with a group tab (manual check).
