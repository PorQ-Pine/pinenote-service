use pinenote_service::types::rockchip_ebc::{
    DitherMode, DriverMode, Hint as CoreHint, HintBitDepth, HintConvertMode,
};
use tokio::sync::{mpsc, oneshot};
use zbus::{
    fdo, interface,
    object_server::SignalEmitter,
    zvariant::{Type, Value},
};

use crate::ebc::{self, OffScreenError};

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
    ebc_tx: ebc::CommandSender,
}

impl PineNoteCtl {
    pub fn new(ebc_tx: mpsc::Sender<ebc::Command>) -> Self {
        Self {
            ebc_tx: ebc_tx.into(),
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
        self.ebc_tx
            .send(ebc::Command::GlobalRefresh)
            .await
            .map_err(internal_error)
    }

    async fn dump_framebuffers(&self, directory: String) -> fdo::Result<()> {
        self.ebc_tx
            .send(ebc::Command::FbDumpToDir(directory))
            .await
            .map_err(internal_error)
    }

    async fn dump(&self, path: String) -> fdo::Result<()> {
        self.ebc_tx
            .send(ebc::Command::Dump(path))
            .await
            .map_err(internal_error)
    }

    async fn cycle_driver_mode(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        let (tx, reply) = oneshot::channel::<DriverMode>();

        let driver_mode = self
            .ebc_tx
            .with_reply(ebc::Property::DriverMode(tx), reply)
            .await
            .map_err(internal_error)?;

        let new_mode = driver_mode.cycle_next();
        if new_mode != driver_mode {
            self.set_driver_mode(new_mode).await?;
            self.driver_mode_changed(&emitter).await?;
        }
        Ok(())
    }

    async fn cycle_dither_mode(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        let (tx, reply) = oneshot::channel::<DitherMode>();

        let dither_mode = self
            .ebc_tx
            .with_reply(ebc::Property::DitherMode(tx), reply)
            .await
            .map_err(internal_error)?;

        let new_mode = dither_mode.cycle_next();
        if new_mode != dither_mode {
            self.set_dither_mode(new_mode).await?;
            self.dither_mode_changed(&emitter).await?;
        }

        Ok(())
    }

    async fn set_off_screen(
        &self,
        path: String,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        let (tx, rx) = oneshot::channel::<Result<(), OffScreenError>>();

        let res = self
            .ebc_tx
            .with_reply(ebc::Command::OffScreen(path.clone(), tx), rx)
            .await
            .map_err(internal_error)?;

        if let Err(e) = res {
            match e {
                OffScreenError::LoadFailed => Err(fdo::Error::FileNotFound(path))?,
                OffScreenError::DecodeFailed => Err(fdo::Error::Failed(format!(
                    "Failed to load '{path}': Bad format"
                )))?,
                OffScreenError::UploadFailed => {
                    self.off_screen_override_changed(&emitter).await?;
                    Err(fdo::Error::Failed(
                        "Could not upload image to driver".into(),
                    ))?;
                }
            }
        } else {
            self.off_screen_override_changed(&emitter).await?
        }

        Ok(())
    }

    #[zbus(property)]
    async fn off_screen_override(&self) -> fdo::Result<String> {
        let (tx, rx) = oneshot::channel::<String>();

        self.ebc_tx
            .with_reply(ebc::Property::OffScreenOverride(tx), rx)
            .await
            .map_err(internal_error)
    }

    #[zbus(property)]
    async fn off_screen_disable(&self) -> fdo::Result<bool> {
        let (tx, rx) = oneshot::channel::<bool>();

        self.ebc_tx
            .with_reply(ebc::Property::OffScreenDisable(tx), rx)
            .await
            .map_err(internal_error)
    }

    #[zbus(property)]
    async fn set_off_screen_disable(&self, disable: bool) -> Result<(), zbus::Error> {
        self.ebc_tx
            .send(ebc::Property::SetOffScreenDisable(disable))
            .await
            .map_err(internal_error)
            .map_err(zbus::Error::from)
    }

    #[zbus(property)]
    async fn default_hint(&self) -> fdo::Result<Hint> {
        let (tx, rx) = oneshot::channel::<CoreHint>();

        self.ebc_tx
            .with_reply(ebc::Property::DefaultHint(tx), rx)
            .await
            .map_err(internal_error)
            .map(|ch| ch.into())
    }

    #[zbus(property)]
    async fn set_default_hint(&self, hint: Hint) -> Result<(), zbus::Error> {
        let hint: CoreHint = hint.into();

        self.ebc_tx
            .send(ebc::Property::SetDefaultHint(hint))
            .await
            .map_err(internal_error)
            .map_err(zbus::Error::from)
    }

    #[zbus(property)]
    async fn driver_mode(&self) -> fdo::Result<DriverMode> {
        let (tx, reply) = oneshot::channel::<DriverMode>();

        self.ebc_tx
            .with_reply(ebc::Property::DriverMode(tx), reply)
            .await
            .map_err(internal_error)
    }

    #[zbus(property)]
    async fn set_driver_mode(&self, driver_mode: DriverMode) -> Result<(), zbus::Error> {
        if driver_mode == DriverMode::ZeroWaveform {
            Err(fdo::Error::InvalidArgs("Value not supported".into()))?
        }

        self.ebc_tx
            .send(ebc::Property::SetDriverMode(driver_mode))
            .await
            .map_err(internal_error)
            .map_err(zbus::Error::from)
    }

    #[zbus(property)]
    async fn dither_mode(&self) -> fdo::Result<DitherMode> {
        let (tx, reply) = oneshot::channel::<DitherMode>();

        self.ebc_tx
            .with_reply(ebc::Property::DitherMode(tx), reply)
            .await
            .map_err(internal_error)
    }

    #[zbus(property)]
    async fn set_dither_mode(&self, dither_mode: DitherMode) -> Result<(), zbus::Error> {
        self.ebc_tx
            .send(ebc::Property::SetDitherMode(dither_mode))
            .await
            .map_err(internal_error)
            .map_err(zbus::Error::from)
    }

    #[zbus(property)]
    async fn redraw_delay(&self) -> fdo::Result<u16> {
        let (tx, reply) = oneshot::channel::<u16>();

        self.ebc_tx
            .with_reply(ebc::Property::RedrawDelay(tx), reply)
            .await
            .map_err(internal_error)
    }

    #[zbus(property)]
    async fn set_redraw_delay(&self, redraw_delay: u16) -> Result<(), zbus::Error> {
        self.ebc_tx
            .send(ebc::Property::SetRedrawDelay(redraw_delay))
            .await
            .map_err(internal_error)
            .map_err(zbus::Error::from)
    }
}
