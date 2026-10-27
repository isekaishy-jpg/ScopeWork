# Proposal: a shared work queue for tool work outside projects

Status: implemented 2026-10-27. Source: TODO "A work queue in the app for tool work".

## Research

**How agents share the verify worktree today.** Rust verification and commits for 3-3-5a go through one worktree, `wow-335a-rust-verify`. Each session's own commit script (`mycommit.sh`, in that session's temporary folder) guards it with a hand-written fair ticket lock in the GitHub folder:
- `verify.lock/pid` holds the owner;
- `verify.queue/<time>_<pid>` files are the tickets, and only the oldest live ticket may take the lock.

Every session carries its own copy of the script, `research queue` and the dashboard cannot see the queue, and one session made a second worktree and script to avoid sharing.

**What Scopework already has.** `research job run` holds a lease on named resources while a command runs. It provides most of what the agents built by hand:
- fair ordering, with classes and ageing;
- joining of same-key work;
- the lease ends when the holder's connection does, so a dead holder releases it at once;
- the same exclusive lock files in direct mode, with fair tickets;
- visibility in `research queue`, `job status` and the dashboard's queue panes.

**What it lacks.** A lease belongs to one project:
- its lock files are named after the project;
- the coordinator keeps one lease queue per project;
- `job run` requires a project with a catalog;
- the command runs in the project folder.

A repository shared by sessions belongs to no project, and leasing it through one project would hide it from every other.

**Waiting.** Without a coordinator, lock files wait at most the lock wait (`--wait`, default 120 s) and then fail with `busy`. A coordinator lease waits until granted.

## Design

Shared resources: the same leases, scoped to the research home instead of a project.

- `research job run --shared --resource NAME [--key K] [--class C] [--payload-file F] -- COMMAND` leases home-wide resources. `--shared` replaces the project argument; `--resource` is required, since shared resources have no declarations.
  - The command runs in the caller's current folder, not a project folder.
  - Its payload and result files go in the home's `tools/scratch`.
  - The contract is the same as `job run`: payloads, results, exit codes and joining.
- **Coordinator.** One home-wide lease queue beside the per-project ones, with the same ordering, classes, ageing, joining and release on disconnect. Requests name `"shared": true` instead of a project.
- **Lock files.** Shared resources take exclusive lock files named `_shared.res-NAME` under `projects/.project-locks/`. The underscore can never begin a project ID, so they cannot collide with a project's. Direct-mode requests queue on them with fair tickets, as project resources do.
- **Visibility.**
  - `research queue --shared` and `research job status --shared` show held and waiting shared leases (or, without a coordinator, the lock holders).
  - Every project's `research queue` answer carries `shared_leases`, so the dashboard's queue panes list shared work too, marked `shared`.
- **Guidance.** docs/cli.md gives the verify worktree as the example, with a long `--wait` for direct mode:

  ```
  research --wait 3600 job run --shared --resource verify-worktree --class job -- bash mycommit.sh
  ```

  Projects can then drop their own lock scripts; that change belongs to the projects.

## Acceptance

- Two shared requests for the same resource run one after the other, through the coordinator and in direct mode. Different resources run side by side.
- A held shared lease shows in `queue --shared` and in a project's `queue` as `shared_leases`.
- A holder that dies releases the lease, and the next request runs.
- `--shared` without `--resource`, or with a project, is refused.
- The command runs in the caller's folder.
- The full suite, Clippy and fmt are clean.
