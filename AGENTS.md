# Project rules

Build one Rust GUI for managing isolated binary research projects. Initial projects are Stock and Forever; additional projects use the same structure.

- Keep the approved dashboard design. Do not redesign it while implementing research storage or workflows.
- Canonical hierarchy: App -> Architecture category -> Chunk -> Function. Research composes upward. Every node has exactly one canonical Markdown report in its project directory.
- Update canonical files in place; do not create overlapping follow-up reports. Keep each function's supporting data in functions/<RVA>/data/ beside its research.md, and link it from that report.
- Each parent report must synthesize its children. All required children and the parent's own report must be complete before the parent can be complete.
- Keep project data and claims isolated by project and exact binary identity. Never import historical completion claims as accepted work automatically.
- This repository is a research management tool, not a client or rendering implementation.
- Keep source installations and binaries read-only. Do not commit binaries, private captures, credentials, or research datasets.
- Preserve provenance and distinguish observed, specification-defined, and inferred claims.
- Do not add tests or run test commands unless the user asks for testing or verification.

- Every research project owns tools/profiles, tools/scripts, tools/instrumentation and tools/bin. Shared tool databases and working data belong in tools/workspaces, tools/logs or tools/scratch. Function-specific captures, exports, logs and evidence belong in that function's data/ folder. Do not create project-level data/evidence dumps or parallel research reports. Canonical chunk and architecture reports are <kind>/<id>/research.md; the app report is app.md.
