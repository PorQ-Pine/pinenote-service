use anyhow::Result;
use tokio::{signal, sync::mpsc};

pub mod bridge {
    use tokio::sync::mpsc;

    use crate::ebc;

    pub mod sway;

    pub async fn start(tx: mpsc::Sender<ebc::Command>) -> Option<String> {
        let res = sway::start(tx.clone()).await;

        match res {
            Ok(s) => Some(s),
            Err(e) => {
                eprintln!("{e:#?}");
                None
            }
        }
    }
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

    let selected_bridge = bridge::start(tx.clone()).await.unwrap_or_default();

    let _dbus_ctx = dbus::Context::initialize(tx.clone(), selected_bridge).await?;

    match signal::ctrl_c().await {
        Ok(()) => {}
        Err(err) => {
            eprintln!("Unable to listen for shutdown signal: {}", err);
        }
    };

    unimplemented!("Termination not implemented yet")
}
