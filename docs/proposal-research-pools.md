# Proposal: restricted research pools replace quarantine

Status: implemented 2026-10-27. Source: TODO "Restricted research pools with agent registration, replacing quarantine", with the user's decisions of 2026-10-25.

## Research

**What the user found.** An agent that hits a safety stop on a record cannot complete that record, but may use the record's research once someone else has completed it. The stops are often false positives. The agents keep their own "do not publish" list in their session notes, for example "exceptions for the user (do not publish): 0x0040BACF, 0x0040C172, 0x0040C200". They do not use quarantine.

**Why quarantine fits neither half.** A quarantined record counts as complete without research, so its work disappears from the meters instead of being done by someone who can. Quarantine seals structure but leaves report text writable by anyone, so it does not keep other agents from writing to the record and hitting the stop again.

**Current state.** No project in the research home has a `quarantine.json`, so no live data needs migrating. The format still needs a migration path, because 0.14.0a can write one.

**Where quarantine reaches into the code** (2026-10-27, about 30 places):
- effective status (`Index::with_quarantine`), the meters and parent completion;
- the reopen walk (`sealed`), batch structure checks, removal and prune, import publication, `node create`;
- completion, reopen, status reset, flags and claims (`refuse_quarantined`);
- the commit gate for an unreadable file (`commit_or_stage`);
- history labels, the GUI browser filter and record banner, and drift watching.

**Who is asking.** Pools check the caller. Since 2cbfd7f, `research::caller()` names the submitter of the current operation, through the coordinator too.

## Design

### Pools and agents

- **Agents register** in the research home: `research agent register NAME` adds a name to `agents.json`; `research agent list` shows them.
  - An agent without a registration registers itself; nothing approves it.
  - Agents identify themselves by `SCOPEWORK_SUBMITTER`, which the project tools already set. `agent register` with no name registers the caller.
  - Like `settings.json`, the file has its own lock and atomic replace. It is not presentation data, so it is a separate file.
- **Pools belong to a project**, in a journaled `pools.json` sidecar beside the catalog (schema 1).
  - Each pool has a name, a reason and its members.
  - Each pooled record has the pool, an origin (`safety-stop` or `human`, as quarantine had), a reason, who added it and when.
  - A log keeps every addition, removal, join and leave with its reason.
- **Membership.** `research pool join PROJECT POOL [--agent NAME]` (default: the caller) and `pool leave`. Only registered agents can join. The app provides only the capability: the human directs which agents join which pool, so there is no approval step.

### Entering a pool

`research pool add PROJECT POOL NODE... --origin safety-stop|human --reason TEXT` creates the pool if it does not exist and adds the records.
- An agent that hits a safety stop adds the record with `--origin safety-stop` and leaves it.
- The human can add records too, from the CLI or the GUI.
- Chunks and functions can be pooled; a pooled chunk covers its functions, as a quarantined chunk did.
- `pool remove` takes a record back out, with a reason.
- Adding a record keeps its status, report, claims and flags; nothing is cleared.

### What a pool does

- **Writes are for members.** Every write that changes a pooled record (or a function of a pooled chunk) is refused unless the caller is a member of that pool. The writes covered:
  - report saves (`node save`, `save-batch`, evidence-only saves), completion, reopen and status reset;
  - review flags, claims and proofs;
  - changing its own prerequisites;
  - creating records in it, moving it or records into or out of it, renaming it, changing its membership, and removing it (batch, delete, prune, import);
  - the refusal is `pooled` and names the pool.
- **The human always may.** `--directed-by` with the human's instruction still overrides, and history records it, as with quarantine. The dashboard acts for the human, so its writes are allowed.
- **Mechanical rewrites are not changes by an agent.** A rename elsewhere rewriting a citation inside a pooled report, and link repairs, still apply, as they do for completed records.
- **Reads are free.** Anyone can read a pooled record and name it as a prerequisite of their own work.
- **Open work for members only.** `status` reports each record's `pool`. A pooled record is `ready_for_review` / `ready_for_synthesis` only for members of its pool, so other agents never see it as available work. `node show` gives `pool`, and whether the caller `may_write`.
- **Status is the record's own.** Unlike quarantine, a pooled record is open work until it is researched. Meters, parent completion and the reopen walk treat it like any record.
- **After completion.** A completed record becomes available to build on: every agent may read it and use it as a prerequisite. It stays in the pool, so non-members still cannot write to it and trigger the stop again.

### Quarantine goes

- `node quarantine`, `unquarantine` and `quarantined` are removed; `pool` replaces them.
- `research pool migrate-quarantine PROJECT` moves every recorded quarantine into a pool named `quarantine`, keeping origin, reason and who added it, and empties `quarantine.json` (sidecars are journaled by content, so the file stays, holding nothing), in one transaction. The records' effective status becomes their own: a record that was complete only through quarantine shows its real state.
- Migration stays manual. While a project still has a `quarantine.json` with records, every write to that project is refused with `quarantine_unmigrated` naming the command. Nothing unseals silently, the same rule as an unreadable file today.
- An unreadable `pools.json` refuses every write (`pools_unreadable`) and reads as no pools, as quarantine did.

### Interfaces

- **CLI:**
  - `agent register|list`;
  - `pool add|remove|join|leave|list|show|migrate-quarantine`;
  - `pool` fields in `node show` and `status`;
  - `status --summary` counts `pooled`.
- **GUI:**
  - the record browser marks pooled records and its filter becomes **In a pool**;
  - the record panel shows the pool, its reason and members;
  - **Pool** controls let the human add or remove a record and choose the pool.
- **History:** sidecar changes name `pools.json`; directed changes keep their instruction.
- **Coordinator:** pool changes and joins are sidecar-only record operations that may share a commit; `pools.json` is watched for drift.

## Acceptance

- A registered member edits, completes and claims a pooled record. A non-member gets `pooled` for each covered write, and a directed write and the dashboard succeed.
- A non-member can read a pooled record and name it as a prerequisite. Its completion counts for the non-member's dependent work.
- A pooled record's status is its own: meters and parent completion count it as open until it is completed.
- `status` shows pooled records as ready only to members of their pool.
- A pooled chunk covers its functions.
- Joining needs a registered agent. Registration with no name registers the caller.
- `migrate-quarantine` moves records into the `quarantine` pool, removes the file and is recorded in history. A project with unmigrated quarantine refuses writes until then.
- An unreadable `pools.json` refuses writes.
- The full suite, Clippy and fmt are clean.
