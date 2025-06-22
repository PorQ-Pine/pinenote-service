use anyhow::Result;
use tokio::sync::mpsc;
use zbus::{connection, fdo};

use crate::ebc;

pub mod pinenotectl {
    use pinenote_service::types::rockchip_ebc::{Hint as CoreHint, HintBitDepth, HintConvertMode};
    use zbus::zvariant::{Type, Value};

    pub mod ebc1;
    pub use ebc1::Ebc1;

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
}

pub struct Context {
    _connection: connection::Connection,
}

fn internal_error(e: anyhow::Error) -> fdo::Error {
    eprintln!("{e:#?}");
    fdo::Error::Failed("Internal error".into())
}

const DBUS_NAME: &str = "org.pinenote.PineNoteCtl";
const DBUS_PATH: &str = "/org/pinenote/PineNoteCtl";

impl Context {
    pub async fn initialize(tx: mpsc::Sender<ebc::Command>) -> Result<Self> {
        let ebc1 = pinenotectl::Ebc1::new(tx.clone());

        let _connection = connection::Builder::session()?
            .name(DBUS_NAME)?
            .serve_at(DBUS_PATH, ebc1)?
            .build()
            .await?;

        Ok(Self { _connection })
    }
}
