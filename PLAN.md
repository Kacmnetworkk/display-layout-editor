# DisplayeditorRav implementation plan

## Product goal
Build an original Omarchy-native desktop utility that makes Hyprland monitor configuration visual and safe. The main surface is **Operate**, with a secondary **Configure** inspector: the display canvas is dominant, while detailed options remain one click away.

## Safety contract
- Read live state from `hyprctl monitors all -j`; never infer connected outputs from a stale file.
- Start an out-of-process watchdog before previewing; it restores the captured runtime layout if the GUI crashes or never confirms.
- Preview the complete layout in one Hyprland Lua evaluation, then re-query live state.
- Persist only after explicit **Keep & save**.
- Back up `~/.config/hypr/monitors.lua` before every persistent write.
- Write atomically, reload, then validate with `hyprctl configerrors`.
- Never modify `/usr/share/omarchy/`.
- **Reset editor** means discard pending UI changes and re-read live state. Restoring Omarchy defaults is a separate destructive action and will not be automated in v0.1.

## Milestones
1. Domain model and tests: parse Hyprland JSON/modes, select highest mode, normalize/anchor layouts, generate Omarchy Lua.
2. Native Rust/egui shell with Omarchy-friendly dark tokens.
3. Drag-to-arrange canvas with edge snapping and monitor selection.
4. Inspector with display, resolution/refresh, scale, rotation, primary anchor, highest-resolution, and identify controls.
5. Runtime preview, countdown confirmation, rollback, atomic persistence, backups, and error reporting.
6. Test suite, formatter/lints, release build, smoke test, and desktop launcher.

## UX composition
- Top bar: app identity, live connection state, Refresh.
- Center: large gridded arrangement canvas; monitors are proportionally sized, numbered, draggable, selected with one accent outline.
- Right inspector: selected output metadata and compact dropdowns.
- Bottom action bar: Reset editor, Preview changes, status/error text.
- Preview state: persistent warning bar with countdown, Revert, and Keep & save.

## Visual direction
Near-black layered surfaces, cool neutral text, thin borders, and a restrained amber accent chosen to harmonize with Omarchy rather than copy a branded app. Six-pixel control radii, 8px spacing rhythm, readable 13–16px UI type, no gradients/glass/equal-card filler.

## Non-goals for v0.1
- Mirroring, HDR/color profiles, VRR, and disconnected-output provisioning.
- Editing packaged Omarchy defaults.
- Claiming a Hyprland “primary” flag that does not exist; **Make primary** means anchoring that output at logical `0x0` and treating it as the app’s reference display.
