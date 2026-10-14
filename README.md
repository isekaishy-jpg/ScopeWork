# Research Dashboard

A compact Rust desktop dashboard for binary research, with a separate tab for Stock and Forever.

Each project has one compact page of classic Task Manager-style horizontal meters. Meters wrap into a grid as the window resizes; overflow scrolls when more research items are added. Each shows total functions, done functions, and the current in-progress chunk name and function count. Green means done, red means in progress, and grey means not done. Hover or click a chunk to inspect it. The bottom status strip shows project totals.

All current categories, counts, and statuses are labeled sample data. The research inventory, deliverables, and acceptance rules will be defined later. Existing research claims have not been imported.

## Run

```powershell
cargo run --release
```

Windows executable: `target/release/research-dashboard.exe`.

## Research boundaries

Keep Stock and Forever evidence and acceptance records separate. Pin real inventories to exact binary identities. Groups can reference shared functions, but totals must count unique functions. Completion must eventually be supported by agreed deliverables and evidence, not function counts alone. Preserve an unclassified category until binary evidence supports classification.
