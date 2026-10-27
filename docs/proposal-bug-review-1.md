# Proposal: app-wide bug review, round 1

Status: implemented 2026-10-27. The second part of the app-wide review: bugs, repeated until a round finds none. Fixes are the obvious ones only.

## Method

- Leads recorded during the code-quality rounds.
- A pass over every `unwrap`/`expect` outside tests for panics reachable from user data. None: each relies on an invariant the code establishes (a validated catalog, a constant regex, the parent of a joined path, a node already checked to exist).
- Reading the coordinator's client, pipe and lifetime code.

## Findings and fixes

### 1. A coordinator outlives its deleted research home (medium)

**Problem.**
- A coordinator exits only after ten idle minutes.
- If its research home is deleted while it starts or runs, it keeps serving a home that no longer exists. Windows lets the open `.coordinator.lock` be deleted with the folder.
- This is how test runs leave `research.exe ... serve` processes behind, whose workspaces are already gone. They hold `target\debug\research.exe` until they exit, which fails the next build.
- The sequence: a command in `auto` mode starts a coordinator, gives up waiting after five seconds under load and runs in direct mode. The test's teardown asks for a stop before the coordinator listens, then deletes the home.

**Design.** The idle watch, which wakes every five seconds, also stops the coordinator once its lock file is gone with no client connected and no queued work: the home it serves is gone.

### 2. A busy pipe is reported as "no coordinator" (medium)

**Problem.**
- `pipe::connect` returns `None` when every pipe instance stays busy for five seconds, as it does when nothing is listening.
- For commands in `auto` mode that is the intended fallback to direct mode.
- But `coordinator status` then answers `running: false`, and `coordinator stop` answers the same without stopping anything.
- A coordinator of another version also answers as not running.

**Design.**
- A pipe that stays busy is an error (`coordinator_busy`), not an absence.
- `connect` keeps its mode rules: in `auto` an error still means direct mode.
- `status` and `stop` see the real outcome. Nothing listening is `running: false`; a busy, refused or failed connection is an error naming why.
- As management commands, they reach a running coordinator whatever `SCOPEWORK_COORDINATOR` says. That setting decides where ordinary commands run, not whether a coordinator can be inspected or stopped. Before, with `off`, `status` reported nothing running even when a coordinator served the home.

### 3. `with_lock_wait` is not panic-safe (low)

**Problem.** It restores the thread's previous lock wait only on a normal return, the same pattern fixed in `with_caller`. A panic the thread survives leaves the inner wait in force.

**Design.** A drop guard restores it however `work` ends.

## Acceptance

- A coordinator whose home is deleted exits within its next idle check, about five seconds.
- `coordinator status` and `stop` report a busy or refusing coordinator as an error, not as not running.
- A unit test panics inside a nested `with_lock_wait` and checks the outer wait is restored.
- The full suite, Clippy and fmt are clean.
