# Proposal: app-wide bug review, round 2

Status: implemented 2026-10-27. Round 2 started from a CI failure. Run 23 (`8a7045c`) failed in its test step, and run 24, on a later commit with the same code, passed, so a test is flaky on the CI runner. The run's log needs a signed-in session, so the failing test is not known. The suspects were reproduced locally by running the coordinator and lease test binaries several times in parallel, which loads the machine the way a CI runner is loaded.

## Findings and fixes

### 1. A coordinator stuck on a deleted home never exits (medium)

**Problem.** Round 1 made a coordinator exit once its home is gone, but only when no client is connected and no work is queued. Under load, coordinators stayed alive for minutes after their homes were deleted: work stuck on the deleted home kept them from ever looking idle. They hold `target\debug\research.exe` and fail the next build.

**Design.**
- After the home has been gone for 30 seconds, the coordinator exits regardless of stuck work.
- Nothing can commit to a deleted home anyway.

### 2. Exiting recreates a deleted home (low)

**Problem.** On its way out, the coordinator flushes its read and commit counts into project history. On a deleted home, opening the project recreates its `projects` folder. A test's teardown, or a user who removed a home, finds an empty home folder come back.

**Design.** Once the home is gone, the coordinator skips the flush: there is nothing to record into.

### 3. Tests hid why a command failed (test quality)

**Problem.** The revision-mixing test's readers parsed a command's output without checking that it succeeded, so a failure showed as a JSON parse panic. The round-1 exit test also called `coordinator status` twice and treated a coordinator closing the handshake while exiting as a test failure.

**Design.**
- Readers assert success and report the command's error.
- The exit test makes one status call per check and treats any answer other than a successful "running" as not running.

## Verification

- Four parallel copies of the coordinator and lease test binaries leave no coordinator processes 40 seconds later; before, two remained for minutes.
- The full suite passes, and Clippy and fmt are clean.

## Open

Under that artificial load, a `required` read through the coordinator once exited unsuccessfully with no output at all. Error answers always carry a message and panics print, so its cause is not visible from the code alone. It is recorded in TODO.md for a reproducible capture; it was not seen in a normal suite run.
