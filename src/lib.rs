use serde::Deserialize;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq)]
pub struct DisplayMode {
    pub raw: String,
    pub width: u32,
    pub height: u32,
    pub refresh_hz: f32,
}

impl DisplayMode {
    pub fn config_value(&self) -> String {
        format!("{}x{}@{:.2}", self.width, self.height, self.refresh_hz)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Display {
    pub id: i32,
    pub name: String,
    pub description: String,
    pub width: u32,
    pub height: u32,
    pub refresh_hz: f32,
    pub position: [i32; 2],
    pub scale: f32,
    pub transform: u8,
    pub focused: bool,
    pub available_modes: Vec<DisplayMode>,
    pub selected_mode: usize,
}

impl Display {
    pub fn selected_mode(&self) -> Option<&DisplayMode> {
        self.available_modes.get(self.selected_mode)
    }

    pub fn logical_size(&self) -> [f32; 2] {
        let mode = self.selected_mode();
        let (mut width, mut height) = mode
            .map(|m| (m.width as f32, m.height as f32))
            .unwrap_or((self.width as f32, self.height as f32));
        if matches!(self.transform, 1 | 3 | 5 | 7) {
            std::mem::swap(&mut width, &mut height);
        }
        [width / self.scale.max(0.25), height / self.scale.max(0.25)]
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawMonitor {
    id: i32,
    name: String,
    #[serde(default)]
    description: String,
    width: u32,
    height: u32,
    refresh_rate: f32,
    x: i32,
    y: i32,
    scale: f32,
    #[serde(default)]
    transform: u8,
    #[serde(default)]
    focused: bool,
    #[serde(default)]
    available_modes: Vec<String>,
}

pub fn parse_mode(raw: &str) -> Option<DisplayMode> {
    let cleaned = raw.strip_suffix("Hz").unwrap_or(raw);
    let (size, refresh) = cleaned.rsplit_once('@')?;
    let (width, height) = size.split_once('x')?;
    Some(DisplayMode {
        raw: raw.to_owned(),
        width: width.parse().ok()?,
        height: height.parse().ok()?,
        refresh_hz: refresh.parse().ok()?,
    })
}

pub fn parse_hyprland_monitors(json: &str) -> Result<Vec<Display>, String> {
    let raw: Vec<RawMonitor> = serde_json::from_str(json).map_err(|error| error.to_string())?;
    Ok(raw
        .into_iter()
        .map(|monitor| {
            let available_modes: Vec<_> = monitor
                .available_modes
                .iter()
                .filter_map(|mode| parse_mode(mode))
                .collect();
            let selected_mode = available_modes
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    mode_distance(a, monitor.width, monitor.height, monitor.refresh_rate).total_cmp(
                        &mode_distance(b, monitor.width, monitor.height, monitor.refresh_rate),
                    )
                })
                .map(|(index, _)| index)
                .unwrap_or(0);
            Display {
                id: monitor.id,
                name: monitor.name,
                description: monitor.description,
                width: monitor.width,
                height: monitor.height,
                refresh_hz: monitor.refresh_rate,
                position: [monitor.x, monitor.y],
                scale: monitor.scale,
                transform: monitor.transform,
                focused: monitor.focused,
                available_modes,
                selected_mode,
            }
        })
        .collect())
}

fn mode_distance(mode: &DisplayMode, width: u32, height: u32, refresh: f32) -> f32 {
    (mode.width.abs_diff(width) + mode.height.abs_diff(height)) as f32 * 1000.0
        + (mode.refresh_hz - refresh).abs()
}

pub fn highest_mode_index(display: &Display) -> Option<usize> {
    display
        .available_modes
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| {
            (a.width as u64 * a.height as u64)
                .cmp(&(b.width as u64 * b.height as u64))
                .then_with(|| a.refresh_hz.total_cmp(&b.refresh_hz))
        })
        .map(|(index, _)| index)
}

pub fn normalize_positions(displays: &mut [Display]) {
    let Some(min_x) = displays.iter().map(|display| display.position[0]).min() else {
        return;
    };
    let min_y = displays
        .iter()
        .map(|display| display.position[1])
        .min()
        .unwrap_or(0);
    for display in displays {
        display.position[0] -= min_x;
        display.position[1] -= min_y;
    }
}

pub fn anchor_display(displays: &mut [Display], index: usize) {
    let Some(anchor) = displays.get(index) else {
        return;
    };
    let [offset_x, offset_y] = anchor.position;
    for display in displays {
        display.position[0] -= offset_x;
        display.position[1] -= offset_y;
    }
}

pub fn snap_position_to_displays(
    displays: &[Display],
    moving_index: usize,
    proposed: [i32; 2],
    threshold: i32,
) -> [i32; 2] {
    let Some(moving) = displays.get(moving_index) else {
        return proposed;
    };
    let moving_size = moving.logical_size().map(|value| value.round() as i32);
    let mut x_candidates = Vec::new();
    let mut y_candidates = Vec::new();

    for (index, other) in displays.iter().enumerate() {
        if index == moving_index {
            continue;
        }
        let other_size = other.logical_size().map(|value| value.round() as i32);
        let left = other.position[0];
        let right = left + other_size[0];
        let top = other.position[1];
        let bottom = top + other_size[1];
        x_candidates.extend([left, right, left - moving_size[0], right - moving_size[0]]);
        y_candidates.extend([top, bottom, top - moving_size[1], bottom - moving_size[1]]);
    }

    [
        nearest_within(proposed[0], &x_candidates, threshold),
        nearest_within(proposed[1], &y_candidates, threshold),
    ]
}

fn nearest_within(value: i32, candidates: &[i32], threshold: i32) -> i32 {
    candidates
        .iter()
        .copied()
        .min_by_key(|candidate| candidate.abs_diff(value))
        .filter(|candidate| candidate.abs_diff(value) <= threshold as u32)
        .unwrap_or(value)
}

pub fn render_omarchy_lua(displays: &[Display], gdk_scale: u8) -> String {
    let mut output = String::from(
        "-- Generated by DisplayeditorRav. Manual edits may be replaced.\n\
         -- See https://wiki.hypr.land/Configuring/Basics/Monitors/\n\n",
    );
    output.push_str(&format!("hl.env(\"GDK_SCALE\", \"{}\")\n\n", gdk_scale));
    output.push_str("-- Safe fallback for newly connected outputs.\n");
    output.push_str(
        "hl.monitor({ output = \"\", mode = \"preferred\", position = \"auto\", scale = \"auto\" })\n\n",
    );
    for display in displays {
        let mode = display
            .selected_mode()
            .map(DisplayMode::config_value)
            .unwrap_or_else(|| {
                format!(
                    "{}x{}@{:.2}",
                    display.width, display.height, display.refresh_hz
                )
            });
        output.push_str(&format!(
            "hl.monitor({{ output = \"{}\", mode = \"{}\", position = \"{}x{}\", scale = {:.2}, transform = {} }})\n",
            display.name,
            mode,
            display.position[0],
            display.position[1],
            display.scale,
            display.transform
        ));
    }
    output
}

pub fn runtime_monitor_value(display: &Display) -> String {
    let mode = display
        .selected_mode()
        .map(DisplayMode::config_value)
        .unwrap_or_else(|| "preferred".to_owned());
    format!(
        "hl.monitor({{ output = {}, mode = {}, position = {}, scale = {:.2}, transform = {}, disabled = false }})",
        lua_string(&display.name),
        lua_string(&mode),
        lua_string(&format!("{}x{}", display.position[0], display.position[1])),
        display.scale,
        display.transform
    )
}

pub fn runtime_layout_lua(displays: &[Display]) -> String {
    let mut lua = String::from("local layout = {");
    for display in displays {
        let mode = display
            .selected_mode()
            .map(DisplayMode::config_value)
            .unwrap_or_else(|| "preferred".to_owned());
        lua.push_str(&format!(
            "{{ output = {}, mode = {}, position = {}, scale = {:.2}, transform = {}, disabled = false }},",
            lua_string(&display.name),
            lua_string(&mode),
            lua_string(&format!("{}x{}", display.position[0], display.position[1])),
            display.scale,
            display.transform
        ));
    }
    lua.push_str("} for _, spec in ipairs(layout) do hl.monitor(spec) end");
    lua
}

fn lua_string(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('\"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    )
}

pub fn validate_layout(displays: &[Display]) -> Vec<String> {
    let mut errors = Vec::new();
    if displays.is_empty() {
        errors.push("At least one active display is required".to_owned());
        return errors;
    }

    for display in displays {
        let Some(mode) = display.selected_mode() else {
            errors.push(format!("{} has no selected mode", display.name));
            continue;
        };
        if display.scale <= 0.0 {
            errors.push(format!("{} scale must be positive", display.name));
            continue;
        }
        let (width, height) = if matches!(display.transform, 1 | 3 | 5 | 7) {
            (mode.height as f32, mode.width as f32)
        } else {
            (mode.width as f32, mode.height as f32)
        };
        let logical_width = width / display.scale;
        let logical_height = height / display.scale;
        if (logical_width - logical_width.round()).abs() > 0.001
            || (logical_height - logical_height.round()).abs() > 0.001
        {
            errors.push(format!(
                "{} scale must produce an integral logical size",
                display.name
            ));
        }
    }

    for left_index in 0..displays.len() {
        for right_index in (left_index + 1)..displays.len() {
            let left = &displays[left_index];
            let right = &displays[right_index];
            let [left_width, left_height] = left.logical_size();
            let [right_width, right_height] = right.logical_size();
            let overlaps = (left.position[0] as f32) < right.position[0] as f32 + right_width
                && left.position[0] as f32 + left_width > right.position[0] as f32
                && (left.position[1] as f32) < right.position[1] as f32 + right_height
                && left.position[1] as f32 + left_height > right.position[1] as f32;
            if overlaps {
                errors.push(format!(
                    "{} and {} overlap; active displays must not overlap",
                    left.name, right.name
                ));
            }
        }
    }
    errors
}

pub fn save_config_with_backup(path: &Path, contents: &str) -> io::Result<Option<PathBuf>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let backup = if path.exists() {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("monitors.lua");
        let backup_path = path.with_file_name(format!("{file_name}.bak.{timestamp}"));
        fs::copy(path, &backup_path)?;
        Some(backup_path)
    } else {
        None
    };

    let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
    let mut candidate = fs::File::create(&temporary)?;
    candidate.write_all(contents.as_bytes())?;
    candidate.sync_all()?;
    drop(candidate);
    fs::rename(&temporary, path)?;
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(backup)
}
