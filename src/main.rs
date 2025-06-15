use anyhow::Result;
use tokio::{signal, sync::mpsc};
use zbus::connection;

pub mod bridge {
    pub mod sway;
}

pub mod dbus;

pub mod ebc {
    pub mod command;
    pub use command::*;
    pub mod ctl;
    pub use ctl::*;
}

#[tokio::main]
async fn main() -> Result<()> {
    let (tx, rx) = mpsc::channel(100);
    let mut ebc = ebc::Ctl::new()?;

    tokio::spawn(async move {
        ebc.serve(rx).await;
    });

    let pinenote_ctl = dbus::PineNoteCtl::new(tx.clone());

    let _zbus_connection = connection::Builder::session()?
        .name("org.pinenote.PineNoteCtl")?
        .serve_at("/org/pinenote/PineNoteCtl", pinenote_ctl)?
        .build()
        .await?;

    let mut sway_bridge = bridge::sway::SwayBridge::new().await?;

    tokio::spawn(async move {
        let _ = sway_bridge.run(tx.clone()).await;
    });

    match signal::ctrl_c().await {
        Ok(()) => {}
        Err(err) => {
            eprintln!("Unable to listen for shutdown signal: {}", err);
        }
    };

    unimplemented!("Termination not implemented yet")
}
