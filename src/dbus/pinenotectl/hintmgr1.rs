use nix::libc::pid_t;
use pinenote_service::types::{Rect, rockchip_ebc::Hint};
use serde::Deserialize;
use tokio::sync::{mpsc, oneshot};
use zbus::{
    fdo, interface,
    zvariant::{Type, Value},
};

use crate::{dbus, ebc};

#[derive(Type, Value, Deserialize)]
struct Window {
    title: String,
    area: Rect,
    hint: String,
    visible: bool,
    z_index: i32,
}

pub struct HintMgr1 {
    tx: ebc::CommandSender,
}

impl HintMgr1 {
    pub fn new(tx: mpsc::Sender<ebc::Command>) -> Self {
        Self { tx: tx.into() }
    }

    async fn send_win(&self, win: ebc::Window) -> fdo::Result<()> {
        self.tx.send(win).await.map_err(dbus::internal_error)
    }
}

fn parse_hint(hint: String) -> fdo::Result<Option<Hint>> {
    let ret = if hint.is_empty() {
        None
    } else {
        Some(
            Hint::try_from_human_readable(hint.as_str())
                .map_err(|_| fdo::Error::InvalidArgs(format!("Unrecognized Hint {hint}")))?,
        )
    };

    Ok(ret)
}

fn validate_rect(rect: Rect) -> fdo::Result<Rect> {
    let Rect { x1, y1, x2, y2 } = rect;

    if  x1 < 0 || y1 < 0 || x1 > x2 || y1 > y2 {
        Err(fdo::Error::InvalidArgs("Bad Rectangle".into()))
    } else { Ok(Rect { x1, y1, x2, y2 })}
}

/// DBus interface to manage per Window Hints.
///
/// # Window Attributes
/// A Window is described by the following attributes:
/// | Attr    | Signature | Description                         |
/// |---------|-----------|-------------------------------------|
/// | title   | s         | The window title, currently unused. |
/// | area    | (iiii)    | Area occupied by the window.        |
/// | hint    | s         | Window's rendering hints            |
/// | visible | b         | Whether the window is visible       |
/// | z-index | i         | Arbitrary z-index. Higher is above. |
///
/// ## Area
/// The window area is described by a rectangle defined by two points, and
/// represented by 4 int32. The first pair represent the top-left corner
/// coordinates, the second represent the bottom right corner.
///
/// ## Hint
/// The rendering hints are represented by a human readable string with the
/// following format:
/// <bitdepth>[|<convert>][|<redraw>]
///
/// `bitdepth` can be one of:
/// - Y1: 1bpp rendering (B/W)
/// - Y2: 2bpp rendering (4 value grayscale)
/// - Y4: 4bpp rendering (16 value grayscale)
///
/// `convert` defines how the rgb values are converted, and can be one of:
/// - T: Thresholding - Uses a hard threshold to lower the bitdepth (default)
/// - D: Dithering - Uses a dithering algorithm to approximate higher bit depth
///
/// `redraw` defines whether 2-pass rendering is used to improve responsiveness
/// - R: 2 pass rendering is active
/// - r: 2 pass rendering is inactive (default)
///
/// ## Z-Indexing
/// The z-index represent the relative 'height' of a window. Window with a
/// lower z-index will be considered to be rendered behind ones with a higher
/// z-index. When rendering, only the highest window rendering hint is used.
///
/// Window with a same z-index are assumed not to be overlapping by the service
/// without any validation. It is safe to have overlapping windows at the same
/// z-index, but which window rendering hint will be used is undefined.
#[interface(name = "org.pinenote.HintMgr1")]
impl HintMgr1 {
    /// Register a new Application.
    ///
    /// Application are just a way to manage several window at the same time.
    /// To add a new window, you first have to register a new application with
    /// the service. This will return an arbitrary key that can be used to add
    /// window for that application.
    /// When an application is removed, all remaining window metadata are
    /// discarded.
    async fn app_register(&self, pid: i32) -> fdo::Result<String> {
        let (tx, rx) = oneshot::channel::<String>();

        if pid <= 0 {
            return Err(fdo::Error::UnixProcessIdUnknown(format!("Bad PID {pid}")));
        }

        let pid: pid_t = pid;

        // TODO: Check if getting PID when pid == 0 is possible. My (limited)
        // testing with busctl and dbus-send always returns systemd pid

        self.tx
            .with_reply(ebc::Application::Add(pid, tx), rx)
            .await
            .map_err(dbus::internal_error)
    }

    /// Unregister an application
    ///
    /// This method remove an application and all of its associated window.
    async fn app_remove(&self, app_key: String) -> fdo::Result<()> {
        self.tx
            .send(ebc::Application::Remove(app_key))
            .await
            .map_err(dbus::internal_error)
    }

    /// Adds a new window
    ///
    /// This method register a new Window and specifies its attribute. If the
    /// method succeed, an UUID is returned to refer back to this specific
    /// window.
    async fn window_add(&self, app_key: String, win: Window) -> fdo::Result<String> {
        let (reply, rx) = oneshot::channel::<String>();
        let Window {
            title,
            area,
            hint,
            visible,
            z_index,
        } = win;

        let hint = parse_hint(hint)?;
        let area = validate_rect(area)?;

        let add = ebc::Window::Add {
            app_key,
            title,
            area,
            hint,
            visible,
            z_index,
            reply,
        };
        self.tx
            .with_reply(add, rx)
            .await
            .map_err(dbus::internal_error)
    }

    /// Update every window attribute at once
    ///
    /// Since updating an attribute can cause the service to re-compute all
    /// hints, this method is to be prefered if several attribute should be
    /// updated.
    async fn window_update(&self, win_key: String, win: Window) -> fdo::Result<()> {
        let Window {
            title,
            area,
            hint,
            visible,
            z_index,
        } = win;

        let hint = parse_hint(hint)?;
        let area = validate_rect(area)?;

        let update = ebc::WindowUpdate {
            title: Some(title),
            area: Some(area),
            hint: Some(hint),
            visible: Some(visible),
            z_index: Some(z_index),
        };

        self.send_win(ebc::Window::Update { win_key, update }).await
    }

    /// Update the window title.
    async fn window_update_title(&self, win_key: String, title: String) -> fdo::Result<()> {
        let update = ebc::WindowUpdate {
            title: Some(title),
            ..Default::default()
        };

        self.send_win(ebc::Window::Update { win_key, update }).await
    }

    /// Update the window area
    async fn window_update_area(&self, win_key: String, area: Rect) -> fdo::Result<()> {
        let area = validate_rect(area)?;

        let update = ebc::WindowUpdate {
            area: Some(area),
            ..Default::default()
        };

        self.send_win(ebc::Window::Update { win_key, update }).await
    }

    /// Set or unset the window rendering hints.
    async fn window_update_hint(&self, win_key: String, hint: String) -> fdo::Result<()> {
        let hint = parse_hint(hint)?;

        let update = ebc::WindowUpdate {
            hint: Some(hint),
            ..Default::default()
        };

        self.send_win(ebc::Window::Update { win_key, update }).await
    }

    /// Set or unset the window's visible flag
    async fn window_update_visible(&self, win_key: String, visible: bool) -> fdo::Result<()> {
        let update = ebc::WindowUpdate {
            visible: Some(visible),
            ..Default::default()
        };

        self.send_win(ebc::Window::Update { win_key, update }).await
    }

    /// Set the window's z-index
    async fn window_update_zindex(&self, win_key: String, z_index: i32) -> fdo::Result<()> {
        let update = ebc::WindowUpdate {
            z_index: Some(z_index),
            ..Default::default()
        };

        self.send_win(ebc::Window::Update { win_key, update }).await
    }

    /// Remove a window
    async fn window_remove(&self, key: String) -> fdo::Result<()> {
        self.send_win(ebc::Window::Remove(key)).await
    }
}
