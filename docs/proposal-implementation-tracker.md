# Proposal: the implementation tracker

Status: implemented 2026-10-24. Research: [research-implementation-tracker.md](research-implementation-tracker.md).

## Problem

A project's research has no counterpart for its recreation. Agents building the Rust recreation (`wow-335a-rust` for `3-3-5a`) have nowhere to record:
- how the code is organised;
- which researched functions each part implements;
- which are done;
- what was needed beyond the research to make the recreation match.

The user wants a tracker, not a proof: completion is the agent's discipline, and the running product is the proof.

## Decisions (open questions from the research, answered within the request)

- **Scope:** an implementation belongs to one project. An implementation drawing on several projects is not part of this change.
- **Where:** an inner view of the dashboard's project, switched by **Research | Implementation** tabs at the right end of the project tab row. The research view and the layout are unchanged.
- **Editing:** agents shape the tree and record progress through the CLI. The dashboard shows the tree, counters and completion, and opens a placed record in Research records. Editing from the GUI is not part of this change.
- **No repository scan:** Scopework does not compare placements with the doc comments in the code.
- **Storage:** journaled sidecars, written in the same transaction as any other change, with history and recovery. They are sharded so that a change copies only the small files it touches into its receipt:
  - `implementation/tree.json`: the repository and the tree's nodes;
  - `implementation/nodes/<id>.json` per node: its placements and implementation-only items.

  The storage backend can take over this model later.

## Model

- **Tree node:** `id`, `parent` (none for a top-level node), `name`. Any depth. Siblings keep the order they were added in. A node's own name is free-form: crate, module, file or any grouping.
- **Placement:** a research function record placed on one node, with `complete`, and when completed, `completed_by` and `completed_at`. A record is placed in at most one node.
- **Implementation-only item:** `id`, `name`, `complete`, `completed_by`, `completed_at`.
- **Counters per node:** research items done of total, and implementation-only items done of total. Each is shown for the node itself and rolled up over its subtree, with totals for the project. Functions not yet placed are counted.
- **Who:** `completed_by` is `SCOPEWORK_SUBMITTER` when set, otherwise the Windows user name. **When** is the Unix time. Receipts carry the history of every change.

## CLI

```text
research impl show PROJECT [--node ID] [--unplaced]
research impl where PROJECT RECORD
research impl repo PROJECT --path PATH [--remote URL]
research impl add PROJECT --name NAME [--parent ID]
research impl rename PROJECT NODE --name NAME
research impl move PROJECT NODE [--parent ID]          # no --parent: top level
research impl remove PROJECT NODE                      # only an empty node
research impl place PROJECT NODE RECORD...
research impl unplace PROJECT RECORD...
research impl item-add PROJECT NODE --name NAME
research impl item-remove PROJECT ITEM
research impl complete PROJECT [--record ID]... [--item ID]...
research impl reopen PROJECT [--record ID]... [--item ID]...
```

Rules:
- Placing a record that is placed elsewhere is refused, naming its current node. Unplace it first.
- Only function records can be placed.
- Names are one line of 1 to 120 characters.
- Through the coordinator, these are record operations: they may share a commit. They read the tracker through the commit's overlay, so a merged batch behaves like the same operations run one at a time.

## Dashboard

- The **Implementation** view lists the tree on the left, each node with `done/total` research items and, when it has any, implementation-only items.
- The right side shows the selected node's placements and items: a check mark, then the name and RVA.
- Clicking a placement opens the record in Research records.
- The bottom shows the project totals as counters (`Research items: 120 / 1,146`, `Implementation-only: 3 / 10`, `Not placed: 29,536`). There are no meters.
- The view reloads when the project changes.
- Research records > Organize shows where a function is implemented.

## Tests

- Integration tests through the CLI:
  - building a tree;
  - placement uniqueness and its error;
  - counters rolled up over nested nodes;
  - completion with who and when;
  - `remove` refusing a non-empty node;
  - `move` refusing a cycle;
  - `where` and `--unplaced`;
  - every change leaving a receipt.
- A coordinator test in which an add and a place merged into one commit give the same result as when run alone.
- A headless dashboard test that switches to the Implementation view on a fixture home and shows the counters.

## Validation

- Integration tests through the CLI cover the tree, placements, items, counters, completion, refusals and history.
- A coordinator test merges a burst of 30 placements and item additions into 5–6 commits without losing any. The same test fails (17 of 24 placements lost) when the tracker reads its files directly instead of through the commit overlay.
- A headless dashboard test switches to the Implementation view and reads the counters.
- Captures of a fixture home show the tree, the selected node's placements and items, and the totals.
