# Scopework maintenance TODO

Recorded 2026-10-16 for the next time work resumes on Scopework. These items are deferred; recording them does not start implementation or mark them resolved.

- [ ] **Completion after function moves.** Review and verify the rule: moving functions into or out of a completed chunk resets that chunk to In Progress. Cover both affected chunks and the resulting parent review state; retain the functions' research and evidence.
- [ ] **Flagging for review.** Add a clear way to flag functions, chunks and architectures for review. Define how flags are displayed, found and cleared, and how they relate to completion status.
- [ ] **Meter status colors.** Most meters currently appear yellow. Restore the agreed mapping consistently: dim red for Not Started, yellow for In Progress, and green for Complete, including complete chunks.
- [ ] **Secondary menus and usability.** Menus beyond the main dashboard still use default styling. Apply the dashboard's Windows 95 Task Manager style consistently, and simplify the menus and workflows so they are easier to understand and use. Preserve the approved main dashboard design.
- [ ] **Installer progress and responsiveness.** Investigate the reported front-loaded work, stalled progress and apparent Windows hangs, followed by an abrupt completion and transition to Finish. Make progress reflect the actual work and improve responsiveness during installation/reinstallation/upgrades. The earlier build-scratch cleanup does not resolve this separate issue.
- [ ] **Code quality and bug review.** Review the codebase for correctness, maintainability and bugs, including the new journal, deletion/pruning and compatibility paths. Record actionable findings and address them in the next maintenance pass.

Use small, self-cleaning fixtures for any later validation. Do not generate mass-file research inventories. Narrow user reports are also filed individually in the active research home's `complaints/` folder; resolve that folder with `research home`.
