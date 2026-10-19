# Proposal: coordinator phase 2, tool coordination

Phase 2 of [the coordinator proposal](proposal-coordinator.md). It builds on design decision D8 in [the design document](design-coordinator.md) and on phase 1 as implemented. Research and proposal are complete, and the proposal is implemented (see "As implemented" at the end). Adopting it in the tools is the project sessions' change.

The goal: tools that share project resources (the annotation ledger, the Ghidra project) wait in one ordered, visible queue. Work of the same kind is collected and done once, and changes made outside that queue are recorded. Scopework orders, batches and relays this work; it never runs a tool and holds no knowledge of any particular tool (criterion C6).

## Research

All measurements are read-only, on `wow-335a-12340`. They come from its tool scripts, `tools/workspaces` run folders and a scratch copy of the ledger.

### How tools share resources today

Every tool that touches the ledger or Ghidra goes through `fresh_re.lock()`:
- **The lock:** a one-byte `msvcrt` lock on `tools/workspaces/workbench.lock`.
- **Waiting:** waiters poll it every 2 s, for up to an hour.
- **Re-entry:** the lock is re-entrant within one process.
- **Visibility:** there is no order or priority, and no way to see who holds it or who waits.

Three kinds of work hold it:

| Work | Entry point | What it does under the lock | Measured cost |
| --- | --- | --- | --- |
| Ledger edit | `annotations.py put` / `put-type` | Parses the 33 MB ledger (55,272 annotations, 580 types), replaces one row, validates it, writes the whole ledger back | About 1.1 s per edit before report hashing and process start: parse 205 ms, serialise 630 ms, write 63 ms, SHA-256 77 ms, catalog parse for report paths 99 ms |
| Ledger apply | `annotations.py apply` | Writes every ledger row to Ghidra in one headless run; answers `current` when the ledger, engine and project already match the last apply | 261 runs in 2.2 days; median 13.9 s, p90 21 s |
| Export batch | `fresh_re.py batch RVA...` | Runs `apply` first, then a read-only headless export of the listed functions, then routes the artifacts to each function's `data/` folder | 568 runs; median 9.1 s for 1 RVA, 10.3 s for 17–64, 11.5 s for 65 or more |

Findings:
1. **The fixed cost dominates batches.** A batch of 65 or more functions costs 2.4 s more than a batch of one. Merging queued batches into one run therefore saves about 9 s per merged request.
2. **Ledger edits rewrite 33 MB each.** Merging N queued edits costs about one edit's 1.1 s instead of N times that.
3. **Applies rarely queue.** Only 12 of 260 gaps between applies were under 60 s, and the median gap was 276 s. Collapsing duplicate applies saves little today, though it costs nothing to support.
4. **Contention has been low so far.** No export batch started within 5 s of the previous one ending (0 of 567), although a waiter polling every 2 s would start within about 2 s. Batches have mostly run one at a time, so merging pays off only when several agents work at once. Phase 2 must measure the gain rather than assume it.
5. **Ghidra project concurrency is unknown.** `batch` opens the project `-readOnly`. Whether two headless processes can share one Ghidra project was not tested, because testing it needs a writable copy of a large Ghidra project. The design treats Ghidra as exclusive, which is what the workbench lock does today.

### A tooling bug found on the way

`catalog.save` writes every report body and evidence to two fixed files: `tools/scratch/canonical-body-input.md` and `canonical-evidence-input.md`. Two agents saving at the same moment can submit each other's text. The expected token rejects a save only when the record changed, not when the body file did, so the wrong text can be accepted. The fix is a unique temporary file per call. It is the project's tooling change, recommended under Rollout, and needs no Scopework change.

## Design decisions

D8 chose generic leases on named resources, optional payloads and keys, a manifest plus flags, runtime drift detection and an advisory static audit. This proposal settles the remaining questions.

**P1. What a lease is.** A lease is an open pipe connection that holds named resources.

| Option | Holder death | Cost |
| --- | --- | --- |
| A. Lease tokens with heartbeats and expiry | Detected after the expiry; a slow holder can lose its lease mid-write | Timers, renewal and clock rules |
| **B. The lease lives as long as the holder's connection** | Detected at once: Windows closes the pipe when the process ends | None beyond phase 1's pipe |

**Choice: B.** No timeouts are needed, and a holder can never lose its lease while it still runs. Python reaches the pipe with the standard library (25 µs per round trip, measured in phase 1 research).

**P2. Merging and collapsing are one mechanism.** A request names a `key`; a payload is optional. When a lease is granted:
- every queued request with the same key and resources joins it, within the bounds;
- the holder receives all payloads and returns a result per payload;
- requests without a payload are collapsed: each receives the holder's overall result as `covered`.

A request joins only a lease granted after the request was queued. The design's rule therefore holds by construction: a collapsed request is covered only by work that started after it asked.

**P3. Several resources are granted all at once or not at all.** `apply` needs `ledger` and `ghidra` together. A request lists its resources and is granted only when all are free, so no holder ever waits while holding a resource, and deadlock cannot occur. Ordering uses phase 1's classes and ageing, across operations and leases of the same project.

**P4. Direct mode.** Without a coordinator, `job` falls back to per-resource lock files that reuse the project lock queue (`.project-locks/<project>.res-<resource>.lock`, fair arrival order, `research locks` visibility). Nothing merges: every request is its own lease with itself as the only payload. Results are therefore the same, only slower.

**P5. Drift detection without a watcher thread per file.** The coordinator records each declared file's size, modification time and hash:
- when a lease on its resource is released;
- after its own commits.

A 2 s poll compares size and time, then the hash. A change with no lease held on the resource and no commit of its own is drift, recorded as a history entry. The catalog and sidecars need no declaration: a change to `project.json` or a sidecar without a new transaction receipt is drift. The Ghidra project directory is never watched (D8).

**P6. The static audit stays textual.** Scopework does not embed a Python parser (C7). `tools audit` scans tool sources for imports of declared modules, `research` subcommands and subprocess executables. It is advisory only: it lists what tools appear to use, against the manifest's declared kinds.

## Proposal

### Manifest

`tools/coordinator.toml` in the project (optional; flags override every field):

```toml
version = 1

[resources.ledger]
files = ["tools/profiles/annotations.json"]   # watched for drift

[resources.ghidra]
files = []                                    # exclusive, never watched

[kinds.ledger-put]
resources = ["ledger"]
key = "ledger-put"          # queued puts merge into one ledger write
class = "normal"

[kinds.annotations-apply]
resources = ["ledger", "ghidra"]
key = "annotations-apply"   # no payload: duplicates are covered
class = "job"

[kinds.export-batch]
resources = ["ghidra"]
key = "export-batch"        # holders put option differences in their payloads
class = "normal"
```

Unknown fields are refused with the line, so typos surface. Resource and kind names are lowercase words joined by hyphens.

### CLI

```powershell
research job run PROJECT --kind K [--resource R ...] [--key K] [--class C] [--payload-file F] -- COMMAND ARGS...
research job lease PROJECT ...same selectors...      # pipe clients use the protocol directly
research job status PROJECT                          # held and waiting leases, joined payloads
research tools audit PROJECT [--json]                # advisory
```

- **`job run`** is the adapter for any tool. It requests the lease and waits. When granted, it writes the payloads to a file named by `SCOPEWORK_JOB_PAYLOADS`, runs the command, and reads per-payload results from `SCOPEWORK_JOB_RESULTS`. It returns its own payload's result and holds the lease exactly as long as the command runs. The command's exit code is the job's exit code.
- **`research queue`** and the GUI queue panes list leases beside write operations, with kind, resources, class, joined count and holder.
- **Errors** carry kinds: `lease_refused` (an unknown resource, or the bounds exceeded), `holder_lost` (the holder died before reporting; joined requests re-queue at their original position), `job_failed` (the command failed; joined requests receive the same failure).

### Protocol additions (protocol 2)

Role `tool`. A client of protocol 1 is unaffected; a protocol-1 coordinator answers a lease request with `unsupported`, and `job` then falls back to direct mode.

| Message | Shape |
| --- | --- |
| Request | `{"id": n, "lease": {"project": p, "resources": [..], "key": k?, "class": c?, "kind": k?, "payload": {...}?}}` |
| Grant | `{"id": n, "granted": {"lease": L, "payloads": [{"payload_id": i, "submitter": s, "payload": {...}}]}}` |
| Results | `{"id": n, "results": [{"payload_id": i, "ok": {...}} or {"payload_id": i, "error": "..."}], "overall": {...}?}` |
| Release | closing the connection, or `{"id": n, "release": L}` |
| Joined answer | `{"id": n, "done": {"by": L, "result": {...}}}`, `{"id": n, "covered": {"by": L, "result": {...}}}` or `{"id": n, "retry": "holder_lost"}` |

The bounds are the phase 1 merge bounds: 256 payloads or 8 MB per lease, and at most 1 MB per payload. A grant always includes the holder's own payload first. Results must name every payload. One that is missing is answered `error: "no result"`, so a joined requester never waits forever.

### Serial equivalence for tools

A holder must give each payload the result it would have had alone, in queue order. That is the tool's contract, stated in the CLI guide. For the ledger it means applying puts in order, so a later put of the same `(kind, rva)` replaces the earlier one. Each row is validated on its own, so a bad row fails alone and the others are still written. Scopework cannot check this contract; the tests below check the coordinator's side of it.

### Drift

A drift entry is appended to `tools/workspaces/history/tooling.jsonl`:

```json
{"kind": "tooling", "entry_kind": "drift", "summary": "tools/profiles/annotations.json changed without a ledger lease", "refs": ["tools/profiles/annotations.json"], "diff": {"resource": "ledger", "before_sha256": "...", "after_sha256": "...", "holders_since_last_check": []}}
```

`coordinator status` counts drift per resource. Drift is recorded only, never blocked or reverted.

### Module plan

| Module | Change |
| --- | --- |
| `src/coordinator/leases.rs` (new) | Lease queue per project shared with the operation queue's ordering; grant, join, results, holder loss |
| `src/coordinator/drift.rs` (new) | File stamps per declared resource, 2 s poll, drift entries |
| `src/coordinator/manifest.rs` (new) | `coordinator.toml` parsing and validation, flag overrides |
| `src/coordinator/mod.rs`, `client.rs` | Protocol 2, role `tool`, queue view with leases |
| `src/research.rs` | Resource lock files for direct mode, reusing the lock queue |
| `src/cli.rs` | `job run|lease|status`, `tools audit` |
| `src/audit.rs` (new) | Textual source scan for the advisory audit |

The writer and the lease queue share one ordering function, so classes and ageing mean the same thing for both.

### Validation plan

Integration tests in `tests/coordinator_leases.rs`. Fixtures stay small, and stand-in tools are scripts that write files:
- exclusive resources: two holders never overlap;
- holder death releases at once; joined requests get `holder_lost` and run next in order;
- merged payloads: each requester receives its own result, and a missing result becomes `no result`;
- collapse covers only requests queued before the grant; a request arriving during the run gets the next lease;
- multi-resource grants: none is granted partly, and an apply never waits while holding the ledger;
- direct mode without a coordinator: same results, no merging, and lock files are visible to `research locks`;
- drift: a change without a lease is recorded once with the right hashes; a change under a lease is not drift;
- manifest: unknown fields refused with the line; flags override; an undeclared kind can still lease with flags;
- `job run`: the payload and result file contract, the exit code passed through, and the lease held exactly while the command runs;
- a protocol-1 coordinator answers `unsupported` and `job` falls back.

The gain is measured, not assumed:
- `coordinator status` reports per kind the leases, the payloads joined, and the wait and hold times;
- a daily `coordinator-leases` tooling entry records the same, as phase 1 does for reads.

### Rollout

Scopework ships the lease machinery. The tools change in the project sessions, in this order:
1. **Fix the shared scratch files in `catalog.save`** (unique temporary files). This is independent of phase 2 and should happen now.
2. **`fresh_re.lock()` becomes a lease client.** During the transition it takes the lease first and then `workbench.lock`. A tool that has not been updated still excludes correctly, and in that state the file lock is never contended. The file lock is removed once no old processes can remain.
3. **`annotations.py put` becomes a `ledger-put` payload.** The holder applies the merged puts in one read and one write.
4. **`annotations.py apply` and `fresh_re batch` declare their kinds.** `batch` merges payloads that share options into one headless run.
5. **`catalog.py` uses the pipe instead of starting `research.exe`.** This removes two process starts per save and sets `SCOPEWORK_SUBMITTER`.

Each step can be adopted alone and rolled back alone.

### Acceptance criteria

- All validation tests pass on Windows; fmt, clippy `-D warnings` and the full suite are clean.
- Under a synthetic load of four concurrent stand-in tools, merged ledger writes finish in at most 1.5 times the time of a single write per lease, and exclusivity is never violated.
- Drift entries appear for out-of-lease changes and never for in-lease ones.
- `research queue` and the GUI pane show leases consistently with `job status`.

### Tradeoffs

- **Tools opt in.** Tools that have not adopted leases get no merging or ordering. They still exclude through the transition double-lock.
- **Scopework cannot enforce serial equivalence for tool work.** It is a documented contract, checked only by each tool's own tests.
- **Ghidra stays exclusive.** That is safe and matches today's behaviour. If shared read-only opens prove safe later, a `--shared` mode can be added without a format change.
- **Drift is detected within 2 s, not prevented.**

### Risks

- **Holders that report wrong results.** A holder that applies a batch partly and reports success misleads its joined requesters. Mitigation: the contract, plus `job run`'s rule that a non-zero exit fails every joined payload.
- **Long holders starve others.** A Ghidra job of several minutes holds `ghidra` throughout. Ageing reorders waiting requests but cannot pre-empt a holder. `job status` shows the hold time.
- **The manifest drifts from the tools.** The audit reports kinds used but not declared, and kinds declared but unused. Drift entries catch the effects at runtime.

## As implemented (2026-10-18)

Implemented as proposed, with these deviations:
- **No `retry` message.** When a holder is lost before reporting, its joined requests wait again at their original place without being told; their clients are always ready to be granted the lease themselves. This removes a message type and the `holder_lost` error.
- **No `job lease` command.** A lease lives as long as a connection, which a one-shot CLI process cannot keep. Pipe clients lease with the protocol directly, as documented in the CLI guide.
- **Protocol 2 without a protocol-1 answer.** Protocol 1 never shipped. Clients and coordinators of different protocols refuse each other at the handshake, and `job run` then falls back to lock files under `auto`.
- **Catalog and sidecar drift uses the revision, not receipts.** Several Scopework writes leave no receipt: the research scope, and import reservation, cancellation and publication. Every Scopework write raises the catalog revision (a layout migration raises its layout version), so a change with neither is drift. Receipts are still read to recognise ledger repaths.
- **`interactive` is not a tool class.** It stays reserved for the GUI; tools use `normal`, `bulk` or `job`.
- **Result reporting is best effort.** If the coordinator is gone after the command ran, `job run` still returns its own result, and unanswered joined requests wait again.

Validation:
- **Integration tests:** `tests/coordinator_leases.rs` drives `job run` with a stand-in tool. It covers:
  - joining with per-payload results;
  - covering;
  - exclusivity;
  - failure (`job_failed`);
  - a dead holder;
  - direct-mode lock files shown by `research locks`;
  - drift: leased writes and Scopework's scope write are not drift, while a hand edit is recorded once.
- **Unit tests** cover the manifest, the lease queue (joining, reservation, holder loss), the drift rules and the audit.
- **The time criterion for merged ledger writes** can only be measured once `annotations.py` adopts `ledger-put`. Until then the tests show the structure it depends on: N queued payloads run as one command.
