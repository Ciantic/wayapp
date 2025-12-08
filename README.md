# EGUI + WGPU + Smithay Client Toolkit

No winit was used during creation of this thing.

## Notes

Maybe change `Application` to hold only `Weak` references to the `WindowContainer`/`LayerSurfaceContainer`/`PopupContainer`/`SubsurfaceContainer`, because it's not the responsibility of the `Application` to keep those alive, it's the responsibility of the main.


