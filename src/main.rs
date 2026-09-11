use displayeditor_rav::{
    Display, anchor_display, highest_mode_index, parse_hyprland_monitors, render_omarchy_lua,
    runtime_layout_lua, save_config_with_backup, snap_position_to_displays, validate_layout,
};
use eframe::egui::{
    self, Align, Color32, ComboBox, CornerRadius, FontId, Layout, Pos2, Rect, Sense, Stroke,
    StrokeKind, Vec2,
};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const BG: Color32 = Color32::from_rgb(10, 11, 12);
const PANEL: Color32 = Color32::from_rgb(17, 18, 20);
const SURFACE: Color32 = Color32::from_rgb(27, 29, 32);
const BORDER: Color32 = Color32::from_rgb(54, 57, 63);
const TEXT: Color32 = Color32::from_rgb(241, 242, 243);
const MUTED: Color32 = Color32::from_rgb(151, 156, 166);
const ACCENT: Color32 = Color32::from_rgb(230, 168, 54);
const DANGER: Color32 = Color32::from_rgb(229, 92, 92);
const DOCS_URL: &str = "https://kacm1network.org";
const APP_ICON_SVG: &str = include_str!("../assets/displayeditor-rav.svg");

struct Preview {
    original: Vec<Display>,
    deadline: Instant,
    watchdog_ack: PathBuf,
    watchdog_layout: PathBuf,
}

struct RavApp {
    displays: Vec<Display>,
    baseline: Vec<Display>,
    selected: usize,
    status: String,
    error: Option<String>,
    preview: Option<Preview>,
    gdk_scale: u8,
    show_about: bool,
    show_health: bool,
    identify_until: Option<Instant>,
}

impl RavApp {
    fn load() -> Self {
        let mut app = Self {
            displays: Vec::new(),
            baseline: Vec::new(),
            selected: 0,
            status: "Reading connected displays…".to_owned(),
            error: None,
            preview: None,
            gdk_scale: 1,
            show_about: false,
            show_health: true,
            identify_until: None,
        };
        app.refresh();
        app
    }

    fn refresh(&mut self) {
        match read_live_displays() {
            Ok(displays) if !displays.is_empty() => {
                self.gdk_scale = if displays.iter().any(|d| d.scale >= 1.75) {
                    2
                } else {
                    1
                };
                self.baseline = displays.clone();
                self.displays = displays;
                self.selected = self.selected.min(self.displays.len() - 1);
                self.status = format!("{} connected displays", self.displays.len());
                self.error = None;
            }
            Ok(_) => self.error = Some("Hyprland reported no active displays.".to_owned()),
            Err(error) => self.error = Some(error),
        }
    }

    fn preview(&mut self) {
        if self.preview.is_some() || self.displays.is_empty() {
            return;
        }
        let validation_errors = validate_layout(&self.displays);
        if !validation_errors.is_empty() {
            self.error = Some(validation_errors.join(" · "));
            return;
        }

        let original = self.baseline.clone();
        let (watchdog_layout, watchdog_ack) = match arm_watchdog(&original) {
            Ok(paths) => paths,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        match apply_runtime(&self.displays) {
            Ok(()) => {
                self.preview = Some(Preview {
                    original,
                    deadline: Instant::now() + Duration::from_secs(15),
                    watchdog_ack,
                    watchdog_layout,
                });
                self.status = "Preview active — confirm within 15 seconds".to_owned();
                self.error = None;
            }
            Err(error) => {
                acknowledge_watchdog_paths(&watchdog_ack, &watchdog_layout);
                self.error = Some(error);
            }
        }
    }

    fn revert_preview(&mut self, reason: &str) {
        let Some(preview) = self.preview.take() else {
            return;
        };
        match apply_runtime(&preview.original) {
            Ok(()) => {
                acknowledge_watchdog(&preview);
                self.displays = preview.original.clone();
                self.baseline = preview.original;
                self.status = reason.to_owned();
                self.error = None;
            }
            Err(error) => {
                self.error = Some(format!(
                    "Could not restore display layout; watchdog remains armed: {error}"
                ))
            }
        }
    }

    fn keep_and_save(&mut self) {
        let config_path = config_path();
        let lua = render_omarchy_lua(&self.displays, self.gdk_scale);
        if let Err(error) = preflight_lua(&config_path, &lua) {
            self.error = Some(error);
            return;
        }
        let backup = match save_config_with_backup(&config_path, &lua) {
            Ok(backup) => backup,
            Err(error) => {
                self.error = Some(format!("Could not save config: {error}"));
                return;
            }
        };

        let reload = Command::new("hyprctl").arg("reload").output();
        let errors = Command::new("hyprctl").arg("configerrors").output();
        let live = read_live_displays();
        let validation_error = match (reload, errors, live) {
            (Ok(reload), Ok(errors), Ok(live))
                if reload.status.success()
                    && errors.status.success()
                    && String::from_utf8_lossy(&errors.stdout).trim().is_empty()
                    && runtime_matches(&self.displays, &live) =>
            {
                None
            }
            (_, Ok(errors), _) if !String::from_utf8_lossy(&errors.stdout).trim().is_empty() => {
                Some(format!(
                    "Hyprland config error: {}",
                    String::from_utf8_lossy(&errors.stdout).trim()
                ))
            }
            _ => Some("Hyprland did not realize the requested layout".to_owned()),
        };

        if let Some(error) = validation_error {
            let restore_result = restore_persisted_config(&config_path, backup.as_deref());
            let _ = Command::new("hyprctl").arg("reload").output();
            self.revert_preview("Invalid saved layout reverted");
            self.error = Some(match restore_result {
                Ok(()) => format!("{error}; original configuration restored"),
                Err(restore_error) => {
                    format!("{error}; configuration restore failed: {restore_error}")
                }
            });
            return;
        }

        if let Some(preview) = self.preview.take() {
            acknowledge_watchdog(&preview);
        }
        self.baseline = self.displays.clone();
        self.status = backup.map_or_else(
            || format!("Saved {}", config_path.display()),
            |path| format!("Saved; backup: {}", path.display()),
        );
        self.error = None;
    }

    fn top_bar(&mut self, root: &mut egui::Ui) {
        egui::Panel::top("top")
            .frame(egui::Frame::new().fill(PANEL).inner_margin(16.0))
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("DISPLAYEDITOR RAV")
                            .strong()
                            .color(TEXT),
                    );
                    ui.label(egui::RichText::new("Omarchy monitor control").color(MUTED));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("About").clicked() {
                            self.show_about = true;
                        }
                        if ui.button("Docs").clicked() {
                            open_docs();
                        }
                        if ui.button("Checks").clicked() {
                            self.show_health = true;
                        }
                        if ui.button("Install launcher").clicked() {
                            match install_desktop_entry() {
                                Ok(path) => {
                                    self.status =
                                        format!("Installed app selector entry: {}", path.display())
                                }
                                Err(error) => {
                                    self.error =
                                        Some(format!("Could not install launcher: {error}"))
                                }
                            }
                        }
                        if ui.button("Refresh").clicked() && self.preview.is_none() {
                            self.refresh();
                        }
                        ui.colored_label(ACCENT, "Hyprland live");
                    });
                });
            });
    }

    fn inspector(&mut self, root: &mut egui::Ui) {
        egui::Panel::right("inspector_v2")
            .resizable(false)
            .exact_size(360.0)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(18.0))
            .show(root, |ui| {
                ui.heading("Display settings");
                ui.add_space(12.0);
                if self.displays.is_empty() {
                    ui.colored_label(MUTED, "No connected display available.");
                    return;
                }

                ui.label(egui::RichText::new("Display").color(MUTED));
                ComboBox::from_id_salt("display_choice")
                    .selected_text(format!(
                        "{} — {}",
                        self.selected + 1,
                        self.displays[self.selected].name
                    ))
                    .width(270.0)
                    .show_ui(ui, |ui| {
                        for (index, display) in self.displays.iter().enumerate() {
                            ui.selectable_value(
                                &mut self.selected,
                                index,
                                format!("{} — {}", index + 1, display.name),
                            );
                        }
                    });

                let display = &mut self.displays[self.selected];
                ui.add_space(12.0);
                ui.label(egui::RichText::new(&display.description).color(MUTED));
                ui.monospace(&display.name);
                ui.separator();

                let selected_label = display
                    .selected_mode()
                    .map(|mode| {
                        format!(
                            "{} × {}  {:.2} Hz",
                            mode.width, mode.height, mode.refresh_hz
                        )
                    })
                    .unwrap_or_else(|| "Current / preferred".to_owned());
                ui.label(egui::RichText::new("Resolution & refresh").color(MUTED));
                ComboBox::from_id_salt("mode_choice")
                    .selected_text(selected_label)
                    .width(270.0)
                    .show_ui(ui, |ui| {
                        for (index, mode) in display.available_modes.iter().enumerate() {
                            ui.selectable_value(
                                &mut display.selected_mode,
                                index,
                                format!(
                                    "{} × {}  {:.2} Hz",
                                    mode.width, mode.height, mode.refresh_hz
                                ),
                            );
                        }
                    });

                if ui.button("Use highest resolution").clicked()
                    && let Some(index) = highest_mode_index(display)
                {
                    display.selected_mode = index;
                }

                ui.add_space(10.0);
                ui.label(egui::RichText::new("Scale").color(MUTED));
                ComboBox::from_id_salt("scale_choice")
                    .selected_text(format!("{:.0}%", display.scale * 100.0))
                    .width(270.0)
                    .show_ui(ui, |ui| {
                        for scale in [1.0, 1.25, 1.5, 1.75, 2.0] {
                            ui.selectable_value(
                                &mut display.scale,
                                scale,
                                format!("{:.0}%", scale * 100.0),
                            );
                        }
                    });

                ui.label(egui::RichText::new("Orientation").color(MUTED));
                ComboBox::from_id_salt("orientation_choice")
                    .selected_text(transform_label(display.transform))
                    .width(270.0)
                    .show_ui(ui, |ui| {
                        for (value, label) in [
                            (0, "Landscape"),
                            (1, "Portrait (90°)"),
                            (2, "Landscape flipped"),
                            (3, "Portrait (270°)"),
                        ] {
                            ui.selectable_value(&mut display.transform, value, label);
                        }
                    });

                ui.add_space(12.0);
                if ui.button("Identify displays").clicked() {
                    self.identify_until = Some(Instant::now() + Duration::from_secs(5));
                    notify_identify(&self.displays);
                }
                if ui.button("Make primary / anchor at 0 × 0").clicked() {
                    anchor_display(&mut self.displays, self.selected);
                }
                if ui.button("Align all display tops").clicked() {
                    for display in &mut self.displays {
                        display.position[1] = 0;
                    }
                }

                ui.add_space(12.0);
                ui.label(egui::RichText::new("Backups").color(MUTED));
                if let Some(backup) = latest_backup(&config_path()) {
                    ui.small(format!(
                        "Latest: {}",
                        backup.file_name().unwrap_or_default().to_string_lossy()
                    ));
                    if ui.button("Restore latest backup").clicked() {
                        match restore_backup_and_reload(&backup) {
                            Ok(displays) => {
                                self.baseline = displays.clone();
                                self.displays = displays;
                                self.selected =
                                    self.selected.min(self.displays.len().saturating_sub(1));
                                self.status = format!("Restored backup: {}", backup.display());
                                self.error = None;
                            }
                            Err(error) => {
                                self.error = Some(format!("Backup restore failed: {error}"))
                            }
                        }
                    }
                } else {
                    ui.small("No monitor backups found yet.");
                }

                ui.add_space(18.0);
                ui.label(egui::RichText::new("Position").color(MUTED));
                ui.horizontal(|ui| {
                    ui.label("X");
                    ui.add(egui::DragValue::new(
                        &mut self.displays[self.selected].position[0],
                    ));
                    ui.label("Y");
                    ui.add(egui::DragValue::new(
                        &mut self.displays[self.selected].position[1],
                    ));
                });
                ui.small(
                    "Drag the numbered display in the canvas or enter exact logical coordinates.",
                );
            });
    }

    fn canvas(&mut self, root: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(18.0))
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Arrange displays");
                    ui.label(
                        egui::RichText::new("Drag to match their physical placement").color(MUTED),
                    );
                });
                ui.add_space(10.0);
                let available = ui.available_size() - Vec2::new(0.0, 74.0);
                let (canvas, _) =
                    ui.allocate_exact_size(available.max(Vec2::new(300.0, 240.0)), Sense::hover());
                ui.painter().rect_filled(
                    canvas,
                    CornerRadius::same(8),
                    Color32::from_rgb(13, 14, 16),
                );
                ui.painter().rect_stroke(
                    canvas,
                    CornerRadius::same(8),
                    Stroke::new(1.0, BORDER),
                    StrokeKind::Inside,
                );
                draw_grid(ui, canvas);

                if !self.displays.is_empty() {
                    let (origin, pixels_per_logical) = layout_transform(&self.displays, canvas);
                    let overlaps = overlapping_indices(&self.displays);
                    let identifying = self
                        .identify_until
                        .is_some_and(|until| Instant::now() < until);
                    for index in 0..self.displays.len() {
                        let logical = self.displays[index].logical_size();
                        let position = self.displays[index].position;
                        let display_name = self.displays[index].name.clone();
                        let min = origin
                            + Vec2::new(
                                position[0] as f32 * pixels_per_logical,
                                position[1] as f32 * pixels_per_logical,
                            );
                        let rect = Rect::from_min_size(
                            min,
                            Vec2::new(
                                (logical[0] * pixels_per_logical).max(86.0),
                                (logical[1] * pixels_per_logical).max(56.0),
                            ),
                        );
                        let response = ui.interact(
                            rect,
                            ui.id().with(("display", index)),
                            Sense::click_and_drag(),
                        );
                        if response.clicked() || response.dragged() {
                            self.selected = index;
                        }
                        if response.dragged() {
                            let delta = response.drag_delta() / pixels_per_logical;
                            let proposed = [
                                snap(
                                    self.displays[index].position[0] + delta.x.round() as i32,
                                    10,
                                ),
                                snap(
                                    self.displays[index].position[1] + delta.y.round() as i32,
                                    10,
                                ),
                            ];
                            self.displays[index].position =
                                snap_position_to_displays(&self.displays, index, proposed, 16);
                        }
                        let selected = index == self.selected;
                        ui.painter().rect_filled(
                            rect,
                            CornerRadius::same(6),
                            if selected {
                                Color32::from_rgb(48, 42, 28)
                            } else {
                                SURFACE
                            },
                        );
                        ui.painter().rect_stroke(
                            rect,
                            CornerRadius::same(6),
                            Stroke::new(
                                if selected || overlaps.contains(&index) {
                                    2.0
                                } else {
                                    1.0
                                },
                                if overlaps.contains(&index) {
                                    DANGER
                                } else if selected {
                                    ACCENT
                                } else {
                                    BORDER
                                },
                            ),
                            StrokeKind::Inside,
                        );
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            if identifying {
                                format!("{}", index + 1)
                            } else {
                                format!("{}\n{}", index + 1, display_name)
                            },
                            FontId::proportional(if identifying { 42.0 } else { 15.0 }),
                            TEXT,
                        );
                    }
                }

                if !self.displays.is_empty() {
                    let errors = validate_layout(&self.displays);
                    if !errors.is_empty() {
                        ui.add_space(8.0);
                        ui.colored_label(DANGER, errors.join(" · "));
                    }
                }

                ui.add_space(14.0);
                self.action_bar(ui);
            });
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        egui::Window::new("About DisplayeditorRav")
            .open(&mut self.show_about)
            .resizable(false)
            .show(ctx, |ui| {
                ui.heading("DisplayeditorRav");
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                ui.label("Safe visual display layout editor for Omarchy and Hyprland.");
                ui.separator();
                ui.label("License: MIT");
                ui.hyperlink_to("Documentation", DOCS_URL);
                ui.small("Runtime changes are previewed with a rollback watchdog. Persistent saves only touch ~/.config/hypr/monitors.lua.");
            });
    }

    fn health_window(&mut self, ctx: &egui::Context) {
        egui::Window::new("First-run checks")
            .open(&mut self.show_health)
            .resizable(false)
            .show(ctx, |ui| {
                for check in health_checks() {
                    ui.horizontal(|ui| {
                        ui.colored_label(
                            if check.ok { ACCENT } else { DANGER },
                            if check.ok { "OK" } else { "FIX" },
                        );
                        ui.label(check.label);
                        ui.small(check.detail);
                    });
                }
                ui.separator();
                ui.hyperlink_to("Usage documentation", DOCS_URL);
            });
    }

    fn action_bar(&mut self, ui: &mut egui::Ui) {
        if let Some(preview) = &self.preview {
            let seconds = preview
                .deadline
                .saturating_duration_since(Instant::now())
                .as_secs()
                + 1;
            ui.horizontal(|ui| {
                ui.colored_label(ACCENT, format!("Keep this layout? Reverting in {seconds}s"));
                if ui.button("Revert now").clicked() {
                    self.revert_preview("Preview reverted");
                }
                if ui.button("Keep & save").clicked() {
                    self.keep_and_save();
                }
            });
        } else {
            ui.horizontal(|ui| {
                if ui.button("Reset editor").clicked() {
                    self.displays = self.baseline.clone();
                    self.status = "Pending changes discarded".to_owned();
                }
                if ui.button("Preview changes").clicked() {
                    self.preview();
                }
                ui.label(egui::RichText::new(&self.status).color(MUTED));
            });
        }
        if let Some(error) = &self.error {
            ui.colored_label(DANGER, error);
        }
    }
}

impl eframe::App for RavApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self
            .preview
            .as_ref()
            .is_some_and(|preview| Instant::now() >= preview.deadline)
        {
            self.revert_preview("Preview timed out and was reverted");
        }
        if self.preview.is_some() || self.identify_until.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(250));
        }
        if self
            .identify_until
            .is_some_and(|until| Instant::now() >= until)
        {
            self.identify_until = None;
        }
        self.top_bar(ui);
        self.inspector(ui);
        self.canvas(ui);
        self.about_window(ui.ctx());
        self.health_window(ui.ctx());
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(preview) = self.preview.take()
            && apply_runtime(&preview.original).is_ok()
        {
            acknowledge_watchdog(&preview);
        }
    }
}

struct HealthCheck {
    label: &'static str,
    ok: bool,
    detail: String,
}

fn health_checks() -> Vec<HealthCheck> {
    let config = config_path();
    vec![
        HealthCheck {
            label: "hyprctl available",
            ok: Command::new("hyprctl")
                .arg("version")
                .output()
                .is_ok_and(|o| o.status.success()),
            detail: "Needed to read, preview, and reload monitor layouts.".to_owned(),
        },
        HealthCheck {
            label: "luac available",
            ok: Command::new("luac").arg("-v").output().is_ok(),
            detail: "Needed to preflight generated Lua before saving.".to_owned(),
        },
        HealthCheck {
            label: "Hyprland config path writable",
            ok: config.parent().is_some_and(|p| {
                p.exists()
                    && !p
                        .metadata()
                        .map(|m| m.permissions().readonly())
                        .unwrap_or(true)
            }),
            detail: config.display().to_string(),
        },
    ]
}

fn read_live_displays() -> Result<Vec<Display>, String> {
    let output = Command::new("hyprctl")
        .args(["monitors", "all", "-j"])
        .output()
        .map_err(|error| format!("Could not run hyprctl: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    parse_hyprland_monitors(&String::from_utf8_lossy(&output.stdout))
}

fn apply_runtime(displays: &[Display]) -> Result<(), String> {
    let lua = runtime_layout_lua(displays);
    let output = Command::new("hyprctl")
        .args(["eval", &lua])
        .output()
        .map_err(|error| format!("Could not run hyprctl: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Hyprland rejected the layout: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let errors = Command::new("hyprctl")
        .arg("configerrors")
        .output()
        .map_err(|error| format!("Could not validate Hyprland: {error}"))?;
    if !errors.status.success() || !String::from_utf8_lossy(&errors.stdout).trim().is_empty() {
        return Err(format!(
            "Hyprland reported: {}",
            String::from_utf8_lossy(&errors.stdout).trim()
        ));
    }
    Ok(())
}

fn runtime_matches(requested: &[Display], live: &[Display]) -> bool {
    requested.iter().all(|wanted| {
        live.iter()
            .find(|actual| actual.name == wanted.name)
            .is_some_and(|actual| {
                let wanted_mode = wanted.selected_mode();
                actual.position == wanted.position
                    && (actual.scale - wanted.scale).abs() < 0.01
                    && actual.transform == wanted.transform
                    && wanted_mode.is_none_or(|mode| {
                        actual.width == mode.width
                            && actual.height == mode.height
                            && (actual.refresh_hz - mode.refresh_hz).abs() < 0.1
                    })
            })
    })
}

fn arm_watchdog(original: &[Display]) -> Result<(PathBuf, PathBuf), String> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let base =
        std::env::temp_dir().join(format!("displayeditor-rav-{}-{nonce}", std::process::id()));
    let layout_path = base.with_extension("lua");
    let ack_path = base.with_extension("keep");
    fs::write(&layout_path, runtime_layout_lua(original))
        .map_err(|error| format!("Could not arm safety watchdog: {error}"))?;
    let executable = std::env::current_exe()
        .map_err(|error| format!("Could not locate safety watchdog: {error}"))?;
    Command::new(executable)
        .args([
            "--watchdog",
            &layout_path.to_string_lossy(),
            &ack_path.to_string_lossy(),
            "18",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("Could not start safety watchdog: {error}"))?;
    Ok((layout_path, ack_path))
}

fn acknowledge_watchdog(preview: &Preview) {
    acknowledge_watchdog_paths(&preview.watchdog_ack, &preview.watchdog_layout);
}

fn acknowledge_watchdog_paths(ack_path: &Path, layout_path: &Path) {
    let _ = fs::write(ack_path, b"keep");
    let _ = fs::remove_file(layout_path);
}

fn run_watchdog(layout_path: &Path, ack_path: &Path, seconds: u64) {
    std::thread::sleep(Duration::from_secs(seconds));
    if !ack_path.exists()
        && let Ok(lua) = fs::read_to_string(layout_path)
    {
        let _ = Command::new("hyprctl").args(["eval", &lua]).output();
    }
    let _ = fs::remove_file(layout_path);
    let _ = fs::remove_file(ack_path);
}

fn preflight_lua(config_path: &Path, lua: &str) -> Result<(), String> {
    let parent = config_path
        .parent()
        .ok_or_else(|| "Monitor configuration has no parent directory".to_owned())?;
    let candidate = parent.join(format!(
        ".displayeditor-rav-preflight-{}.lua",
        std::process::id()
    ));
    fs::write(&candidate, lua)
        .map_err(|error| format!("Could not create Lua preflight file: {error}"))?;
    let output = Command::new("luac").args(["-p"]).arg(&candidate).output();
    let _ = fs::remove_file(&candidate);
    match output {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => Err(format!(
            "Generated monitor configuration failed Lua validation: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Err(error) => Err(format!("Could not run luac preflight: {error}")),
    }
}

fn install_desktop_entry() -> std::io::Result<PathBuf> {
    let home = std::env::var_os("HOME").unwrap_or_else(|| "/home/kacm".into());
    let applications = PathBuf::from(&home).join(".local/share/applications");
    let icons = PathBuf::from(&home).join(".local/share/icons/hicolor/scalable/apps");
    fs::create_dir_all(&applications)?;
    fs::create_dir_all(&icons)?;
    let icon_path = icons.join("displayeditor-rav.svg");
    fs::write(&icon_path, APP_ICON_SVG)?;
    let executable = std::env::current_exe()?;
    let desktop_path = applications.join("displayeditor-rav.desktop");
    fs::write(
        &desktop_path,
        format!(
            "[Desktop Entry]\nType=Application\nName=DisplayeditorRav\nGenericName=Display Settings\nComment=Visual monitor layout editor for Omarchy\nExec={}\nIcon=displayeditor-rav\nTerminal=false\nCategories=Settings;HardwareSettings;\nKeywords=display;monitor;hyprland;omarchy;settings;\nStartupNotify=true\n",
            executable.display()
        ),
    )?;
    let _ = Command::new("update-desktop-database")
        .arg(&applications)
        .output();
    Ok(desktop_path)
}

fn open_docs() {
    let _ = Command::new("xdg-open").arg(DOCS_URL).spawn();
}

fn notify_identify(displays: &[Display]) {
    let summary = displays
        .iter()
        .enumerate()
        .map(|(index, display)| format!("{}: {}", index + 1, display.name))
        .collect::<Vec<_>>()
        .join("  •  ");
    let _ = Command::new("hyprctl")
        .args(["notify", "2", "5000", "rgb(e6a836)", &summary])
        .output();
}

fn latest_backup(config_path: &Path) -> Option<PathBuf> {
    let parent = config_path.parent()?;
    let prefix = format!(
        "{}.bak.",
        config_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("monitors.lua")
    );
    fs::read_dir(parent)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&prefix))
        })
        .max()
}

fn restore_backup_and_reload(backup: &Path) -> Result<Vec<Display>, String> {
    let config = config_path();
    save_config_with_backup(
        &config,
        &fs::read_to_string(backup).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let reload = Command::new("hyprctl")
        .arg("reload")
        .output()
        .map_err(|error| format!("Could not reload Hyprland: {error}"))?;
    if !reload.status.success() {
        return Err(String::from_utf8_lossy(&reload.stderr).trim().to_owned());
    }
    let errors = Command::new("hyprctl")
        .arg("configerrors")
        .output()
        .map_err(|error| format!("Could not validate Hyprland: {error}"))?;
    if !errors.status.success() || !String::from_utf8_lossy(&errors.stdout).trim().is_empty() {
        return Err(String::from_utf8_lossy(&errors.stdout).trim().to_owned());
    }
    read_live_displays()
}

fn overlapping_indices(displays: &[Display]) -> HashSet<usize> {
    let mut overlaps = HashSet::new();
    for left_index in 0..displays.len() {
        for right_index in (left_index + 1)..displays.len() {
            let left = &displays[left_index];
            let right = &displays[right_index];
            let [left_width, left_height] = left.logical_size();
            let [right_width, right_height] = right.logical_size();
            let has_overlap = (left.position[0] as f32) < right.position[0] as f32 + right_width
                && left.position[0] as f32 + left_width > right.position[0] as f32
                && (left.position[1] as f32) < right.position[1] as f32 + right_height
                && left.position[1] as f32 + left_height > right.position[1] as f32;
            if has_overlap {
                overlaps.insert(left_index);
                overlaps.insert(right_index);
            }
        }
    }
    overlaps
}

fn restore_persisted_config(path: &Path, backup: Option<&Path>) -> std::io::Result<()> {
    if let Some(backup) = backup {
        fs::copy(backup, path)?;
    } else if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn config_path() -> PathBuf {
    let home = std::env::var_os("HOME").unwrap_or_else(|| "/home/kacm".into());
    PathBuf::from(home).join(".config/hypr/monitors.lua")
}

fn transform_label(transform: u8) -> &'static str {
    match transform {
        1 => "Portrait (90°)",
        2 => "Landscape flipped",
        3 => "Portrait (270°)",
        _ => "Landscape",
    }
}

fn snap(value: i32, grid: i32) -> i32 {
    ((value as f32 / grid as f32).round() as i32) * grid
}

fn layout_transform(displays: &[Display], canvas: Rect) -> (Pos2, f32) {
    let min_x = displays.iter().map(|d| d.position[0]).min().unwrap_or(0) as f32;
    let min_y = displays.iter().map(|d| d.position[1]).min().unwrap_or(0) as f32;
    let max_x = displays
        .iter()
        .map(|d| d.position[0] as f32 + d.logical_size()[0])
        .fold(1.0_f32, f32::max);
    let max_y = displays
        .iter()
        .map(|d| d.position[1] as f32 + d.logical_size()[1])
        .fold(1.0_f32, f32::max);
    let content = Vec2::new((max_x - min_x).max(1.0), (max_y - min_y).max(1.0));
    let scale = ((canvas.width() - 80.0) / content.x)
        .min((canvas.height() - 80.0) / content.y)
        .clamp(0.025, 0.25);
    let drawn = content * scale;
    let origin = canvas.center() - drawn / 2.0 - Vec2::new(min_x * scale, min_y * scale);
    (origin, scale)
}

fn draw_grid(ui: &egui::Ui, rect: Rect) {
    let color = Color32::from_rgba_unmultiplied(255, 255, 255, 10);
    let step = 24.0;
    let mut x = rect.left();
    while x < rect.right() {
        ui.painter().line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(1.0, color),
        );
        x += step;
    }
    let mut y = rect.top();
    while y < rect.bottom() {
        ui.painter().line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            Stroke::new(1.0, color),
        );
        y += step;
    }
}

fn configure_visuals(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = BG;
    visuals.widgets.inactive.bg_fill = SURFACE;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(37, 39, 43);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    visuals.widgets.active.bg_fill = Color32::from_rgb(48, 42, 28);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    visuals.selection.bg_fill = Color32::from_rgb(105, 76, 25);
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    ctx.set_visuals(visuals);
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|arg| arg == "--watchdog") {
        if let (Some(layout), Some(ack), Some(seconds)) = (args.get(2), args.get(3), args.get(4)) {
            run_watchdog(
                Path::new(layout),
                Path::new(ack),
                seconds.parse().unwrap_or(18),
            );
        }
        return Ok(());
    }

    if args.iter().any(|arg| arg == "--install-desktop") {
        match install_desktop_entry() {
            Ok(path) => {
                println!("Installed app selector entry: {}", path.display());
                return Ok(());
            }
            Err(error) => {
                eprintln!("Launcher install failed: {error}");
                std::process::exit(1);
            }
        }
    }

    if args.iter().any(|arg| arg == "--check") {
        match read_live_displays() {
            Ok(displays) => {
                println!("DisplayeditorRav: {} connected displays", displays.len());
                for display in displays {
                    println!(
                        "{}: {}x{} at {}x{}, scale {:.2}, {} modes",
                        display.name,
                        display.width,
                        display.height,
                        display.position[0],
                        display.position[1],
                        display.scale,
                        display.available_modes.len()
                    );
                }
                return Ok(());
            }
            Err(error) => {
                eprintln!("DisplayeditorRav check failed: {error}");
                std::process::exit(1);
            }
        }
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("DisplayeditorRav")
            .with_inner_size([1180.0, 720.0])
            .with_min_inner_size([900.0, 580.0]),
        ..Default::default()
    };
    eframe::run_native(
        "DisplayeditorRav",
        options,
        Box::new(|creation| {
            configure_visuals(&creation.egui_ctx);
            Ok(Box::new(RavApp::load()))
        }),
    )
}
