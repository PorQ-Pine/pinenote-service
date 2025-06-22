use pinenote_service::types::rockchip_ebc::{Hint as CoreHint, HintBitDepth, HintConvertMode};
use tokio::sync::mpsc;
use zbus::{
    fdo, interface,
    zvariant::{Type, Value},
};

pub mod ebc1;
pub use ebc1::Ebc1;

pub mod hintmgr1;
pub use hintmgr1::HintMgr1;

use crate::{dbus, ebc};

#[derive(Type, Value)]
pub struct Hint {
    bit_depth: HintBitDepth,
    convert: HintConvertMode,
    redraw: bool,
}

impl From<CoreHint> for Hint {
    fn from(value: CoreHint) -> Self {
        Self {
            bit_depth: value.bit_depth(),
            convert: value.convert_mode(),
            redraw: value.redraw(),
        }
    }
}

impl From<Hint> for CoreHint {
    fn from(value: Hint) -> Self {
        Self::new(value.bit_depth, value.convert, value.redraw)
    }
}

pub struct PineNoteCtl {
    tx: ebc::CommandSender,
    active_bridge: String,
}

impl PineNoteCtl {
    pub fn new(tx: mpsc::Sender<ebc::Command>, bridge: String) -> Self {
        let active_bridge: String = if bridge.is_empty() {
            "generic".into()
        } else {
            bridge
        };

        Self {
            tx: tx.into(),
            active_bridge,
        }
    }
}

#[interface(name = "org.pinenote.PineNoteCtl1")]
impl PineNoteCtl {
    async fn dump(&self, path: String) -> fdo::Result<()> {
        self.tx
            .send(ebc::Command::Dump(path))
            .await
            .map_err(dbus::internal_error)
    }

    #[zbus(property)]
    async fn active_bridge(&self) -> String {
        self.active_bridge.clone()
    }
}
