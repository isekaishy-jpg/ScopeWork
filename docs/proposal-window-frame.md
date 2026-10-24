# Proposal: a window frame that matches its fill

Status: implemented 2026-10-24. Research: [research-write-queue-window.md](research-write-queue-window.md).

## Problem

The lower part of the Write queue window is see-through. The classic double bevel is drawn around the window's reserved size (`max_rect`, its 520×300 `default_size`). The fill, egui's window frame, covers only the content, because egui sizes a window that is not resizable to its content. Any non-modal window that is not resizable would show the same gap; the Write queue window is the only one today.

## Design

`classic::Window::show` draws the bevel around what the fill covers:
- **Resizable windows:** the reserved size united with the content, as before. Their frame fills the whole reserved size.
- **Other windows:** the content rectangle (`min_rect`). The window's title bar spans the reserved width, so the width is unchanged; the height follows the content.

The alternative, painting face colour over the reserved size, was rejected. The window would look taller than it is, and clicks in the painted band would still go to the window behind it.

## Validation

`examples/capture_panels.rs` gains `queue-light` and `queue-dark` shots, which open the Write queue over the other windows. Against a fixture home, the shots before the change showed the gap: the face ended below "Waiting" and the window behind showed through to the border. After the change the bevel ends with the face. Every other shot (records tabs, dialogs, history) is pixel-identical before and after.

No unit test: the defect is in painting, and the capture is the check.
