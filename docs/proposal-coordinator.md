# Proposal: the Scopework coordinator

Research and design for unblocked reads, prioritised and merged writes, and tool coordination when several agents work on one project. Follows [the 2026-10-18 work brief](proposal-2026-10-18.md). This is a proposal; nothing here is implemented yet.

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
| Windows named pipe | The pipe's access list limits it to the user; no port, no firewall. | A small FFI module over `windows-sys`, which is already in the build graph through eframe. |
| Loopback TCP | Any local process can connect; needs a token file. | Standard library only. |
| Spool folder of request files | Filesystem permissions. | Polling latency; many small files. |

Named pipes are selected. They are the platform's intended local IPC, carry the user's identity, and add no new crate.

## Design

### One coordinator process per research home

`research serve` runs one long-lived process per research home. A lock file in the home keeps it single-instance. The first CLI call that finds none starts it detached, the way the GUI already launches import runners. It exits after a configurable idle period. It holds no state that is not also on disk: catalogs, reports and journals stay exactly as today, and crash recovery is still the pending journal.

The CLI keeps its commands and output. When a coordinator is reachable, it sends the command over the pipe and prints the answer; otherwise it runs the command itself as today (**direct mode**). The GUI becomes a client the same way. A `research session` client keeps one pipe open for many commands.

### Reads from immutable snapshots

For each open project, the coordinator holds a **snapshot**: an immutable value containing the committed catalog, its index, sidecars, and report texts loaded on demand for that revision. A commit builds the next snapshot and installs it with a single atomic swap. A reader takes whatever snapshot is current when it starts and keeps it until it finishes, so:

- reads never wait for writes, and writes never wait for reads;
- an answer always comes from exactly one committed revision; there is no window and no retry;
- `status`, `node show`, `read-batch` and the GUI stop re-parsing the catalog on every call.

Report texts are cached per revision and evicted by a byte budget (Solcache). A commit replaces only the entries it changed. Report files edited outside Scopework are noticed through the existing size-and-modification-time stamps before the coordinator serves their text.

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

**Drift audit.** `research tools audit PROJECT` reads the current scripts and reports, per tool:

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
