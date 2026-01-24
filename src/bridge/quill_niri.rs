use std::sync::Mutex;

use crate::ebc;
use anyhow::{Context, Result, bail};
use niri_ipc::{Event, Request, Response, WindowGeometry, socket::Socket};
use quill_data_provider_lib::{EinkWindowSetting, load_settings};
use tokio::sync::{
    mpsc::{self, Sender},
    oneshot,
};

const QUILL_NIRI_BRIDGE: &str = "Quill niri";

pub struct QuillNiriBridge {
    settings: Vec<EinkWindowSetting>,
}

impl QuillNiriBridge {
    const OUTPUT_NAME: &str = "DPI-1";

    pub async fn new() -> Result<Self> {
        let mut settings: Vec<EinkWindowSetting> = Vec::new();
        match load_settings() {
            Ok(loaded_settings) => settings = loaded_settings,
            Err(err) => eprintln!("Failed to load settings: {:?}", err),
        }
        let bridge = Self { settings };
        Ok(bridge)
    }

    pub async fn main_manage(&mut self, tx: &Sender<ebc::Command>) {
        let mut socket = get_socket().await;
        println!("Requesting windows");

        let Some(windows_regular) = try_fetch(&mut socket, Request::Windows, |r| match r {
            Response::Windows(w) => Some(w),
            _ => None,
        }) else {
            return;
        };
        println!("Windows are: {:#?}", windows_regular);

        let Some(workspaces) = try_fetch(&mut socket, Request::Workspaces, |r| match r {
            Response::Workspaces(w) => Some(w),
            _ => None,
        }) else {
            return;
        };
        println!("Workspaces are: {:#?}", windows_regular);
        let focused_workspace_id = workspaces
            .iter()
            .find(|ws| ws.is_focused)
            .map(|ws| ws.id)
            .unwrap_or(0);

        let Some(outputs) = try_fetch(&mut socket, Request::Outputs, |r| match r {
            Response::Outputs(w) => Some(w),
            _ => None,
        }) else {
            return;
        };
        println!("Outputs are: {:#?}", outputs);
        let (screen_w, screen_h) = {
            let output = match outputs.get(Self::OUTPUT_NAME) {
                Some(o) => o,
                None => {
                    eprintln!("Error: Output '{}' not found.", Self::OUTPUT_NAME);
                    return;
                }
            };

            let mode_idx = match output.current_mode {
                Some(idx) => idx,
                None => {
                    eprintln!("Error: No current mode set for output.");
                    return;
                }
            };

            match output.modes.get(mode_idx) {
                Some(mode) => (mode.width as i32, mode.height as i32),
                None => {
                    eprintln!("Error: Current mode index {} is out of bounds.", mode_idx);
                    return;
                }
            }
        };
        
        let Some(windows_geometries) =
            try_fetch(&mut socket, Request::WindowGeometries, |r| match r {
                Response::WindowGeometries(w) => Some(w),
                _ => None,
            })
        else {
            return;
        };
        println!("Windows geometries are: {:#?}", windows_geometries);

        let mut new_niri_windows: Vec<NiriWindows> = Vec::new();

        // Only those with settings attached to them
        for window in windows_regular {
            if let Some(app_id) = window.app_id {
                for setting in self.settings.iter() {
                    if app_id == setting.app_id {
                        // Make sure it's on the same workspace
                        match window.workspace_id {
                            Some(id) => {
                                if id != focused_workspace_id {
                                    continue;
                                }
                            }
                            None => {
                                // Not sure
                                continue;
                            }
                        }
                        // Find the geometry for it now
                        for geometry in windows_geometries.iter() {
                            if geometry.id == window.id {
                                new_niri_windows.push(NiriWindows {
                                    app_id: app_id.clone(),
                                    setting: setting.clone(),
                                    geometry: geometry.clone(),
                                });
                            }
                        }
                    }
                }
            }
        }
        println!("New niri windows are: {:?}", new_niri_windows);
        cleanup_and_position_windows(&mut new_niri_windows, screen_w, screen_h);

        // Now, remove windows that are not visible right now
        println!("Main manage exit");
    }

    pub async fn run(mut self, tx: Sender<ebc::Command>) -> Result<()> {
        println!("Bridge niri started");
        let mut socket = get_socket().await;

        let reply = socket.send(Request::EventStream).unwrap();
        if matches!(reply, Ok(Response::Handled)) {
            let mut read_event = socket.read_events();

            while let Ok(event) = read_event() {
                match event {
                    Event::WindowsChanged { .. } => {
                        println!("Trigger: WindowsChanged");
                        self.main_manage(&tx).await;
                    }
                    Event::WindowOpenedOrChanged { .. } => {
                        println!("Trigger: WindowOpenedOrChanged");
                        self.main_manage(&tx).await;
                    }
                    Event::WindowClosed { .. } => {
                        println!("Trigger: WindowClosed");
                        self.main_manage(&tx).await;
                    }
                    Event::WindowLayoutsChanged { .. } => {
                        println!("Trigger: WindowLayoutsChanged");
                        self.main_manage(&tx).await;
                    }
                    Event::WindowFocusChanged { .. } => {
                        println!("Trigger: WindowFocusChanged");
                        self.main_manage(&tx).await;
                    }
                    Event::WorkspaceActivated { .. } => {
                        println!("Trigger: WorkspaceActivated");
                        self.main_manage(&tx).await;
                    }
                    _ => {
                        println!("Received event: {:?}", event);
                    }
                }
            }
        }

        eprintln!("Quill niri bridge ended");
        Ok(())
    }
}

pub async fn start(tx: mpsc::Sender<ebc::Command>) -> Result<String> {
    let quill_niri_bridge = QuillNiriBridge::new()
        .await
        .context("While trying to start Quill niri bridge")?;

    tokio::spawn(async move {
        let _ = quill_niri_bridge.run(tx).await;
    });

    Ok(QUILL_NIRI_BRIDGE.into())
}

async fn get_socket() -> Socket {
    let niri_socket_env = std::env::var("NIRI_SOCKET");
    let socket = if let Ok(niri_socket) = niri_socket_env.clone() {
        Socket::connect_to(niri_socket).unwrap()
    } else {
        Socket::connect().unwrap()
    };
    socket
}

#[derive(Clone, Debug)]
struct NiriWindows {
    app_id: String,
    setting: EinkWindowSetting,
    geometry: WindowGeometry,
}

fn cleanup_and_position_windows(
    windows: &mut Vec<NiriWindows>,
    screen_width: i32,
    screen_height: i32,
) {
    // 1. Remove windows that are completely off-screen
    windows.retain(|w| {
        let geo = &w.geometry;

        let is_completely_off = (geo.x + geo.width <= 0) ||   // Too far left
            (geo.x >= screen_width)   ||   // Too far right
            (geo.y + geo.height <= 0) ||   // Too far up
            (geo.y >= screen_height); // Too far down

        !is_completely_off // Keep if NOT completely off
    });

    // 2. Clamp the remaining windows so they are fully visible
    for window in windows.iter_mut() {
        let geo = &mut window.geometry;

        // Shrink if window is larger than screen
        geo.width = geo.width.min(screen_width);
        geo.height = geo.height.min(screen_height);

        // Clamp coordinates
        geo.x = geo.x.clamp(0, screen_width - geo.width);
        geo.y = geo.y.clamp(0, screen_height - geo.height);
    }
}

fn try_fetch<T, F>(socket: &mut Socket, req: Request, extract: F) -> Option<T>
where
    F: FnOnce(Response) -> Option<T>,
{
    match socket.send(req).unwrap() {
        Ok(res) => extract(res).or_else(|| {
            eprintln!("Error: Received unexpected response variant.");
            None
        }),
        Err(e) => {
            eprintln!("Error: Failed to get reply: {:?}", e);
            None
        }
    }
}
