use std::io::Write;

use anyhow::{anyhow, Context, Result};
use nix::libc::pid_t;
use tokio::{signal, sync::{mpsc, oneshot}};
use pinenote_service::{
    drivers::rockchip_ebc::RockchipEbc,
    pixel_manager::{Application, PixelManager, Window, WindowData},
    types::{rockchip_ebc::Hint, Rect}
};
use zbus::connection;

pub mod bridge {
    pub mod sway;
}

pub mod dbus;

pub enum EbcCommand {
    GlobalRefresh,
    AddApplication(pid_t, oneshot::Sender<String>),
    RemoveApplication(String),
    AddWindow {
        app_key: String,
        title: String,
        area: Rect,
        hint: Option<Hint>,
        visible: bool,
        z_index: i32,
        ret: oneshot::Sender<String>
    },
    UpdateWindow {
        win_key: String,
        title: Option<String>,
        area: Option<Rect>,
        hint: Option<Option<Hint>>,
        visible: Option<bool>,
        z_index: Option<i32>
    },
    RemoveWindow(String),
    Property(EbcProperty),
    Dump(String)
}

impl EbcCommand {
    fn get_context_str(&self) -> &'static str {
        use self::EbcCommand::*;

        match self {
            GlobalRefresh => "GlobalRefresh",
            AddApplication(_, _) => "AddApplication",
            RemoveApplication(_) => "RemoveApplication",
            AddWindow { .. } => "AddWindow",
            UpdateWindow { .. } => "UpdateWindow",
            RemoveWindow(_) => "RemoveWindow",
            Property(p) => p.get_context_str(),
            Dump(_) => "Dump"
        }
    }
}

pub enum EbcProperty {
    DefaultHint(oneshot::Sender<Hint>),
    SetDefaultHint(Hint)
}

impl EbcProperty {
    fn get_context_str(&self) -> &'static str {
        use self::EbcProperty::*;

        match self {
            DefaultHint(_) => "Property::GetDefaultHint",
            SetDefaultHint(_) => "Property::SetDefaultHint",
        }
    }
}

impl From<EbcProperty> for EbcCommand {
    fn from(value: EbcProperty) -> Self {
        Self::Property(value)
    }
}

struct EbcCtl {
    driver: RockchipEbc,
    pixel_manager: PixelManager,
}

impl EbcCtl {
    pub fn new() -> Result<EbcCtl> {
        let driver = RockchipEbc::new();

        let default_hint = driver.default_hint()?;
        let screen_area = driver.screen_area()?;

        Ok(EbcCtl {
            driver: RockchipEbc::new(),
            pixel_manager: PixelManager::new(default_hint, screen_area)
        })
    }

    fn recompute_hints(&self) -> Result<()> {
        let hints = self.pixel_manager.compute_hints().context("Failed to compute new hints")?;

        self.driver.upload_rect_hints(hints).context("Failed to upload hints")
    }

    fn dump(&self, mut output: impl Write) {
        let _ = writeln!(output, "=========== EBC_CTL DUMP ===========");
        let _ = writeln!(output, "PixelManager: ");
        let _ = writeln!(output, "{:#?}", self.pixel_manager);
        let _ = writeln!(output, "=========== ! EBC_CTL DUMP ===========");
    }

    async fn dispatch_props(&mut self, prop_cmd: EbcProperty) -> Result<()> {
        use self::EbcProperty::*;

        match prop_cmd {
            DefaultHint(tx) => {
                let h = self.pixel_manager.default_hint;
                tx.send(h).map_err(|_| anyhow!("Failed to send back default hint"))?;
            },
            SetDefaultHint(h) => {
                self.pixel_manager.default_hint = h;

                self.recompute_hints()?;
            }
        }

        Ok(())
    }

    async fn dispatch(&mut self, cmd: EbcCommand) -> Result<()> {
        match cmd {
            EbcCommand::GlobalRefresh => {
                self.driver.global_refresh()
                    .context("RockchipEbc::global_refresh failed")?;
            }
            EbcCommand::AddApplication(pid, ret) => {
                let app_key = self.pixel_manager.app_add(Application::new("", pid));
                ret.send(app_key).map_err(|e| anyhow!("Failed to send response: {e}"))?;
            },
            EbcCommand::RemoveApplication(app_id) => {
                self.pixel_manager.app_remove(&app_id);
                self.recompute_hints()?;
            },
            EbcCommand::AddWindow { app_key, title, area, hint, visible, z_index, ret } => {
                let win_key = self.pixel_manager.window_add(Window::new(app_key, title, area, hint, visible, z_index))
                    .context("PixelManager::window_add failed")?;

                ret.send(win_key).map_err(|e| anyhow!("Failed to send response: {e:?}"))?;

                self.recompute_hints()?;
            },
            EbcCommand::UpdateWindow { win_key, title, area, hint, visible, z_index } => {
                let win = self.pixel_manager.window(&win_key).context("Failed to get window {win_key}")?;

                let update = WindowData {
                    title: title.unwrap_or(win.data.title.clone()),
                    area: area.unwrap_or(win.data.area.clone()),
                    hint: hint.unwrap_or(win.data.hint),
                    visible: visible.unwrap_or(win.data.visible),
                    z_index: z_index.unwrap_or(win.data.z_index),
                };

                self.pixel_manager.window_update(&win_key, update).context("Failed to update window {win_key}")?;

                self.recompute_hints()?;
            }
            EbcCommand::RemoveWindow(win_id) => {
                self.pixel_manager.window_remove(win_id);
                self.recompute_hints()?;
            }
            EbcCommand::Dump(path) => {
                if path == "-" {
                    self.dump(std::io::stderr())
                } else if let Ok(f) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)
                {
                    self.dump(f);
                } else {
                    self.dump(std::io::stderr());
                }
            }
            EbcCommand::Property(p) => {
                self.dispatch_props(p).await?;
            }
        };

        Ok(())
    }

    pub async fn serve(&mut self, mut rx: mpsc::Receiver<EbcCommand>) {
        while let Some(cmd) = rx.recv().await {
            let ctx = cmd.get_context_str();

            if let Err(e) = self.dispatch(cmd).await
                .with_context(|| format!("While handling {ctx}"))
            {
                eprintln!("{e:?}")
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let (tx, rx) = mpsc::channel(100);
    let mut ebc = EbcCtl::new()?;

    tokio::spawn(async move { ebc.serve(rx).await; });

    let pinenote_ctl = dbus::PineNoteCtl::new(tx.clone());

    let _zbus_connection = connection::Builder::session()?
        .name("org.pinenote.PineNoteCtl")?
        .serve_at("/org/pinenote/PineNoteCtl", pinenote_ctl)?
        .build()
        .await?;

    let mut sway_bridge = bridge::sway::SwayBridge::new().await?;

    tokio::spawn(async move { let _ = sway_bridge.run(tx.clone()).await; });

    match signal::ctrl_c().await {
        Ok(()) => {},
        Err(err) => {
            eprintln!("Unable to listen for shutdown signal: {}", err);
        },
    };

    unimplemented!("Termination not implemented yet")
}
