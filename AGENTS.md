# Project rules

Build one Rust GUI for managing isolated binary research projects. Initial projects are Stock and Forever; additional projects use the same structure.

- Keep the approved dashboard design. Do not redesign it while implementing research storage or workflows.
- Canonical hierarchy: App -> Architecture category -> Chunk -> Function. Research composes upward. Every node has exactly one canonical Markdown report in its project directory.
- Update canonical files in place; do not create overlapping follow-up reports. Keep each function's supporting data in functions/<RVA>/data/ beside its research.md, and link it from that report.
- Each parent report must synthesize its children. All required children and the parent's own report must be complete before the parent can be complete.
- Research assignments have fixed scopes. A function report establishes that function's behavior, inputs, outputs, side effects, boundaries and connections with evidence. A chunk synthesizes completed function reports into control flow, data flow and external interactions. An architecture synthesizes completed chunks into subsystem behavior and interfaces. The app synthesizes completed architectures to fulfill the project's declared research scope and relevant subsystem interactions.
- Completed children make a parent ready for synthesis; they do not automatically complete it. Review the parent's own understanding separately at every level.
- Record missing knowledge outside the assigned scope as a separate research task. Make it a prerequisite only when necessary to establish the current scope's behavior. Connections and call edges alone do not expand an assignment or require researching the entire reachable graph.
- Keep project data and claims isolated by project and exact binary identity. Never import historical completion claims as accepted work automatically.
- This repository is a research management tool, not a client or rendering implementation.
- Keep source installations and binaries read-only. Do not commit binaries, private captures, credentials, or research datasets.
- Preserve provenance and distinguish observed, specification-defined, and inferred claims.
- Do not add tests or run test commands unless the user asks for testing or verification.
- File narrow complaints about the app as individual Markdown reports in the single complaints/ folder in the active research home (`research home` prints it). Follow that folder's README.md; filing the report is the entire process.

- Every research project owns tools/profiles, tools/scripts, tools/instrumentation and tools/bin. Shared tool databases and working data belong in tools/workspaces, tools/logs or tools/scratch. Function-specific captures, exports, logs and evidence belong in that function's data/ folder. Do not create project-level data/evidence dumps or parallel research reports. Canonical chunk and architecture reports are <kind>/<id>/research.md; the app report is app.md.

- Maintain function prerequisites through the app or research deps set. Each function has dep.toml and prereq.toml as synchronized generated views of the project dependency graph. Do not duplicate shared function research or count it twice. Prerequisite changes require dependent-report review; adding a use does not reopen the shared prerequisite.

- Use the documented research CLI for catalog/report mutations; read docs/cli.md. Resolve the home with research home; never assume the checkout is the data home. Do not migrate or import historical material automatically. Carry-over is manual. Keep app installation separate from research; uninstall must preserve user research.

- Every project must declare research scope (objective, included behavior, exclusions and boundaries). Never infer whole-binary scope from binary identity. Completion requires a set scope; the app report attests coverage of that scope. Scope changes reopen app synthesis; explicitly update any affected child assignments.
