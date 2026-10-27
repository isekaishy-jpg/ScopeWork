# Proposal: a record-level precondition for `deps set`

Status: implemented 2026-10-27. Source: TODO "`deps set` without project-wide conflicts"; the research agents raise it as their top friction.

## Research

`deps set PROJECT NODE --expect TOKEN RVA...` replaces a function's prerequisite list. The token comes from `node show` as `structural_token`. It is `s4:` plus a hash of two inputs:

1. the record's draft token: project, binary, its tree position (lineage), its current prerequisites, its report body and evidence;
2. the project's `structural_epoch`. Every structural change anywhere in the project bumps this counter: any `deps set`, batch moves, renames and membership changes, removals, binary amendments and import publications.

Input 2 is the problem. When several sessions work one project, every structural change by any of them makes every other session's outstanding token stale, even for unrelated functions in other architectures. The 3-3-5a project tool (`function_research.py`) therefore loops up to eight times: it reads a fresh token and applies it at once. A token taken a moment before applying protects nothing, so the loop also retries blindly through a real conflict on the record itself. The precondition causes conflicts, and in practice no longer checks anything.

What the epoch protects, and where each case is already checked:

| Change elsewhere | Already covered without the epoch |
| --- | --- |
| This function's prerequisites changed | They are in the record's own state |
| A named prerequisite was removed | `deps set` fails: that RVA has no report |
| Another function's edges now make a cycle | `validate_dependencies` runs on the change |
| The function or its prerequisite is quarantined | `check_seal` runs on the change |
| The binary identity was amended (RVAs mean something else) | The binary is part of the token below |

`deps set` runs as a structural write in the coordinator: the token check and the change execute together in the project's writer, so a check against the current state cannot race with another writer.

## Design

A prerequisite list describes what a function depends on, not where it sits in the tree or what its report currently says. The only real race for `deps set` is two writers changing the same function's prerequisite list. The user agreed (2026-10-27) that only that should conflict.

- `node show` issues `structural_token` as `s5:` plus a hash of the purpose (`prerequisites`), the project ID, the binary identity, the record ID and its current prerequisite IDs in order.
- A move, rename, report save, completion or any change to another record leaves it unchanged. A `deps set` on the same function, or a binary amendment, changes it.
- `--expect` still accepts `s4:` tokens, with their old meaning, so tools that hold one keep working; and unprefixed legacy tokens keep their project-revision meaning.
- Everything else stays checked when `deps set` runs, as today: the RVAs resolve to function reports, the graph has no cycle, and the seal applies.
- `structural_epoch` itself stays. Import reservations use it, and old `s4:` tokens need it.
- docs/cli.md says what the token binds, and asks tools to take the token when they review prerequisites rather than just before applying, now that only real conflicts fail. Project tools can then drop their retry loops; that change belongs to the projects.

Not included: running `deps set` as a mergeable record write in the coordinator queue instead of a structural commit. That is a throughput question, separate from false conflicts.

## Acceptance

- A token taken before an unrelated `deps set`, a move of the record itself, a rename, or a report save on the record is still accepted.
- A token taken before another `deps set` on the same function is refused with `record_conflict`, as is one taken before a binary amendment.
- An `s4:` token keeps its old meaning: accepted while nothing structural changed, refused after any structural change.
- The full test suite passes and Clippy reports no warnings.
