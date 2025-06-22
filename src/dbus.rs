use anyhow::Result;
use tokio::sync::mpsc;
use zbus::{connection, fdo};

use crate::ebc;

pub mod pinenotectl;

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
    pub async fn initialize(tx: mpsc::Sender<ebc::Command>, bridge: String) -> Result<Self> {
        let ctl1 = pinenotectl::PineNoteCtl::new(tx.clone(), bridge);
        let ebc1 = pinenotectl::Ebc1::new(tx.clone());
        let hintmgr1 = pinenotectl::HintMgr1::new(tx.clone());

        let _connection = connection::Builder::session()?
            .name(DBUS_NAME)?
            .serve_at(DBUS_PATH, ctl1)?
            .serve_at(DBUS_PATH, ebc1)?
            .serve_at(DBUS_PATH, hintmgr1)?
            .build()
            .await?;

        Ok(Self { _connection })
    }
}
