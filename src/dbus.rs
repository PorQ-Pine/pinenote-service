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
    ebc_tx: ebc::CommandSender
}

impl PineNoteCtl {
    pub fn new(ebc_tx: mpsc::Sender<ebc::Command>) -> Self {
        Self {
            ebc_tx: ebc_tx.into()
        }
    }
}

fn internal_error(e: anyhow::Error) -> fdo::Error {
    eprintln!("{e:#?}");
    fdo::Error::Failed("Internal error".into())
}

#[interface(name = "org.pinenote.Ebc1")]
impl PineNoteCtl {
    async fn global_refresh(&self) -> fdo::Result<()> {
        self.ebc_tx.send(ebc::Command::GlobalRefresh).await
            .map_err(internal_error)
    }

    async fn dump_framebuffers(&self, directory: String) -> fdo::Result<()> {
        self.ebc_tx.send(ebc::Command::FbDumpToDir(directory)).await
            .map_err(internal_error)
    }

    async fn dump(&self, path: String) -> fdo::Result<()> {
        self.ebc_tx.send(ebc::Command::Dump(path)).await
            .map_err(internal_error)
    }

    #[zbus(property)]
    async fn default_hint(&self) -> fdo::Result<Hint> {
        let (tx, rx) = oneshot::channel::<CoreHint>();

        self.ebc_tx.with_reply(ebc::Property::DefaultHint(tx), rx).await
            .map_err(internal_error)
            .map(|ch| ch.into())
    }

    #[zbus(property)]
    async fn set_default_hint(&self, hint: Hint) -> Result<(), zbus::Error> {
        let hint : CoreHint = hint.into();

        self.ebc_tx.send(ebc::Property::SetDefaultHint(hint)).await
            .map_err(internal_error)
            .map_err(zbus::Error::from)
    }
}
