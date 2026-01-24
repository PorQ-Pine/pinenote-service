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
        // println!("Requesting windows");

        let Some(windows_regular) = try_fetch(&mut socket, Request::Windows, |r| match r {
            Response::Windows(w) => Some(w),
            _ => None,
        }) else {
            return;
        };
        println!("Windows are: {:?}", windows_regular);

        let Some(workspaces) = try_fetch(&mut socket, Request::Workspaces, |r| match r {
            Response::Workspaces(w) => Some(w),
            _ => None,
        }) else {
            return;
        };
        println!("Workspaces are: {:?}", windows_regular);
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
        // println!("Outputs are: {:#?}", outputs);
        let (screen_w, screen_h) = {
            let output = match outputs.get(Self::OUTPUT_NAME) {
                Some(o) => o,
                None => {
                    eprintln!("Error: Output '{}' not found.", Self::OUTPUT_NAME);
                    return;
                }
            };

            match output.logical {
                Some(logical_mode) => (logical_mode.width as i32, logical_mode.height as i32),
                None => {
                    eprintln!("No logical screen info");
                    return;
                }
            }
        };
        println!("Screen size is: {}x{}", screen_w, screen_h);

        let Some(windows_geometries) =
            try_fetch(&mut socket, Request::WindowGeometries, |r| match r {
                Response::WindowGeometries(w) => Some(w),
                _ => None,
            })
        else {
            return;
        };
        println!("Windows geometries are: {:?}", windows_geometries);

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

        // TODO: remove floating windows from this, or maybe not???
        // Clip windows that are on screen above 10px
        new_niri_windows.sort_by_key(|w| w.geometry.x);
        windows_on_screen(&mut new_niri_windows, screen_w, screen_h);

        println!("New niri windows are: {:#?}", new_niri_windows);

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
                        // println!("Received event: {:?}", event);
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

fn windows_on_screen(windows: &mut Vec<NiriWindows>, screen_width: i32, screen_height: i32) {
    windows.retain_mut(|w| {
        let gx = w.geometry.x;
        let gy = w.geometry.y;
        let gw = w.geometry.width;
        let gh = w.geometry.height;

        let left = gx.max(0);
        let top = gy.max(0);
        let right = (gx + gw).min(screen_width);
        let bottom = (gy + gh).min(screen_height);

        let width = right - left;
        let height = bottom - top;

        if left < right && top < bottom && width >= 10 && height >= 10 {
            w.geometry.x = left;
            w.geometry.y = top;
            w.geometry.width = right - left;
            w.geometry.height = bottom - top;
            true
        } else {
            false
        }
    });
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
