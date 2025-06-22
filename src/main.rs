use anyhow::Result;
use tokio::{signal, sync::mpsc};

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

    let sway_tx = tx.clone();
    let mut sway_bridge = bridge::sway::SwayBridge::new().await?;

    tokio::spawn(async move {
        let _ = sway_bridge.run(sway_tx).await;
    });

    let _dbus_ctx = dbus::Context::initialize(tx.clone()).await?;

    match signal::ctrl_c().await {
        Ok(()) => {}
        Err(err) => {
            eprintln!("Unable to listen for shutdown signal: {}", err);
        }
    };

    unimplemented!("Termination not implemented yet")
}
