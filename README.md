# Wayapp

No winit was used during creation of this thing.

This repository aims to not use cross-platform libraries for handling windows, instead it uses just wayland APIs via Smithay's libraries. If you target just Linux then adding cross-platform overhead is not necessary.

## EGUI

Currently uses only EGUI WGPU rendering.

## ICED

I don't know will I ever get to ICED integration, but it is planned.

## Change log

- 2026-07-03: EGUI WGPU defaults to transparent clear pass, EGUI then decides the background color.

## Development notes

- Remember to run `cargo upgrade` for updating dependencies before `cargo publish`.

## IME Panel

To register an application as virtual keyboard, apparently:

```
$ cat /usr/share/applications/com.github.maliit.keyboard.desktop
[Desktop Entry]
Name=Maliit
Exec=maliit-keyboard
Type=Application
X-KDE-Wayland-VirtualKeyboard=true
Icon=input-keyboard-virtual
NoDisplay=true
```

Then it probably appears in the KDE settings?

If one changes it from KDE settings KWIN config file changes:

```
$ cat ~/.config/kwinrc
[Wayland]
InputMethod[$e]=/usr/share/applications/com.github.maliit.keyboard.desktop
VirtualKeyboardEnabled=true
```
