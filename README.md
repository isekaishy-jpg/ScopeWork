# Research Dashboard

A Rust desktop GUI for binary research projects. Initial projects: Stock and Forever.

The first desktop UI is implemented with eframe/egui. It includes project switching, category and text filters, segmented function-group bars, group inspection, expandable deliverables, and project notes saved locally between sessions. Sample preview is enabled initially and clearly marks all counts, categories, and completion states as illustrative. Disable it to see the empty project state.

No existing research claims are imported. Deliverables, acceptance criteria, and the underlying research data model remain to be agreed with the project owner.

## Run

With a Rust toolchain and the Windows MSVC build tools installed:

```powershell
cargo run --release
```

The standalone Windows application is produced at `target/release/research-dashboard.exe`. Project notes and UI preferences use eframe's local application storage; they are not committed or uploaded. Exit normally to save them. The UI currently reads only built-in illustrative data, never a research binary or installation.

## Proposed structure

Project → pinned binary/build → category → understanding/architecture item → required deliverables → evidence and function groups.

- Inventory the entire binary, even while active research focuses on rendering and the asset platform.
- Derive candidate categories and function groups from binary evidence: calls, references, shared data, ownership, and lifecycle. Keep inferred assignments provisional and retain an explicit unclassified bucket.
- Display function counts and segmented progress bars. Each segment identifies a group of functions that must be understood together; green means its required deliverables have been accepted.
- Put concrete deliverables below each understanding/architecture research item. The architecture item is the durable research output for later use.
- Keep function coverage and accepted deliverable progress separate. Function counts are not a measure of semantic completeness or effort.
- Count unique functions within an inventory. Groups may reference shared functions without double-counting them in totals.
- Tie progress to evidence and explicit acceptance criteria, not an agent's completion statement.
- Keep Stock and Forever project identities, evidence, function inventories, and acceptance records separate. Any explicit cross-project reference must retain provenance.
- Preserve exact binary identity, analysis-tool provenance, original addresses/RVAs, claim classification, and revision history.

## Decisions pending discussion

The initial deliverables and their acceptance criteria; category and group review workflow; inventory denominator and external/thunk treatment; precise completion rules; research storage and import format; and who may accept or reopen research.

Existing reports and completion states require review before migration. No old completion state is automatically trusted.
