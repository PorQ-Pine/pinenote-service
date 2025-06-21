use anyhow::Context;
use nix::libc::pid_t;
use pinenote_service::types::{
    Rect,
    rockchip_ebc::{DitherMode, DriverMode, Hint},
};
use tokio::sync::{mpsc, oneshot};

use super::OffScreenError;

pub enum Command {
    Application(Application),
    Dump(String),
    FbDumpToDir(String),
    GlobalRefresh,
    Property(Property),
    SetMode(DriverMode, DitherMode, u16),
    Window(Window),
    OffScreen(String, oneshot::Sender<Result<(), OffScreenError>>),
}

pub enum Application {
    Add(pid_t, oneshot::Sender<String>),
    Remove(String),
}

pub enum Property {
    DefaultHint(oneshot::Sender<Hint>),
    SetDefaultHint(Hint),
    DriverMode(oneshot::Sender<DriverMode>),
    SetDriverMode(DriverMode),
    DitherMode(oneshot::Sender<DitherMode>),
    SetDitherMode(DitherMode),
    RedrawDelay(oneshot::Sender<u16>),
    SetRedrawDelay(u16),
    OffScreenDisable(oneshot::Sender<bool>),
    SetOffScreenDisable(bool),
    OffScreenOverride(oneshot::Sender<String>),
}

pub enum Window {
    Add {
        app_key: String,
        title: String,
        area: Rect,
        hint: Option<Hint>,
        visible: bool,
        z_index: i32,
        reply: oneshot::Sender<String>,
    },
    Update {
        win_key: String,
        title: Option<String>,
        area: Option<Rect>,
        hint: Option<Option<Hint>>,
        visible: Option<bool>,
        z_index: Option<i32>,
    },
    Remove(String),
}

pub trait CommandStr {
    fn get_command_str(&self) -> String;
}

impl CommandStr for Command {
    fn get_command_str(&self) -> String {
        use self::Command::*;

        match self {
            Application(a) => format!("Window::{}", a.get_command_str()),
            Dump(_) => "Dump".into(),
            FbDumpToDir(_) => "FrameBufferDumpToDir".into(),
            GlobalRefresh => "GlobalRefresh".into(),
            Property(p) => format!("Property::{}", p.get_command_str()),
            SetMode(_, _, _) => "SetMode".into(),
            Window(w) => format!("Window::{}", w.get_command_str()),
            OffScreen(_, _) => "OffScreen".into(),
        }
    }
}

impl CommandStr for Application {
    fn get_command_str(&self) -> String {
        match self {
            Self::Add(p, _) => format!("Add({p})"),
            Self::Remove(k) => format!("Remove({k})"),
        }
    }
}

impl CommandStr for Property {
    fn get_command_str(&self) -> String {
        use self::Property::*;
        match self {
            DefaultHint(_) => "DefaultHint::Get".into(),
            SetDefaultHint(_) => "DefaultHint::Set".into(),
            DriverMode(_) => "DriverMode::Get".into(),
            SetDriverMode(_) => "DriverMode::Set".into(),
            DitherMode(_) => "DitherMode::Get".into(),
            SetDitherMode(_) => "DitherMode::Set".into(),
            RedrawDelay(_) => "RedrawDelay::Get".into(),
            SetRedrawDelay(_) => "RedrawDelay::Set".into(),
            OffScreenDisable(_) => "OffScreenDisable::Get".into(),
            SetOffScreenDisable(_) => "OffScreenDisable::Set".into(),
            OffScreenOverride(_) => "OffScreenOverride".into(),
        }
    }
}

impl CommandStr for Window {
    fn get_command_str(&self) -> String {
        match self {
            Self::Add { app_key, .. } => format!("Add({app_key})"),
            Self::Update { win_key, .. } => format!("Update({win_key})"),
            Self::Remove(k) => format!("Remove({k})"),
        }
    }
}

impl From<Application> for Command {
    fn from(value: Application) -> Self {
        Self::Application(value)
    }
}

impl From<Property> for Command {
    fn from(value: Property) -> Self {
        Self::Property(value)
    }
}

impl From<Window> for Command {
    fn from(value: Window) -> Self {
        Self::Window(value)
    }
}

pub struct CommandSender(mpsc::Sender<Command>);

impl CommandSender {
    async fn do_send(&self, cmd: Command, ctx: &String) -> anyhow::Result<()> {
        self.0
            .send(cmd)
            .await
            .with_context(|| format!("Failed to send {ctx}"))
    }

    pub async fn send(&self, cmd: impl Into<Command>) -> anyhow::Result<()> {
        let cmd = cmd.into();
        let ctx_str = cmd.get_command_str();

        self.do_send(cmd, &ctx_str).await
    }

    pub async fn with_reply<T>(
        &self,
        cmd: impl Into<Command>,
        reply: oneshot::Receiver<T>,
    ) -> anyhow::Result<T> {
        let cmd = cmd.into();
        let ctx_str = cmd.get_command_str();

        self.do_send(cmd, &ctx_str).await?;
        reply
            .await
            .with_context(|| format!("Failed to get reply from {ctx_str}"))
    }
}

impl From<mpsc::Sender<Command>> for CommandSender {
    fn from(value: mpsc::Sender<Command>) -> Self {
        Self(value)
    }
}
