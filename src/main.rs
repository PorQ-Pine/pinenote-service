use anyhow::Result;
use tokio::{signal, sync::mpsc};

#[cfg(feature = "bridges")]
pub mod bridge {
    use tokio::sync::mpsc;

    use crate::ebc;

    #[cfg(feature = "sway")]
    pub mod sway;

    pub async fn start(tx: mpsc::Sender<ebc::Command>) -> Option<String> {
        #[cfg(feature = "sway")]
        let res = sway::start(tx.clone()).await;

        // Add here other bridges with AND for the check to work
        #[cfg(not(feature = "sway"))]
        {
            compile_error!(
                "bridges feature is enabled but no specific bridges are enabled, this is wrong, enable sway feature for example"
            );
        }

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

    #[cfg(feature = "bridges")]
    let selected_bridge = bridge::start(tx.clone()).await.unwrap_or_default();
    #[cfg(not(feature = "bridges"))]
    let selected_bridge = String::new();

    let _dbus_ctx = dbus::Context::initialize(tx.clone(), selected_bridge).await?;

    match signal::ctrl_c().await {
        Ok(()) => {}
        Err(err) => {
            eprintln!("Unable to listen for shutdown signal: {}", err);
        }
    };

    Ok(())
}
