# Proposal: the Scopework coordinator

Research and design for unblocked reads, prioritised and merged writes, and tool coordination when several agents work on one project. Follows [the 2026-10-18 work brief](proposal-2026-10-18.md). Research is complete; the design stage is still to do, so the Design section is a draft. Nothing here is implemented yet.

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

**Status of this document.** The design stage, which evaluates the alternatives for each major decision against explicit criteria, has not been done. Until it is, the selections under Design below are a draft.

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

**Status.** No design assumption remains untested apart from the different-user rejection above. Only the read-during-write rate is unmeasured; phase 1 adds the read log that measures it.

## Design

### One coordinator process per research home

`research serve` runs one long-lived process per research home. A lock file in the home keeps it single-instance. The first CLI call that finds none starts it detached, the way the GUI already launches import runners. It exits after a configurable idle period. It holds no state that is not also on disk: catalogs, reports and journals stay exactly as today, and crash recovery is still the pending journal.

The CLI keeps its commands and output. When a coordinator is reachable, it sends the command over the pipe and prints the answer; otherwise it runs the command itself as today (**direct mode**). The GUI becomes a client the same way. A `research session` client keeps one pipe open for many commands.

### Reads from immutable snapshots

For each open project, the coordinator holds a **snapshot**: an immutable value containing the committed catalog, its index, sidecars, and report texts loaded on demand for that revision. A commit builds the next snapshot and installs it with a single atomic swap. A reader takes whatever snapshot is current when it starts and keeps it until it finishes, so:

- reads never wait for writes, and writes never wait for reads;
- an answer always comes from exactly one committed revision; there is no window and no retry;
- `status`, `node show`, `read-batch` and the GUI stop re-parsing the catalog on every call.

Report texts are cached per revision and evicted by a byte budget. The fourth pass found Solcache suits only writer-side retention, and that in-place report writes need a before-text ordering rule. A commit replaces only the entries it changed. Report files edited outside Scopework are noticed through the existing size-and-modification-time stamps before the coordinator serves their text.

### Pending-write context on reads

Every read answer includes the snapshot `revision` and a `pending` list for the records it returned (not the whole queue). Each entry holds:

- the operation kind and state (`queued` or `running`);
- who submitted it and the queue position;
- what it will change: text, status, location, membership or ledger rows.

An agent that needs the result can `wait --revision-above`; one that only reads moves on, informed. The GUI shows the same on the record header and greys editing of a record with a running write.

### Write queues, priorities and merging

The coordinator keeps one queue per **named resource**. Built-in resources are each project's `catalog` and its `ledger`; tools can declare more, such as `ghidra`. Each operation carries a **priority class**:

- **interactive:** GUI actions, single saves and completions;
- **normal:** typical tool writes;
- **bulk:** imports, large batches and evidence passes;
- **job:** long tool runs.

Order is FIFO within a class, with ageing so a lower class is never starved. Running work is never pre-empted.

**Merging.** An operation kind may declare a **merge key**. When a writer finishes, the coordinator takes the head of the queue plus every compatible queued operation with the same key, up to a size bound. Nothing is delayed to wait for more. Each one is validated on its own, in arrival order, against the in-memory state as it stands after the ones before it. Rejected operations get their own error. The accepted ones commit as one transaction: one journal, one catalog write, one fsync. The first merge keys:

- completions;
- report and evidence saves;
- claims, flags and review-queue changes;
- ledger row edits.

The rule for every merged operation is that it would have produced the same result run alone, in that order. Token checks, prerequisites and completion rules are unchanged.

**CPU work** runs on one Solworker runtime per process:

| Class | Work |
| --- | --- |
| High | Validating and rendering an interactive batch, building the next snapshot |
| Mid | Normal-class validation, report hashing for commits |
| Low | Background index rebuilds, the reference index, bulk evidence passes |

Each project's committing writer is an owner thread (`SWOwner`) that holds the mutable state. Batches validate in parallel with `try_spawn_batch` behind one completion. Snapshots are `SWShared`. A notification route wakes the pipe server when queue or snapshot state changes, and it pushes those events to subscribed clients. The GUI then updates on change instead of polling on a timer. The GUI's and the CLI's own runtimes are replaced by one long-lived runtime per process.

### Tool coordination: declared, not inferred

A tool declares itself in a manifest in the project's `tools/` folder, or with flags on `research job`. The declaration gives the operation kind, the resources it touches, its priority class, and whether queued requests may be merged or collapsed, with the key. Two forms:

- **Coordinator operations** for writes Scopework can perform itself. Ledger `put` and `put-type` become ledger operations. The coordinator validates, merges and writes them, replacing direct file writes and the ledger race with moves.
- **Leases** for work that stays in the tool's own process, such as headless Ghidra. A tool asks for a lease on its declared resources, waits in the fair queue and releases when done. A collapsible job (an annotation apply, an inventory export) whose request is still queued when an identical job finishes is answered "covered by job N" and never runs twice.

An undeclared tool still works. It can take a plain named lease and gets no merging. Nothing breaks when a tool changes or a new one appears.

**Drift audit.** (The fourth pass found static analysis cannot tell which files a tool writes; drift on shared resources must be detected at runtime, with this static audit only advisory.) `research tools audit PROJECT` reads the current scripts and reports, per tool:

- the files and resources it writes;
- the CLI commands it calls;
- the locks it takes.

It flags differences from the declarations, such as a tool writing the ledger without declaring it, or merge candidates. The report is advisory; the coordinator never infers behaviour from it.

The three shared modules are the natural adoption points. `catalog.py` gains the speed-up from the CLI routing alone. A pipe-speaking `catalog.py` would also avoid the process start per call. `annotations.py` and `fresh_re.lock` would move to ledger operations and leases. Those are tooling changes, made by the project's own sessions when they choose.

### The queue counter

The dashboard footer gets a queue pane: running and waiting operations for the selected project, by class (for example `Queue 1 running · 4 waiting`). Hovering or clicking lists each operation: kind, submitter, records or resource, and waiting time. The Research records status bar shows the same for its project. With no coordinator, the pane shows the direct-mode lock queue (`research locks`), so it is never blank or wrong. `research queue PROJECT` gives the same view on the CLI.

### Direct mode and compatibility

Direct mode stays fully correct. It is today's journaled transactions, the fair lock queue and the blocked-recovery reads from the 2026-10-18 work.

The coordinator takes a project's exclusive lock only while it commits, as a direct writer does. Before each commit it checks whether `project.json` changed since its snapshot, and reloads if so. Direct-mode writers, older Scopework versions and external edits therefore stay safe, at the price of a reload. There is only ever one writer at a time per project, whichever path it came through.

Mixed operation is a supported state, not an error. It is also the state during every upgrade.

## Phasing

1. **Coordinator core.**
   - `serve`, the pipe protocol, auto-start and idle exit;
   - snapshot reads with pending context;
   - per-project write queues with priority classes;
   - merging for completions and report and evidence saves;
   - CLI and GUI routing with direct-mode fallback;
   - one Solworker runtime per process;
   - the footer queue pane.
2. **Tool coordination.**
   - ledger operations;
   - named leases with collapsing;
   - manifests and `research tools audit`;
   - an optional pipe client for `catalog.py`.
3. **Storage.** With all writes coming from one process, incremental catalog writes (open TODO) become possible without changing readers.

Each phase is independently useful and releasable.

## Acceptance criteria (phase 1)

- With a write running, `node show` and `status` answer from the last committed revision without waiting.
- Answers carry `revision` and a correct `pending` list.
- A reader never sees a mixed revision, under a concurrent stress test on a small fixture.
- Forty queued single-record completions commit as fewer transactions than operations. Results and rejections are identical to applying them one by one. Measured on a fixture: commits per second and per-operation latency, before and after.
- Interactive operations overtake queued bulk ones; bulk ones are not starved.
- The footer pane and `research queue` agree with the coordinator's queue.
- Killing the coordinator mid-commit leaves a pending journal that the next coordinator or direct-mode command recovers, exactly as today.
- With the coordinator absent or a different version, every command works in direct mode.
- An external `project.json` change is picked up before the next commit.
- fmt, clippy `-D warnings` and tests pass.

## Risks and open questions

- **One process becomes critical.** Mitigation: no state outside disk, journal recovery unchanged, direct-mode fallback, and a watchdog restart by the next CLI call. A coordinator bug could still affect every agent at once, which argues for releasing phase 1 behind a `SCOPEWORK_COORDINATOR=off|auto` switch.
- **Memory.** A snapshot of 31,600 records plus cached report texts is tens of megabytes per open project; the report cache needs a byte budget.
- **Merging changes failure isolation.** Each operation is still validated alone and gets its own result, but one journal now holds several operations. History must still show each operation with its submitter.
- **Process starts remain** for tools that call `research.exe` per command until `catalog.py` speaks to the pipe. The routing removes lock waiting, not process creation.
- **Open:**
  - the idle-exit period;
  - whether the GUI may start the coordinator or only connect to it;
  - whether the coordinator may ever run tool commands itself (proposed: no; leases only).
