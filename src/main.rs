use anyhow::Result;
use nix::libc::pid_t;
use tokio::{signal, sync::{mpsc, oneshot}};

use pinenote_service::{drivers::rockchip_ebc::RockchipEbc, pixel_manager::{Application, PixelManager, PixelManagerError, Window}, types::{rockchip_ebc::Hint, Rect}};
use zbus::{connection, interface};

struct EbcCtl {
    driver: RockchipEbc,
    pixel_manager: PixelManager
}

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
        ret: oneshot::Sender<Result<String, PixelManagerError>>
    },
    RemoveWindow(String)
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

    fn recompute_hints(&self) {
        let res = self.pixel_manager.compute_hints().map_err(anyhow::Error::new)
            .and_then(|hints| self.driver.upload_rect_hints(hints).map_err(anyhow::Error::new));

        if let Err(e) = res {
            eprintln!("{:#}", e);
        }
    }

    pub async fn serve(&mut self, mut rx: mpsc::Receiver<EbcCommand>) {
        while let Some(cmd) = rx.recv().await {
            match cmd {
                EbcCommand::GlobalRefresh => {
                    if let Err(e) = self.driver.global_refresh() {
                        eprintln!("{e}");
                    }
                },
                EbcCommand::AddApplication(pid, ret) => {
                    let app_key = self.pixel_manager.app_add(Application::new("", pid));
                    if let Err(e) = ret.send(app_key) {
                        eprintln!("Error: Could not send response to AddApplication: {e}");
                    }
                },
                EbcCommand::RemoveApplication(app_id) => {
                    self.pixel_manager.app_remove(&app_id);
                    self.recompute_hints();
                },
                EbcCommand::AddWindow { app_key, title, area, hint, visible, z_index, ret } => {
                    let win_key = self.pixel_manager.window_add(Window::new(app_key, title, area, hint, visible, z_index));

                    if let Err(ref e) = win_key {
                        eprintln!("{e}");
                    }

                    if let Err(_) = ret.send(win_key) {
                        eprintln!("Error: Could not send response to AddWindow");
                    }

                    self.recompute_hints();
                },
                EbcCommand::RemoveWindow(win_id) => {
                    self.pixel_manager.window_remove(win_id);

                    self.recompute_hints();
                }
            };
        }
    }
}

struct PineNoteCtl {
    ebc_tx: mpsc::Sender<EbcCommand>
}

impl PineNoteCtl {
    pub fn new(ebc_tx: mpsc::Sender<EbcCommand>) -> Self {
        Self {
            ebc_tx
        }
    }
}

#[interface(name = "org.pinenote.Ebc1")]
impl PineNoteCtl {
    async fn global_refresh(&self) -> Result<(), zbus::fdo::Error> {
        if let Err(e) = self.ebc_tx.send(EbcCommand::GlobalRefresh).await {
            eprintln!("Failed to trigger global refresh: {e}");
            Err(zbus::fdo::Error::Failed("InternalError".into()))
        } else {
            Ok(())
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let (tx, rx) = mpsc::channel(100);
    let mut ebc = EbcCtl::new()?;

    tokio::spawn(async move { ebc.serve(rx).await; });

    let pinenote_ctl = PineNoteCtl::new(tx.clone());

    let _zbus_connection = connection::Builder::session()?
        .name("org.pinenote.PineNoteCtl")?
        .serve_at("/org/pinenote/PineNoteCtl", pinenote_ctl)?
        .build()
        .await?;

    match signal::ctrl_c().await {
        Ok(()) => {},
        Err(err) => {
            eprintln!("Unable to listen for shutdown signal: {}", err);
        },
    };

    unimplemented!("Termination not implemented yet")
}
