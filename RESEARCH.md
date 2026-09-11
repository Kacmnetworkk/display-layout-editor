# Research notes

## Local target
- Omarchy: `4.0.3-1`
- Hyprland: `0.56.2`
- Active user configuration: `~/.config/hypr/monitors.lua`
- Packaged default (read-only): `/usr/share/omarchy/config/hypr/monitors.lua`
- The user entrypoint `~/.config/hypr/hyprland.lua` loads Omarchy defaults first and then `require("hypr.monitors")`.

## Current monitor API
Hyprland 0.56 uses Lua-native monitor rules:

```lua
hl.monitor({
  output = "HDMI-A-1",
  mode = "3840x2160@60.00",
  position = "1536x0",
  scale = 1.25,
  transform = 0,
})
```

Runtime preview should use the same API through `hyprctl eval`, not the legacy comma-separated `hyprctl keyword monitor` form:

```bash
hyprctl eval 'hl.monitor({ output = "HDMI-A-1", mode = "3840x2160@60.00", position = "1536x0", scale = 1.25, transform = 0 })'
```

Connected outputs and choices come from `hyprctl monitors all -j`. Logical layout sizes are resolution divided by scale; transforms 1, 3, 5, and 7 swap logical width and height. Outputs must not overlap. Hyprland has no general “primary monitor” configuration field, so the editor labels its `0x0` anchor behavior honestly.

## Omarchy-safe persistence
1. Read the current file and live monitor state.
2. Arm an independent watchdog, then preview the complete layout in one `hyprctl eval` call.
3. Require confirmation while a 15-second timer is active; the watchdog restores the captured runtime layout after a crash or missing confirmation.
4. Back up and atomically replace only `~/.config/hypr/monitors.lua`.
5. Run `hyprctl reload`, require empty `hyprctl configerrors`, and verify the realized monitor state.
6. Restore both the backup and runtime layout if persistence validation fails.
7. Never modify `/usr/share/omarchy/`.

Omarchy’s explicit default restore command is `omarchy refresh config hypr/monitors.lua`; it backs up the user version. The app intentionally keeps that separate from its non-destructive “Reset editor” control.

## Official sources
- https://wiki.hypr.land/configuring/core/monitors/
- https://wiki.hypr.land/configuring/core/monitors/positioning/
- https://wiki.hypr.land/configuring/core/monitors/modes/
- https://wiki.hypr.land/configuring/core/monitors/output-selection/
- https://wiki.hypr.land/configuring/core/advanced-configuration/using-hyprctl/
- https://github.com/basecamp/omarchy/tree/v4.0.3/config/hypr

## Design decision
The product uses an **Operate** surface with a secondary **Configure** inspector: a large proportional arrangement canvas, a compact right-side settings rail, and a bottom safety action bar. It uses precise near-black surfaces, thin cool borders, and a restrained amber interactive accent. The layout is original and Omarchy-friendly rather than a Windows clone.
