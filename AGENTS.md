# Project rules

Build one Rust GUI for managing isolated binary research projects. Initial projects are Stock and Forever; additional projects use the same structure.

- Keep the approved dashboard design. Do not redesign it while implementing research storage or workflows.
- Canonical hierarchy: App -> Architecture category -> Chunk -> Function. Research composes upward. Every node has exactly one canonical Markdown report in its project directory.
- Update canonical files in place; do not create overlapping follow-up reports. Keep supporting evidence in the owning project's evidence directory and link it from the function report.
- Each parent report must synthesize its children. All required children and the parent's own report must be complete before the parent can be complete.
- Keep project data and claims isolated by project and exact binary identity. Never import historical completion claims as accepted work automatically.
- This repository is a research management tool, not a client or rendering implementation.
- Keep source installations and binaries read-only. Do not commit binaries, private captures, credentials, or research datasets.
- Preserve provenance and distinguish observed, specification-defined, and inferred claims.
- Do not add tests or run test commands unless the user asks for testing or verification.

- Every research project owns tools/profiles, tools/scripts, tools/instrumentation and tools/bin. Store generated material in its own data/analysis, data/captures, data/exports, data/logs or data/scratch. Retain cited evidence under evidence. Never scatter outputs outside the owning project or create parallel research reports there. Keep run input hashes and tool/profile provenance in data/logs.
