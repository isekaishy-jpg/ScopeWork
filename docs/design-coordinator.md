# Design: the Scopework coordinator

Design stage for [the coordinator proposal](proposal-coordinator.md). For each major decision this document states the alternatives, evaluates them against fixed criteria, records the choice and its consequences, and names what remains for the proposal. Measurements cited here are from the proposal's research sections.

## Criteria

Every decision is judged against the same seven criteria, in this priority order:

| # | Criterion | Meaning |
| --- | --- | --- |
| C1 | Correctness | No torn or mixed-revision answer, and no accepted operation that serial execution would reject. Crash recovery stays as strong as today. |
| C2 | Reads never wait on writes | A read's latency does not include any commit's duration. |
| C3 | Write throughput under contention | Commits per second and queue wait when several agents write at once (live peak: 57 per minute, about one second each on 0.8.0). |
| C4 | Failure blast radius | What one bug, crash or bad operation can affect, and how recovery works. |
| C5 | Compatibility | Direct mode, older Scopework versions, upgrades and the existing CLI contract keep working. |
| C6 | Tool independence | Scopework holds no knowledge of particular tools; tooling can change daily. |
| C7 | Cost to build and maintain | Code size, unsafe surface and the number of new concepts. |

C1 can never be traded for anything else. C2 and C6 are user requirements. C3–C7 decide between options that pass those.

## D1. How reads avoid waiting

| Option | C1 | C2 | C3 | C4 | C5 | C7 |
| --- | --- | --- | --- | --- | --- | --- |
| **A.** Keep shared project locks (today) | Yes | No: a read waits for a running commit | No change | Small | Full | None |
| **B.** Lock-free reads in every process: read files, then consult the pending journal for before-texts of reports a running commit changes | Yes, if every reader re-checks for a pending journal or newer receipt after reading | Yes | No change | Small | Full | High: subtle ordering logic in every command, every process still parses the 7.8 MB catalog (21 ms) per call |
| **C.** One coordinator per research home serving reads from immutable in-memory snapshots | Yes, with the before-text rule (D6) | Yes | Enables merging (D5) | Larger: one process serves all agents; mitigated by direct-mode fallback | Fallback to A | High, but in one place |

B meets C1 and C2 but does nothing for queueing, priorities, merging or the queue counter, which are also requirements. Its ordering logic would live in every command path. C concentrates the same correctness work in one place and is the only option that also delivers the other requirements.

**Decision: C.** Direct mode keeps A as the fallback.

**Consequences:**
- reads cost a pipe round trip (about 11–25 µs) instead of a process start, a parse and a lock;
- answers carry a revision and pending context (D7);
- the coordinator must be robust, see D2 and D10.

## D2. Coordinator process and lifecycle

| Option | C2/C3 | C4 | C5 | C7 |
| --- | --- | --- | --- | --- |
| **A.** Windows service | Yes | Runs as another account: research homes, file ownership and pipe identity become cross-user problems | Needs administrator install and service management | High |
| **B.** Hosted in the GUI | Only while the GUI runs; agents are usually headless | GUI crash stops every agent's coordinator | Partial | Medium |
| **C.** Started at logon (scheduled task) | Yes | Separate from any client | Extra installer surface; runs while unused | Medium |
| **D.** Started on demand by the first client (CLI or GUI), detached, one per research home, exits when idle | Yes | Separate from any client; restarted by the next client after a crash | No installer change; works for every home, including `--home` overrides | Low |

**Decision: D.**

**Single instance:** the pipe's first-instance flag plus a lock file in the home.

**Idle exit:** after a configurable period with no clients and no queued work. Default 10 minutes.

**Shutdown:** the coordinator owns a hidden top-level window on its own thread, which the fifth research pass tested. On `WM_QUERYENDSESSION` it stops admitting work. On `WM_ENDSESSION` it finishes the commit in progress and exits. Upgrades, uninstall, logoff and shutdown then close it gracefully through Restart Manager with no installer custom action. `research coordinator stop` does the same on request.

**Upgrades:** handled by the version handshake (D4).

## D3. Transport

| Option | Isolation | C7 |
| --- | --- | --- |
| **A.** Loopback TCP with a token file | Any local process can try to connect; the token protects. Port conflicts possible. | Standard library only |
| **B.** Spool folder of request and response files | Filesystem permissions | Polling latency; many small files in the home |
| **C.** Named pipe | Owner-only descriptor, remote clients rejected, first-instance creation, client check of the server's user. All but the different-user rejection are tested. | About 40 lines of FFI over `windows-sys` (already in the build graph); clients use plain file opens, from Rust and from Python |

**Decision: C.**

**Pipe name:** `\\.\pipe\scopework-<hash>`, where the hash covers the user's SID and the canonical research-home path. Different users and different homes never share a pipe.

**Security:** the descriptor is `D:P(A;;GA;;;OW)`. A client that finds a server running as another user refuses it and falls back to direct mode. This user-mismatch path gets a unit test with a substituted SID.

## D4. Protocol and versioning

| Option | C5 | C7 |
| --- | --- | --- |
| **A.** Argument passthrough: a line holds the CLI arguments; the answer is the same JSON the CLI prints, its error JSON and exit code | Identical behaviour through the coordinator and in direct mode, because both run the same command dispatcher | Low |
| **B.** A typed request per operation | Two code paths that can diverge | High |
| **C.** A, plus a few typed messages for things the CLI has no command for (handshake, subscriptions, events) | As A | Low |

**Decision: C.**

**Handshake:** every connection begins with a hello giving the protocol number, the product version and the build.
- A client newer than the coordinator asks it to retire: drain, then exit. The client then starts its own.
- A client older than the coordinator runs in direct mode, which is still correct.
- So a coordinator never serves a client of another protocol version.

**Read answers** gain `revision` and `pending` fields (D7). Existing fields keep their meaning, so tools that ignore unknown fields keep working.

## D5. Write queue, priorities and merging

**Ordering.**

| Option | Requirement 3 | C1 | C7 |
| --- | --- | --- | --- |
| **A.** FIFO only (today's fair lock queue) | No priorities | Yes | Low |
| **B.** Solworker resource demand ranks (`SWPriority`) | Built for consumer demand on resource production; Solworker's guide says application ordering belongs to the application | Yes | Medium, and a misuse |
| **C.** The writer's own queue: four classes (interactive, normal, bulk, job), FIFO within a class, ageing | Yes | Yes: tokens make any order safe | Medium |

**Decision: C.** Ageing raises an operation one class after 5 s of waiting (tunable), so bulk work is never starved. The default class follows the submitter and the operation:

| Class | Default for |
| --- | --- |
| Interactive | GUI actions |
| Normal | Agent operations on one record |
| Bulk | Batches of more than 32 records, imports, evidence-only passes |
| Job | Leases (D8) |

A client may lower its class; only the GUI may use interactive.

**Merging.**

| Option | C1 | C3 | C4 |
| --- | --- | --- | --- |
| **A.** No merging | Yes | About 0.3–0.4 s fixed per commit on the live project after phase 0 (D9 lowers this) | One operation per journal |
| **B.** Gather window: wait up to N ms for more operations | Yes | Better under load, worse at low load (added latency) | Several operations per journal |
| **C.** Drain on free: when the writer becomes free, take the queue head plus every compatible queued operation, up to a bound; never wait for more | Yes | Equal to A at low load; at peaks, the fixed cost is paid once per batch | Several operations per journal; each validated alone |

**Decision: C.** Bound: 256 operations or 8 MB of report text per commit.

**Compatible operations:** same project and same resource, in the merge groups below.

| Group | Members | Commits |
| --- | --- | --- |
| Records | Completions, report and evidence saves, reopen, review flags, claims, review-queue changes | Together |
| Structure | Node batches, moves, deletions, link repair, prerequisite changes, scope and binary identity | Alone, one operation per commit; they relocate files, and later operations would otherwise read reports at moved paths |

**Validation runs in queue order against the in-memory state left by the operations before it.** A merged commit therefore accepts and rejects exactly what serial execution in that order would (proposal, third pass, question 3). All validation finishes before anything is written. A rejected operation gets its own error. An I/O failure while committing fails every operation in that commit with the same error, and the journal recovers as today.

**Journal and history** gain an `operations` list: submitter, class, kind, records, and result per operation. `history show` lists operations, not only commits.

## D6. Snapshots and in-place report writes

The fourth research pass found that commits rewrite report files in place. A reader of an older snapshot that falls back to a file read could therefore see text from a newer commit.

| Option | C1 | C2 | C7 |
| --- | --- | --- | --- |
| **A.** Versioned report files (write the new text under a new name) | Yes | Yes | Breaks the canonical-report layout the project rules require |
| **B.** Readers hash what they read and compare with a hash recorded in the snapshot | Yes, if every report's hash is in the snapshot | Yes | High: hashes for all 31,603 reports kept current |
| **C.** Before-text rule. Each snapshot holds a text map behind a read-write lock. A reader takes the read lock for a lookup, and for a file read on a miss. Before touching any report, the writer takes the write lock and stores the previous text of every report it will change. | Yes | Yes: writes wait only for file reads already in flight (milliseconds); reads never wait for a commit | Low |

**Decision: C.**

**Report cache:** a byte budget of 32 MB per open project by default. The hourly working set measured about 3 MB; all reports total 100 MB. A new snapshot inherits the old one's unchanged texts. Solcache is not used for the concurrent map, because its keyed cache is unlocked and its cleanup context is tied to one thread (fourth pass). It may be used later for writer-side retention.

**External edits:** a report file changed outside Scopework is still detected at commit time by the existing token checks.

## D7. Pending-write context on reads

Every read answer includes `revision` (the snapshot's catalog revision) and a `pending` list for the records in the answer. Each entry gives:
- operation kind and state (queued or running);
- submitter and queue position;
- `affects`: one or more of `text`, `status`, `place`, `membership` and `ledger`.

**Bookkeeping is not listed.** A revision bump on an ancestor that is not complete is not a change to that record. Before D9's change, such bumps would otherwise put a pending entry on the App and every architecture for 85% of writes.

The GUI shows the same on the record header, and disables editing of a record with a running write.

**Alternative rejected:** listing the whole queue on every answer. It costs size, and agents would have to filter it themselves.

## D8. Tool coordination

**What the coordinator does for tools.**

| Option | C6 | C4 | C7 |
| --- | --- | --- | --- |
| **A.** The coordinator runs declared tool commands itself | Needs the tools' environment, paths and secrets in a long-lived process | A bad tool job runs inside the coordinator's lifetime and identity | High |
| **B.** Scopework implements tool operations natively (for example ledger validation) | No: hard-codes tool knowledge that changes daily | Medium | High, and never finished |
| **C.** Generic leases on named resources. A request may carry an opaque payload and a merge key. The holder of a mergeable lease receives every queued payload with the same key, does the work once and reports a result per payload. Collapsible jobs answer queued identical requests "covered by" | Yes: the work stays in the tool; Scopework only orders, batches and relays | A lease holder that dies is detected (as with lock tickets) and its lease is released | Medium |

**Decision: C.** Annotation edits become payloads on a `ledger` resource. The `annotations.py` holder applies all queued edits in one validated write and one Ghidra apply.

**A collapsed request is covered only by a job that started after the request was queued.** Otherwise the job might not have seen the state the requester needed.

**Declarations.**

| Option | C6 | C7 |
| --- | --- | --- |
| **A.** A manifest file in the project's `tools/` folder | Changes with the tools | Low |
| **B.** Flags on each `research job` call | Always current | Low |
| **C.** Both: the manifest gives defaults, and flags override them | Yes | Low |

**Decision: C.** An undeclared tool can still take a plain lease.

**Drift detection.** The fourth research pass showed static analysis cannot resolve 89% of tool write targets.

| Option | Reliability |
| --- | --- |
| **A.** Static audit only | Misses most writes |
| **B.** Runtime detection of changes to shared resources made outside a declared operation or lease | Exact for what matters |
| **C.** B, plus the static audit as an advisory report of modules, commands and subprocesses used | Exact, with hints |

**Decision: C.** Runtime drift is recorded as a history entry naming the resource, the time and the lease holders at that time.

**Shared resources watched:**
- the catalog;
- sidecars;
- the annotation ledger;
- reports, at commit through tokens.

The Ghidra project directory is excluded, because Ghidra writes it constantly; it is covered by its lease only.

## D9. Changes that do not need the coordinator (phase 0)

Three findings help today's direct-mode agents and lower the coordinator's commit cost. None depends on the coordinator.

1. **Invalidate only what a change can affect** (third pass, question 1). Bump revision and clear acceptance only for complete ancestors and for prerequisite consumers, not for every ancestor.
   - **Effect:** fewer records per commit, smaller journals, no spurious schema-2 conflicts on chunks and architectures.
   - **Compatibility:** older versions that still bump more remain correct alongside.
   - **Alternative rejected:** keeping the bumps and filtering them in pending context only. That leaves the conflicts and the journal size.
2. **Cheaper fingerprints** (third pass, question 2).
   - Hash the bytes read at open for the before-state.
   - Serialise the after-state once and use the same bytes for its hash and the write.
   - In-process commits skip re-reading and re-verifying the journal they have just written; crash recovery keeps full verification.
   - About 190 ms becomes about 40 ms per commit on the live project, with no journal format change.
   - **Deferred:** a faster non-cryptographic fingerprint, which needs a journal schema bump.
3. **One Solworker runtime per process**, replacing a runtime per hash pass, GUI refresh, browser search and import.

These ship first, with their own regression tests.

## D10. Coexistence with direct mode and older versions

| Option | C1 | C5 |
| --- | --- | --- |
| **A.** The coordinator holds each open project's exclusive lock permanently | Yes | Direct-mode and older clients block until it exits |
| **B.** The coordinator takes the exclusive lock only to commit; before each commit and on reads, it checks the catalog's size and modification time and reloads when another process committed | Yes | Full |

**Decision: B.**
- The stat check costs microseconds per read.
- A reload costs about 50 ms, which matters only while older agents remain. All recent catalog changes on the live project are journaled commits (third pass, question 5).
- `SCOPEWORK_COORDINATOR=off|auto|required` selects direct mode, normal operation, or failure when no coordinator is available. The default is `auto`.

## D11. The queue counter

| Option | C7 | Accuracy |
| --- | --- | --- |
| **A.** The GUI polls `research locks` / `research queue` | Low | Up to the poll interval stale; extra process per poll |
| **B.** The GUI subscribes to queue events over its pipe connection | Low, once events exist | Immediate |

**Decision: B with A as the fallback.** In direct mode the pane shows the lock queue from `research locks`. The pane shows running and waiting operations by class, and lists each operation on demand. `research queue PROJECT` gives the same on the CLI.

## D12. Solworker layout

| Thread or class | Work |
| --- | --- |
| One runtime per process, High class | Interactive validation, snapshot builds |
| Mid class | Normal validation, report hashing for commits |
| Low class | Bulk validation, reference index, background hashing |
| One owner thread (`SWOwner`) per open project | The writer; the queue lives in its state |
| Plain std threads | Pipe connections, which block on I/O; Solworker workers must not block |
| Dedicated thread | The hidden shutdown window |

A notification route wakes the event pusher when queue or snapshot state changes.

**Alternative rejected:** Solworker external producers for pipe I/O. Each connection is a long-lived session, not a provider result.

## Summary of decisions

| Decision | Choice |
| --- | --- |
| D1 Reads | Coordinator snapshots; locks only in direct mode |
| D2 Lifecycle | On-demand per home, idle exit, hidden shutdown window, `coordinator stop` |
| D3 Transport | Named pipe, owner-only, first-instance, server identity check |
| D4 Protocol | Argument passthrough, plus handshake and events |
| D5 Queue | Writer-owned classes with ageing; drain-on-free merging of record operations; structure alone |
| D6 Snapshots | Before-text rule; 32 MB report cache |
| D7 Pending context | Per-record, bookkeeping excluded |
| D8 Tools | Generic leases with payload batching and collapsing; manifest plus flags; runtime drift detection, static audit advisory |
| D9 Phase 0 | Narrow invalidation, cheaper fingerprints, one Solworker runtime |
| D10 Coexistence | Lock only to commit; reload on outside commits |
| D11 Counter | Pushed events; lock queue in direct mode |
| D12 Solworker | One runtime; owner thread per project; std threads for pipes |

## Open for the proposal

- Concrete defaults to confirm: idle exit (10 min), ageing (5 s), merge bound (256 operations or 8 MB), report cache (32 MB). These are starting values to tune from the phase 1 read log and queue metrics.
- Which merge-group boundaries need tests beyond the serial-equivalence property.
- How `catalog.py` adopts the pipe and how `annotations.py` adopts ledger leases. Tooling changes, made by the project's sessions.
