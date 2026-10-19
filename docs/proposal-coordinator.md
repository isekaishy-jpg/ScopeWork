# Proposal: the Scopework coordinator

Research and design for unblocked reads, prioritised and merged writes, and tool coordination when several agents work on one project. Follows [the 2026-10-18 work brief](proposal-2026-10-18.md). Research, design and proposal are complete. The alternatives and the reasons for each choice are in [the design document](design-coordinator.md); the concrete plan is under Proposal below. Phases 0 and 1 are implemented (see "Phase 1 as implemented"). Phases 2 and 3 have their own proposals: [phase 2, tool coordination](proposal-coordinator-phase2.md) and [phase 3, storage](proposal-coordinator-phase3.md).

## Requirements

Set by the user on 2026-10-18:

1. **A read is never blocked by a write**, without accepting any risk of a torn or half-applied answer. Transactions are frequent with several agents, so "check and retry" designs are not acceptable.
2. **Reads say when a write is happening** to what they returned, so an agent knows its answer is about to change.
3. **Priority queues per named resource**, with same-kind operations merged and run once (for example all queued annotation edits).
4. **A queue counter in the dashboard footer.**
5. **Tooling is a moving target.** Tools declare what they do, and a recurring audit reports drift; Scopework never hard-codes knowledge of particular tools.
6. **Use Solworker properly**, rather than as the occasional thread pool it is today.

## Research findings

### What writes look like on the live project

Read-only measurements on `wow-335a-12340` (31,603 records, 7.8 MB compact catalog), from transaction receipt times:

| Measure | Value |
| --- | --- |
| Commits, last 24 h | 20,438 (14.2 per minute) |
| Commits, busiest 10 minutes | 566 (56.6 per minute) |
| Gap between consecutive commits | p10 0.94 s, median 1.22 s, p90 1.7 s; never below 0.25 s |
| Composition of the last 600 commits | 557 single-record completions (93%), 15 report saves, 14 claim/flag sidecars, 13 moves |

Commits are serialised at roughly one per second, back to back. The gap never drops below a quarter second, so throughput is capped by the fixed cost of each commit. The queue is not empty: writes are waiting.

Fixed CPU work per catalog pass, measured in memory on a copy of the catalog (release build):

| Step | Time |
| --- | --- |
| Parse the catalog | 21 ms |
| Serialise it | 9 ms |
| SHA-256 of the serialised catalog | 28 ms |
| Clone and compare | 6 ms |

A commit repeats these several times: opening, journal fingerprints, recovery verification, writing. It also fsyncs 7.8 MB and, from the tooling, pays two process starts (`node show` for the token, then `node complete`). That cost is the same whether a commit completes one record or fifty. Merging queued operations into one commit therefore multiplies write throughput, most of all at peaks.

### How tooling reaches Scopework

The live project has 1,386 Python scripts and 22 Ghidra Java scripts. 1,242 of the scripts were last modified on a single day, most of them single-task review scripts, so the set changes constantly. They converge on three shared modules:

| Module | Used by | Role |
| --- | --- | --- |
| `catalog.py` | 622 scripts | Runs `research.exe` once per call, retrying `busy`, `os error 32` and `os error 5` for up to 300 s with 20–120 ms sleeps. A save is two process starts. |
| `annotations.py` | 552 scripts | Edits the ledger (`put`, `put-type`) and applies it to Ghidra, under the workbench lock. `apply` already sends every pending row in one Ghidra transaction. |
| `fresh_re.py` | most tools | Owns `workbench.lock`, a one-byte `msvcrt` lock polled every second for up to an hour, and headless Ghidra runs (about 12 s each). |

Two facts follow. Catalog operations reach Scopework only through the CLI, so routing the CLI through a coordinator helps all 622 scripts without changing them. Tool coordination, ledger writes and Ghidra runs go through one module-level lock that has no order, priority or visibility. That lock is where named resources and merging belong.

### How Scopework uses Solworker today

Solworker's guide says an application owns one runtime and hands lanes to its subsystems. Scopework instead builds a new runtime for every bulk report-hash pass, every GUI refresh, every browser search and every import. It uses one execution class (Low), scoped `for_each_chunk`, simple `try_spawn` and one owner inbox. It does not use:

- the Mid and High classes;
- groups and dependency edges;
- batched submission;
- resource demand ranks (`SWPriority`);
- work sets;
- shared immutable results (`SWShared`);
- external producers;
- notification routes.

Solworker schedules work inside one process. Every agent command is its own `research.exe`, so no use of Solworker can order or merge work across agents while that stays true. Solworker also says ordering by application semantics belongs to the application, and the application should not run a second runnable queue. Writes to one project are serial by nature, so a domain queue of pending operations is legitimate; the CPU work inside it (validation, rendering, hashing) belongs on Solworker.

### Transport options

| Option | Isolation | Cost |
| --- | --- | --- |
| Windows named pipe | Default security lets Everyone and Anonymous read (fourth pass); an explicit owner-only descriptor limits it to the user. No port, no firewall. | A small FFI module over `windows-sys`, which is already in the build graph through eframe. |
| Loopback TCP | Any local process can connect; needs a token file. | Standard library only. |
| Spool folder of request files | Filesystem permissions. | Polling latency; many small files. |

Named pipes are selected. They are the platform's intended local IPC, can be restricted to the user with an explicit security descriptor, and add no new crate.

### Closing the research gaps (2026-10-18, second pass)

**Commit cost after the work-brief fixes.** The one-per-second figures above come from 0.8.0, so the new build was timed two ways.

On a disposable 100-function fixture (release build, wall time per command including process start):

| Command | Median | p90 |
| --- | --- | --- |
| `project revision` (process start and open) | 11 ms | 13 ms |
| `node show` | 12 ms | 14 ms |
| `node save` | 44 ms | 53 ms |
| `node complete` | 40 ms | 47 ms |

At the live project's size, measured in memory on a copy of its 7.8 MB catalog with a temporary test that was not committed:

| Commit step | Time |
| --- | --- |
| Build the index | 30 ms |
| Journal delta with both catalog fingerprints | 93–103 ms |
| Verify the journal against the catalog | 93–96 ms |
| Compare catalogs | 3 ms |
| Write and fsync the catalog | 26–27 ms |

With opening the catalog (parse 21 ms plus validation), a commit on the live project now costs roughly 0.3–0.4 s of fixed work, before any report reads. The four full-catalog serialise-and-hash passes used as fingerprints are the largest single part, about 190 ms. That fixed cost is the same for one operation or many, so merging still multiplies throughput. Cheaper fingerprints (hashing the changed records plus a running catalog digest) are a separate, smaller gain worth weighing in the design.

**Which records commits touch.** In the last 1,959 commits:
- the median commit changed 4 records (p90 also 4);
- 85% of commits changed the App record, an architecture and a chunk.

Completing or saving a function bumps the revision of every ancestor and clears its acceptance, even when those ancestors are not complete. The 137 minutes sampled had sessions in 13 architectures (one architecture had 773 of the commits), but every session's commits touched the same ancestors.

Consequences for the design:
- **Pending context needs a distinction.** Without one, a read of the App or an architecture would almost always show a pending write. Context must separate bookkeeping (an ancestor's revision bump) from changes to the record's own content, status or place.
- **Record-level preconditions conflict on ancestors.** A schema-2 precondition on a chunk or architecture conflicts with nearly every commit beneath it. Preconditions on functions, which is the common move, are unaffected.
- **Ancestor bumps merge cleanly.** They commute, so merged completions under one architecture share them.
- **Some bumps may be unnecessary.** Bumping ancestors that are not complete appears unnecessary for correctness: draft tokens use lineage, not revisions, and completion tokens include child states. Removing those bumps would cut conflicts and noise. This needs its own correctness review in the design stage.

**Reads during writes.** Reads are not logged anywhere, so how often a read lands on a record under a running write cannot be measured from history. Proxies:
- `catalog.py` always reads a record (`node show`) immediately before writing it, so every tool write is preceded by a read of the same record;
- with ancestors touched by 85% of commits at about 14 commits per minute, a read of the App or an architecture coincides with a write most of the time.

Measuring it directly needs a read log, which phase 1 should add (counts only).

**Solworker mechanisms, checked in source** (pinned revision `273b45b`):

| Mechanism | What the source confirms |
| --- | --- |
| `SWRuntime::owner(state, capacity)` | Creates a thread-bound owner whose state need not be `Send`. |
| `SWOwnerSender::try_post(phase, FnOnce(&mut O))` | Bounded and reserved before acceptance; returns an observable delivery; the callback runs on the owner thread. |
| `SWLane::try_spawn_batch` | Takes owned `'static` closures, in bounded portions, with explicit partial rejection; it never runs inline. |
| `notification_route` | Takes a `Fn() -> io::Result<()> + Send + Sync` signal. |
| `SWTask::into_shared` | Gives `SWShared` for `Send + Sync` results. |

So the per-project writer fits an owner thread. Owner callbacks run in arrival order, though, so priority classes and merging must be the writer's own queue state, with posted callbacks only enqueueing. That matches Solworker's guidance: application order lives in the application, CPU work goes on lanes. Batched validation needs the inputs owned, which an `Arc` snapshot provides.

**Named pipes, proven in a spike.** A throwaway program in the scratchpad (not the repository) built offline against `windows-sys` 0.61.2, the version already in `Cargo.lock`. One server thread per connection, four concurrent clients and 8,000 JSON-line round trips gave a median of 11 µs (p99 24 µs, max 166 µs). Clients opened the pipe with plain `std::fs::OpenOptions`, so the CLI side needs no foreign API. The server side is about 40 lines of FFI: `CreateNamedPipeW` with remote clients rejected, then `ConnectNamedPipe`. Transport cost is negligible next to a process start (about 11 ms) or a commit (about 300 ms). Opening the pipe from Python was verified in the third pass below.

(At this point the design stage had not been done; it is now in [the design document](design-coordinator.md).)

### Remaining research questions (2026-10-18, third pass)

**1. Are ancestor revision bumps needed?** I traced every reader of a record's own revision (`node.revision`) in the source:

| Reader | Uses |
| --- | --- |
| Completion tokens (`edit_tokens::completion`) | The revision, status, effective state and accepted hash of every record in the closure: required descendants plus prerequisites. |
| Prerequisite stamps (`dependency_stamps`, `catalog_index::state`) | `revision:hash` of each prerequisite; prerequisites are always functions. |
| Schema-2 batch preconditions and the GUI's context-only toggle | The updated record's revision. |

Draft tokens hash the lineage (identity, parent, kind, name, RVA, membership), prerequisites and text, not revisions. Structural tokens use the project-wide `structural_epoch`.

When a child changes, every completion token above it already changes, through the child's own revision and status. Bumping the revision of an ancestor that is not complete therefore protects nothing. It does three things:
- it makes 85% of commits touch the App, an architecture and a chunk;
- it inflates journals;
- it makes schema-2 preconditions on chunks and architectures conflict with any work beneath them.

Ancestors that are complete must still be reopened, with a bumped revision and cleared acceptance. Prerequisite consumers must still be reopened through their stamps.

Conclusion: invalidation can be limited to complete ancestors and prerequisite consumers, without weakening any token or check. The same applies to the copies of this logic in report saves, batches, deletion and import publication. This is a behaviour change that needs its own tests (stale completion tokens must still be rejected).

**2. What do the commit fingerprints protect?** Each pending journal records SHA-256 fingerprints of the whole serialised catalog before and after. They exist for crash recovery: on the next open, recovery must prove that `project.json` is exactly the before or the after state, and must refuse a catalog changed by anything else meanwhile. That guarantee is needed and stays.

The cost comes from how it is obtained. One commit serialises and hashes the catalog four times:
- before and after, when the journal is written;
- again in `resume`, which re-reads the journal it has just written and reconstructs and re-hashes both states;
- once more to write the catalog.

Most of that is redundant when the same process wrote the journal. The SHA-256 rate here is about 280 MB/s, because this CPU (Core i5-9600K) has no SHA instructions, so each hash of the 7.8 MB catalog costs 28 ms.

Options for the design stage:
- serialise the after-state once, hashing and writing the same bytes;
- reuse the hash of the bytes read at open for the before-state;
- skip the re-verification when committing in-process, keeping it for recovery after a crash;
- optionally, switch the fingerprint to a fast non-cryptographic 128-bit hash in a new journal schema. It is an identity check, not a security boundary.

The first three cut roughly 190 ms to about 40 ms per commit, with no format change.

**3. Which merged operations stay correct?** Token inputs:

| Token | Changes when |
| --- | --- |
| Draft (`d4`) | The record's lineage, prerequisites or text change. Unaffected by siblings or children; their renaming re-renders the report header but not the body or evidence the token hashes. |
| Completion (`c4`) | The draft token changes, or anything in the closure changes. |
| Structural (`s4`) | The project-wide `structural_epoch` changes. Every batch, move, prerequisite change or deletion advances it, so it is as coarse as the old global revision. |
| Unprefixed (legacy) | The project revision changes. |

**Merge rule.** In a merged commit, each operation is validated against the in-memory state left by the operations before it, in queue order. A merged batch therefore accepts and rejects exactly what the same operations would run one at a time in that order; tokens prevent any interleaving that serial execution would refuse. Priority classes change only which order is chosen, never what an order may do.

**What can merge:**
- **Completions and report saves** of any records.
- **Flags, claims and review-queue changes.** These are sidecar edits, applied in order.
- **Ledger row edits.**

**What should commit alone:** operations that relocate reports or change hierarchy (moves, deletions, link repair). Later operations in the same batch would otherwise need a virtual file layout to read reports at their new paths.

**Failure handling.** All validation happens before anything is written. An I/O failure during the commit fails every operation in the batch with the same error. Each merged operation keeps its own submitter and result in history.

The coarse structural epoch is a separate conflict source, worth narrowing to the records a change touches.

**4. Upgrades with a running coordinator.** I tested a running copy of `research.exe` in the scratchpad: it **cannot be overwritten or deleted**, but **can be renamed**, after which a new file can take its name.

Windows Installer does not use renaming. Scopework's package (`installer/Wizard.wxs`) relies on the standard files-in-use and Restart Manager dialogs, and `docs/installation.md` tells users to close Scopework and agent jobs before upgrading.

A coordinator that is started without a window gives those dialogs nothing to close gracefully. The installer would end up asking to reboot or terminating it. Termination is crash-safe thanks to the journal, but it is a poor upgrade path. The design therefore needs:
- **a stop command** that drains the queue, finishes the commit in progress and exits;
- **an installer custom action** that runs it for every coordinator of the user before files are replaced;
- **a short idle exit**;
- **a version handshake:** a client of version X that finds a coordinator of version Y asks it to drain and exit, then starts its own, so mixed versions never share a project for long.

The installer action and the handshake are new installer work, so they belong in phase 1. Otherwise the first release containing a coordinator cannot be upgraded cleanly.

**5. How often does the catalog change outside the coordinator?** In the live project, all of the last 20,000 revisions have a journal receipt. The 731 revisions without one (1.66% of all) are from an early period: catalog creation and settings written without a journal. In steady state, then, every catalog change is a journaled commit by some Scopework process.

Outside the coordinator, those are direct-mode commands, older Scopework versions and the GUI if it writes directly. The coordinator notices each before its next commit by the catalog's size and time. Reloading costs about 50 ms (parse 21 ms, index 30 ms) plus a snapshot rebuild. While agents are still on 0.8.0, a reload per agent commit (about 14 a minute here) is affordable. Once every client uses the coordinator, reloads stop.

**6. Python as a pipe client.** Verified with the spike server: a plain `open(r'\\.\pipe\…', 'r+b', buffering=0)` in Python made 500 JSON-line round trips with a median of 25 µs (p99 169 µs), and the server saw a clean end when the client closed. A pipe-speaking `catalog.py` needs only the standard library.

**Status.** Five assumptions remained after this pass; the fourth pass below tests them.

### Assumptions tested (2026-10-18, fourth pass)

The third pass left five design assumptions untested. Each is now tested; one earlier claim was wrong and is corrected above.

**1. Named-pipe security (earlier claim corrected).** The spike read the security descriptor of a pipe created with default security. It grants full access to SYSTEM, Administrators and the creating user, and read access to Everyone (`WD`) and Anonymous (`AN`):

```text
D:(A;;FA;;;SY)(A;;FA;;;BA)(A;;FA;;;<user>)(A;;FR;;;WD)(A;;FR;;;AN)
```

The earlier statement that a pipe's access list "limits it to the user" was therefore wrong. A pipe created with the explicit, protected descriptor `D:P(A;;GA;;;OW)` has only an owner entry, and the owner can still connect. Requirements for the design:
- an explicit owner-only descriptor;
- remote clients rejected (`PIPE_REJECT_REMOTE_CLIENTS`, already used in the spike);
- protection against another process creating the pipe name first: `FILE_FLAG_FIRST_PIPE_INSTANCE` on the server, and a client check that the server process belongs to the same user. Not tested: these two (only the owner-only descriptor was).

**2. Restart Manager with a windowless coordinator.** A copy of `research.exe` was started with `CREATE_NO_WINDOW`, the way Scopework launches background runners, and registered with Restart Manager as an installer would:

| Step | Result |
| --- | --- |
| Classification | `RmConsole`, not restartable |
| Graceful shutdown (`RmShutdown` without force) | Failed with error 351 after about 30 s; the process kept running |
| Forced shutdown | Terminated it (exit code 1) |

The standard files-in-use path therefore cannot stop a windowless coordinator gracefully. It ends in a forced kill or a reboot prompt. The kill is crash-safe because of the journal, but it is not an acceptable upgrade path. Phase 1 needs one of:
- an installer custom action that runs a stop command, which drains and exits, before files are replaced;
- a hidden top-level window that answers Windows' shutdown messages, so Restart Manager can close the coordinator gracefully. Not tested.

The version handshake from the third pass stays necessary either way.

**3. Snapshot memory.** Private memory sampled while a temporary, uncommitted test loaded a copy of the live 7.8 MB catalog:

| State | Private memory |
| --- | --- |
| Catalog parsed | about 8 MB (including a small process baseline) |
| Catalog and index | about 15 MB |
| A second catalog generation, as during a snapshot swap | about 22 MB |

A snapshot of the 31,603-record project is therefore about 14 MB, and about 21 MB at the moment of a swap.

Report working set (sizes of the reports of records changed by commits):
- last 10 minutes: 268 records, 1.0 MB;
- last 60 minutes: 787 records, 3.0 MB;
- all 31,603 reports: 100 MB (median 2.8 KB, p99 8.7 KB, largest 219 KB).

A report cache of a few tens of MB covers the active set many times over. The earlier "tens of megabytes per open project" holds.

**4. Solcache for the report cache: a partial fit.** In Solcache's documentation at the pinned revision:
- `SCCache` has no internal lock and needs `&mut` to change;
- `SCCleanupContext` is neither `Send` nor `Sync`, so deferred releases drain on the thread that created it;
- retention works by byte budget (`maintain` with `SCByteLimit`) and keeps entries that callers still share.

That suits retention owned by one thread, such as the writer. It does not suit reads from many pipe threads without sending each read through that thread. That would make reads wait behind commits again, which the requirements rule out. The snapshot's report texts therefore need their own concurrent structure, with Solcache used at most for retention on the writer's side.

The same check exposed a correctness hazard. **Commits modify report files in place.** A reader holding an older snapshot that misses its in-memory text and falls back to reading the file could read text written by a newer commit: a torn answer. The design needs an ordering rule. Before a commit touches any report, the writer records that report's previous text in the outgoing snapshot, under a lock that also covers the snapshot's own file reads. A reader of the old snapshot then gets either the recorded text or a file read completed before the commit began, never the new text. This is a correctness requirement for snapshot reads, not an optimisation.

**5. Drift-audit feasibility.** A read-only probe parsed all 1,386 Python tool scripts with Python's syntax-tree module (`ast`):

| Finding | Count |
| --- | --- |
| Scripts that both call the shared modules and write files directly | 1,292 |
| Scripts using only the shared modules | 74 |
| Scripts writing only directly | 17 |
| Scripts launching subprocesses | 314 |
| Direct file writes found | 10,745 |
| Writes whose target is not a literal or a module constant | 9,518 (89%) |

Many unresolvable targets are per-run output folders, but static analysis cannot tell those apart from shared resources. So:
- **Static analysis can report reliably** which shared modules, CLI commands and subprocesses a tool uses.
- **It cannot report** which files a tool writes.

Drift detection for shared resources (catalog, ledger, sidecars, reports, the Ghidra project) must be runtime: the coordinator records any change to them made outside a declared operation or held lease, with the lease holders at that time. The static audit stays an advisory report of module and command usage.

**Status.** Two items remained assumptions after this pass: the pipe first-instance and same-user client check, and the hidden-window shutdown alternative. The fifth pass below tests both.

### Last two assumptions tested (2026-10-18, fifth pass)

Both were tested in the scratchpad spike (not the repository), built offline against the `windows-sys` version already in `Cargo.lock`.

**Pipe-name squatting and server identity.**

| Check | Result |
| --- | --- |
| Creating the pipe with `FILE_FLAG_FIRST_PIPE_INSTANCE` while another handle already holds the name | Refused with access denied (error 5); an impostor that got there first is detected. |
| The same flag on a free name | Succeeds. |
| Further instances by the same server | Allowed, so concurrent clients still work. |
| Client lookup of the serving process (`GetNamedPipeServerProcessId`, Rust and Python ctypes) | Returns the server's process ID. The client then compared that process's user SID with its own; in Python this ran across two separate processes. |

Limit: only the matching-user case could be exercised, because this machine has a single interactive account; a rejection of a different user was not produced. The client check must also handle a server process that has already exited.

**Graceful shutdown through a hidden window.** The spike ran as a windowless background process started with `CREATE_NO_WINDOW`, like the earlier test, but owning a hidden top-level window with a message loop.

| Step | Result |
| --- | --- |
| Classification | Still `RmConsole`, not restartable |
| Messages delivered by graceful `RmShutdown` | `WM_QUERYENDSESSION` (with `ENDSESSION_CLOSEAPP`), then `WM_ENDSESSION` |
| Outcome | The process recorded "drained and exiting" and exited with code 0; `RmShutdown` returned success in 0.4 s |

Without the window, the same request failed after 30 s (fourth pass).

A coordinator that keeps a hidden window on a dedicated thread can therefore be closed gracefully by the standard files-in-use path during upgrade or uninstall, and by logoff and shutdown. On the query it would stop admitting work; on the end message it would finish the commit in progress (about 0.3 s) and exit. No installer custom action is required for that path. A version handshake is still needed so a newer client retires an older coordinator it finds running.

**Status.** No design assumption remains untested apart from the different-user rejection above, which cannot be produced on this machine without creating a second Windows account; it is covered by a unit test instead. Only the read-during-write rate is unmeasured; phase 1 adds the read log that measures it.

## Proposal

The research above and [the design document](design-coordinator.md) settle what to build and why. This section specifies how: phases, interfaces and formats, the module plan, validation, rollout, tradeoffs, and the path for later extension. Phases 0 and 1 are specified fully. Phases 2 and 3 are specified to the level their earlier phases depend on, and get their own proposal before implementation.

### Phase 0: changes that need no coordinator

Each item is a separate commit with its own tests and can ship on its own. Together they lower the cost of every commit that direct-mode agents make today.

**0.1 Narrow invalidation** (design D9.1).

One shared function, `reopen_affected(project, changed, before)`, replaces the six copies of the invalidation walk:
- `Library::invalidate` and `Library::invalidate_dependents` (report saves);
- the walks in `save_reports`, `batch` and `remove_nodes`;
- the walk in import publication.

Rules:
- **A record whose own content, membership, place or acceptance changes** gets a revision bump. If complete, it is reopened: status In Progress, acceptance cleared, and a review-queue entry where structural changes apply today.
- **An ancestor of a changed record** gets a revision bump and is reopened only if it is complete.
- **A prerequisite consumer** of a changed function is reopened (revision bump, acceptance and stamps cleared) as today. Its own ancestors follow the ancestor rule.
- **Ancestors that are not complete** are left untouched.

Mixed versions: 0.8.0 writers that still bump every ancestor remain correct alongside. Bumping more is always safe.

**0.2 Cheaper fingerprints** (design D9.2).

`catalog_batch::resume` splits in two:
- **`commit(root, project, journal)`** applies a journal this process has just built in memory. It does not re-read or re-verify the journal, and writes the after-catalog from bytes serialised once.
- **`recover(root, project)`** keeps today's full verification for a pending journal found on open.

`journal_history::pending_bytes` takes the before-catalog fingerprint from the bytes read when the project was opened for writing, kept on the `Library`, and the after-catalog bytes from the single serialisation. The journal format is unchanged, so older versions can still recover new journals.

*Implementation note (phase 0.2).*
- **The before fingerprint is still computed by serialising the catalog.** Catalogs written by older versions or by migration are pretty-printed, while recovery fingerprints the compact serialisation, so the bytes read at open cannot stand in for it.
- **Measured on a copy of the live catalog:** building the journal now takes about 112 ms, against about 200 ms before (journal 95 ms, re-verification 93 ms, compare 3 ms, second serialisation about 9 ms). That saves about 90 ms per commit, not the estimated 150 ms. The rest is the delta itself and its two hashes, which leaves room for the faster fingerprint listed under the future extension path.

**0.3 One Solworker runtime per process** (design D9.3).

A process-wide runtime, created on first use, replaces the per-call runtimes:
- `background::runtime(2)` per GUI snapshot;
- `background::runtime(1)` in the browser;
- `background::runtime(WORKERS)` per import;
- `bulk_runtime()` per report-hash pass.

Classes: Low for hashing and imports, Mid for GUI snapshots, High for interactive browser queries. Worker counts follow the machine: Solworker's recovered policy, physical cores minus one reserved, clamped. The owner inbox for GUI snapshot delivery keeps its own small runtime, because owner callbacks need no workers.

### Phase 1: the coordinator core

**Process and lifecycle.**

| Interface | Behaviour |
| --- | --- |
| `research serve [--home PATH]` | Runs the coordinator for one research home. Started detached by clients, never by hand in normal use. |
| `research coordinator status` | Version, uptime, open projects, queue lengths, and read-log counts. |
| `research coordinator stop` | Drains: stops admitting, finishes the commit in progress, answers queued requests "retry in direct mode", then exits. |
| `SCOPEWORK_COORDINATOR` | `off` always uses direct mode; `auto` (default) uses a coordinator, starting one if absent; `required` fails if none can be reached. |
| `SCOPEWORK_SUBMITTER` | The name a client reports for its operations (for example an agent session name). The default is `pid <n> (<command>)`. |

**Start and single instance.** A client that finds no pipe starts `research serve` detached (no window, like import runners). It retries the connection for up to 2 s, then falls back to direct mode for that command. A second `serve` for the same home fails to create the pipe (first-instance flag) and exits.

**Idle exit.** After 10 minutes with no connection and no queued work.

**Shutdown window.** A dedicated thread owns a hidden top-level window. `WM_QUERYENDSESSION` stops admission and returns `TRUE`. `WM_ENDSESSION` (ending) drains as `coordinator stop` does and exits.

**Pipe.**
- Name: `\\.\pipe\scopework-<16 hex digits of SHA-256 over the user SID, a newline and the canonical, lower-cased research-home path>`.
- Creation: duplex, byte mode, `PIPE_REJECT_REMOTE_CLIENTS`, `FILE_FLAG_FIRST_PIPE_INSTANCE` for the first instance, descriptor `D:P(A;;GA;;;OW)`.
- Client check: `GetNamedPipeServerProcessId` gives the server; its token user must equal the client's, otherwise the client uses direct mode and warns on stderr.
- One std thread per connection.

**Protocol** (UTF-8, one JSON object per line).

The client sends `{"hello": {"protocol": 1, "version": "<product version>", "build": <build number or 0>, "role": "cli" | "gui" | "tool", "submitter": "<name>"}}`. The server answers one of:

| Answer | Meaning |
| --- | --- |
| `{"hello": {"accepted": true, "version": "...", "build": n}}` | Same protocol; go ahead. |
| `{"hello": {"accepted": false, "reason": "older-client"}}` | The client falls back to direct mode. |
| `{"hello": {"accepted": false, "reason": "retiring"}}` | The client was newer: the server drains and exits, and the client starts its own after the pipe disappears. |

The other message types:

| Message | Content |
| --- | --- |
| Request | `{"id": n, "args": ["node", "show", "proj", "42"], "class": "normal"}`. `class` is optional; only the GUI may send `interactive`. |
| Response | `{"id": n, "exit": 0, "stdout": <the JSON the CLI would print>, "stderr": [<warning or error JSON lines>]}` |
| Subscription | `{"subscribe": ["queue", "snapshot"], "project": "proj"}` |
| Event | `{"event": "queue", "project": "proj", "running": [...], "waiting": [...]}` or `{"event": "snapshot", "project": "proj", "revision": n}` |

The CLI prints `stdout` and `stderr` exactly as in direct mode and exits with `exit`, so scripts cannot tell the two paths apart except through the added fields below.

**Read answers.** `node show`, `node read-batch`, `node list`, `status`, `review-queue` and `deps get` gain:
- `"revision"`: the snapshot's catalog revision;
- `"pending"`: a list of `{"node", "operation", "kind", "state": "running" | "queued", "position", "submitter", "affects": [...]}` for the records in the answer.

`affects` values are `text`, `status`, `place`, `membership` and `ledger`. Bookkeeping revision bumps do not appear, and after 0.1 they mostly no longer exist. Direct-mode answers carry `"revision"` and `"pending": []`, so tools see one shape either way.

**Queue.** One queue per open project for the `catalog` resource, owned by that project's writer thread.
- Classes: interactive, normal, bulk, job, with defaults as in design D5.
- Ageing: one class up per 5 s waited.
- Selection when the writer is free: the highest-class head, then, if it is a record operation, every queued record operation in queue order, up to 256 operations or 8 MB of report text.
- Structural operations always commit alone.
- `research queue PROJECT` returns `{"project", "running": [...], "waiting": [...]}`, each entry with id, kind, class, submitter, records and waited milliseconds.

**Merged commits.** The writer validates and applies each operation in turn to a working copy of the catalog and the affected report texts. These are the same checks the CLI runs today, refactored from `Library` methods into functions over that working state. Rejected operations are answered at once and leave no trace in the commit. If at least one is accepted, the writer builds one journal, applies the before-text rule (below), commits with 0.2's `commit`, publishes the next snapshot, and answers every accepted operation.

The journal's top level gains `"operations": [{"id", "submitter", "class", "kind", "nodes": [...]}]`. Older versions ignore it. `history show` lists those operations under their commit.

**Snapshots and the before-text rule.** A snapshot holds:
- the catalog and its index;
- sidecars;
- a revision;
- a `RwLock<HashMap<node id, Arc<str>>>` of report texts.

Readers look up under the read lock. On a miss they read the file under the same read lock and insert it with a brief write lock.

Before a commit touches any report, the writer takes the outgoing snapshot's write lock and inserts the previous text of every report it will change, then releases it and writes files. The next snapshot starts from the outgoing map minus the changed entries, then gets the new texts. The report cache budget is 32 MB per open project; least-recently-read entries beyond it are dropped from new snapshots.

**Coexistence.**
- The writer takes the project's exclusive file lock only for the commit itself.
- Before each commit, and on each read, the coordinator compares the catalog's size and modification time with its snapshot's. If another process committed, it rebuilds the snapshot first.
- Direct-mode commands keep today's locks and fair queue.

**GUI.** The GUI connects as role `gui`, sends its actions as interactive requests and subscribes to queue and snapshot events. The dashboard footer gets a queue pane showing `Queue: n running, m waiting`; clicking it lists the queue. The Research records status bar shows the same for its project. Without a coordinator, the pane shows the lock queue from `research locks`, polled every 2 s.

**Read log.** The coordinator counts reads per record class (App, architecture, chunk, function), and how many of them had a pending entry. `coordinator status` reports the counts, and a daily summary line is appended to `tools/workspaces/history/tooling.jsonl` as kind `coordinator-reads`. This is the measurement research could not make.

### Phase 1 as implemented (2026-10-18)

Phase 1 shipped with these deviations from the text above. Each keeps the requirements (reads never wait for writers, in-flight writes visible to readers, no added risk inside transactions); the tests in `tests/coordinator.rs` cover them.

- **Writer thread.** Each project's writer is a plain thread waiting on a condition variable rather than an `SWOwner`. The queue needs ordering by class and ageing plus merge selection under one lock, which the owner's FIFO lanes do not give. Solworker still runs the parallel work inside commands (hashing, snapshots, imports) on the one shared runtime from phase 0.
- **No report cache.** Snapshots hold no report texts. A read takes the file from disk and checks the snapshot's overlay before and after. The overlay receives a file's previous text before any commit changes the file, so a reader still sees its own revision. The 32 MB cache can come later if reads show disk cost; the read log now measures that.
- **GUI writes stay direct.** The GUI's eight write actions run in-process as before. Direct commits are correct beside a coordinator, which picks them up on the next read, and a GUI write waits at most one coordinator batch for the lock. Routing them through argument passthrough would add temporary files for report bodies without changing results.
- **Polling instead of events.** The footer and status-bar queue panes poll `queue` every 2 s, connecting per round and never starting a coordinator (the lock queue without one). These polls do not count as activity, so an open dashboard does not keep an idle coordinator running. The `subscribe`/`event` messages are not implemented yet; the protocol version reserves them.
- **Read classes.** Reads without a record (status, lists, revision) count under a `project` class beside app, architecture, chunk and function. Counts are flushed on the first idle moment of a new UTC day and on shutdown.

### Phases 2 and 3 (outline)

**Phase 2, tool coordination** (design D8):
- `research job lease PROJECT RESOURCE [--payload FILE] [--merge-key K] [--collapse-key K] [--class job]`, with release and status;
- payload batching and "covered by" answers;
- a manifest at `tools/coordinator.toml` declaring kinds, resources, classes and keys, with flags overriding it;
- runtime drift events for the catalog, sidecars and ledger;
- `research tools audit PROJECT` (advisory).

Adopting it in `catalog.py` and `annotations.py` is the project sessions' change. Its proposal, with the payload and result formats: [phase 2](proposal-coordinator-phase2.md).

**Phase 3, storage:** incremental catalog writes owned by the coordinator, and a cache of derived effective status. Its proposal, with measurement gates: [phase 3](proposal-coordinator-phase3.md).

### Module plan

| Phase | Location | Change |
| --- | --- | --- |
| 0 | `src/research.rs`, `src/catalog_batch.rs`, `src/catalog_remove.rs`, `src/imports.rs` | `reopen_affected` replaces the six walks |
| 0 | `src/catalog_batch.rs`, `src/journal_history.rs` | `commit` and `recover` replace `resume`; fingerprint from open bytes and a single serialisation |
| 0 | `src/background.rs`, `src/report_hashes.rs`, `src/catalog_browser.rs`, `src/imports.rs` | Process-wide runtime |
| 1 | `src/coordinator/mod.rs` | `serve`, lifecycle, idle exit, shutdown window |
| 1 | `src/coordinator/pipe.rs` | Server creation, descriptor, first instance, client identity check; the only FFI, over `windows-sys` |
| 1 | `src/coordinator/protocol.rs` | Message types, handshake |
| 1 | `src/coordinator/snapshot.rs` | Snapshot, text map, before-text rule |
| 1 | `src/coordinator/queue.rs` | Classes, ageing, selection, merge bounds |
| 1 | `src/coordinator/writer.rs` | Owner thread; validation and application over working state; merged commit |
| 1 | `src/operations.rs` | Today's command checks as functions over working state, used by both the CLI's direct path and the writer, so the two cannot diverge |
| 1 | `src/bin/research.rs` | Routing: try the coordinator, else direct mode; `serve`, `coordinator`, `queue` commands |
| 1 | `src/records_ui.rs`, `src/main.rs`, `src/research.rs` | GUI client role, footer and status-bar queue panes |
| 1 | `Cargo.toml` | `windows-sys` as a direct dependency at the version already locked, Windows only |

### Validation plan

Classified as in the user's general workflow (`grimoire/DESIGN.md`, testing section): permanent tests protect lasting behaviour; disposable checks answer one question; benchmarks support a decision.

**Permanent tests (phase 0):**
- Saving and completing a function leaves incomplete ancestors' revisions unchanged and reopens a complete ancestor.
- Prerequisite consumers are reopened and their stale completion tokens rejected.
- A stale draft, completion or structural token is still rejected after each kind of change.
- An interrupted in-process commit (fault injection, as existing recovery tests do) still recovers through full verification.
- Hashing, GUI snapshots, the browser and imports all run on the process-wide runtime.

**Permanent tests (phase 1), all on small self-cleaning fixtures:**
- **Protocol:** handshake outcomes for the same, an older and a newer protocol; the CLI prints identical output through the coordinator and in direct mode for every command group.
- **Reads during a held commit:** a read during a commit held at a fault point answers the previous revision without waiting, with correct `pending`. A concurrent stress test of readers and a writer rewriting the same reports never returns text from a revision other than the answer's.
- **Merging:** forty queued record operations, including deliberately stale tokens, produce results identical to serial application in queue order. A structural operation never shares a commit.
- **Queue order:** interactive operations overtake bulk; ageing promotes waiting bulk work.
- **Crash and coexistence:**
  - killing the coordinator at each commit fault point leaves a journal that a direct-mode command recovers;
  - a direct-mode commit is picked up before the coordinator's next read or commit.
- **Pipe security:** a second server for the same name is refused; a substituted foreign SID is rejected by the client check.
- **Shutdown:** `coordinator stop` and the shutdown window drain and exit. The window path is tested by sending the messages to the window directly.
- **GUI:** the queue pane, `research queue` and the queue's own state agree.

**Benchmarks (decision support, not permanent):**
- Commit cost before and after 0.2, in memory at the live project's catalog size. This is the scale check the project rules allow.
- On a 100-function fixture: commits per second and per-operation latency for 40 concurrent record operations, direct mode against the coordinator.

The results go into the qualification note of the release that carries each phase.

**Verification on the live project** happens only after the user installs a release, and only as read-only observation:
- the commit cadence from receipts;
- `coordinator status` read counts;
- the queue pane.

### Rollout and compatibility

- **Phase 0 can ship first,** as soon as the versioning item is settled. It changes no file format and helps every direct-mode agent.
- **Phase 1 ships with `SCOPEWORK_COORDINATOR=auto`.** Setting `off` restores exactly the phase 0 behaviour, which makes it the immediate rollback if a coordinator problem appears in live use.
- **Mixed versions are safe in every combination:**
  - older CLIs never contact the coordinator, which picks up their commits;
  - a newer client retires an older coordinator;
  - an older client falls back to direct mode.
- **No catalog or report format changes.** Journals gain an `operations` list that older versions ignore when recovering.
- **Upgrades and uninstall** close the coordinator gracefully through the shutdown window; no installer change is needed. Installation notes gain one line: agents' commands keep working during an upgrade, in direct mode, while the coordinator restarts.

### Acceptance criteria

**Phase 0:**

- Completing or saving a record no longer changes the revision of ancestors that are not complete. Complete ancestors and prerequisite consumers are still reopened, and stale completion, draft and structural tokens are still rejected. Regression tests cover each.
- An in-process commit serialises the after-catalog once and does not re-read its journal. Crash recovery still verifies both fingerprints; the existing recovery tests pass.
- One Solworker runtime per process, shared by hashing, GUI snapshots, the browser and imports.

**Phase 1:**

- With a write running, `node show` and `status` answer from the last committed revision without waiting. Answers carry `revision` and correct `pending` entries, with bookkeeping excluded.
- Under a concurrent stress test on a small fixture, no read returns a mixed revision. This includes reads of reports a running commit rewrites.
- Forty queued record operations commit in fewer transactions than operations, with results and rejections identical to applying them one at a time in queue order. Commits per second and per-operation latency are measured before and after, on a fixture.
- Interactive operations overtake queued bulk ones; aged bulk operations are not starved.
- The footer pane, `research queue` and the coordinator's queue agree.
- Killing the coordinator mid-commit leaves a pending journal that the next coordinator or direct-mode command recovers.
- A graceful Restart Manager shutdown drains and exits.
- With the coordinator absent, or a newer or older version present, every command still works.
- A second server for the same pipe name is refused.
- A client rejects a server of another user (unit test with a substituted SID).
- fmt, clippy `-D warnings` and all tests pass.

### Tradeoffs accepted

- **A long-lived process becomes central** for unblocked reads, merging and the queue view. That is bounded by direct-mode fallback, no state outside disk, and the `off` switch (design D1, D2).
- **Several operations per journal** in exchange for paying the fixed commit cost once. Each operation keeps its own result and history entry (D5).
- **New correctness code** (the before-text rule) instead of changing the canonical report layout (D6).
- **About 40 lines of FFI** for the pipe instead of an unauthenticated loopback port (D3).
- **Tool work stays in the tools.** Scopework batches and orders it but never runs it, at the price of tools adopting leases themselves (D8).
- **Narrow invalidation changes which revision numbers move.** Tools that watched ancestor revisions to notice child work would have to look at `pending`, history or the children instead. In the live project, 548 tool scripts read a revision, all of them the project revision for schema-1 batch preconditions. None reads a record's revision, so none is affected.

### Future extension path

- **Phase 2:** leases, payload batching, collapsing, manifests and drift detection, using phase 1's queue and events unchanged.
- **Phase 3:** incremental catalog writes and a derived-status cache, possible once one process makes every write.
- **A faster fingerprint** (a journal schema bump) if phase 0 measurements show hashing still matters.
- **Typed requests** for the GUI and tools, beside argument passthrough, if events and passthrough prove too coarse.
- **Conditional jobs** as lease conditions evaluated by the coordinator ("when the ledger has no pending edits").
- **A history view in the GUI**, fed by the same events.
- **Narrower structural tokens** that conflict only on the records a change touches (research, third pass).
- **Schema-2 batches in tooling.** The live project's 548 scripts still send schema-1 batches with the project-wide revision, so every unrelated commit can reject them. Moving them to schema 2's record-level preconditions is a tooling change worth recommending alongside phase 2.

### Risks

- **One process serves every agent.** A coordinator bug affects all of them at once. Mitigations: no state outside disk, unchanged journal recovery, automatic restart by the next client, and `SCOPEWORK_COORDINATOR=off` as an immediate way back to direct mode.
- **Merging puts several operations in one journal.** Each keeps its own validation, result and history entry. An I/O failure fails the whole commit, and recovery is unchanged.
- **The before-text rule is new correctness code.** It is the one place where a mistake could serve a torn answer, so it gets a dedicated concurrent stress test.
- **Process starts remain** for tools that call `research.exe` per command, until `catalog.py` speaks to the pipe (phase 2). The coordinator removes lock waiting and catalog parsing, not process creation.
- **Unverified on this machine:** a different-user rejection. It is covered by a unit test only.
- **Default values** (idle exit, ageing, merge bound, cache budget) are starting points to tune from phase 1 measurements.
