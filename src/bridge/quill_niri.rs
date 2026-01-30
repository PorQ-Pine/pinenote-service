use crate::ebc::{self, CommandSender};
use anyhow::{Context, Result};
use inotify::{Inotify, WatchMask};
use niri_ipc::{Event, Request, Response, WindowGeometry, socket::Socket};
use nix::libc::pid_t;
use pinenote_service::types::{Rect, rockchip_ebc::Hint};
use qoms_lib::find_session;
use quill_data_provider_lib::{
    Dithering, DriverMode, EinkWindowSetting, RedrawOptions, TresholdLevel, load_settings,
};
use tokio::{
    sync::{
        Mutex,
        mpsc::{self, Sender},
        oneshot,
    },
    time::sleep,
};

use std::{
    collections::{HashMap, HashSet},
    sync::OnceLock,
    time::Duration,
};

const QUILL_NIRI_BRIDGE: &str = "Quill niri";

pub struct QuillNiriBridge {
    app_meta: HashMap<pid_t, (String, HashSet<i64>)>,
    window_meta: HashMap<i64, (String, NiriWindows)>,
    previous_windows: Vec<NiriWindows>,
}

static SETTINGS: OnceLock<Mutex<Vec<EinkWindowSetting>>> = OnceLock::new();

impl QuillNiriBridge {
    const OUTPUT_NAME: &str = "DPI-1";

    pub async fn new() -> Result<Self> {
        let bridge = Self {
            app_meta: HashMap::new(),
            window_meta: HashMap::new(),
            previous_windows: Vec::new(),
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
        scale: f64,
        socket: &mut Socket,
    ) -> Result<()> {
        let (rtx, rx) = oneshot::channel::<String>();

        let cmd = ebc::command::Window::Add {
            app_key: app_key.clone(),
            title: win.title.clone(),
            area: Rect::from_xywh(
                (win.geometry.x  as f64 * scale) as i32,
                (win.geometry.y as f64 * scale) as i32,
                (win.geometry.width as f64 * scale) as i32,
                (win.geometry.height as f64 * scale) as i32,
            ),
            hint: Some(setting_to_hint(&win.setting, win.focused, socket).await),
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
        let (screen_w, screen_h, scale) = {
            let output = match outputs.get(Self::OUTPUT_NAME) {
                Some(o) => o,
                None => {
                    eprintln!("Error: Output '{}' not found.", Self::OUTPUT_NAME);
                    return;
                }
            };

            match output.logical {
                Some(logical_mode) => (
                    logical_mode.width as i32,
                    logical_mode.height as i32,
                    logical_mode.scale,
                ),
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
                let settings = SETTINGS.get_or_init(|| Mutex::new(Vec::new())).lock().await;
                for setting in settings.iter() {
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
                                    geometry: OurWindowGeometry::from(geometry.clone()),
                                });
                            }
                        }
                    }
                }
            }
        }

        // TODO: remove floating windows from this, or maybe not???
        new_niri_windows.sort_by_key(|w| w.geometry.x);
        windows_on_screen(&mut new_niri_windows, screen_w, screen_h);

        if self.previous_windows == new_niri_windows {
            println!("Windows did not change, not doing anything");
            return;
        } else {
            println!("Windows changed, updating things");
            self.previous_windows = new_niri_windows.clone();
        }

        println!("New niri windows are: {:#?}", new_niri_windows);

        for win in new_niri_windows {
            let pid = win.geometry.id as pid_t;
            match self.add_app(pid, tx).await {
                Ok(app_key) => {
                    if let Err(e) = self.add_window(win, app_key, tx, scale, &mut socket).await {
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
            let (evt_tx, mut evt_rx) = mpsc::unbounded_channel();

            tokio::task::spawn_blocking(move || {
                let mut read_event = socket.read_events();
                while let Ok(event) = read_event() {
                    let _ = evt_tx.send(event);
                }
            });

            while let Some(event) = evt_rx.recv().await {
                match event {
                    Event::WindowsChanged { .. }
                    | Event::WindowOpenedOrChanged { .. }
                    | Event::WindowClosed { .. }
                    | Event::WindowLayoutsChanged { .. }
                    | Event::WindowFocusChanged { .. }
                    | Event::WorkspaceActivated { .. } => {
                        // We clear the channer before running manage so we are sure the windows are in their final destination
                        sleep(Duration::from_millis(10)).await;
                        while let Ok(_) = evt_rx.try_recv() {
                            // Just dropping the events
                            sleep(Duration::from_millis(5)).await;
                        }
                        self.main_manage(&mut tx).await;
                    }
                    _ => {}
                }
            }
        }

        eprintln!("Quill niri bridge ended");
        Ok(())
    }
}

pub async fn load_settings_internal(username: String) -> bool {
    println!("Reading settings...");
    let settings = match load_settings(username.clone()) {
        Ok(settings) => settings,
        Err(err) => {
            eprintln!("Load settings internal failed: {:?}", err);
            return false;
        }
    };
    println!(
        "Readed settings succesfully for username: {}",
        username.clone()
    );
    let mut guard = SETTINGS.get_or_init(|| Mutex::new(Vec::new())).lock().await;
    *guard = settings;
    true
}

pub async fn start(tx: mpsc::Sender<ebc::Command>) -> Result<String> {
    let quill_niri_bridge = QuillNiriBridge::new()
        .await
        .context("While trying to start Quill niri bridge")?;

    tokio::spawn(async move {
        println!("Settings watcher init");
        const SHORT_DELAY: Duration = Duration::from_secs(1);
        const LONG_DELAY: Duration = Duration::from_secs(5);
        SETTINGS.get_or_init(|| Mutex::new(Vec::new()));
        let mut delay = SHORT_DELAY;
        let mut username = "".to_string();
        let mut inotify = Inotify::init().expect("Failed to initialize inotify");
        let mut inotify_set = false;
        let mut inotify_descriptors = Vec::new();
        loop {
            // println!("Settings watcher loop");
            if let Some((_id, username2)) = find_session().await {
                if username != username2 || !inotify_set {
                    let path = format!("/home/{}/.config/eink_window_settings/", username2);

                    for desc in inotify_descriptors.drain(..) {
                        let _ = inotify.watches().remove(desc);
                    }

                    match inotify.watches().add(path, WatchMask::ALL_EVENTS) {
                        Ok(descriptor) => {
                            inotify_descriptors.push(descriptor);
                            inotify_set = true;
                            username = username2;
                            if load_settings_internal(username.clone()).await {
                                delay = LONG_DELAY;
                            } else {
                                delay = SHORT_DELAY;
                            }
                            println!("Inotify set!");
                        }
                        Err(err) => eprintln!("Inotify failed: {:?}", err),
                    }
                }
            } else {
                eprintln!("Failed to get session");
            }

            if inotify_set {
                let mut buffer = [0; 254];
                let mut readed_settings = false;
                loop {
                    match inotify.read_events(&mut buffer) {
                        Ok(_) => {
                            if !readed_settings {
                                if load_settings_internal(username.clone()).await {
                                    delay = LONG_DELAY;
                                } else {
                                    delay = SHORT_DELAY;
                                }
                                readed_settings = true;
                            }
                        }
                        Err(e) => {
                            if e.kind() == std::io::ErrorKind::WouldBlock {
                                break;
                            }
                            eprintln!("Inotify failed: {:?}", e);
                            inotify_set = false;
                            break;
                        }
                    }
                }
            }

            // println!("Delay in watcher lopp... {:?}", delay);
            sleep(delay).await;
        }
    });

    // So settings load on start of the bridge
    sleep(Duration::from_secs(1)).await;

    tokio::spawn(async move {
        let _ = quill_niri_bridge.run(tx).await;
    });

    Ok(QUILL_NIRI_BRIDGE.into())
}

async fn get_socket() -> Socket {
    let base_run_dir = "/run/user";

    loop {
        // 1. Read the base /run/user directory
        if let Ok(mut user_dirs) = tokio::fs::read_dir(base_run_dir).await {
            // Iterate through user folders (e.g., /run/user/1000)
            while let Ok(Some(user_entry)) = user_dirs.next_entry().await {
                let user_path = user_entry.path();

                if user_path.is_dir() {
                    // 2. Read the contents of each user directory
                    if let Ok(mut sockets) = tokio::fs::read_dir(&user_path).await {
                        while let Ok(Some(socket_entry)) = sockets.next_entry().await {
                            let path = socket_entry.path();
                            let file_name = path.file_name().unwrap_or_default().to_string_lossy();

                            // 3. Check for niri-wayland prefix
                            if file_name.starts_with("niri.wayland-") {
                                // Try to connect; if it succeeds, return the socket
                                if let Ok(socket) = Socket::connect_to(&path) {
                                    return socket;
                                }
                            }
                        }
                    }
                }
            }
        }

        // 4. Wait a second before re-scanning the filesystem
        tokio::time::sleep(Duration::from_secs(1)).await;
        eprintln!("Waiting for niri socket...");
    }
}

#[derive(Clone, Debug, PartialEq)]
struct NiriWindows {
    app_id: String,
    title: String,
    focused: bool,
    setting: EinkWindowSetting,
    geometry: OurWindowGeometry,
}

// Because niri doesn't do partialeq defines
#[derive(Clone, Debug, PartialEq)]
pub struct OurWindowGeometry {
    pub id: u64,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl From<WindowGeometry> for OurWindowGeometry {
    fn from(window: WindowGeometry) -> Self {
        Self {
            id: window.id,
            x: window.x,
            y: window.y,
            width: window.width,
            height: window.height,
        }
    }
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

// Globals bad, but if I need to pass an argument though 3 functions for no reason only for checking if something changed, uh, let me commit this sin then
// Last bool is for initial applying
static GLOBAL_EINK_SETTINGS: OnceLock<
    Mutex<(TresholdLevel, Dithering, RedrawOptions, DriverMode, bool)>,
> = OnceLock::new();
async fn setting_to_hint(setting: &EinkWindowSetting, focused: bool, socket: &mut Socket) -> Hint {
    use pinenote_service::types::rockchip_ebc::{HintBitDepth, HintConvertMode};
    use quill_data_provider_lib::{BitDepth, Conversion, DriverMode, Redraw};

    let mut treshold: TresholdLevel = Default::default();
    let mut dithering_mode: Dithering = Default::default();
    let mut redraw_options: RedrawOptions = Default::default();

    let hint = match &setting.settings {
        DriverMode::Normal(bit_depth) => match bit_depth {
            BitDepth::Y1(conv, level) => {
                let cm = match conv {
                    Conversion::Tresholding => {
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
                    Conversion::Tresholding => HintConvertMode::Threshold,
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
        let mut older_settings = GLOBAL_EINK_SETTINGS
            .get_or_init(|| Mutex::new(Default::default()))
            .lock()
            .await;
        let is_different_settings = match (&older_settings.3, &setting.settings) {
            (DriverMode::Normal(_), DriverMode::Fast(_)) => true,
            (DriverMode::Fast(_), DriverMode::Normal(_)) => true,
            _ => false,
        };
        if older_settings.0 != treshold
            || older_settings.1 != dithering_mode
            || older_settings.2 != redraw_options
            || is_different_settings
            || !older_settings.4
        {
            treshold.set().await;
            dithering_mode.set().await;
            redraw_options.set().await;
            setting.settings.set().await; // Sets normal or fast globally based on this focused window settings
            // Now we need to "rewrite" things on the screen
            // Hacky but whatever
            sleep(Duration::from_millis(25)).await;
            socket
                .send(Request::Action(niri_ipc::Action::ToggleDebugTint {}))
                .ok();
            sleep(Duration::from_millis(10)).await;
            socket
                .send(Request::Action(niri_ipc::Action::ToggleDebugTint {}))
                .ok();

            // Save to older settings
            *older_settings = (treshold, dithering_mode, redraw_options, setting.settings, true);
        }
    }

    hint
}
