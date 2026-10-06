use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};
use ntfs_manager::{
    disk::{DiskService, DiskVolume},
    helper_client::HelperClient,
    ntfs3g, platform,
    settings::Settings,
};
use serde::{Deserialize, Serialize};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalPosition},
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS},
    window::{Window, WindowId, WindowLevel},
};
use wry::{WebView, WebViewBuilder};

const APPLICATION_PATH: &str = "/Applications/NTFS Manager.app";
const PANEL_HTML: &str = include_str!("../../assets/panel.html");
const PANEL_WIDTH: f64 = 390.0;
const PANEL_HEIGHT: f64 = 590.0;
const MIN_HELPER_VERSION: &str = "0.4.2";
const RELEASES_API_URL: &str =
    "https://api.github.com/repos/CrazyBoy49z/ntfs-manager/releases/latest";
const UPDATE_CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Debug)]
enum UserEvent {
    Tray(TrayIconEvent),
    Ipc(String),
    Tick,
    Snapshot {
        volumes: std::result::Result<Vec<DiskVolume>, String>,
        helper_version: Option<String>,
    },
    ActionCompleted {
        label: String,
        errors: Vec<String>,
    },
    SetupCompleted {
        result: std::result::Result<SetupOutcome, String>,
    },
    MoveCompleted {
        result: std::result::Result<(), String>,
    },
    UpdateChecked {
        result: std::result::Result<Option<UpdateInfo>, String>,
        manual: bool,
    },
    UpdateInstalled {
        result: std::result::Result<(), String>,
    },
}

#[derive(Clone, Debug)]
enum SetupOutcome {
    Repaired,
    FullSetupOpened,
}

#[derive(Clone, Debug, Serialize)]
struct UpdateInfo {
    version: String,
    tag: String,
    zip_url: String,
    checksum_url: String,
}

#[derive(Debug, Deserialize)]
struct UiCommand {
    action: String,
    #[serde(default)]
    device: Option<String>,
    #[serde(default)]
    enabled: Option<bool>,
}

#[derive(Serialize)]
struct UiState<'a> {
    version: &'static str,
    summary: String,
    auto_mount: bool,
    launch_at_login: bool,
    check_updates: bool,
    installed_in_applications: bool,
    helper_ready: bool,
    helper_version: Option<&'a str>,
    action_in_flight: bool,
    error: Option<String>,
    update_available: Option<&'a UpdateInfo>,
    update_checking: bool,
    update_installing: bool,
    volumes: &'a [DiskVolume],
}

struct App {
    disks: DiskService,
    helper: HelperClient,
    proxy: EventLoopProxy<UserEvent>,
    tray: Option<TrayIcon>,
    window: Option<Window>,
    webview: Option<WebView>,
    volumes: Vec<DiskVolume>,
    helper_version: Option<String>,
    refresh_in_flight: bool,
    action_in_flight: bool,
    setup_in_flight: bool,
    auto_repair_attempted: bool,
    update_available: Option<UpdateInfo>,
    update_check_in_flight: bool,
    update_install_in_flight: bool,
    last_update_check: Option<Instant>,
    dismissed_update_version: Option<String>,
    panel_visible: bool,
    status_override: Option<(String, Instant)>,
}

impl App {
    fn new(proxy: EventLoopProxy<UserEvent>) -> Self {
        Self {
            disks: DiskService::new(),
            helper: HelperClient::new(),
            proxy,
            tray: None,
            window: None,
            webview: None,
            volumes: Vec::new(),
            helper_version: None,
            refresh_in_flight: false,
            action_in_flight: false,
            setup_in_flight: false,
            auto_repair_attempted: false,
            update_available: None,
            update_check_in_flight: false,
            update_install_in_flight: false,
            last_update_check: None,
            dismissed_update_version: None,
            panel_visible: false,
            status_override: None,
        }
    }

    fn initialize(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let tray = TrayIconBuilder::new()
            .with_icon_templated(tray_template_icon()?)
            .with_autosave_name("dev.step2.ntfs-manager")
            .with_tooltip("NTFS Manager")
            .build()
            .context("failed to create menu-bar item")?;

        let window = event_loop
            .create_window(
                Window::default_attributes()
                    .with_title("NTFS Manager")
                    .with_inner_size(LogicalSize::new(PANEL_WIDTH, PANEL_HEIGHT))
                    .with_min_inner_size(LogicalSize::new(PANEL_WIDTH, PANEL_HEIGHT))
                    .with_max_inner_size(LogicalSize::new(PANEL_WIDTH, PANEL_HEIGHT))
                    .with_resizable(false)
                    .with_decorations(false)
                    .with_transparent(true)
                    .with_blur(true)
                    .with_visible(false)
                    .with_window_level(WindowLevel::AlwaysOnTop),
            )
            .context("failed to create NTFS Manager popover")?;

        let ipc_proxy = self.proxy.clone();
        let webview = WebViewBuilder::new()
            .with_html(PANEL_HTML)
            .with_transparent(true)
            .with_accept_first_mouse(true)
            .with_ipc_handler(move |request| {
                let _ = ipc_proxy.send_event(UserEvent::Ipc(request.body().clone()));
            })
            .build(&window)
            .context("failed to create NTFS Manager webview")?;

        self.tray = Some(tray);
        self.window = Some(window);
        self.webview = Some(webview);
        self.request_refresh();
        if Settings::load().unwrap_or_default().check_updates {
            self.request_update_check(false);
        }

        Ok(())
    }

    fn request_refresh(&mut self) {
        if self.refresh_in_flight {
            return;
        }

        self.refresh_in_flight = true;

        let disks = self.disks.clone();
        let helper = self.helper.clone();
        let proxy = self.proxy.clone();

        thread::spawn(move || {
            let helper_version = helper.version().ok();
            let volumes = disks.external_volumes().map_err(|err| format!("{err:#}"));
            let _ = proxy.send_event(UserEvent::Snapshot {
                volumes,
                helper_version,
            });
        });
    }

    fn apply_snapshot(
        &mut self,
        volumes: std::result::Result<Vec<DiskVolume>, String>,
        helper_version: Option<String>,
    ) {
        self.refresh_in_flight = false;
        self.helper_version = helper_version;

        match volumes {
            Ok(volumes) => self.volumes = volumes,
            Err(err) => self.set_error(format!("Scan error: {err}"), 20),
        }

        if is_installed_in_applications()
            && !self.helper_is_compatible()
            && !self.setup_in_flight
            && !self.auto_repair_attempted
            && !auto_repair_prompted_for_current_version()
            && runtime_dependencies_ready()
        {
            self.start_setup(true);
        }

        self.push_state();
    }

    fn helper_is_compatible(&self) -> bool {
        self.helper_version
            .as_deref()
            .is_some_and(|version| version_at_least(version, MIN_HELPER_VERSION))
    }

    fn summary(&self) -> String {
        let ntfs = self
            .volumes
            .iter()
            .filter(|volume| volume.is_ntfs)
            .collect::<Vec<_>>();

        if ntfs.is_empty() {
            return "No NTFS volumes connected".to_string();
        }

        let mounted = ntfs.iter().filter(|volume| volume.mounted).count();
        let writable = ntfs.iter().filter(|volume| volume.writable).count();

        format!(
            "{} NTFS · {} mounted · {} read/write",
            ntfs.len(),
            mounted,
            writable
        )
    }

    fn current_error(&mut self) -> Option<String> {
        match self.status_override.as_ref() {
            Some((message, until)) if Instant::now() < *until => Some(message.clone()),
            Some(_) => {
                self.status_override = None;
                None
            }
            None => None,
        }
    }

    fn push_state(&mut self) {
        let settings = Settings::load().unwrap_or_default();
        let error = self.current_error();
        let state = UiState {
            version: env!("CARGO_PKG_VERSION"),
            summary: self.summary(),
            auto_mount: settings.auto_mount,
            launch_at_login: settings.launch_at_login,
            check_updates: settings.check_updates,
            installed_in_applications: is_installed_in_applications(),
            helper_ready: self.helper_is_compatible(),
            helper_version: self.helper_version.as_deref(),
            action_in_flight: self.action_in_flight
                || self.setup_in_flight
                || self.update_install_in_flight,
            error,
            update_available: self.update_available.as_ref(),
            update_checking: self.update_check_in_flight,
            update_installing: self.update_install_in_flight,
            volumes: &self.volumes,
        };

        let Ok(json) = serde_json::to_string(&state) else {
            return;
        };

        if let Some(webview) = self.webview.as_ref() {
            let script = format!("window.ntfsManager && window.ntfsManager.updateState({json});");
            let _ = webview.evaluate_script(&script);
        }

        if let Some(tray) = self.tray.as_ref() {
            let _ = tray.set_tooltip(Some(&state.summary));
        }
    }

    fn set_error(&mut self, message: String, seconds: u64) {
        self.status_override = Some((message, Instant::now() + Duration::from_secs(seconds)));
    }

    fn toggle_panel(&mut self, rect: tray_icon::Rect) {
        let Some(window) = self.window.as_ref() else {
            return;
        };

        if self.panel_visible {
            window.set_visible(false);
            self.panel_visible = false;
            return;
        }

        let panel_width = window.outer_size().width as f64;
        let panel_height = window.outer_size().height as f64;
        let mut x = rect.position.x + (f64::from(rect.size.width) / 2.0) - (panel_width / 2.0);
        let mut y = rect.position.y + f64::from(rect.size.height) + 5.0;

        if let Some(monitor) = window.current_monitor() {
            let position = monitor.position();
            let size = monitor.size();
            let left = position.x as f64 + 8.0;
            let right = position.x as f64 + size.width as f64 - panel_width - 8.0;
            let top = position.y as f64 + 8.0;
            let bottom = position.y as f64 + size.height as f64 - panel_height - 8.0;

            x = x.max(left).min(right.max(left));
            if y > bottom {
                y = (rect.position.y - panel_height - 5.0).max(top);
            }
        }

        window.set_outer_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
        window.set_visible(true);
        window.focus_window();
        self.panel_visible = true;
        self.request_refresh();
        self.push_state();
    }

    fn hide_panel(&mut self) {
        if let Some(window) = self.window.as_ref() {
            window.set_visible(false);
        }
        self.panel_visible = false;
    }

    fn handle_ipc(&mut self, event_loop: &ActiveEventLoop, body: &str) {
        let command = match serde_json::from_str::<UiCommand>(body) {
            Ok(command) => command,
            Err(err) => {
                self.set_error(format!("UI command error: {err}"), 10);
                self.push_state();
                return;
            }
        };

        match command.action.as_str() {
            "ready" | "refresh" => {
                self.request_refresh();
                self.push_state();
            }
            "set-auto-mount" => {
                if let Some(enabled) = command.enabled {
                    if let Err(err) = Settings::set_auto_mount(enabled) {
                        self.set_error(format!("Settings error: {err}"), 20);
                    }
                    self.request_refresh();
                    self.push_state();
                }
            }
            "set-launch-at-login" => {
                if let Some(enabled) = command.enabled {
                    if let Err(err) = set_launch_at_login(enabled) {
                        self.set_error(format!("Launch at login error: {err}"), 20);
                    }
                    self.push_state();
                }
            }
            "set-check-updates" => {
                if let Some(enabled) = command.enabled {
                    if let Err(err) = Settings::set_check_updates(enabled) {
                        self.set_error(format!("Settings error: {err}"), 20);
                    }
                    if enabled {
                        self.request_update_check(false);
                    }
                    self.push_state();
                }
            }
            "mount" => {
                if let Some(device) = command.device {
                    self.start_mount_devices(vec![device], "Mount".to_string());
                }
            }
            "unmount" => {
                if let Some(device) = command.device {
                    self.start_unmount_devices(vec![device], "Unmount".to_string());
                }
            }
            "mount-all" => {
                let targets = self
                    .volumes
                    .iter()
                    .filter(|volume| volume.is_ntfs && !volume.writable)
                    .map(|volume| volume.device.clone())
                    .collect::<Vec<_>>();
                self.start_mount_devices(targets, "Mount all".to_string());
            }
            "open-volume" => {
                if let Some(device) = command.device {
                    if let Some(path) = self
                        .volumes
                        .iter()
                        .find(|volume| volume.device == device)
                        .and_then(|volume| volume.mount_point.as_deref())
                    {
                        if let Err(err) = Command::new("/usr/bin/open").arg(path).spawn() {
                            self.set_error(format!("Finder error: {err}"), 20);
                        }
                    }
                }
            }
            "open-logs" => self.open_logs(),
            "check-update-now" => self.request_update_check(true),
            "install-update" => self.start_update(),
            "dismiss-update" => {
                self.dismissed_update_version = self
                    .update_available
                    .as_ref()
                    .map(|update| update.version.clone());
                self.update_available = None;
                self.push_state();
            }
            "repair" => self.start_setup(false),
            "move-to-applications" => self.start_move_to_applications(),
            "quit" => event_loop.exit(),
            _ => {}
        }
    }

    fn start_mount_devices(&mut self, targets: Vec<String>, label: String) {
        if self.action_in_flight || targets.is_empty() || !self.helper_is_compatible() {
            return;
        }

        self.action_in_flight = true;
        self.push_state();

        let helper = self.helper.clone();
        let proxy = self.proxy.clone();

        thread::spawn(move || {
            let mut errors = Vec::new();
            for device in targets {
                if let Err(err) = helper.mount(&device, None) {
                    errors.push(format!("{device}: {err}"));
                }
            }
            let _ = proxy.send_event(UserEvent::ActionCompleted { label, errors });
        });
    }

    fn start_unmount_devices(&mut self, targets: Vec<String>, label: String) {
        if self.action_in_flight || targets.is_empty() || !self.helper_is_compatible() {
            return;
        }

        self.action_in_flight = true;
        self.push_state();

        let helper = self.helper.clone();
        let proxy = self.proxy.clone();

        thread::spawn(move || {
            let mut errors = Vec::new();
            for device in targets {
                if let Err(err) = helper.unmount(&device) {
                    errors.push(format!("{device}: {err}"));
                }
            }
            let _ = proxy.send_event(UserEvent::ActionCompleted { label, errors });
        });
    }

    fn finish_action(&mut self, label: String, errors: Vec<String>) {
        self.action_in_flight = false;

        if errors.is_empty() {
            self.status_override = Some((
                format!("{label} completed"),
                Instant::now() + Duration::from_secs(4),
            ));
        } else {
            let error = errors.join("; ");
            eprintln!("{label} failed: {error}");
            self.set_error(format!("{label} failed: {error}"), 30);
        }

        self.request_refresh();
        self.push_state();
    }

    fn start_setup(&mut self, automatic: bool) {
        if self.setup_in_flight || !is_installed_in_applications() {
            return;
        }

        if automatic {
            self.auto_repair_attempted = true;
            if let Err(err) = mark_auto_repair_prompted_for_current_version() {
                eprintln!("failed to persist automatic repair prompt state: {err:#}");
            }
        }

        self.setup_in_flight = true;
        self.push_state();

        let proxy = self.proxy.clone();
        thread::spawn(move || {
            let result = if runtime_dependencies_ready() {
                silent_repair_components().map(|_| SetupOutcome::Repaired)
            } else {
                launch_full_setup_terminal().map(|_| SetupOutcome::FullSetupOpened)
            }
            .map_err(|err| format!("{err:#}"));

            let _ = proxy.send_event(UserEvent::SetupCompleted { result });
        });
    }

    fn finish_setup(&mut self, result: std::result::Result<SetupOutcome, String>) {
        self.setup_in_flight = false;

        match result {
            Ok(SetupOutcome::Repaired) => {
                self.status_override = Some((
                    "Components repaired".to_string(),
                    Instant::now() + Duration::from_secs(5),
                ));
            }
            Ok(SetupOutcome::FullSetupOpened) => {
                self.status_override = Some((
                    "Setup opened in Terminal for missing dependencies".to_string(),
                    Instant::now() + Duration::from_secs(12),
                ));
            }
            Err(err) => self.set_error(format!("Setup failed: {err}"), 30),
        }

        self.request_refresh();
        self.push_state();
    }

    fn start_move_to_applications(&mut self) {
        if self.action_in_flight || is_installed_in_applications() {
            return;
        }

        let source = match current_app_path() {
            Ok(path) => path,
            Err(err) => {
                self.set_error(format!("Move failed: {err}"), 20);
                self.push_state();
                return;
            }
        };

        self.action_in_flight = true;
        self.push_state();

        let proxy = self.proxy.clone();
        thread::spawn(move || {
            let result = move_app_to_applications(&source).map_err(|err| format!("{err:#}"));
            let _ = proxy.send_event(UserEvent::MoveCompleted { result });
        });
    }

    fn open_logs(&self) {
        let Some(home) = env::var_os("HOME") else {
            return;
        };

        let logs = PathBuf::from(home)
            .join("Library")
            .join("Logs")
            .join("NTFS Manager");
        let _ = fs::create_dir_all(&logs);
        let _ = Command::new("/usr/bin/open").arg(logs).spawn();
    }

    fn request_update_check(&mut self, manual: bool) {
        if self.update_check_in_flight || self.update_install_in_flight {
            return;
        }

        if !manual && !Settings::load().unwrap_or_default().check_updates {
            return;
        }

        self.update_check_in_flight = true;
        self.push_state();

        let proxy = self.proxy.clone();
        thread::spawn(move || {
            let result = check_latest_release().map_err(|err| format!("{err:#}"));
            let _ = proxy.send_event(UserEvent::UpdateChecked { result, manual });
        });
    }

    fn finish_update_check(
        &mut self,
        result: std::result::Result<Option<UpdateInfo>, String>,
        manual: bool,
    ) {
        self.update_check_in_flight = false;
        self.last_update_check = Some(Instant::now());

        match result {
            Ok(Some(update)) => {
                if manual
                    || self.dismissed_update_version.as_deref() != Some(update.version.as_str())
                {
                    self.update_available = Some(update);
                }
            }
            Ok(None) => {
                self.update_available = None;
                if manual {
                    self.status_override = Some((
                        "У вас остання версія NTFS Manager".to_string(),
                        Instant::now() + Duration::from_secs(5),
                    ));
                }
            }
            Err(err) => {
                eprintln!("update check failed: {err}");
                if manual {
                    self.set_error(format!("Update check failed: {err}"), 20);
                }
            }
        }

        self.push_state();
    }

    fn start_update(&mut self) {
        if self.update_install_in_flight {
            return;
        }

        let Some(update) = self.update_available.clone() else {
            self.request_update_check(true);
            return;
        };

        if !is_installed_in_applications() {
            self.set_error(
                "Move NTFS Manager to /Applications before updating".to_string(),
                20,
            );
            self.push_state();
            return;
        }

        self.update_install_in_flight = true;
        self.push_state();

        let proxy = self.proxy.clone();
        thread::spawn(move || {
            let result = install_update(&update).map_err(|err| format!("{err:#}"));
            let _ = proxy.send_event(UserEvent::UpdateInstalled { result });
        });
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.tray.is_none() {
            if let Err(err) = self.initialize(event_loop) {
                eprintln!("failed to initialize NTFS Manager: {err:#}");
                event_loop.exit();
            }
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Tray(event) => {
                if let TrayIconEvent::Click {
                    rect,
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    self.toggle_panel(rect);
                }
            }
            UserEvent::Ipc(body) => self.handle_ipc(event_loop, &body),
            UserEvent::Tick => {
                self.request_refresh();

                let settings = Settings::load().unwrap_or_default();
                let update_due = self
                    .last_update_check
                    .is_none_or(|checked| checked.elapsed() >= UPDATE_CHECK_INTERVAL);
                if settings.check_updates && update_due && !self.update_check_in_flight {
                    self.request_update_check(false);
                }

                if self.panel_visible {
                    self.push_state();
                }
            }
            UserEvent::Snapshot {
                volumes,
                helper_version,
            } => self.apply_snapshot(volumes, helper_version),
            UserEvent::ActionCompleted { label, errors } => {
                self.finish_action(label, errors);
            }
            UserEvent::SetupCompleted { result } => self.finish_setup(result),
            UserEvent::MoveCompleted { result } => {
                self.action_in_flight = false;
                match result {
                    Ok(()) => event_loop.exit(),
                    Err(err) => {
                        self.set_error(format!("Move failed: {err}"), 30);
                        self.push_state();
                    }
                }
            }
            UserEvent::UpdateChecked { result, manual } => {
                self.finish_update_check(result, manual);
            }
            UserEvent::UpdateInstalled { result } => {
                self.update_install_in_flight = false;
                match result {
                    Ok(()) => event_loop.exit(),
                    Err(err) => {
                        self.set_error(format!("Update failed: {err}"), 30);
                        self.push_state();
                    }
                }
            }
        }
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(window) = self.window.as_ref() else {
            return;
        };

        if window.id() != window_id {
            return;
        }

        match event {
            WindowEvent::CloseRequested => self.hide_panel(),
            WindowEvent::Focused(false) if self.panel_visible => self.hide_panel(),
            _ => {}
        }
    }
}

fn version_parts(version: &str) -> Option<[u64; 3]> {
    let normalized = version.trim().trim_start_matches('v');
    let core = normalized
        .split_once('-')
        .map_or(normalized, |(core, _)| core);
    let mut parts = core.split('.');

    Some([
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    ])
}

fn version_at_least(version: &str, minimum: &str) -> bool {
    match (version_parts(version), version_parts(minimum)) {
        (Some(version), Some(minimum)) => version >= minimum,
        _ => false,
    }
}

fn version_is_newer(candidate: &str, current: &str) -> bool {
    match (version_parts(candidate), version_parts(current)) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => false,
    }
}

fn check_latest_release() -> Result<Option<UpdateInfo>> {
    let user_agent = format!("NTFS-Manager/{}", env!("CARGO_PKG_VERSION"));
    let output = Command::new("/usr/bin/curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--connect-timeout",
            "5",
            "--max-time",
            "20",
            "--header",
            "Accept: application/vnd.github+json",
            "--header",
            &format!("User-Agent: {user_agent}"),
            RELEASES_API_URL,
        ])
        .output()
        .context("failed to check GitHub Releases")?;

    if !output.status.success() {
        bail!(
            "GitHub Releases request failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let release: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("invalid GitHub release response")?;
    let tag = release
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .context("latest release has no tag_name")?
        .to_string();
    let version = tag.trim_start_matches('v').to_string();

    if !version_is_newer(&version, env!("CARGO_PKG_VERSION")) {
        return Ok(None);
    }

    let assets = release
        .get("assets")
        .and_then(serde_json::Value::as_array)
        .context("latest release has no assets")?;

    let zip = assets
        .iter()
        .find(|asset| {
            asset
                .get("name")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|name| name.ends_with("macOS-universal.zip"))
        })
        .context("latest release has no Universal macOS ZIP")?;

    let zip_name = zip
        .get("name")
        .and_then(serde_json::Value::as_str)
        .context("release ZIP has no name")?;
    let zip_url = zip
        .get("browser_download_url")
        .and_then(serde_json::Value::as_str)
        .context("release ZIP has no download URL")?
        .to_string();

    let checksum_name = format!("{zip_name}.sha256");
    let checksum_url = assets
        .iter()
        .find_map(|asset| {
            let name = asset.get("name")?.as_str()?;
            if name == checksum_name {
                asset
                    .get("browser_download_url")?
                    .as_str()
                    .map(str::to_string)
            } else {
                None
            }
        })
        .context("latest release has no SHA-256 checksum asset")?;

    Ok(Some(UpdateInfo {
        version,
        tag,
        zip_url,
        checksum_url,
    }))
}

fn download_file(url: &str, destination: &Path) -> Result<()> {
    let output = Command::new("/usr/bin/curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--connect-timeout",
            "10",
            "--max-time",
            "300",
            "--output",
        ])
        .arg(destination)
        .arg(url)
        .output()
        .with_context(|| format!("failed to download {url}"))?;

    if !output.status.success() {
        bail!(
            "download failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(())
}

fn sha256(path: &Path) -> Result<String> {
    let output = Command::new("/usr/bin/shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .with_context(|| format!("failed to hash {}", path.display()))?;

    if !output.status.success() {
        bail!(
            "SHA-256 failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .map(str::to_string)
        .context("shasum returned no digest")
}

fn plist_value(app: &Path, key: &str) -> Result<String> {
    let plist = app.join("Contents").join("Info.plist");
    let output = Command::new("/usr/bin/plutil")
        .args(["-extract", key, "raw"])
        .arg(&plist)
        .output()
        .with_context(|| format!("failed to inspect {}", plist.display()))?;

    if !output.status.success() {
        bail!(
            "plutil failed for {key}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn verify_update_bundle(app: &Path, expected_version: &str) -> Result<()> {
    verify_app_bundle(app)?;

    let identifier = plist_value(app, "CFBundleIdentifier")?;
    if identifier != "dev.step2.ntfs-manager" {
        bail!("downloaded app has unexpected bundle identifier: {identifier}");
    }

    let version = plist_value(app, "CFBundleShortVersionString")?;
    if version != expected_version {
        bail!("downloaded app version {version} does not match expected {expected_version}");
    }

    Ok(())
}

fn copy_update_direct(source: &Path, destination: &Path, expected_version: &str) -> Result<()> {
    let parent = destination
        .parent()
        .context("Applications destination has no parent")?;
    let pid = std::process::id();
    let staging = parent.join(format!(".NTFS Manager.update-{pid}.app"));
    let backup = parent.join(format!(".NTFS Manager.backup-{pid}.app"));

    let _ = fs::remove_dir_all(&staging);
    let _ = fs::remove_dir_all(&backup);

    let copy = Command::new("/usr/bin/ditto")
        .arg(source)
        .arg(&staging)
        .output()
        .context("failed to stage app update")?;

    if !copy.status.success() {
        bail!(
            "cannot stage app update: {}",
            String::from_utf8_lossy(&copy.stderr).trim()
        );
    }

    verify_update_bundle(&staging, expected_version)?;

    if destination.exists() {
        fs::rename(destination, &backup).context("failed to move current app aside")?;
    }

    if let Err(err) = fs::rename(&staging, destination) {
        if backup.exists() {
            let _ = fs::rename(&backup, destination);
        }
        return Err(err).context("failed to activate app update");
    }

    let _ = fs::remove_dir_all(&backup);
    Ok(())
}

fn copy_update_privileged(source: &Path, destination: &Path) -> Result<()> {
    let parent = destination
        .parent()
        .context("Applications destination has no parent")?;
    let pid = std::process::id();
    let staging = parent.join(format!(".NTFS Manager.update-{pid}.app"));
    let backup = parent.join(format!(".NTFS Manager.backup-{pid}.app"));

    let command = format!(
        "/bin/rm -rf {staging} {backup};          /usr/bin/ditto {source} {staging};          if [ -e {destination} ]; then /bin/mv {destination} {backup}; fi;          /bin/mv {staging} {destination};          /bin/rm -rf {backup}",
        staging = shell_quote(&staging.to_string_lossy()),
        backup = shell_quote(&backup.to_string_lossy()),
        source = shell_quote(&source.to_string_lossy()),
        destination = shell_quote(&destination.to_string_lossy()),
    );

    let script = r#"
on run argv
    do shell script (item 1 of argv) with administrator privileges
end run
"#;

    let output = Command::new("/usr/bin/osascript")
        .args(["-e", script, "--", &command])
        .output()
        .context("failed to request permission to install update")?;

    if !output.status.success() {
        bail!(
            "privileged update failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(())
}

fn install_update(update: &UpdateInfo) -> Result<()> {
    let destination = Path::new(APPLICATION_PATH);
    if !is_installed_in_applications() {
        bail!("NTFS Manager must be in /Applications before it can update itself");
    }

    let temp = env::temp_dir().join(format!(
        "ntfs-manager-update-{}-{}",
        std::process::id(),
        update.version
    ));
    let _ = fs::remove_dir_all(&temp);
    fs::create_dir_all(&temp)?;

    let zip = temp.join("update.zip");
    let checksum = temp.join("update.zip.sha256");
    let extracted = temp.join("extracted");

    download_file(&update.zip_url, &zip)?;
    download_file(&update.checksum_url, &checksum)?;

    let expected = fs::read_to_string(&checksum)
        .context("failed to read downloaded checksum")?
        .split_whitespace()
        .next()
        .context("downloaded checksum is empty")?
        .to_ascii_lowercase();
    let actual = sha256(&zip)?.to_ascii_lowercase();

    if expected != actual {
        bail!("downloaded update failed SHA-256 verification");
    }

    fs::create_dir_all(&extracted)?;
    let unpack = Command::new("/usr/bin/ditto")
        .args(["-x", "-k"])
        .arg(&zip)
        .arg(&extracted)
        .output()
        .context("failed to extract update")?;

    if !unpack.status.success() {
        bail!(
            "failed to extract update: {}",
            String::from_utf8_lossy(&unpack.stderr).trim()
        );
    }

    let source = extracted.join("NTFS Manager.app");
    if !source.exists() {
        bail!("downloaded archive does not contain NTFS Manager.app");
    }

    verify_update_bundle(&source, &update.version)?;

    if copy_update_direct(&source, destination, &update.version).is_err() {
        copy_update_privileged(&source, destination)?;
        verify_update_bundle(destination, &update.version)?;
    }

    let _ = fs::remove_dir_all(&temp);
    open_installed_app(destination)
}

fn auto_repair_prompt_path() -> Result<PathBuf> {
    let home = env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join("NTFS Manager")
        .join(format!(
            "auto-repair-prompted-v{}",
            env!("CARGO_PKG_VERSION")
        )))
}

fn auto_repair_prompted_for_current_version() -> bool {
    auto_repair_prompt_path()
        .map(|path| path.exists())
        .unwrap_or(false)
}

fn mark_auto_repair_prompted_for_current_version() -> Result<()> {
    let path = auto_repair_prompt_path()?;
    let parent = path
        .parent()
        .context("automatic repair prompt path has no parent")?;
    fs::create_dir_all(parent)?;
    fs::write(path, b"prompted\n")?;
    Ok(())
}

fn runtime_dependencies_ready() -> bool {
    Path::new("/Library/Filesystems/macfuse.fs").exists() && ntfs3g::find_binary().is_ok()
}

fn resources_dir() -> Result<PathBuf> {
    Ok(current_app_path()?.join("Contents").join("Resources"))
}

fn silent_repair_components() -> Result<()> {
    let resources = resources_dir()?;
    let script_path = resources.join("repair-components.sh");
    if !script_path.exists() {
        bail!("repair-components.sh is missing from app resources");
    }

    let home = env::var("HOME").context("HOME is not set")?;
    let uid = unsafe { libc::getuid() };
    let gid = unsafe { libc::getgid() };
    let launch_at_login = Settings::load().unwrap_or_default().launch_at_login;

    let shell = format!(
        "/bin/bash {} {} {} {} {}",
        shell_quote(&script_path.to_string_lossy()),
        shell_quote(&home),
        uid,
        gid,
        if launch_at_login { 1 } else { 0 }
    );

    let apple_script = r#"
on run argv
    set commandText to item 1 of argv
    do shell script commandText with administrator privileges
end run
"#;

    let output = Command::new("/usr/bin/osascript")
        .args(["-e", apple_script, "--", &shell])
        .output()
        .context("failed to request administrator permission for component repair")?;

    if !output.status.success() {
        bail!(
            "component repair failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(())
}

fn launch_full_setup_terminal() -> Result<()> {
    let setup = resources_dir()?.join("bootstrap.command");
    if !setup.exists() {
        bail!("bootstrap.command is missing from app resources");
    }

    let script = r#"
on run argv
    set setupPath to item 1 of argv
    tell application "Terminal"
        activate
        do script "/bin/bash " & quoted form of setupPath & "; exit"
    end tell
end run
"#;

    let output = Command::new("/usr/bin/osascript")
        .args(["-e", script, "--"])
        .arg(&setup)
        .output()
        .context("failed to start full dependency setup")?;

    if !output.status.success() {
        bail!(
            "failed to start dependency setup: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn set_launch_at_login(enabled: bool) -> Result<()> {
    let settings = Settings::set_launch_at_login(enabled)?;
    let home = env::var_os("HOME").context("HOME is not set")?;
    let launch_agents = PathBuf::from(home).join("Library").join("LaunchAgents");
    let target = launch_agents.join("dev.step2.ntfs-manager.menubar.plist");

    fs::create_dir_all(&launch_agents)?;

    if !settings.launch_at_login {
        if target.exists() {
            fs::remove_file(&target)
                .with_context(|| format!("failed to remove {}", target.display()))?;
        }
        return Ok(());
    }

    let template = resources_dir()?
        .join("launchd")
        .join("dev.step2.ntfs-manager.menubar.plist");
    let contents = fs::read_to_string(&template)
        .with_context(|| format!("failed to read {}", template.display()))?;
    let home = env::var("HOME").context("HOME is not set")?;
    fs::write(&target, contents.replace("__HOME__", &home))
        .with_context(|| format!("failed to write {}", target.display()))?;

    Ok(())
}

fn current_app_path() -> Result<PathBuf> {
    let executable = env::current_exe().context("failed to locate current executable")?;
    let macos_dir = executable.parent().context("invalid app executable path")?;
    let contents_dir = macos_dir.parent().context("invalid app Contents path")?;
    let app = contents_dir.parent().context("invalid app bundle path")?;

    Ok(app.to_path_buf())
}

fn is_installed_in_applications() -> bool {
    current_app_path()
        .map(|path| path == Path::new(APPLICATION_PATH))
        .unwrap_or(false)
}

fn move_app_to_applications(source: &Path) -> Result<()> {
    let destination = Path::new(APPLICATION_PATH);

    if source == destination {
        return Ok(());
    }

    if destination.exists() {
        match fs::remove_dir_all(destination) {
            Ok(()) => {}
            Err(_) => {
                privileged_copy_app(source, destination)?;
                open_installed_app(destination)?;
                return Ok(());
            }
        }
    }

    if fs::rename(source, destination).is_err() {
        let output = Command::new("/usr/bin/ditto")
            .arg(source)
            .arg(destination)
            .output()
            .context("failed to copy app with ditto")?;

        if !output.status.success() {
            privileged_copy_app(source, destination)?;
        } else {
            let _ = fs::remove_dir_all(source);
        }
    }

    verify_app_bundle(destination)?;
    open_installed_app(destination)?;

    Ok(())
}

fn privileged_copy_app(source: &Path, destination: &Path) -> Result<()> {
    let script = r#"
on run argv
    set sourceApp to item 1 of argv
    set destinationApp to item 2 of argv
    do shell script "/bin/rm -rf " & quoted form of destinationApp & " && /usr/bin/ditto " & quoted form of sourceApp & " " & quoted form of destinationApp with administrator privileges
end run
"#;

    let output = Command::new("/usr/bin/osascript")
        .args(["-e", script, "--"])
        .arg(source)
        .arg(destination)
        .output()
        .context("failed to request administrator permission")?;

    if !output.status.success() {
        bail!(
            "administrator copy failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    verify_app_bundle(destination)
}

fn verify_app_bundle(app: &Path) -> Result<()> {
    let output = Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(app)
        .output()
        .context("failed to verify copied app")?;

    if !output.status.success() {
        bail!(
            "copied app failed code-signature verification: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(())
}

fn open_installed_app(app: &Path) -> Result<()> {
    let output = Command::new("/usr/bin/open")
        .arg("-n")
        .arg(app)
        .output()
        .context("failed to launch app from Applications")?;

    if !output.status.success() {
        bail!(
            "failed to open installed app: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(())
}

fn tray_template_icon() -> Result<Icon> {
    const WIDTH: u32 = 18;
    const HEIGHT: u32 = 18;

    let mut rgba = vec![0_u8; (WIDTH * HEIGHT * 4) as usize];

    let mut set_alpha = |x: u32, y: u32, alpha: u8| {
        let index = ((y * WIDTH + x) * 4) as usize;
        rgba[index] = 255;
        rgba[index + 1] = 255;
        rgba[index + 2] = 255;
        rgba[index + 3] = alpha;
    };

    for y in 2..15 {
        let inset = if y < 5 {
            2
        } else if y < 9 {
            1
        } else {
            0
        };
        let left = 3 + inset;
        let right = 14 - inset;

        for x in left..=right {
            set_alpha(x, y, 255);
        }
    }

    for y in 7..10 {
        for x in 6..12 {
            set_alpha(x, y, 0);
        }
    }

    for x in 5..13 {
        set_alpha(x, 13, 0);
    }

    set_alpha(4, 15, 210);
    set_alpha(5, 15, 255);
    set_alpha(6, 15, 255);
    set_alpha(7, 15, 255);
    set_alpha(8, 15, 255);
    set_alpha(9, 15, 255);
    set_alpha(10, 15, 255);
    set_alpha(11, 15, 255);
    set_alpha(12, 15, 255);
    set_alpha(13, 15, 210);

    Icon::from_rgba(rgba, WIDTH, HEIGHT).context("failed to create tray template icon")
}

fn main() -> Result<()> {
    platform::require_macos()?;

    let mut builder = EventLoop::<UserEvent>::with_user_event();
    builder
        .with_activation_policy(ActivationPolicy::Accessory)
        .with_default_menu(false)
        .with_activate_ignoring_other_apps(false);

    let event_loop = builder.build()?;

    let tray_proxy = event_loop.create_proxy();
    TrayIconEvent::set_event_handler(Some(move |event| {
        let _ = tray_proxy.send_event(UserEvent::Tray(event));
    }));

    let tick_proxy = event_loop.create_proxy();
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(3));
        if tick_proxy.send_event(UserEvent::Tick).is_err() {
            break;
        }
    });

    let app_proxy = event_loop.create_proxy();
    let mut app = App::new(app_proxy);
    event_loop.run_app(&mut app)?;

    Ok(())
}
