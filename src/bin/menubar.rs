use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};
use ntfs_manager::{
    disk::{DiskService, NtfsVolume},
    helper_client::HelperClient,
    platform,
    settings::Settings,
};
use tray_icon::{
    menu::{CheckMenuItem, MenuEvent, MenuItem, PredefinedMenuItem, Submenu, SubmenuBuilder},
    TrayIcon, TrayIconBuilder,
};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS},
    window::WindowId,
};

const APPLICATION_PATH: &str = "/Applications/NTFS Manager.app";

#[derive(Debug)]
enum UserEvent {
    Menu(MenuEvent),
    Tick,
    Snapshot {
        volumes: std::result::Result<Vec<NtfsVolume>, String>,
        helper_version: Option<String>,
    },
    ActionCompleted {
        label: String,
        errors: Vec<String>,
    },
    MoveCompleted {
        result: std::result::Result<(), String>,
    },
}

struct VolumeMenuActions {
    device: String,
    name: String,
    mount_point: Option<String>,
    mount: MenuItem,
    open: MenuItem,
    unmount: MenuItem,
}

struct MenuState {
    auto_mount: CheckMenuItem,
    mount_all: MenuItem,
    unmount_all: MenuItem,
    refresh: MenuItem,
    move_to_applications: MenuItem,
    install_repair: MenuItem,
    open_logs: MenuItem,
    quit: MenuItem,
    volumes: Vec<VolumeMenuActions>,
}

enum VolumeAction {
    Mount { device: String, name: String },
    Open { path: String },
    Unmount { device: String, name: String },
}

struct App {
    disks: DiskService,
    helper: HelperClient,
    proxy: EventLoopProxy<UserEvent>,
    tray: Option<TrayIcon>,
    menu: Option<MenuState>,
    volumes: Vec<NtfsVolume>,
    setup_launched: bool,
    refresh_in_flight: bool,
    action_in_flight: bool,
    status_override: Option<(String, Instant)>,
}

impl App {
    fn new(proxy: EventLoopProxy<UserEvent>) -> Self {
        Self {
            disks: DiskService::new(),
            helper: HelperClient::new(),
            proxy,
            tray: None,
            menu: None,
            volumes: Vec::new(),
            setup_launched: false,
            refresh_in_flight: false,
            action_in_flight: false,
            status_override: None,
        }
    }

    fn initialize_tray(&mut self) -> Result<()> {
        let tray = TrayIconBuilder::new()
            .with_title("NTFS")
            .with_tooltip("NTFS Manager")
            .build()
            .context("failed to create menu-bar item")?;

        self.tray = Some(tray);
        self.rebuild_menu("Starting…", false)?;
        self.request_refresh();

        Ok(())
    }

    fn rebuild_menu(&mut self, status: &str, helper_current: bool) -> Result<()> {
        let settings = Settings::load().unwrap_or_default();
        let installed = is_installed_in_applications();

        let app_header = PredefinedMenuItem::section_header("NTFS Manager");
        let status_item = MenuItem::new(status, false, None);
        let auto_mount = CheckMenuItem::new(
            "Auto-mount NTFS volumes",
            helper_current && installed,
            settings.auto_mount,
            None,
        );

        let separator1 = PredefinedMenuItem::separator();
        let volumes_header = PredefinedMenuItem::section_header("Volumes");
        let empty_volume = MenuItem::new("No NTFS volumes connected", false, None);

        let mut volume_submenus = Vec::<Submenu>::new();
        let mut volume_actions = Vec::<VolumeMenuActions>::new();

        for volume in &self.volumes {
            let mounted = volume.mounted || volume.mount_point.is_some();
            let state = volume_state(volume);
            let name = volume.name.clone().unwrap_or_else(|| volume.device.clone());
            let title = format!("{name} — {state}");
            let size = volume
                .size_bytes
                .map(format_bytes)
                .unwrap_or_else(|| "Unknown size".to_string());
            let details = MenuItem::new(format!("{} · {size}", volume.device), false, None);
            let separator = PredefinedMenuItem::separator();
            let mount = MenuItem::new(
                "Mount read/write",
                helper_current && !self.action_in_flight && !volume.writable,
                None,
            );
            let open = MenuItem::new("Open in Finder", volume.mount_point.is_some(), None);
            let unmount = MenuItem::new(
                "Unmount",
                helper_current && !self.action_in_flight && mounted,
                None,
            );

            let submenu = SubmenuBuilder::new()
                .text(title)
                .enabled(true)
                .items(&[&details, &separator, &mount, &open, &unmount])
                .build()
                .context("failed to build volume submenu")?;

            volume_submenus.push(submenu);
            volume_actions.push(VolumeMenuActions {
                device: volume.device.clone(),
                name,
                mount_point: volume.mount_point.clone(),
                mount,
                open,
                unmount,
            });
        }

        let mount_all = MenuItem::new(
            "Mount all read/write",
            helper_current
                && !self.action_in_flight
                && self.volumes.len() > 1
                && self.volumes.iter().any(|volume| !volume.writable),
            None,
        );
        let unmount_all = MenuItem::new(
            "Unmount all",
            helper_current
                && !self.action_in_flight
                && self.volumes.len() > 1
                && self
                    .volumes
                    .iter()
                    .any(|volume| volume.mounted || volume.mount_point.is_some()),
            None,
        );

        let separator2 = PredefinedMenuItem::separator();
        let app_header2 = PredefinedMenuItem::section_header("Application");
        let refresh = MenuItem::new("Refresh", !self.refresh_in_flight, None);
        let move_to_applications = MenuItem::new(
            "Move to Applications…",
            !installed && !self.action_in_flight,
            None,
        );
        let install_repair = MenuItem::new(
            "Install / Repair Components…",
            installed && !self.action_in_flight,
            None,
        );
        let open_logs = MenuItem::new("Open Logs", true, None);
        let separator3 = PredefinedMenuItem::separator();
        let version = MenuItem::new(
            format!("Version {}", env!("CARGO_PKG_VERSION")),
            false,
            None,
        );
        let quit = MenuItem::new("Quit NTFS Manager", true, None);

        let mut builder = SubmenuBuilder::new()
            .text("NTFS Manager")
            .enabled(true)
            .item(&app_header)
            .item(&status_item)
            .item(&auto_mount)
            .item(&separator1)
            .item(&volumes_header);

        if volume_submenus.is_empty() {
            builder = builder.item(&empty_volume);
        } else {
            for submenu in &volume_submenus {
                builder = builder.item(submenu);
            }
        }

        if self.volumes.len() > 1 {
            builder = builder.item(&mount_all).item(&unmount_all);
        }

        builder = builder.item(&separator2).item(&app_header2).item(&refresh);

        if !installed {
            builder = builder.item(&move_to_applications);
        }

        builder = builder
            .item(&install_repair)
            .item(&open_logs)
            .item(&separator3)
            .item(&version)
            .item(&quit);

        let root = builder.build().context("failed to build tray menu")?;

        if let Some(tray) = self.tray.as_ref() {
            tray.set_menu(Some(Box::new(root)));
        }

        self.menu = Some(MenuState {
            auto_mount,
            mount_all,
            unmount_all,
            refresh,
            move_to_applications,
            install_repair,
            open_logs,
            quit,
            volumes: volume_actions,
        });

        Ok(())
    }

    fn request_refresh(&mut self) {
        if self.refresh_in_flight {
            return;
        }

        self.refresh_in_flight = true;

        if let Some(menu) = self.menu.as_ref() {
            menu.refresh.set_enabled(false);
        }

        let disks = self.disks.clone();
        let helper = self.helper.clone();
        let proxy = self.proxy.clone();

        thread::spawn(move || {
            let helper_version = helper.version().ok();
            let volumes = disks.ntfs_volumes().map_err(|err| format!("{err:#}"));

            let _ = proxy.send_event(UserEvent::Snapshot {
                volumes,
                helper_version,
            });
        });
    }

    fn apply_snapshot(
        &mut self,
        volumes: std::result::Result<Vec<NtfsVolume>, String>,
        helper_version: Option<String>,
    ) {
        self.refresh_in_flight = false;

        match volumes {
            Ok(volumes) => self.volumes = volumes,
            Err(err) => {
                self.status_override = Some((
                    format!("Scan error: {err}"),
                    Instant::now() + Duration::from_secs(30),
                ));
            }
        }

        let installed = is_installed_in_applications();
        let helper_current = helper_version.as_deref() == Some(env!("CARGO_PKG_VERSION"));

        if installed && !helper_current && !self.setup_launched {
            match self.launch_setup() {
                Ok(()) => {
                    self.setup_launched = true;
                    self.status_override = Some((
                        "Updating NTFS Manager components…".to_string(),
                        Instant::now() + Duration::from_secs(30),
                    ));
                }
                Err(err) => {
                    self.status_override = Some((
                        format!("Setup error: {err}"),
                        Instant::now() + Duration::from_secs(30),
                    ));
                }
            }
        }

        let default_status = if !installed {
            "Move to Applications to finish setup".to_string()
        } else if helper_current {
            summary_status(&self.volumes)
        } else if let Some(version) = helper_version.as_deref() {
            format!("Updating helper {version} → {}…", env!("CARGO_PKG_VERSION"))
        } else if self.setup_launched {
            "Installing components…".to_string()
        } else {
            "Setup required".to_string()
        };

        let status = match self.status_override.as_ref() {
            Some((message, until)) if Instant::now() < *until => message.clone(),
            Some(_) => {
                self.status_override = None;
                default_status.clone()
            }
            None => default_status,
        };

        if let Err(err) = self.rebuild_menu(&status, helper_current) {
            eprintln!("failed to rebuild tray menu: {err:#}");
        }

        if let Some(tray) = self.tray.as_ref() {
            let _ = tray.set_tooltip(Some(&status));
            tray.set_title(Some(if !installed || !helper_current {
                "NTFS…"
            } else if self.volumes.is_empty() {
                "NTFS"
            } else if self.volumes.iter().any(|volume| volume.writable) {
                "NTFS●"
            } else {
                "NTFS•"
            }));
        }
    }

    fn launch_setup(&self) -> Result<()> {
        let setup = setup_script_path()?;

        if !setup.exists() {
            bail!("bootstrap.command is missing from app resources");
        }

        if !is_installed_in_applications() {
            bail!("move NTFS Manager.app to /Applications first");
        }

        Command::new("/usr/bin/open")
            .arg(&setup)
            .spawn()
            .context("failed to open first-run setup in Terminal")?;

        Ok(())
    }

    fn start_mount_devices(&mut self, targets: Vec<String>, label: String) {
        if self.action_in_flight || targets.is_empty() {
            return;
        }

        self.action_in_flight = true;
        self.status_override = Some((
            format!("{label}…"),
            Instant::now() + Duration::from_secs(30),
        ));

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

        self.request_refresh();
    }

    fn start_unmount_devices(&mut self, targets: Vec<String>, label: String) {
        if self.action_in_flight || targets.is_empty() {
            return;
        }

        self.action_in_flight = true;
        self.status_override = Some((
            format!("{label}…"),
            Instant::now() + Duration::from_secs(30),
        ));

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

        self.request_refresh();
    }

    fn start_mount_all(&mut self) {
        let targets = self
            .volumes
            .iter()
            .filter(|volume| !volume.writable)
            .map(|volume| volume.device.clone())
            .collect::<Vec<_>>();

        self.start_mount_devices(targets, "Mount all".to_string());
    }

    fn start_unmount_all(&mut self) {
        let targets = self
            .volumes
            .iter()
            .filter(|volume| volume.mounted || volume.mount_point.is_some())
            .map(|volume| volume.device.clone())
            .collect::<Vec<_>>();

        self.start_unmount_devices(targets, "Unmount all".to_string());
    }

    fn finish_action(&mut self, label: String, errors: Vec<String>) {
        self.action_in_flight = false;

        if errors.is_empty() {
            self.status_override = Some((
                format!("{label} completed"),
                Instant::now() + Duration::from_secs(5),
            ));
        } else {
            let full_error = errors.join("; ");
            eprintln!("{label} failed: {full_error}");

            let short_error = full_error.chars().take(180).collect::<String>();
            self.status_override = Some((
                format!("{label} failed: {short_error}"),
                Instant::now() + Duration::from_secs(30),
            ));
        }

        self.request_refresh();
    }

    fn start_move_to_applications(&mut self) {
        if self.action_in_flight || is_installed_in_applications() {
            return;
        }

        let source = match current_app_path() {
            Ok(path) => path,
            Err(err) => {
                self.status_override = Some((
                    format!("Move failed: {err}"),
                    Instant::now() + Duration::from_secs(30),
                ));
                self.request_refresh();
                return;
            }
        };

        self.action_in_flight = true;
        self.status_override = Some((
            "Moving NTFS Manager to Applications…".to_string(),
            Instant::now() + Duration::from_secs(30),
        ));

        let proxy = self.proxy.clone();
        thread::spawn(move || {
            let result = move_app_to_applications(&source).map_err(|err| format!("{err:#}"));
            let _ = proxy.send_event(UserEvent::MoveCompleted { result });
        });

        self.request_refresh();
    }

    fn open_logs(&self) {
        let Some(home) = std::env::var_os("HOME") else {
            return;
        };

        let logs = PathBuf::from(home)
            .join("Library")
            .join("Logs")
            .join("NTFS Manager");
        let _ = fs::create_dir_all(&logs);
        let _ = Command::new("/usr/bin/open").arg(logs).spawn();
    }

    fn handle_menu(&mut self, event_loop: &ActiveEventLoop, event: MenuEvent) {
        let (
            is_quit,
            is_refresh,
            is_move,
            is_install_repair,
            is_open_logs,
            is_auto_mount,
            is_mount_all,
            is_unmount_all,
            volume_action,
        ) = {
            let Some(menu) = self.menu.as_ref() else {
                return;
            };

            let volume_action = menu.volumes.iter().find_map(|action| {
                if event.id() == action.mount.id() {
                    Some(VolumeAction::Mount {
                        device: action.device.clone(),
                        name: action.name.clone(),
                    })
                } else if event.id() == action.open.id() {
                    action
                        .mount_point
                        .clone()
                        .map(|path| VolumeAction::Open { path })
                } else if event.id() == action.unmount.id() {
                    Some(VolumeAction::Unmount {
                        device: action.device.clone(),
                        name: action.name.clone(),
                    })
                } else {
                    None
                }
            });

            (
                event.id() == menu.quit.id(),
                event.id() == menu.refresh.id(),
                event.id() == menu.move_to_applications.id(),
                event.id() == menu.install_repair.id(),
                event.id() == menu.open_logs.id(),
                event.id() == menu.auto_mount.id(),
                event.id() == menu.mount_all.id(),
                event.id() == menu.unmount_all.id(),
                volume_action,
            )
        };

        if is_quit {
            event_loop.exit();
            return;
        }

        if let Some(action) = volume_action {
            match action {
                VolumeAction::Mount { device, name } => {
                    self.start_mount_devices(vec![device], format!("Mount {name}"));
                }
                VolumeAction::Open { path } => {
                    if let Err(err) = Command::new("/usr/bin/open").arg(path).spawn() {
                        self.status_override = Some((
                            format!("Finder error: {err}"),
                            Instant::now() + Duration::from_secs(30),
                        ));
                        self.request_refresh();
                    }
                }
                VolumeAction::Unmount { device, name } => {
                    self.start_unmount_devices(vec![device], format!("Unmount {name}"));
                }
            }
            return;
        }

        if is_refresh {
            self.request_refresh();
            return;
        }

        if is_move {
            self.start_move_to_applications();
            return;
        }

        if is_install_repair {
            match self.launch_setup() {
                Ok(()) => {
                    self.setup_launched = true;
                    self.status_override = Some((
                        "Setup opened in Terminal".to_string(),
                        Instant::now() + Duration::from_secs(10),
                    ));
                }
                Err(err) => {
                    self.status_override = Some((
                        format!("Setup error: {err}"),
                        Instant::now() + Duration::from_secs(30),
                    ));
                }
            }
            self.request_refresh();
            return;
        }

        if is_open_logs {
            self.open_logs();
            return;
        }

        if is_auto_mount {
            let settings = Settings::load().unwrap_or_default();
            match Settings::set_auto_mount(!settings.auto_mount) {
                Ok(_) => self.request_refresh(),
                Err(err) => {
                    self.status_override = Some((
                        format!("Settings error: {err}"),
                        Instant::now() + Duration::from_secs(30),
                    ));
                    self.request_refresh();
                }
            }
            return;
        }

        if is_mount_all {
            self.start_mount_all();
            return;
        }

        if is_unmount_all {
            self.start_unmount_all();
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.tray.is_none() {
            if let Err(err) = self.initialize_tray() {
                eprintln!("failed to initialize NTFS Manager menu bar: {err:#}");
                event_loop.exit();
            }
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Menu(event) => self.handle_menu(event_loop, event),
            UserEvent::Tick => self.request_refresh(),
            UserEvent::Snapshot {
                volumes,
                helper_version,
            } => self.apply_snapshot(volumes, helper_version),
            UserEvent::ActionCompleted { label, errors } => {
                self.finish_action(label, errors);
            }
            UserEvent::MoveCompleted { result } => match result {
                Ok(()) => event_loop.exit(),
                Err(err) => {
                    self.action_in_flight = false;
                    self.status_override = Some((
                        format!("Move failed: {err}"),
                        Instant::now() + Duration::from_secs(30),
                    ));
                    self.request_refresh();
                }
            },
        }
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        _event: WindowEvent,
    ) {
    }
}

fn summary_status(volumes: &[NtfsVolume]) -> String {
    match volumes {
        [] => "No NTFS volumes connected".to_string(),
        [volume] => {
            let name = volume.name.as_deref().unwrap_or(&volume.device);
            format!("{name} · {}", volume_state(volume))
        }
        volumes => {
            let writable = volumes.iter().filter(|volume| volume.writable).count();
            let readonly = volumes
                .iter()
                .filter(|volume| {
                    !volume.writable && (volume.mounted || volume.mount_point.is_some())
                })
                .count();
            let unmounted = volumes.len().saturating_sub(writable + readonly);

            let mut parts = vec![format!("{} NTFS volumes", volumes.len())];
            if writable > 0 {
                parts.push(format!("{writable} read/write"));
            }
            if readonly > 0 {
                parts.push(format!("{readonly} read-only"));
            }
            if unmounted > 0 {
                parts.push(format!("{unmounted} unmounted"));
            }

            parts.join(" · ")
        }
    }
}

fn volume_state(volume: &NtfsVolume) -> &'static str {
    if volume.writable {
        "Read/write"
    } else if volume.mounted || volume.mount_point.is_some() {
        "Read-only"
    } else {
        "Not mounted"
    }
}

fn format_bytes(bytes: u64) -> String {
    const GB: f64 = 1_000_000_000.0;
    const TB: f64 = 1_000_000_000_000.0;

    if bytes as f64 >= TB {
        format!("{:.1} TB", bytes as f64 / TB)
    } else {
        format!("{:.0} GB", bytes as f64 / GB)
    }
}

fn current_app_path() -> Result<PathBuf> {
    let executable = std::env::current_exe().context("failed to locate current executable")?;
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

fn setup_script_path() -> Result<PathBuf> {
    let app = current_app_path()?;
    Ok(app
        .join("Contents")
        .join("Resources")
        .join("bootstrap.command"))
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

fn main() -> Result<()> {
    platform::require_macos()?;

    let mut builder = EventLoop::<UserEvent>::with_user_event();
    builder
        .with_activation_policy(ActivationPolicy::Accessory)
        .with_default_menu(false)
        .with_activate_ignoring_other_apps(false);

    let event_loop = builder.build()?;

    let menu_proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |event| {
        let _ = menu_proxy.send_event(UserEvent::Menu(event));
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
