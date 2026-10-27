# Proposal: fixes from the 2026-10-27 review of the shared-work batch

Status: implemented 2026-10-27. Research is the review itself: a high-effort read of `ae82f78..378ca59` (the `deps set` token, proofs, the caller fix, research pools and the shared work queue). It reported ten findings, and the user asked for all ten to be fixed. Numbers follow the review's ranking.

## 1. Non-members can take records out of a pool (high)

**Problem.** `pool remove` and `pool add` never check pool access. Any agent can take a record out of a pool it is not in, or move it into a pool it then joins, and then change it freely. That defeats the restriction.

**Design.**
- Taking a record out of its pool, or moving it to another pool, changes a pooled record, so it needs a member of its current pool, `--directed-by`, or the dashboard.
- Adding a record that is in no pool stays open to everyone, so an agent that hits a safety stop can always pool the record.
- `pool add` and `pool remove` gain `--directed-by`.

## 2. Pool checks scan every node per record (medium)

**Problem.** `PoolFile::covering` searches `project.nodes` linearly, and `check_pool` calls it for each touched record. A whole-project batch in a 30,000-function project then does about 900 million comparisons in the project writer.

**Design.** Build a parent map once per check; `covering` takes it. Every caller that checks many records uses the map, as quarantine's `check_seal` did.

## 3. A damaged `proofs.json` breaks every read (medium)

**Problem.** `node show`, `status`, `node unproven`, `project show` and the GUI's record load all fail when `proofs.json` cannot be parsed. Flags and pools refuse writes but keep reads working.

**Design.**
- Reads treat an unreadable `proofs.json` as holding no proofs and no requirement.
- Proof writes and function completion read it strictly and refuse with `proofs_unreadable`. Completion fails closed, because the requirement is unknown.

## 4. `with_caller` is not panic-safe (low)

**Problem.** If an operation panics inside `with_caller` and the coordinator's writer survives, the thread keeps that operation's caller.

**Design.** A drop guard restores the previous caller on every exit.

## 5. Moving a record between pools loses history; reasons never update (low)

**Problem.** Re-adding a record that sits in another pool logs only `add`, not its removal from the old pool. Re-adding it to the same pool with a new reason changes nothing.

**Design.**
- A move logs `remove` from the old pool and then `add` to the new one.
- Re-adding to the same pool with a different reason or origin updates the record and logs `update`.

## 6. The dashboard offers "Take out of pool" for inherited membership (low)

**Problem.** A function pooled through its chunk shows the button, and it always fails.

**Design.** For such a function, the group names the chunk that carries the pool and offers no button.

## 7. The dashboard hides direct-mode shared work (low)

**Problem.** Without a coordinator, `research queue` reports resource lock holders, but the dashboard's queue pane reads only operations and leases. Shared resources held in direct mode never show.

**Design.** The queue pane counts held resource locks from a lock-queue answer as running, marked with their resource. A shared one is marked `shared`.

## 8. `node show` parses `pools.json` three times (low)

**Design.** `record()` reads the pools file once and uses it for the pool, `may_write` and the members list.

## 9. Sidecar transactions are written out three more times (cleanup)

**Problem.** `write_proofs` and `write_pools` repeat the single-sidecar journal that `write_sidecar` builds for claims and the review queue.

**Design.** `write_sidecar` takes the empty text to bootstrap a missing file. Proofs, pools and claims use it.

## 10. Pool checks are scattered across write paths (altitude)

**Problem.** About twelve write paths each call `check_pool` themselves. A new write path that forgets the call lets non-members change pooled records, and nothing notices.

**Design.**
- Every `Journal` names the records the operation changes on purpose: `changes`.
  - It is a required field, never serialized, so a new write path cannot compile without stating it.
  - Mechanical effects are not in it: the reopen walk, citation rewrites, link repairs and identity amendments.
- One guard, `Library::check_journal`, checks `changes` against the pools with the journal's `directed_by`. It runs where every journal commits: `commit_or_stage`, and the batch and removal commits.
- The scattered calls go, apart from the ones that decide something before a journal exists:
  - the import reservation;
  - prune, which passes over pooled records.

## Acceptance

- A non-member's `pool remove`, or its `pool add` of a record pooled elsewhere, is refused with `pooled`; a member's, a directed one and the dashboard's succeed.
- A move between pools logs `remove` and `add`; a new reason logs `update`.
- With a damaged `proofs.json`, `node show` and `status` still answer, while proof writes and function completion refuse with `proofs_unreadable`.
- Every existing pool test still passes through the journal guard.
- The queue pane parses lock-queue resources.
- The full suite, Clippy and fmt are clean.
