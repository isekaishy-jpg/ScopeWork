# Research Dashboard

A compact Rust desktop dashboard for binary research, with a separate tab for Stock and Forever.

The entire application uses classic Task Manager styling: custom blue title bar and window buttons, grey beveled window frame, File/Options/View/Help menus, research tabs, recessed panels, small Tahoma text, and a three-part status strip. Drag the title bar to move the window; double-click it or use the maximize button to maximize. The lower-right grip resizes it.

Each project page has horizontal segmented meters, a total-function coverage meter, and function/chunk count fields. Green means done, red means in progress, and grey means not done. Click a chunk meter to inspect its architecture, name, function count, and state. The initial current chunk is the first in-progress sample chunk. The sample page holds 24 meters in a 740 x 520 window.

Menus provide Exit, Always on top, Clear chunk selection, and About.

All current categories, counts, and statuses are labeled sample data. The research inventory, deliverables, and acceptance rules will be defined later. Existing research claims have not been imported.

## Run

```powershell
cargo run --release
```

Windows executable: `target/release/research-dashboard.exe`.

## Research boundaries

Keep Stock and Forever evidence and acceptance records separate. Pin real inventories to exact binary identities. Groups can reference shared functions, but totals must count unique functions. Completion must eventually be supported by agreed deliverables and evidence, not function counts alone. Preserve an unclassified category until binary evidence supports classification.
