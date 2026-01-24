use crate::ebc::{self, CommandSender};
use anyhow::{Context, Result};
use niri_ipc::{Event, Request, Response, WindowGeometry, socket::Socket};
use nix::libc::pid_t;
use pinenote_service::types::{Rect, rockchip_ebc::Hint};
use quill_data_provider_lib::{
    Dithering, EinkWindowSetting, RedrawOptions, TresholdLevel, load_settings,
};
use tokio::sync::{
    mpsc::{self, Sender},
    oneshot,
};

use std::collections::{HashMap, HashSet};

const QUILL_NIRI_BRIDGE: &str = "Quill niri";

pub struct QuillNiriBridge {
    settings: Vec<EinkWindowSetting>,
    app_meta: HashMap<pid_t, (String, HashSet<i64>)>,
    window_meta: HashMap<i64, (String, NiriWindows)>,
}

impl QuillNiriBridge {
    const OUTPUT_NAME: &str = "DPI-1";

    pub async fn new() -> Result<Self> {
        let mut settings: Vec<EinkWindowSetting> = Vec::new();
        match load_settings() {
            Ok(loaded_settings) => settings = loaded_settings,
            Err(err) => eprintln!("Failed to load settings: {:?}", err),
        }
        let bridge = Self {
            settings,
            app_meta: HashMap::new(),
            window_meta: HashMap::new(),
        };
        Ok(bridge)
    }

    async fn remove_all(&mut self, tx: &mut ebc::CommandSender) -> Result<()> {
        let app_keys: Vec<String> = self.app_meta.values().map(|(key, _)| key.clone()).collect();
        for key in app_keys {
            tx.send(ebc::command::Application::Remove(key)).await.ok();
        }
        self.app_meta.clear();
        self.window_meta.clear();
        Ok(())
    }

    async fn add_app(&mut self, pid: pid_t, tx: &mut ebc::CommandSender) -> Result<String> {
        let (ret_tx, ret_rx) = oneshot::channel::<String>();
        let app_key = tx
            .with_reply(ebc::command::Application::Add(pid, ret_tx), ret_rx)
            .await
            .context(format!("Failed to add application with PID {}", pid))?;

        self.app_meta
            .insert(pid, (app_key.clone(), Default::default()));

        Ok(app_key)
    }

    async fn add_window(
        &mut self,
        win: NiriWindows,
        app_key: String,
        tx: &mut ebc::CommandSender,
    ) -> Result<()> {
        let (rtx, rx) = oneshot::channel::<String>();

        let cmd = ebc::command::Window::Add {
            app_key: app_key.clone(),
            title: win.title.clone(),
            area: Rect::from_xywh(
                win.geometry.x,
                win.geometry.y,
                win.geometry.width,
                win.geometry.height,
            ),
            hint: Some(setting_to_hint(&win.setting, win.focused).await),
            visible: true,
            fullscreen: false,
            z_index: 0,
            reply: rtx,
        };

        let win_key = tx
            .with_reply(cmd, rx)
            .await
            .with_context(|| format!("Failed to add window for app '{}'", win.app_id))?;

        let win_id = win.geometry.id as i64;
        let pid = win_id as pid_t;

        self.window_meta.insert(win_id, (win_key, win.clone()));
        if let Some(app) = self.app_meta.get_mut(&pid) {
            app.1.insert(win_id);
        }

        Ok(())
    }

    pub async fn main_manage(&mut self, tx: &mut ebc::CommandSender) {
        if let Err(e) = self.remove_all(tx).await {
            eprintln!("Failed to remove all apps/windows: {:?}", e);
        }

        let mut socket = get_socket().await;
        println!("Requesting windows");

        let Some(windows_regular) = try_fetch(&mut socket, Request::Windows, |r| match r {
            Response::Windows(w) => Some(w),
            _ => None,
        }) else {
            return;
        };

        let Some(workspaces) = try_fetch(&mut socket, Request::Workspaces, |r| match r {
            Response::Workspaces(w) => Some(w),
            _ => None,
        }) else {
            return;
        };
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

        let Some(windows_geometries) =
            try_fetch(&mut socket, Request::WindowGeometries, |r| match r {
                Response::WindowGeometries(w) => Some(w),
                _ => None,
            })
        else {
            return;
        };

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
                                    title: window.title.clone().unwrap_or_default(),
                                    focused: window.is_focused,
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

        for win in new_niri_windows {
            let pid = win.geometry.id as pid_t;
            match self.add_app(pid, tx).await {
                Ok(app_key) => {
                    if let Err(e) = self.add_window(win, app_key, tx).await {
                        eprintln!("Failed to add window: {:?}", e);
                    }
                }
                Err(e) => {
                    eprintln!("Failed to add app: {:?}", e);
                }
            }
        }
        println!("Main manage exit");
    }

    pub async fn run(mut self, tx: Sender<ebc::Command>) -> Result<()> {
        println!("Bridge niri started");
        let mut socket = get_socket().await;
        let mut tx: CommandSender = tx.into();

        let reply = socket.send(Request::EventStream).unwrap();
        if matches!(reply, Ok(Response::Handled)) {
            let mut read_event = socket.read_events();

            while let Ok(event) = read_event() {
                match event {
                    Event::WindowsChanged { .. } => {
                        println!("Trigger: WindowsChanged");
                        self.main_manage(&mut tx).await;
                    }
                    Event::WindowOpenedOrChanged { .. } => {
                        println!("Trigger: WindowOpenedOrChanged");
                        self.main_manage(&mut tx).await;
                    }
                    Event::WindowClosed { .. } => {
                        println!("Trigger: WindowClosed");
                        self.main_manage(&mut tx).await;
                    }
                    Event::WindowLayoutsChanged { .. } => {
                        println!("Trigger: WindowLayoutsChanged");
                        self.main_manage(&mut tx).await;
                    }
                    Event::WindowFocusChanged { .. } => {
                        println!("Trigger: WindowFocusChanged");
                        self.main_manage(&mut tx).await;
                    }
                    Event::WorkspaceActivated { .. } => {
                        println!("Trigger: WorkspaceActivated");
                        self.main_manage(&mut tx).await;
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
    title: String,
    focused: bool,
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

async fn setting_to_hint(setting: &EinkWindowSetting, focused: bool) -> Hint {
    use pinenote_service::types::rockchip_ebc::{HintBitDepth, HintConvertMode};
    use quill_data_provider_lib::{BitDepth, Conversion, DriverMode, Redraw};

    let mut treshold: TresholdLevel = Default::default();
    let mut dithering_mode: Dithering = Default::default();
    let mut redraw_options: RedrawOptions = Default::default();

    let hint = match &setting.settings {
        DriverMode::Normal(bit_depth) => match bit_depth {
            BitDepth::Y1(conv) => {
                let cm = match conv {
                    Conversion::Tresholding(level) => {
                        treshold = *level;
                        HintConvertMode::Threshold
                    }
                    Conversion::Dithering(mode) => {
                        dithering_mode = *mode;
                        HintConvertMode::Dither
                    }
                };
                Hint::new(HintBitDepth::Y1, cm, false)
            }
            BitDepth::Y2(conv, redraw) => {
                let cm = match conv {
                    Conversion::Tresholding(level) => {
                        treshold = *level;
                        HintConvertMode::Threshold
                    }
                    Conversion::Dithering(mode) => {
                        dithering_mode = *mode;
                        HintConvertMode::Dither
                    }
                };
                let r: bool = match redraw {
                    Redraw::FastDrawing(options) => {
                        redraw_options = *options;
                        true
                    }
                    Redraw::DisableFastDrawing => false,
                };
                Hint::new(HintBitDepth::Y2, cm, r)
            }
            BitDepth::Y4(redraw) => {
                let r: bool = match redraw {
                    Redraw::FastDrawing(options) => {
                        redraw_options = *options;
                        true
                    }
                    Redraw::DisableFastDrawing => false,
                };
                Hint::new(HintBitDepth::Y4, HintConvertMode::Threshold, r)
            }
        },
        DriverMode::Fast(mode) => {
            dithering_mode = *mode;
            Hint::new(HintBitDepth::Y1, HintConvertMode::Dither, false)
        }
    };

    if focused {
        treshold.set().await;
        dithering_mode.set().await;
        redraw_options.set().await;
        setting.settings.set().await; // Sets normal or fast globally based on this focused window settings
    }

    hint
}
