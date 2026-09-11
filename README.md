# DisplayeditorRav

A visual, safe monitor arrangement editor for Omarchy/Hyprland, written in Rust with egui.

Documentation is deployed at: https://kacm1network.org

See `PLAN.md` for the product and safety plan.

## Features

- Reads live monitor state from `hyprctl monitors all -j`.
- Drag displays on a proportional canvas with edge snapping.
- Configure resolution, refresh rate, scale, orientation, and logical position.
- Identify displays with a temporary large-number overlay.
- Preview the complete layout through one Lua-native `hyprctl eval`.
- Protect previews with an independent rollback watchdog.
- Save only after **Keep & save**.
- Back up `~/.config/hypr/monitors.lua` before every persistent write.
- Restore the latest generated backup from the app.
- First-run checks for Hyprland tools and config access.
- About panel with version, license, and docs link.

## Usage

1. Run the app.
2. Drag monitor rectangles to match physical placement.
3. Select a monitor in the inspector to change resolution, scale, or orientation.
4. Use **Preview changes**.
5. If the layout works, click **Keep & save** before the countdown expires.
6. If anything looks wrong, click **Revert now** or wait for automatic rollback.

The app writes only `~/.config/hypr/monitors.lua` and never modifies `/usr/share/omarchy/`.

## Install in the Omarchy app selector

Build a release binary, then install the desktop entry and icon:

```bash
cargo build --release
cargo run --release -- --install-desktop
```

Smoke test the release binary:

```bash
target/release/displayeditor-rav --check
```

If the app selector does not refresh immediately:

```bash
omarchy restart shell
```

## Development

```bash
cargo test
cargo run
```

## License

MIT. See `LICENSE`.
