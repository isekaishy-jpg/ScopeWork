# Research: the implementation tracker

Status: research, 2026-10-22. TODO item "Implementation tab (tracker)".

## Request (user, 2026-10-22)

An inner tab for the implementation (the recreation), more free-form than the research tree:

- The tree is the code's own organisation (crates, modules, files), decided by the agent; it need not match the research architectures.
- Items are the researched functions and data, placed where they are implemented, plus implementation-only items needed to make the recreation match.
- Each item has a completion check box; completion is the agent's discipline. It is a tracker; the real proof is the finished product running.
- Counters per node (done of total, for example `0/1,146`), no meters.
- The project records its implementation repository. Agents create the structure and place items.
- From an implementation item an agent can go back to the related research records, to audit or fix them when something was missed.

## What exists to build on

- **Dashboard** (`main.rs`): project tabs, then a panel of meters per architecture with a "Selected meter" detail. An inner tab would sit inside the project panel, switching between the research meters and the implementation tree.
- **Research records window** (`records_ui.rs`): records list on the left (the virtualised tree browser in `catalog_browser.rs`, with filters and search), record detail with Report, Organize and Project tabs on the right. The browser can be reused for the implementation tree.
- **Journaled state** (claims, flags, queue, quarantine) shows the pattern for tracker state until the storage backend exists: a journaled sidecar with history.
- **CLI conventions:** grouped subcommands, JSON answers, record ids.

## The live example (`wow-335a-rust`, branch `proof-harness`)

- A workspace with the crate `wow335a` (65 source files under `src/`) and a `harness` crate. Modules: `crt` (heap, small-block heap, locks, threads, errno, locale, SEH, the C++ exception family across a dozen files), `stl`, `storm`, `blizzard`, `stub`, `x86`.
- Each recreation names its original in a doc comment, for example `` `__sbh_find_block` (VA `0x004133A3`, node 1567) ``. Which file implements which research record can therefore be read from the repository. That is useful to check the tracker against the code, without making placement automatic.
- The modules do not follow the research architectures: the CRT spans several research chunks, and `blizzard` and `storm` cut across others. The user's free-form requirement matches what already happens.

## Requirements found

1. **Tree:** free-form named nodes (crate, module, file or any grouping), any depth, ordered, created, renamed and moved by agents and people.
2. **Placements:** a research record (function or data) placed on a tree node; a record placed in at most one node per implementation. Records not yet placed are visible as such.
3. **Implementation-only items:** named entries on a node, with their own completion mark and counter.
4. **Completion mark per placement and per implementation-only item,** recorded with who set it and when; history like other changes.
5. **Counters per node:** placed research items done of total, and implementation-only items done of total, rolled up the tree. No meters.
6. **Links both ways:** from a placement to its research record (open it in Research records), and from a research record to where it is implemented.
7. **Repository link:** the implementation repository's path (and remote) per implementation.
8. **CLI for agents:** create, move and rename nodes; place records; add implementation-only items; mark complete; list counters.
9. **Scale:** up to every function and data item of a project (`3-3-5a`: 30,682 functions; `forever`: 150,652).

## Open questions for the design step

- **One implementation, several projects:** the Sol client draws on WoW and Forever research. Is an implementation owned by one project, or a home-level object placing records from several projects? This meets the storage research's gap on links between projects.
- **Where the inner tab lives:** on the dashboard (per project, beside the meters), in the Research records window, or both.
- **Consistency checks:** whether Scopework compares placements with the doc-comment references in the repository and reports differences (read-only), given that completion stays the agent's discipline.
- **Storage:** a journaled sidecar now, or wait for the storage backend; the tracker's data model should be part of the backend's model either way.
