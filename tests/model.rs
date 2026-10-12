use displayeditor_rav::{
    highest_mode_index, normalize_positions, parse_ddc_brightness, parse_ddc_buses,
    parse_hyprland_monitors, render_omarchy_lua, runtime_layout_lua, runtime_monitor_value,
    save_config_with_backup, snap_position_to_displays, validate_layout,
};
use std::fs;

const TWO_MONITORS: &str = r#"[
  {"id":0,"name":"eDP-1","description":"Laptop panel","width":1920,"height":1200,
   "refreshRate":60.003,"x":0,"y":0,"scale":1.25,"transform":0,"focused":false,
   "availableModes":["1920x1200@60.00Hz","1920x1080@60.00Hz"]},
  {"id":1,"name":"HDMI-A-1","description":"AOC U2790B","width":3840,"height":2160,
   "refreshRate":59.997,"x":1536,"y":0,"scale":1.25,"transform":0,"focused":true,
   "availableModes":["3840x2160@60.00Hz","2560x1440@59.95Hz","1920x1080@60.00Hz"]}
]"#;

#[test]
fn maps_ddc_buses_to_exact_drm_connector() {
    let detected = "Display 1\n   I2C bus: /dev/i2c-2\n   DRM connector: card1-DP-1\n   Monitor: AOC:U2790B:\n\nDisplay 2\n   I2C bus: /dev/i2c-6\n   DRM connector: card1-HDMI-A-1\n";
    let buses = parse_ddc_buses(detected);
    assert_eq!(buses.get("DP-1"), Some(&2));
    assert_eq!(buses.get("HDMI-A-1"), Some(&6));
    assert_eq!(buses.get("DP-10"), None);
}

#[test]
fn parses_ddc_current_and_max_brightness() {
    assert_eq!(parse_ddc_brightness("VCP 10 C 40 100\n"), Some((40, 100)));
    assert_eq!(parse_ddc_brightness("VCP 10 C 128 255\n"), Some((128, 255)));
    assert_eq!(parse_ddc_brightness("VCP 10 C 0 0\n"), None);
    assert_eq!(parse_ddc_brightness("VCP 12 C 40 100\n"), None);
    assert_eq!(parse_ddc_brightness("DDC communication failed"), None);
}

#[test]
fn brightness_percent_maps_to_supported_monitor_range() {
    assert_eq!(displayeditor_rav::brightness_raw_value(25, 100), Some(25));
    assert_eq!(displayeditor_rav::brightness_raw_value(50, 255), Some(128));
    assert_eq!(displayeditor_rav::brightness_raw_value(1, 100), Some(1));
    assert_eq!(displayeditor_rav::brightness_raw_value(0, 100), None);
    assert_eq!(displayeditor_rav::brightness_raw_value(50, 0), None);
}

#[test]
fn parses_live_hyprland_monitor_json() {
    let displays = parse_hyprland_monitors(TWO_MONITORS).unwrap();
    assert_eq!(displays.len(), 2);
    assert_eq!(displays[1].name, "HDMI-A-1");
    assert_eq!(displays[1].position, [1536, 0]);
    assert_eq!(displays[1].available_modes[0].width, 3840);
    assert_eq!(displays[1].available_modes[0].refresh_hz, 60.0);
}

#[test]
fn chooses_highest_resolution_then_refresh_rate() {
    let displays = parse_hyprland_monitors(TWO_MONITORS).unwrap();
    assert_eq!(highest_mode_index(&displays[1]), Some(0));
}

#[test]
fn normalizes_negative_positions_without_changing_relationships() {
    let mut displays = parse_hyprland_monitors(TWO_MONITORS).unwrap();
    displays[0].position = [-1920, -200];
    displays[1].position = [0, 100];
    normalize_positions(&mut displays);
    assert_eq!(displays[0].position, [0, 0]);
    assert_eq!(displays[1].position, [1920, 300]);
}

#[test]
fn generated_lua_uses_omarchy_monitor_helpers_and_fallback() {
    let displays = parse_hyprland_monitors(TWO_MONITORS).unwrap();
    let lua = render_omarchy_lua(&displays, 1);
    assert!(lua.contains("hl.env(\"GDK_SCALE\", \"1\")"));
    assert!(lua.contains("output = \"HDMI-A-1\""));
    assert!(lua.contains("mode = \"3840x2160@60.00\""));
    assert!(lua.contains("position = \"1536x0\""));
    assert!(lua.contains("output = \"\", mode = \"preferred\", position = \"auto\""));
    assert!(lua.find("output = \"\"").unwrap() < lua.find("output = \"HDMI-A-1\"").unwrap());
}

#[test]
fn rejects_malformed_monitor_json() {
    assert!(parse_hyprland_monitors("not-json").is_err());
}

#[test]
fn renders_runtime_lua_monitor_call() {
    let displays = parse_hyprland_monitors(TWO_MONITORS).unwrap();
    assert_eq!(
        runtime_monitor_value(&displays[1]),
        "hl.monitor({ output = \"HDMI-A-1\", mode = \"3840x2160@60.00\", position = \"1536x0\", scale = 1.25, transform = 0, disabled = false })"
    );
}

#[test]
fn persistent_save_is_atomic_and_backs_up_existing_config() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("monitors.lua");
    fs::write(&path, "old config").unwrap();

    let backup = save_config_with_backup(&path, "new config").unwrap();

    assert_eq!(fs::read_to_string(&path).unwrap(), "new config");
    let backup = backup.expect("an existing file must be backed up");
    assert_eq!(fs::read_to_string(backup).unwrap(), "old config");
}

#[test]
fn snaps_nearby_display_edges_and_tops() {
    let displays = parse_hyprland_monitors(TWO_MONITORS).unwrap();
    let snapped = snap_position_to_displays(&displays, 1, [1542, 6], 12);
    assert_eq!(snapped, [1536, 0]);
}

#[test]
fn renders_complete_runtime_layout_as_one_lua_evaluation() {
    let displays = parse_hyprland_monitors(TWO_MONITORS).unwrap();
    let lua = runtime_layout_lua(&displays);
    assert!(lua.starts_with("local layout = {"));
    assert!(lua.contains("output = \"eDP-1\""));
    assert!(lua.contains("output = \"HDMI-A-1\""));
    assert!(lua.contains("disabled = false"));
    assert!(lua.ends_with("for _, spec in ipairs(layout) do hl.monitor(spec) end"));
}

#[test]
fn rejects_overlapping_displays_before_preview() {
    let mut displays = parse_hyprland_monitors(TWO_MONITORS).unwrap();
    displays[1].position = [100, 100];
    let errors = validate_layout(&displays);
    assert!(errors.iter().any(|error| error.contains("overlap")));
}

#[test]
fn rejects_scale_that_produces_fractional_logical_dimensions() {
    let mut displays = parse_hyprland_monitors(TWO_MONITORS).unwrap();
    displays[0].scale = 1.4;
    let errors = validate_layout(&displays);
    assert!(
        errors
            .iter()
            .any(|error| error.contains("integral logical size"))
    );
}
