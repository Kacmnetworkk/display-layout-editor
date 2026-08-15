use gtk::prelude::*;
use gtk::{
    Align, Application, ApplicationWindow, Box as GtkBox, Button, DropDown, Entry, Frame, Grid,
    Label, Orientation, PolicyType, ScrolledWindow, SpinButton,
};
use serde::Deserialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const START_MARKER: &str = "-- BEGIN display-layout-editor";
const END_MARKER: &str = "-- END display-layout-editor";

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HyprMonitor {
    name: String,
    description: String,
    width: i32,
    height: i32,
    refresh_rate: f64,
    x: i32,
    y: i32,
    scale: f64,
    transform: i32,
    disabled: bool,
}

#[derive(Clone)]
struct MonitorControls {
    monitor: HyprMonitor,
    mode: Entry,
    x: SpinButton,
    y: SpinButton,
    scale: SpinButton,
    transform: DropDown,
}

#[derive(Clone)]
struct LayoutRule {
    output: String,
    mode: String,
    x: i32,
    y: i32,
    scale: f64,
    transform: i32,
}

fn main() {
    if env::args().any(|argument| argument == "--apply-hdmi-left-dp-right") {
        match apply_requested_layout() {
            Ok(message) => println!("{message}"),
            Err(message) => {
                eprintln!("{message}");
                std::process::exit(1);
            }
        }
        return;
    }
    if env::args().any(|argument| argument == "--install-launcher") {
        match install_launcher() {
            Ok(message) => println!("{message}"),
            Err(message) => {
                eprintln!("{message}");
                std::process::exit(1);
            }
        }
        return;
    }

    let app = Application::builder()
        .application_id("org.local.displaylayouteditor")
        .build();
    app.connect_activate(build_ui);
    app.run();
}

fn install_launcher() -> Result<String, String> {
    let home = env::var("HOME").map_err(|_| "HOME is not set.".to_string())?;
    let target =
        PathBuf::from(home).join(".local/share/applications/display-layout-editor.desktop");
    let executable =
        env::current_exe().map_err(|error| format!("Could not locate this executable: {error}"))?;
    let parent = target
        .parent()
        .ok_or_else(|| "Could not determine the applications directory.".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
    fs::write(
        &target,
        format!(
            "[Desktop Entry]\nType=Application\nName=Display Layout Editor\nComment=Edit Hyprland monitor positions, scale, and orientation\nExec={}\nTerminal=false\nCategories=Settings;HardwareSettings;\n",
            executable.display()
        ),
    )
    .map_err(|error| format!("Could not write {}: {error}", target.display()))?;
    Ok(format!("Installed launcher at {}.", target.display()))
}

fn build_ui(app: &Application) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Display Layout Editor")
        .default_width(760)
        .default_height(680)
        .build();

    let root = GtkBox::new(Orientation::Vertical, 12);
    root.set_margin_top(18);
    root.set_margin_bottom(18);
    root.set_margin_start(18);
    root.set_margin_end(18);

    let title = Label::new(Some("Display Layout Editor"));
    title.add_css_class("title-2");
    title.set_halign(Align::Start);
    root.append(&title);

    let help = Label::new(Some(
        "Edit each connected display in logical pixels. Apply saves a managed section in ~/.config/hypr/monitors.lua, backs up the previous file, reloads Hyprland, and reports config errors.",
    ));
    help.set_wrap(true);
    help.set_halign(Align::Start);
    help.add_css_class("dim-label");
    root.append(&help);

    let status = Label::new(None);
    status.set_wrap(true);
    status.set_halign(Align::Start);
    status.add_css_class("dim-label");

    let content = GtkBox::new(Orientation::Vertical, 12);
    let monitors = read_monitors();
    let controls = match monitors {
        Ok(monitors) if !monitors.is_empty() => monitors
            .into_iter()
            .map(|monitor| {
                let (card, control) = display_card(monitor);
                content.append(&card);
                control
            })
            .collect::<Vec<_>>(),
        Ok(_) => {
            status.set_text("No active monitors were reported by Hyprland.");
            Vec::new()
        }
        Err(error) => {
            status.set_text(&format!("Could not read displays: {error}"));
            Vec::new()
        }
    };

    let scroll = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vexpand(true)
        .child(&content)
        .build();
    root.append(&scroll);

    let actions = GtkBox::new(Orientation::Horizontal, 8);
    let preset = Button::with_label("HDMI left portrait / DP right landscape");
    let apply = Button::with_label("Apply layout");
    apply.add_css_class("suggested-action");
    actions.append(&preset);
    actions.append(&apply);
    root.append(&actions);
    root.append(&status);

    let preset_controls = controls.clone();
    let preset_status = status.clone();
    preset.connect_clicked(move |_| {
        match arrange_requested_layout(&preset_controls) {
            Ok(()) => preset_status.set_text(
                "Preset staged: HDMI is left and portrait; DP-1 is right and landscape. Click Apply layout to save it.",
            ),
            Err(error) => preset_status.set_text(&error),
        }
    });

    let apply_controls = controls.clone();
    let apply_status = status.clone();
    apply.connect_clicked(move |_| match rules_from_controls(&apply_controls) {
        Ok(rules) => match save_and_reload(&rules) {
            Ok(message) => apply_status.set_text(&message),
            Err(error) => apply_status.set_text(&format!("Layout was not fully applied: {error}")),
        },
        Err(error) => apply_status.set_text(&error),
    });

    window.set_child(Some(&root));
    window.present();
}

fn display_card(monitor: HyprMonitor) -> (Frame, MonitorControls) {
    let frame = Frame::builder()
        .label(format!("{} — {}", monitor.name, monitor.description))
        .build();
    let grid = Grid::builder()
        .column_spacing(12)
        .row_spacing(8)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();

    let mode = Entry::new();
    mode.set_text(&format!(
        "{}x{}@{:.3}",
        monitor.width, monitor.height, monitor.refresh_rate
    ));
    let x = SpinButton::with_range(-20_000.0, 20_000.0, 1.0);
    x.set_value(f64::from(monitor.x));
    let y = SpinButton::with_range(-20_000.0, 20_000.0, 1.0);
    y.set_value(f64::from(monitor.y));
    let scale = SpinButton::with_range(0.25, 4.0, 0.25);
    scale.set_value(monitor.scale);
    scale.set_digits(2);
    let transform = DropDown::from_strings(&[
        "Landscape (0°)",
        "Portrait clockwise (90°)",
        "Upside down (180°)",
        "Portrait counter-clockwise (270°)",
    ]);
    transform.set_selected(transform_index(monitor.transform));

    attach_row(&grid, 0, "Mode", &mode);
    attach_row(&grid, 1, "X position", &x);
    attach_row(&grid, 2, "Y position", &y);
    attach_row(&grid, 3, "Scale", &scale);
    attach_row(&grid, 4, "Orientation", &transform);

    frame.set_child(Some(&grid));
    (
        frame,
        MonitorControls {
            monitor,
            mode,
            x,
            y,
            scale,
            transform,
        },
    )
}

fn attach_row<W: IsA<gtk::Widget>>(grid: &Grid, row: i32, label: &str, widget: &W) {
    let label = Label::new(Some(label));
    label.set_halign(Align::End);
    grid.attach(&label, 0, row, 1, 1);
    grid.attach(widget, 1, row, 1, 1);
}

fn transform_index(transform: i32) -> u32 {
    match transform {
        1 => 1,
        2 => 2,
        3 => 3,
        _ => 0,
    }
}

fn selected_transform(control: &MonitorControls) -> i32 {
    match control.transform.selected() {
        1 => 1,
        2 => 2,
        3 => 3,
        _ => 0,
    }
}

fn arrange_requested_layout(controls: &[MonitorControls]) -> Result<(), String> {
    let hdmi = controls
        .iter()
        .find(|control| control.monitor.name.starts_with("HDMI"))
        .ok_or_else(|| "No HDMI output is currently connected.".to_string())?;
    let dp = controls
        .iter()
        .find(|control| control.monitor.name == "DP-1")
        .ok_or_else(|| "DP-1 is not currently connected.".to_string())?;

    hdmi.transform.set_selected(1);
    hdmi.x.set_value(0.0);
    hdmi.y.set_value(0.0);
    dp.transform.set_selected(0);
    dp.x.set_value(logical_width(hdmi));
    dp.y.set_value(0.0);
    Ok(())
}

fn logical_width(control: &MonitorControls) -> f64 {
    let (width, height) =
        parse_mode(&control.mode.text()).unwrap_or((control.monitor.width, control.monitor.height));
    let transformed_width = match selected_transform(control) {
        1 | 3 => height,
        _ => width,
    };
    f64::from(transformed_width) / control.scale.value()
}

fn parse_mode(mode: &str) -> Option<(i32, i32)> {
    let dimensions = mode.split('@').next()?;
    let (width, height) = dimensions.split_once('x')?;
    Some((width.trim().parse().ok()?, height.trim().parse().ok()?))
}

fn rules_from_controls(controls: &[MonitorControls]) -> Result<Vec<LayoutRule>, String> {
    if controls.is_empty() {
        return Err("There are no active displays to save.".to_string());
    }

    controls
        .iter()
        .map(|control| {
            let mode = control.mode.text().trim().to_string();
            if !is_safe_mode(&mode) {
                return Err(format!(
                    "{} has an invalid mode. Use preferred or WIDTHxHEIGHT@REFRESH.",
                    control.monitor.name
                ));
            }
            Ok(LayoutRule {
                output: control.monitor.name.clone(),
                mode,
                x: control.x.value_as_int(),
                y: control.y.value_as_int(),
                scale: control.scale.value(),
                transform: selected_transform(control),
            })
        })
        .collect()
}

fn is_safe_mode(mode: &str) -> bool {
    mode == "preferred"
        || (!mode.is_empty()
            && mode.chars().all(|character| {
                character.is_ascii_digit() || matches!(character, 'x' | '@' | '.')
            }))
}

fn apply_requested_layout() -> Result<String, String> {
    let monitors = read_monitors()?;
    let hdmi = monitors
        .iter()
        .find(|monitor| monitor.name.starts_with("HDMI"))
        .ok_or_else(|| "No HDMI output is currently connected.".to_string())?;
    let dp = monitors
        .iter()
        .find(|monitor| monitor.name == "DP-1")
        .ok_or_else(|| "DP-1 is not currently connected.".to_string())?;

    let hdmi_rule = LayoutRule {
        output: hdmi.name.clone(),
        mode: "preferred".to_string(),
        x: 0,
        y: 0,
        scale: hdmi.scale,
        transform: 1,
    };
    let dp_rule = LayoutRule {
        output: dp.name.clone(),
        mode: "preferred".to_string(),
        x: (f64::from(hdmi.height) / hdmi.scale).round() as i32,
        y: 0,
        scale: dp.scale,
        transform: 0,
    };
    save_and_reload(&[hdmi_rule, dp_rule])
}

fn read_monitors() -> Result<Vec<HyprMonitor>, String> {
    let output = Command::new("hyprctl")
        .args(["monitors", "-j"])
        .output()
        .map_err(|error| format!("Could not run hyprctl: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let monitors: Vec<HyprMonitor> = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("Could not parse Hyprland monitor data: {error}"))?;
    Ok(monitors
        .into_iter()
        .filter(|monitor| !monitor.disabled)
        .collect())
}

fn save_and_reload(rules: &[LayoutRule]) -> Result<String, String> {
    let config = config_path()?;
    let original = fs::read_to_string(&config)
        .map_err(|error| format!("Could not read {}: {error}", config.display()))?;
    let backup = backup_path(&config)?;
    fs::copy(&config, &backup)
        .map_err(|error| format!("Could not create backup {}: {error}", backup.display()))?;

    let content = format!(
        "{}{}",
        remove_managed_section(&original),
        managed_section(rules)
    );
    fs::write(&config, content)
        .map_err(|error| format!("Could not save {}: {error}", config.display()))?;

    run_hyprctl(&["reload"])?;
    let errors = run_hyprctl(&["configerrors"])?;
    if errors.trim().is_empty() || errors.to_ascii_lowercase().contains("no errors") {
        Ok(format!(
            "Applied the layout successfully. Backup saved at {}.",
            backup.display()
        ))
    } else {
        Err(format!(
            "Hyprland reloaded, but reported configuration errors: {}. Backup: {}",
            errors.trim(),
            backup.display()
        ))
    }
}

fn config_path() -> Result<PathBuf, String> {
    let home = env::var("HOME").map_err(|_| "HOME is not set.".to_string())?;
    Ok(PathBuf::from(home).join(".config/hypr/monitors.lua"))
}

fn backup_path(config: &Path) -> Result<PathBuf, String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("Could not create backup timestamp: {error}"))?
        .as_secs();
    Ok(PathBuf::from(format!(
        "{}.bak.{timestamp}",
        config.display()
    )))
}

fn remove_managed_section(original: &str) -> String {
    let Some(start) = original.find(START_MARKER) else {
        return original.trim_end().to_string();
    };
    let after_start = &original[start..];
    let Some(end_relative) = after_start.find(END_MARKER) else {
        return original[..start].trim_end().to_string();
    };
    let end = start + end_relative + END_MARKER.len();
    let mut result = original.to_string();
    result.replace_range(start..end, "");
    result.trim_end().to_string()
}

fn managed_section(rules: &[LayoutRule]) -> String {
    let entries = rules
        .iter()
        .map(|rule| {
            format!(
                "hl.monitor({{ output = \"{}\", mode = \"{}\", position = \"{}x{}\", scale = {}, transform = {} }})",
                rule.output, rule.mode, rule.x, rule.y, rule.scale, rule.transform
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "\n\n{START_MARKER}\n-- Managed by Display Layout Editor. Positions use logical pixels.\n{entries}\n{END_MARKER}\n"
    )
}

fn run_hyprctl(arguments: &[&str]) -> Result<String, String> {
    let output = Command::new("hyprctl")
        .args(arguments)
        .output()
        .map_err(|error| format!("Could not run hyprctl: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}
