# Project rules

Build Scopework, a Rust GUI and agent CLI for managing isolated binary research projects. Start with an empty library. Projects are created explicitly; never seed development projects or sample dashboards.

- Keep the approved dashboard design. Do not redesign it while implementing research storage or workflows.
- Canonical hierarchy: App -> Architecture category -> Chunk -> Function. Research composes upward. Every node has exactly one canonical Markdown report in its project directory.
- Update canonical files in place; do not create overlapping follow-up reports. Keep each function's supporting data in architectures/<category>/<chunk>/<RVA>/data/ beside its research.md, and link it from that report.
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

- Every research project owns tools/profiles, tools/scripts, tools/instrumentation and tools/bin. Shared tool databases and working data belong in tools/workspaces, tools/logs or tools/scratch. Function-specific captures, exports, logs and evidence belong in that function's data/ folder. Do not create project-level data/evidence dumps or parallel research reports. Canonical architecture reports are architectures/<category>/research.md and chunk reports are architectures/<category>/<chunk>/research.md; the app report is app.md.

- Maintain function prerequisites through the app or research deps set. Each function has dep.toml and prereq.toml as synchronized generated views of the project dependency graph. Do not duplicate shared function research or count it twice. Prerequisite changes require dependent-report review; adding a use does not reopen the shared prerequisite.

- Use the documented research CLI for catalog/report mutations; read docs/cli.md. Resolve the home with research home; never assume the checkout is the data home. Upgrade the current catalog layout with a recoverable migration; do not import historical material automatically. Carry-over is manual. Keep app installation separate from research; uninstall must preserve user research.

- Research Scope is a short project topic label, such as Rendering or Asset platform, shown above the meters and exposed by the CLI. It tells agents what is being researched; do not turn it into a detailed charter or an additional completion gate. Editing the label does not reopen research. Detailed understanding belongs in canonical reports.

- Preserve installer UpgradeCode and executable/CLI names across branding changes. Research-home selection is shared by GUI and CLI; explicit --home and environment overrides take precedence. Installer choices must be visible before Install, and uninstall must preserve user research.
