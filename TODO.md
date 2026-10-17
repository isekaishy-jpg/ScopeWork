# Scopework maintenance TODO

Recorded 2026-10-16 for the next time work resumes on Scopework. These items are deferred; recording them does not start implementation or mark them resolved.

- [x] **Completion after function moves.** Review and verify the rule: moving functions into or out of a completed chunk resets that chunk to In Progress. Cover both affected chunks and the resulting parent review state; retain the functions' research and evidence.
- [x] **Flagging for review.** Add a clear way to flag functions, chunks and architectures for review. Define how flags are displayed, found and cleared, and how they relate to completion status.
- [x] **Meter status colors.** Most meters currently appear yellow. Restore the agreed mapping consistently: dim red for Not Started, yellow for In Progress, and green for Complete, including complete chunks.
- [x] **Secondary menus and usability.** Menus beyond the main dashboard still use default styling. Apply the dashboard's Windows 95 Task Manager style consistently, and simplify the menus and workflows so they are easier to understand and use. Preserve the approved main dashboard design.
- [x] **Installer progress and responsiveness.** Investigate the reported front-loaded work, stalled progress and apparent Windows hangs, followed by an abrupt completion and transition to Finish. Make progress reflect the actual work and improve responsiveness during installation/reinstallation/upgrades. The earlier build-scratch cleanup does not resolve this separate issue.
- [x] **Code quality and bug review.** Review the codebase for correctness, maintainability and bugs, including the new journal, deletion/pruning and compatibility paths. Record actionable findings and address them in the next maintenance pass.

- [ ] **Unified diff-based project history.** Provide one append-only history log that records every project operation as a diff instead of full snapshots: catalog changes (referencing journal revisions), report section supersession, and an extension point for project tooling (annotation ledger changes, Ghidra applies, placement and generated-artifact changes). Add `research history` reading (filter by node, RVA, kind, date) and a GUI history view; restore stays manual from an entry's diff. Measured motivation and design: research-home complaint `2026-10-17-agent-unified-history-log.md` (40 MB per annotation apply, 264 MB per reorganisation pass, 14 s full-ledger validation repeated per apply; estimated about 3x faster applies and KB-sized history entries).

Resolved in 0.7.0 (2026-10-16); see CHANGELOG.md. Installer progress keeps a documented verbose-log check for any remaining stall.

Use small, self-cleaning fixtures for any later validation. Do not generate mass-file research inventories. Narrow user reports are also filed individually in the active research home's `complaints/` folder; resolve that folder with `research home`.
