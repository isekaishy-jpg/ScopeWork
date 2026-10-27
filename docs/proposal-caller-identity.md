# Proposal: commands run as their caller through the coordinator

Status: implemented 2026-10-27. Found while designing research pools, which check who is asking.

## Research

The coordinator executes every client's command inside its own process. Code that records or checks who is asking read `SCOPEWORK_SUBMITTER` (else the Windows user name) from the environment, which is the coordinator's. The coordinator inherits its environment from the client that started it, so every operation sent through it recorded that first client's name. The implementation tracker's completion marks (`impl complete`) and the new proof records were affected. Each client already sends its submitter in the handshake, and every queued operation keeps it.

## Design

- `research::caller()` names who is asking: the caller set for the current operation, else `SCOPEWORK_SUBMITTER`, else the Windows user name.
- The coordinator runs each write, merged or alone, and each read inside `with_caller(submitter)`, with the submitter of the operation's own client.
- Direct mode is unchanged: its process is the caller.
- The tracker and proofs record `caller()`; pools will check it.

## Acceptance

With a coordinator started by client `alpha`, a proof recorded by client `beta` is recorded as by `beta`. The full suite, Clippy and fmt are clean.
