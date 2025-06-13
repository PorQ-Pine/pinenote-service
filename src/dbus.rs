use tokio::sync::mpsc;
use zbus::{fdo, interface};

use crate::EbcCommand;

pub struct PineNoteCtl {
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
    async fn global_refresh(&self) -> Result<(), fdo::Error> {
        if let Err(e) = self.ebc_tx.send(EbcCommand::GlobalRefresh).await {
            eprintln!("Failed to trigger global refresh: {e:?}");
            Err(zbus::fdo::Error::Failed("InternalError".into()))
        } else {
            Ok(())
        }
    }

    async fn dump(&self, path: String) -> Result<(), fdo::Error> {
        if let Err(e) = self.ebc_tx.send(EbcCommand::Dump(path)).await {
            eprintln!("Failed to send Dump command {e:?}");
            Err(zbus::fdo::Error::Failed("InternalError".into()))
        } else {
            Ok(())
        }
    }
}
