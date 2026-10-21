# Proposal: a GUI crash log

Status: proposed 2026-10-21.

## Problem

On 2026-10-21 the GUI (0.13.1a, Build 000003) exited with `0xC0000409`, fast-fail code 7, while another agent's 3D work exhausted the machine; most other programs crashed too, and the coordinator survived. Scopework left no trace of why. Complaint: `2026-10-21-agent-gui-abort-0xc0000409.md`.

## Research

- The installed executable is byte-identical to `target/release/research-dashboard.exe` of that build, and its PDB was still present. Resolving the fault offset `0x57d6c1` through the system `dbghelp.dll` gives `std/src/alloc.rs:429`: Rust's allocation-failure path. A memory allocation failed and the standard library aborted the process (fast-fail 7 is `abort()`).
- The release profile keeps the default `panic = unwind`. An ordinary panic in the GUI would unwind and exit with code 101; it would not produce fast-fail 7.
- On allocation failure the standard library writes `memory allocation of N bytes failed` to stderr and aborts. A panic writes its message to stderr too. The GUI is a windowed (`windows` subsystem) process: its stderr goes nowhere, so both messages are lost.
- Rust's Windows stderr looks up `GetStdHandle(STD_ERROR_HANDLE)` on each write, so redirecting the process's standard error handle with `SetStdHandle` at startup captures every later write, including the allocation-failure message and panic output.
- A custom allocation-error hook (`std::alloc::set_alloc_error_hook`) is unstable and cannot be used.

## Design

1. **Log file.** At startup the GUI opens `logs/gui.log` in the active research home (append, shared read so it can be viewed while running). If it is over 1 MiB it is first renamed to `logs/gui.log.1`, replacing the previous one, so the log stays bounded. The first line of each run records the time, product version, build number and source revision.
2. **Standard error into the log.** The GUI points its standard error handle at that file, so the standard library's own failure messages (allocation failure, panics, anything written with `eprintln!`) are kept.
3. **Panic detail.** A panic hook writes the time, thread name, message, source location and a captured backtrace to the log, then the default handling continues (the process still ends as before; nothing is silently swallowed).
4. **Startup failure.** If `eframe::run_native` returns an error, it is written to the log as well.
5. **Failure of the log itself.** If the log cannot be opened (read-only home, no home yet), the GUI starts normally without it.

Graceful degradation under memory exhaustion is out of scope: an allocation failure cannot be recovered in safe Rust without the unstable hook, and the coordinator (a separate process) already keeps research data safe. The log makes the next failure diagnosable.

## Acceptance

After a forced panic in a debug build, `logs/gui.log` contains the run header, the panic message with its location and a backtrace. A log over 1 MiB is rotated to `gui.log.1` at the next start. A home that cannot hold the log does not prevent startup.

## Tests

Unit tests for rotation and header formatting in a temporary directory. A debug-only environment variable (`SCOPEWORK_GUI_PANIC_TEST`) makes the GUI panic right after setting up the log, and an integration test runs the GUI binary with it and checks the log contents.

## Future work

The CLI and coordinator already return errors as JSON; a matching file log for the coordinator could follow if a detached coordinator ever fails without a trace.
