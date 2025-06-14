use pinenote_service::types::rockchip_ebc::{Hint as CoreHint, HintBitDepth, HintConvertMode};
use tokio::sync::{mpsc, oneshot};
use zbus::{fdo, interface, zvariant::{Type, Value}};

use crate::ebc;

#[derive(Type, Value)]
struct Hint {
    bit_depth: HintBitDepth,
    convert: HintConvertMode,
    redraw: bool,
}

impl From<CoreHint> for Hint {
    fn from(value: CoreHint) -> Self {
        Self {
            bit_depth: value.bit_depth(),
            convert: value.convert_mode(),
            redraw: value.redraw()
        }
    }
}

impl From<Hint> for CoreHint {
    fn from(value: Hint) -> Self {
        Self::new(value.bit_depth, value.convert, value.redraw)
    }
}

pub struct PineNoteCtl {
    ebc_tx: mpsc::Sender<ebc::Command>
}

impl PineNoteCtl {
    pub fn new(ebc_tx: mpsc::Sender<ebc::Command>) -> Self {
        Self {
            ebc_tx
        }
    }
}

#[interface(name = "org.pinenote.Ebc1")]
impl PineNoteCtl {
    async fn global_refresh(&self) -> Result<(), fdo::Error> {
        if let Err(e) = self.ebc_tx.send(ebc::Command::GlobalRefresh).await {
            eprintln!("Failed to trigger global refresh: {e:?}");
            Err(zbus::fdo::Error::Failed("InternalError".into()))
        } else {
            Ok(())
        }
    }

    async fn dump_framebuffers(&self, directory: String) -> Result<(), fdo::Error> {
        if let Err(e) = self.ebc_tx.send(ebc::Command::FbDumpToDir(directory)).await {
            eprintln!("Could not send FbDumpToDir command: {e:?}");
            Err(zbus::fdo::Error::Failed("InternalError".into()))
        } else {
            Ok(())
        }
    }

    async fn dump(&self, path: String) -> Result<(), fdo::Error> {
        if let Err(e) = self.ebc_tx.send(ebc::Command::Dump(path)).await {
            eprintln!("Failed to send Dump command {e:?}");
            Err(zbus::fdo::Error::Failed("InternalError".into()))
        } else {
            Ok(())
        }
    }

    #[zbus(property)]
    async fn default_hint(&self) -> fdo::Result<Hint> {
        let (tx, rx) = oneshot::channel::<CoreHint>();

        self.ebc_tx.send(ebc::Property::DefaultHint(tx).into()).await
            .map_err(|_| fdo::Error::Failed("Internal Error".into()))?;
        let h = rx.await.map_err(|_| fdo::Error::Failed("Internal error".into()))?;

        Ok(h.into())
    }

    #[zbus(property)]
    async fn set_default_hint(&self, hint: Hint) -> Result<(), zbus::Error> {
        let hint : CoreHint = hint.into();

        self.ebc_tx.send(ebc::Property::SetDefaultHint(hint).into()).await
            .map_err(|_| fdo::Error::Failed("Internal error".into()))?;

        Ok(())
    }
}
