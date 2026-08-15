# Display Layout Editor

A small native GTK4 application for configuring Hyprland monitor layouts on Linux. It is designed for Omarchy/Hyprland desktops and stores persistent display rules in `~/.config/hypr/monitors.lua`.

## Features

- Detects active displays using `hyprctl monitors -j`
- Edits each display's mode, logical X/Y position, scale, and orientation
- Includes an **HDMI left portrait / DP right landscape** preset
- Backs up `~/.config/hypr/monitors.lua` before applying changes
- Writes a clearly marked managed section, reloads Hyprland, and reports configuration errors
- Installs a desktop-menu entry named **Display Layout Editor**

## Requirements

- Hyprland with `hyprctl` available
- GTK 4 development libraries
- Rust 2024 edition toolchain

## Build and run

```bash
cargo run --release
```

To install the executable locally and create the desktop entry:

```bash
cargo build --release
install -m 755 target/release/display-layout-editor ~/.local/bin/display-layout-editor
~/.local/bin/display-layout-editor --install-launcher
```

Then launch **Display Layout Editor** from the application menu, or run:

```bash
display-layout-editor
```

## Command-line preset

Apply the HDMI-left/portrait and DP-1-right/landscape layout without opening the UI:

```bash
display-layout-editor --apply-hdmi-left-dp-right
```

The preset uses the actual output names reported by Hyprland. It selects the first connected HDMI output and requires `DP-1` to be connected.

## Notes

Positions are logical pixels after applying each monitor's scale factor. The editor appends rules between these markers in `~/.config/hypr/monitors.lua`:

```lua
-- BEGIN display-layout-editor
-- END display-layout-editor
```

Future edits replace only that managed section and preserve the rest of the configuration.

## License

Licensed under the [MIT License](LICENSE).
