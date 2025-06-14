use nix::libc::pid_t;
use pinenote_service::types::{rockchip_ebc::Hint, Rect};
use tokio::sync::oneshot;

pub enum Command {
    GlobalRefresh,
    AddApplication(pid_t, oneshot::Sender<String>),
    RemoveApplication(String),
    AddWindow {
        app_key: String,
        title: String,
        area: Rect,
        hint: Option<Hint>,
        visible: bool,
        z_index: i32,
        ret: oneshot::Sender<String>
    },
    UpdateWindow {
        win_key: String,
        title: Option<String>,
        area: Option<Rect>,
        hint: Option<Option<Hint>>,
        visible: Option<bool>,
        z_index: Option<i32>
    },
    RemoveWindow(String),
    Property(Property),
    FbDumpToDir(String),
    Dump(String)
}

impl Command {
    pub fn get_context_str(&self) -> &'static str {
        use self::Command::*;

        match self {
            GlobalRefresh => "GlobalRefresh",
            AddApplication(_, _) => "AddApplication",
            RemoveApplication(_) => "RemoveApplication",
            AddWindow { .. } => "AddWindow",
            UpdateWindow { .. } => "UpdateWindow",
            RemoveWindow(_) => "RemoveWindow",
            Property(p) => p.get_context_str(),
            FbDumpToDir(_) => "FrameBufferDumpToDir",
            Dump(_) => "Dump"
        }
    }
}

pub enum Property {
    DefaultHint(oneshot::Sender<Hint>),
    SetDefaultHint(Hint)
}

impl Property {
    fn get_context_str(&self) -> &'static str {
        use self::Property::*;

        match self {
            DefaultHint(_) => "Property::GetDefaultHint",
            SetDefaultHint(_) => "Property::SetDefaultHint",
        }
    }
}

impl From<Property> for Command {
    fn from(value: Property) -> Self {
        Self::Property(value)
    }
}

