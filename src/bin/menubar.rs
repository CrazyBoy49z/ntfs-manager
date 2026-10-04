use std::{process::Command, thread, time::Duration};

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
    event_loop::{ActiveEventLoop, EventLoop},
    platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS},
    window::WindowId,
};

#[derive(Debug)]
enum UserEvent {
    Menu(MenuEvent),
    Tick,
}

struct MenuState {
    status: MenuItem,
    auto_mount: MenuItem,
    mount_all: MenuItem,
    unmount_all: MenuItem,
    open_first: MenuItem,
    refresh: MenuItem,
    quit: MenuItem,
}

struct App {
    disks: DiskService,
    helper: HelperClient,
    tray: Option<TrayIcon>,
    menu: Option<MenuState>,
    volumes: Vec<NtfsVolume>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            disks: DiskService::new(),
            helper: HelperClient::new(),
            tray: None,
            menu: None,
            volumes: Vec::new(),
        }
    }
}

impl App {
    fn initialize_tray(&mut self) -> Result<()> {
        let status = MenuItem::new("Scanning NTFS volumes…", false, None);
        let auto_mount = MenuItem::new("Auto-mount: loading…", true, None);
        let mount_all = MenuItem::new("Mount all read/write", true, None);
        let unmount_all = MenuItem::new("Unmount all", true, None);
        let open_first = MenuItem::new("Open first mounted volume", true, None);
        let refresh = MenuItem::new("Refresh", true, None);
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
            quit,
        });
        self.tray = Some(tray);
        self.refresh();

        Ok(())
    }

    fn refresh(&mut self) {
        let Some(menu) = self.menu.as_ref() else {
            return;
        };

        match self.disks.ntfs_volumes() {
            Ok(volumes) => {
                self.volumes = volumes;

                let total = self.volumes.len();
                let writable = self.volumes.iter().filter(|volume| volume.writable).count();
                let mounted = self.volumes.iter().filter(|volume| volume.mounted).count();

                let status = if total == 0 {
                    "No NTFS volumes".to_string()
                } else {
                    format!("{total} NTFS · {mounted} mounted · {writable} read/write")
                };

                menu.status.set_text(&status);
                menu.mount_all
                    .set_enabled(self.volumes.iter().any(|volume| !volume.writable));
                menu.unmount_all
                    .set_enabled(self.volumes.iter().any(|volume| volume.mounted));
                menu.open_first.set_enabled(
                    self.volumes
                        .iter()
                        .any(|volume| volume.mount_point.is_some()),
                );

                let settings = Settings::load().unwrap_or_default();
                menu.auto_mount.set_text(if settings.auto_mount {
                    "Auto-mount: On"
                } else {
                    "Auto-mount: Off"
                });

                if let Some(tray) = self.tray.as_ref() {
                    let _ = tray.set_tooltip(Some(&status));
                    tray.set_title(Some(if total == 0 { "NTFS" } else { "NTFS•" }));
                }
            }
            Err(err) => {
                menu.status.set_text(format!("Scan error: {err}"));
            }
        }
    }

    fn handle_menu(&mut self, event_loop: &ActiveEventLoop, event: MenuEvent) {
        let Some(menu) = self.menu.as_ref() else {
            return;
        };

        if event.id() == menu.quit.id() {
            event_loop.exit();
            return;
        }

        if event.id() == menu.refresh.id() {
            self.refresh();
            return;
        }

        if event.id() == menu.auto_mount.id() {
            let settings = Settings::load().unwrap_or_default();
            match Settings::set_auto_mount(!settings.auto_mount) {
                Ok(_) => self.refresh(),
                Err(err) => menu.status.set_text(format!("Settings error: {err}")),
            }
            return;
        }

        if event.id() == menu.mount_all.id() {
            let mut errors = Vec::new();
            let targets = self
                .volumes
                .iter()
                .filter(|volume| !volume.writable)
                .map(|volume| volume.device.clone())
                .collect::<Vec<_>>();

            for device in targets {
                if let Err(err) = self.helper.mount(&device, None) {
                    errors.push(format!("{device}: {err}"));
                }
            }

            self.refresh();
            if !errors.is_empty() {
                if let Some(menu) = self.menu.as_ref() {
                    menu.status
                        .set_text(format!("Mount failed: {}", errors.join("; ")));
                }
            }
            return;
        }

        if event.id() == menu.unmount_all.id() {
            let mut errors = Vec::new();
            let targets = self
                .volumes
                .iter()
                .filter(|volume| volume.mounted)
                .map(|volume| volume.device.clone())
                .collect::<Vec<_>>();

            for device in targets {
                if let Err(err) = self.helper.unmount(&device) {
                    errors.push(format!("{device}: {err}"));
                }
            }

            self.refresh();
            if !errors.is_empty() {
                if let Some(menu) = self.menu.as_ref() {
                    menu.status
                        .set_text(format!("Unmount failed: {}", errors.join("; ")));
                }
            }
            return;
        }

        if event.id() == menu.open_first.id() {
            if let Some(path) = self
                .volumes
                .iter()
                .filter_map(|volume| volume.mount_point.as_deref())
                .next()
            {
                if let Err(err) = Command::new("/usr/bin/open").arg(path).spawn() {
                    menu.status.set_text(format!("Finder error: {err}"));
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
            UserEvent::Tick => self.refresh(),
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

    let mut app = App::default();
    event_loop.run_app(&mut app)?;

    Ok(())
}
