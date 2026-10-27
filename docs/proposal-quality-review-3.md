# Proposal: app-wide code-quality review, round 3

Status: implemented 2026-10-27. Round 3 reads the modules rounds 1 and 2 had not:
- the coordinator (client, pipe, leases, manifest);
- the GUI's drawing helpers;
- the annotation ledger;
- the home-level settings and agents.

It also runs a duplicate-block search over all of `src` (six or more matching lines, test modules excluded). Few blocks repeat now; the findings below are what remains, plus two items found by reading.

## Findings and fixes

1. **The ledger's retry loop is written twice.**
   - `ledger::apply` and `ledger::repin` both stamp the ledger, read it, transform it, and write it back only if no tool changed it meanwhile, up to five tries.
   - Only the transform differs.
   - One `rewrite` helper takes the transform.
2. **The home's locked file update is written twice.** `settings::update` and `agents::register` each create the home, open and lock their lock file, read, change and atomically replace their file. One `paths::locked` helper takes the lock and runs the change.
3. **The aged priority is computed twice.** Queued operations and waiting lease requests both rank by class minus one step per five seconds waited. `Class::aged` computes it.
4. **Priority classes are parsed by hand twice.** The lease request and the request handler each match the class strings, beside `Class::name` and the serde names. `Class::parse` reads them.
5. **The dashboard's bevels draw the same four edges twice.** `bevel` and `double_bevel` each draw the light top-left and dark bottom-right edges. One `edges` helper draws them.

Noted, not changed: `pipe::connect` reports a pipe that stays busy as "no coordinator", so `coordinator stop` can answer that nothing runs. That is a bug, recorded for the bug review.

## Acceptance

- No behaviour change.
- The full suite, Clippy and fmt are clean.
