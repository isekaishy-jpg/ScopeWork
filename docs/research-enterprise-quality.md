# Research: what enterprise-grade quality means for Scopework

Status: research, 2026-11-03, at `9046ba4` (0.15.0a and its review fixes). It decides nothing; it measures where Scopework stands against what enterprise software is usually expected to provide, and proposes an order of work for the user to choose from.

**Method.** Measured the source (`src/`, 28,600 lines in 50 files; `tests/`, 9,700 lines in 26 suites; 358 crates in the lock file), read the build, CI, packaging and docs, and drew on the six review rounds of the last two weeks (quality 1–4, bugs 1–3, the Fable review). Nothing was run against a research home.

## 1. Summary

| Area | Standing | In one line |
|---|---|---|
| Data integrity and recovery | **Strong** | Journaled transactions, roll-forward recovery, fsynced atomic writes, fingerprint chains, `store verify`, crash-point tests. |
| Concurrency | **Strong** | One writer per project, a coordinator with fair queues, leases, lock-order discipline, fault-tolerant readers. |
| Lint and style discipline | **Strong** | `fmt`, Clippy with `-D warnings`, no lint suppressions, every `unsafe` block carries a `SAFETY` note. |
| Release engineering | **Good** | Numbered builds, checksums, draft releases from tags, release notes with a full commit ledger. |
| Testing | **Adequate** | 254 tests with fault injection, but three timing-sensitive tests, no fuzzing or property tests, no coverage or performance measurement, Windows only. |
| Code structure | **Gap** | Three modules over 2,400 lines and functions of 600–750 lines; errors are strings classified by their prefix. |
| Security and supply chain | **Gap** | Sound local design, but unsigned binaries, no dependency audit, git dependencies, and a self-declared caller identity. |
| Observability and support | **Gap** | A GUI crash log and coordinator counters, but no persistent coordinator log and no diagnostics bundle. |
| GUI usability and accessibility | **Gap** | No screen-reader support, almost no keyboard handling, several features CLI-only. |
| Documentation | **Gap** | Good reference material, but four user guides buried among 52 design and proposal documents; no admin or architecture guide. |
| Operations | **Gap** | No backup and restore procedure, per-user install only, Windows only. |

The core that is hard to get right (never losing or corrupting research, many agents at once) is already at a high standard. The gaps are the ones enterprises check first and that are cheaper to close: identity and supply chain, structure and error contracts, accessibility, observability, documentation and operations.

## 2. Findings by area

### 2.1 Data integrity and recovery (strong)

- Every catalog change is a journaled transaction: the pending journal is the commit point, recovery rolls it forward on the next open, and applying it twice changes nothing (`catalog_batch::finish`).
- Files are written atomically (temporary file, `sync_all`, rename with Windows sharing retries; `research.rs` `atomic`).
- Receipts and store entries chain catalog fingerprints; `store verify` replays the whole history; damage is refused, never repaired away (bug review 3).
- Crash points are exercised by fault-injection tests (`SCOPEWORK_CATALOG_FAIL_AT`, `SCOPEWORK_METADATA_FAIL_AT`).
- Format gates stop older builds writing newer catalogs (formats 5–8).

**Gaps.** No documented backup and restore procedure (the CLI guide says only "maintain ordinary backups separately"); no single command that checks a whole project's files the way `store verify` checks the store; the silent coordinated-read failure under load is unresolved (TODO.md).

### 2.2 Security and supply chain (gap)

**Sound:** everything is local, with no network service or telemetry; the coordinator's pipe admits only the current user's SID (`coordinator/pipe.rs`); every path from a request, journal or ledger is contained in its project (`layout::inside`); outputs never overwrite (`create_new`); the 29 `unsafe` blocks are all Win32 calls with safety notes.

**Gaps:**
- **Unsigned binaries and MSI.** SmartScreen warns, and allow-listing policies (AppLocker, WDAC) cannot trust a publisher. Needs a code-signing certificate (a cost and an identity decision for the user).
- **No dependency auditing.** CI runs no `cargo audit` or `cargo deny` (advisories, licences, banned or duplicate crates), and the release has no SBOM.
- **Git dependencies.** `solworker` and `solcache` come from personal GitHub repositories pinned by revision. They are reproducible, but not from a registry, and not audited by the tools above.
- **Caller identity is self-declared.** Who did something comes from `SCOPEWORK_SUBMITTER` or the Windows user name, and `--directed-by` is free text. History and pool access therefore trust the caller. That is fine for one person's agents, but not for an audit trail several people rely on. The pipe already knows the client's token, so the coordinator could record the authenticated Windows account beside the declared name.
- **No written threat model** saying what Scopework protects against (other users on the machine, tools writing the ledger, hostile request files) and what it does not.

### 2.3 Code structure and maintainability (gap)

- **Large modules:** `research.rs` 3,191 lines, `cli.rs` 2,909, `catalog_batch.rs` 2,406, `store.rs` 1,763, `coordinator/mod.rs` 1,685.
- **Very long functions:** `cli::execute` 743 lines, the dashboard's `frame` 644, `apply_batch` 614, `cli::run` 390, `store::sync_with` 242. Each holds many concerns, and each review round found its bugs where those concerns meet.
- **`Library` holds the domain and the storage together** (research-storage-architecture §1). The storage interface the backend plan expects needs that seam anyway.
- **Errors are strings.** `type Result<T> = Result<T, String>`, about 300 error sites. Callers classify an error by a prefix convention (`store_busy:`, `recovery_required:`), and `cli::error_kind` parses it back out of the text. A typo in a prefix silently becomes `operation_failed`, and the list of kinds an agent may see is documented nowhere.
- **Good habits to keep:** no lint suppressions, 1,222 doc-comment lines, every `unsafe` documented, and only 57 `unwrap()` calls in non-test code (most on values checked just before).

### 2.4 Testing (adequate)

- 254 tests: 26 integration suites driving the real `research` binary against throw-away homes, plus unit tests; fault injection at every crash point of a commit; multi-process tests for locks, leases and the coordinator.
- **Gaps:**
  - three tests sleep instead of waiting for a state and fail under load (TODO.md);
  - no fuzzing of the parsers that read untrusted or damaged input (store frames, pending journals, batch and import requests, the ledger);
  - no property tests for the replay and diff code, where an invariant ("undo then redo is the identity") would cover more than examples do;
  - no coverage measurement, no performance or scale tests (the last scale numbers were measured by hand);
  - the GUI is checked only by manual screenshots (`examples/capture_panels.rs`);
  - CI is Windows only.

### 2.5 Performance and scale (known)

Tracked in TODO.md: the whole-catalog fingerprint and rewrite per commit (`project.json`), whole-project status evaluation and its recursion, and the dashboard's re-reads. What is missing is a statement of targets (for example 150,000 records, ten agents, a commit under 50 ms at the median) and benchmarks that hold the code to them.

### 2.6 Observability and support (gap)

- **The coordinator has no log of its own.** It runs detached; its errors live only in `coordinator status` until it exits, and a panic caught in the writer is answered to the client but not written anywhere durable.
- **No diagnostics bundle.** A user reporting a problem has no `research diagnose` that collects versions, coordinator status, recent logs, lock holders and pending journals (without research content).
- **No log levels.** The GUI writes `logs/gui.log`; everything else is answers on stdout and stderr.

### 2.7 Command line and machine interface (adequate)

- **Good:** every command answers JSON with one error shape (`{"error", "kind"}`); writes are token-guarded; the help text comes from the code (clap).
- **Gaps:**
  - error kinds come from message prefixes (§2.3), and nearly every failure exits with code 1, so scripts cannot tell "busy, retry" from "refused" without parsing JSON;
  - the JSON outputs have no schema version, so an output change cannot be detected by agents' tools;
  - the reference is one 694-line `docs/cli.md`, with no per-command pages.

### 2.8 Dashboard usability and accessibility (gap)

- **No screen-reader support.** eframe is built without its AccessKit feature; the `accesskit` crate is in the lock file but not compiled in.
- **Almost no keyboard handling.** Four key-handling sites and three tooltips in all the GUI code. There are no shortcuts and no documented focus order.
- **Features only the CLI has:** the project store and history at a revision, artifacts and spaces, annotations, promotion and most of the coordinator's controls.
- **Known items:** no data counters; the group menu's overflow; the dashboard's lock holds (TODO.md).
- The classic look is the user's approved design. Accessibility can be added without changing it.

### 2.9 Documentation (gap)

- `docs/` holds 59 files: the four user guides (installation, research model, CLI, building) sit among 38 proposals and 14 research and design documents, in one folder with no index.
- There is no architecture overview for a new contributor (the best one, `research-storage-architecture.md`, is a research note), and no administrator guide (deployment, backup, upgrade policy, data locations, what to back up).
- **Good:** README with screenshots, a detailed changelog, release notes with validation and a commit ledger, and a decision trail for every feature.

### 2.10 Packaging, release and operations (good, with gaps)

- **Good:** numbered builds reserved under a lock, MSI with major upgrades and preserved research, checksums, tag-driven draft releases, release notes.
- **Gaps:**
  - unsigned (§2.2);
  - per-user install only (`Scope="perUser"`), with no documented silent-install properties for managed deployment;
  - no update channel or update check;
  - a release's install and upgrade checks are manual and have been skipped (0.14.0a and 0.15.0a both say "Not done: installing over the previous version");
  - Windows only (Linux is a side quest in TODO.md).

## 3. A possible order of work

Each item would go through the workflow on its own; the order puts low-risk, high-value foundations first.

**Tier 1: foundations (low risk, no behaviour change for users)**
1. **Typed errors with stable kinds.** An error enum carrying a kind and a message, kinds documented in the CLI guide, and distinct exit codes for busy, conflict, refused, not found and failed. This removes prefix parsing and gives agents a contract.
2. **Supply-chain checks in CI.** `cargo deny` (advisories, licences, sources) and an SBOM attached to releases.
3. **Reliable tests.** Fix the three timing-dependent tests, reproduce or close the silent read, and add a coverage report.
4. **Coordinator log and `research diagnose`.** A rotating log in the research home, written by the coordinator, for errors, panics and recoveries; and a command that bundles what a bug report needs, without research content.
5. **Backup and restore.** An administrator guide naming what to back up and how to restore, and perhaps `research project archive` (a consistent snapshot taken under the project's lock).
6. **Documentation layout.** User and admin guides apart from design history (`docs/guide/`, `docs/design/`), an index, and an architecture overview.

**Tier 2: structure and assurance**
7. **Split the largest functions and modules:** `cli::execute` into a handler per command, the dashboard's `frame` into panels, and `apply_batch` into phases. This is behaviour-preserving, guarded by the suite.
8. **Fuzzing and property tests** for store frames, journal replay, batch and import requests and the ledger diff.
9. **Performance targets and benchmarks**, feeding the `project.json` and status-cache items already in TODO.md.
10. **JSON schema versions** on outputs that agents' tools parse.

**Tier 3: product and deployment**
11. **Accessibility:** AccessKit, keyboard navigation and shortcuts, within the approved design.
12. **Dashboard parity** for the store, annotations and spaces (deferred items in TODO.md).
13. **Code signing** of the executables and the MSI.
14. **Managed deployment:** a per-machine install option and documented silent-install properties.
15. **Authenticated identity:** the coordinator records the client's Windows account from its pipe token beside the declared name.

**Tier 4:** Linux (TODO.md side quest), and anything the user's definition of enterprise adds (§4).

## 4. Questions for the user

1. **Who is the enterprise customer?** One researcher with many agents (today), a team sharing a research home (on a network share?), or an organisation that needs audit and managed deployment? The answer moves identity, deployment and Linux up or down.
2. **Code signing:** buy a certificate (organisation validation, or EV for immediate SmartScreen trust), and under whose name?
3. **How far may refactoring go?** Splitting the largest functions changes no behaviour, but touches most of the code at once; it could go module by module between feature work.
4. **Accessibility target:** screen readers and keyboard-only use, or keyboard shortcuts only?
