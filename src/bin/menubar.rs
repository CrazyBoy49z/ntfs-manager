use std::{
    path::PathBuf,
    process::Command,
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use ntfs_manager::{
    disk::{DiskService, NtfsVolume},
    helper_client::HelperClient,
    platform,
    settings::Settings,
};
use tray_icon::{
    menu::{MenuEvent, MenuItem, PredefinedMenuItem, SubmenuBuilder},
    TrayIcon, TrayIconBuilder,
};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS},
    window::WindowId,
};

#[derive(Debug)]
enum UserEvent {
    Menu(MenuEvent),
    Tick,
    Snapshot {
        volumes: std::result::Result<Vec<NtfsVolume>, String>,
        helper_version: Option<String>,
    },
    ActionCompleted {
        label: &'static str,
        errors: Vec<String>,
    },
}

struct MenuState {
    status: MenuItem,
    auto_mount: MenuItem,
    mount_all: MenuItem,
    unmount_all: MenuItem,
    open_first: MenuItem,
    refresh: MenuItem,
    install_repair: MenuItem,
    quit: MenuItem,
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
        let status = MenuItem::new("Starting…", false, None);
        let auto_mount = MenuItem::new("Auto-mount: loading…", true, None);
        let mount_all = MenuItem::new("Mount all read/write", false, None);
        let unmount_all = MenuItem::new("Unmount all", false, None);
        let open_first = MenuItem::new("Open first mounted volume", false, None);
        let refresh = MenuItem::new("Refresh", true, None);
        let install_repair = MenuItem::new("Install / Repair Components", true, None);
        let quit = MenuItem::new("Quit NTFS Manager", true, None);

        let separator1 = PredefinedMenuItem::separator();
        let separator2 = PredefinedMenuItem::separator();
        let separator3 = PredefinedMenuItem::separator();

        let menu = SubmenuBuilder::new()
            .text("NTFS Manager")
            .enabled(true)
            .items(&[
                &status,
                &separator1,
                &auto_mount,
                &separator2,
                &mount_all,
                &unmount_all,
                &open_first,
                &refresh,
                &separator3,
                &install_repair,
                &quit,
            ])
            .build()
            .context("failed to build menu")?;

        let tray = TrayIconBuilder::new()
            .with_title("NTFS")
            .with_tooltip("NTFS Manager")
            .with_menu(Box::new(menu))
            .build()
            .context("failed to create menu-bar item")?;

        self.menu = Some(MenuState {
            status,
            auto_mount,
            mount_all,
            unmount_all,
            open_first,
            refresh,
            install_repair,
            quit,
        });
        self.tray = Some(tray);

        self.request_refresh();

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
            Ok(volumes) => {
                self.volumes = volumes;
            }
            Err(err) => {
                if let Some(menu) = self.menu.as_ref() {
                    menu.refresh.set_enabled(true);
                    menu.status.set_text(format!("Scan error: {err}"));
                }
                return;
            }
        }

        let helper_current = helper_version.as_deref() == Some(env!("CARGO_PKG_VERSION"));

        if !helper_current && !self.setup_launched {
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

        let settings = Settings::load().unwrap_or_default();
        let total = self.volumes.len();
        let writable = self.volumes.iter().filter(|volume| volume.writable).count();
        let mounted = self.volumes.iter().filter(|volume| volume.mounted).count();

        let default_status = if helper_current {
            if total == 0 {
                "No NTFS volumes".to_string()
            } else {
                format!("{total} NTFS · {mounted} mounted · {writable} read/write")
            }
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
            None => default_status.clone(),
        };

        let Some(menu) = self.menu.as_ref() else {
            return;
        };

        menu.refresh.set_enabled(true);
        menu.status.set_text(&status);
        menu.auto_mount.set_text(if settings.auto_mount {
            "Auto-mount: On"
        } else {
            "Auto-mount: Off"
        });
        menu.open_first.set_enabled(
            self.volumes
                .iter()
                .any(|volume| volume.mount_point.is_some()),
        );
        menu.mount_all.set_enabled(
            helper_current
                && !self.action_in_flight
                && self.volumes.iter().any(|volume| !volume.writable),
        );
        menu.unmount_all.set_enabled(
            helper_current
                && !self.action_in_flight
                && self.volumes.iter().any(|volume| volume.mounted),
        );

        if let Some(tray) = self.tray.as_ref() {
            let _ = tray.set_tooltip(Some(&status));
            tray.set_title(Some(if helper_current && total > 0 {
                "NTFS•"
            } else if helper_current {
                "NTFS"
            } else {
                "NTFS…"
            }));
        }
    }

    fn launch_setup(&self) -> Result<()> {
        let setup = setup_script_path()?;

        if !setup.exists() {
            anyhow::bail!("bootstrap.command is missing from app resources");
        }

        let app = current_app_path()?;
        if app != std::path::Path::new("/Applications/NTFS Manager.app") {
            anyhow::bail!("move NTFS Manager.app to /Applications first");
        }

        Command::new("/usr/bin/open")
            .arg(&setup)
            .spawn()
            .context("failed to open first-run setup in Terminal")?;

        Ok(())
    }

    fn start_mount_all(&mut self) {
        if self.action_in_flight {
            return;
        }

        let targets = self
            .volumes
            .iter()
            .filter(|volume| !volume.writable)
            .map(|volume| volume.device.clone())
            .collect::<Vec<_>>();

        if targets.is_empty() {
            return;
        }

        self.action_in_flight = true;
        if let Some(menu) = self.menu.as_ref() {
            menu.status.set_text("Mounting…");
            menu.mount_all.set_enabled(false);
            menu.unmount_all.set_enabled(false);
        }

        let helper = self.helper.clone();
        let proxy = self.proxy.clone();

        thread::spawn(move || {
            let mut errors = Vec::new();

            for device in targets {
                if let Err(err) = helper.mount(&device, None) {
                    errors.push(format!("{device}: {err}"));
                }
            }

            let _ = proxy.send_event(UserEvent::ActionCompleted {
                label: "Mount",
                errors,
            });
        });
    }

    fn start_unmount_all(&mut self) {
        if self.action_in_flight {
            return;
        }

        let targets = self
            .volumes
            .iter()
            .filter(|volume| volume.mounted)
            .map(|volume| volume.device.clone())
            .collect::<Vec<_>>();

        if targets.is_empty() {
            return;
        }

        self.action_in_flight = true;
        if let Some(menu) = self.menu.as_ref() {
            menu.status.set_text("Unmounting…");
            menu.mount_all.set_enabled(false);
            menu.unmount_all.set_enabled(false);
        }

        let helper = self.helper.clone();
        let proxy = self.proxy.clone();

        thread::spawn(move || {
            let mut errors = Vec::new();

            for device in targets {
                if let Err(err) = helper.unmount(&device) {
                    errors.push(format!("{device}: {err}"));
                }
            }

            let _ = proxy.send_event(UserEvent::ActionCompleted {
                label: "Unmount",
                errors,
            });
        });
    }

    fn finish_action(&mut self, label: &'static str, errors: Vec<String>) {
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

    fn handle_menu(&mut self, event_loop: &ActiveEventLoop, event: MenuEvent) {
        let (
            is_quit,
            is_refresh,
            is_install_repair,
            is_auto_mount,
            is_mount_all,
            is_unmount_all,
            is_open_first,
        ) = {
            let Some(menu) = self.menu.as_ref() else {
                return;
            };

            (
                event.id() == menu.quit.id(),
                event.id() == menu.refresh.id(),
                event.id() == menu.install_repair.id(),
                event.id() == menu.auto_mount.id(),
                event.id() == menu.mount_all.id(),
                event.id() == menu.unmount_all.id(),
                event.id() == menu.open_first.id(),
            )
        };

        if is_quit {
            event_loop.exit();
            return;
        }

        if is_refresh {
            self.request_refresh();
            return;
        }

        if is_install_repair {
            match self.launch_setup() {
                Ok(()) => {
                    self.setup_launched = true;
                    if let Some(menu) = self.menu.as_ref() {
                        menu.status.set_text("Setup opened in Terminal");
                    }
                }
                Err(err) => {
                    if let Some(menu) = self.menu.as_ref() {
                        menu.status.set_text(format!("Setup error: {err}"));
                    }
                }
            }
            return;
        }

        if is_auto_mount {
            let settings = Settings::load().unwrap_or_default();
            match Settings::set_auto_mount(!settings.auto_mount) {
                Ok(_) => self.request_refresh(),
                Err(err) => {
                    if let Some(menu) = self.menu.as_ref() {
                        menu.status.set_text(format!("Settings error: {err}"));
                    }
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
            return;
        }

        if is_open_first {
            if let Some(path) = self
                .volumes
                .iter()
                .filter_map(|volume| volume.mount_point.as_deref())
                .next()
            {
                if let Err(err) = Command::new("/usr/bin/open").arg(path).spawn() {
                    if let Some(menu) = self.menu.as_ref() {
                        menu.status.set_text(format!("Finder error: {err}"));
                    }
                }
            }
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

fn current_app_path() -> Result<PathBuf> {
    let executable = std::env::current_exe().context("failed to locate current executable")?;
    let macos_dir = executable.parent().context("invalid app executable path")?;
    let contents_dir = macos_dir.parent().context("invalid app Contents path")?;
    let app = contents_dir.parent().context("invalid app bundle path")?;

    Ok(app.to_path_buf())
}

fn setup_script_path() -> Result<PathBuf> {
    let app = current_app_path()?;
    Ok(app
        .join("Contents")
        .join("Resources")
        .join("bootstrap.command"))
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
