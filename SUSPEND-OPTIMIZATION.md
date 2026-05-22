# This branch implements a sloptimization for Wayland suspend event

Idea is to delete WGPU resources on suspend (wayland term for minimize and hiding), and during resume recreate them.

Branch kind of works:

1. Open egui_example.rs
2. maximize a window.
3. Observe that GPU memory usage goes to 300 MB (on 4k screen)
4. Minimize the window, GPU memory usage drops to 163MB

I need to think this a bit further after all it is sloptimization!
