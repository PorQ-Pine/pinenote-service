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

    async fn do_send(&self, cmd: ebc::Command, str: &str) -> fdo::Result<()> {
        if let Err(e) = self.ebc_tx.send(cmd).await {
            eprintln!("Failed to send {str}: {e:?}");
            Err(zbus::fdo::Error::Failed("Internal Error".into()))
        } else {
            Ok(())
        }
    }

    async fn send(&self, cmd: impl Into<ebc::Command>) -> fdo::Result<()> {
        let cmd = cmd.into();
        let cmd_str = cmd.get_context_str();

        self.do_send(cmd, cmd_str).await
    }

    async fn send_with_reply<T>(&self, cmd: impl Into<ebc::Command>, rx: oneshot::Receiver<T>) -> fdo::Result<T> {
        let cmd = cmd.into();
        let cmd_str = cmd.get_context_str();
        self.do_send(cmd, cmd_str).await?;

        match rx.await {
            Ok(v) => Ok(v),
            Err(e) => {
                eprintln!("Failed to receive reply to {cmd_str}: {e:#?}");
                Err(fdo::Error::Failed("Internal Error".into()))
            }
        }
    }
}

#[interface(name = "org.pinenote.Ebc1")]
impl PineNoteCtl {
    async fn global_refresh(&self) -> Result<(), fdo::Error> {
        self.send(ebc::Command::GlobalRefresh).await
    }

    async fn dump_framebuffers(&self, directory: String) -> Result<(), fdo::Error> {
        self.send(ebc::Command::FbDumpToDir(directory)).await
    }

    async fn dump(&self, path: String) -> Result<(), fdo::Error> {
        self.send(ebc::Command::Dump(path)).await
    }

    #[zbus(property)]
    async fn default_hint(&self) -> fdo::Result<Hint> {
        let (tx, rx) = oneshot::channel::<CoreHint>();

        self.send_with_reply(ebc::Property::DefaultHint(tx), rx).await
            .map(|ch| ch.into())
    }

    #[zbus(property)]
    async fn set_default_hint(&self, hint: Hint) -> Result<(), zbus::Error> {
        let hint : CoreHint = hint.into();

        self.send(ebc::Property::SetDefaultHint(hint)).await
            .map_err(zbus::Error::from)
    }
}
