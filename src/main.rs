use anyhow::Result;
use tokio::{signal, sync::mpsc};

use pinenote_service::drivers::rockchip_ebc::RockchipEbc;
use zbus::{connection, interface};

struct EbcCtl {
    driver: RockchipEbc
}

enum EbcCommand {
    GlobalRefresh,
}

impl EbcCtl {
    pub fn new() -> EbcCtl {
        EbcCtl { driver: RockchipEbc::new() }
    }

    pub async fn serve(&self, mut rx: mpsc::Receiver<EbcCommand>) {
        while let Some(cmd) = rx.recv().await {

            match cmd {
                EbcCommand::GlobalRefresh => {
                    if let Err(e) = self.driver.global_refresh() {
                        eprintln!("{e}");
                    }
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
    let ebc = EbcCtl::new();

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
