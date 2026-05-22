# Changelog

## 0.3.0

### Breaking changes: egui & wgpu API updates

This release upgrades **egui** to 0.34 and **wgpu** to 29. You'll need to update your app code when upgrading `wayapp`.

#### `handle_events` closure signature changed

The `ui` closure passed to `handle_events` now receives `&mut egui::Ui` instead of `&egui::Context`.

```diff
- example_window_app.handle_events(&mut app, &events, &mut |ctx| myapp.ui(ctx));
+ example_window_app.handle_events(&mut app, &events, &mut |ui| myapp.ui(ui));
```

Update your `fn ui()` method signature accordingly:

```diff
- fn ui(&mut self, ctx: &egui::Context) {
+ fn ui(&mut self, ui: &mut egui::Ui) {
```

If you were calling methods on `ctx` directly (e.g. `ctx.set_visuals(...)`, `ctx.cumulative_pass_nr()`), use `ui.ctx()` instead:

```diff
- ctx.set_visuals(egui::Visuals::dark());
+ ui.ctx().set_visuals(egui::Visuals::dark());
```

However `set_visuals()` is also available on `ui.set_visuals(...)`.

#### `CentralPanel::show` is deprecated

Use `show_inside` instead:

```diff
- CentralPanel::default().show(ctx, |ui| { ... });
+ CentralPanel::default().show_inside(ui, |ui| { ... });
```

#### Unused import

Remove `use egui::Context;` if you were importing it — it's no longer needed in your `ui()` method.
