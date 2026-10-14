# Research Dashboard

A compact Rust desktop dashboard for binary research, with a separate tab for Stock and Forever.

The entire application uses classic Task Manager styling: custom blue title bar and window buttons, grey beveled window frame, File/Options/View/Help menus, research tabs, recessed panels, small Tahoma text, and a three-part status strip. Drag the title bar to move the window; double-click it or use the maximize button to maximize. The lower-right grip resizes it.

Each project page has horizontal segmented meters, a total-function coverage meter, and function/chunk count fields. The CPU Usage meter in the supplied reference is rotated sideways: thin segments illuminate from left to right, with dim unlit segments and a black seam between two segment banks. Fill is completed functions divided by total functions. Done groups are fully green, active groups are partially red, and untouched groups have only dim grey segments. Sample completed-function counts are explicit; aggregate counts, meter fill, tooltips, and the selected chunk readout use those same values. Click a chunk meter to inspect its architecture, name, function count, and state. The initial current chunk is the first in-progress sample chunk. The sample page holds 24 meters in a 1100 x 820 window: four named chunks per architecture row. Each 32-pixel-high meter has its architecture/chunk header above it and completed/total function counts plus status below it.

Dark mode is the default: charcoal surfaces, soft light text, subdued borders, and the same colored meter segments. Options > Dark mode switches between modern dark and classic light palettes; the choice is saved locally on normal exit. Menus also provide Exit, Always on top, Clear chunk selection, and About.

All current categories, counts, and statuses are labeled sample data. The research inventory, deliverables, and acceptance rules will be defined later. Existing research claims have not been imported.

## Run

```powershell
cargo run --release
```

Windows executable: `target/release/research-dashboard.exe`.

## Research boundaries

Keep Stock and Forever evidence and acceptance records separate. Pin real inventories to exact binary identities. Groups can reference shared functions, but totals must count unique functions. Completion must eventually be supported by agreed deliverables and evidence, not function counts alone. Preserve an unclassified category until binary evidence supports classification.
