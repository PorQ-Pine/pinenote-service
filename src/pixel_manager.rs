//! Pixel Management
//!
//! [PixelManager] goal is to compute a set of visible rectangle for the [driver] to consume. These
//! rectangle describe what [rendering mode] a given region should use.
//!
//! # Application and Window
//!
//! [Application] are not really important. They aims to be a convenient way to find every windows
//! for a given process, and to hold a default render mode applied to every window for this
//! process. If neither the Application hint or the Window hint are set, then the global
//! default_hint will be used instead.
//!
//! A [Window] represent what's being rendered. Every Window is linked to an Application, has a
//! unique identifier and an rectangular area. A window can additionally have a hint, in which case
//! it will be used instead of the per-application one, or the global hint if both are unset.
//!
//! # Render Hint computation
//!
//! The [RectHint] the driver will received are minimized to offload the work done by the kernel in
//! userland. This minimization is achieved by putting every window area in a Z-indexed tree, and
//! reducing overlapping regions, or even removing them altogether.
//!
//! The final output aims to respect two goals:
//! 1. Minimizing the amount of [RectHint] sent
//! 2. Minimizing the size of every [RectHint] sent
//!
//! These two are somewhat incompatible, since to minimize the size you would need to split them so
//! that only their visible area is sent to the driver. To prevent proliferation of rectangles, any
//! given window will only produce one rectangle[^rec_per_win]. However the rectangle produce is
//! always the bounding box of the window's visible area.
//!
//! ## Example
//!
//! Given the following windows:
//! |Z-INDEX| Dimension |Representation|
//! |-------|-----------|--------------|
//! | 0     | {0,0,5,3} | A            |
//! | 1     | {2,0,4,2} | B            |
//! | 2     | {0,2,5,3} | C            |
//!
//! The screen would look something like this:
//! ```txt
//! AABBA
//! AABBA
//! CCCCC
//! ```
//!
//! - C doesn't overlap with B, so B will be sent as-is.
//! - C overlap with A, and fully cover the bottom region, so A rectangle can be reduced.
//! - B overlap with A, but part of A can be both on the left and right, so A cannot be reduced
//!
//! The final dimension for A is computed as {0,0,5,2}.
//!
//! [^rec_per_win]: In the future, a Window may produce more than one rectangle, if it has
//! sub-surfaces with different hints.
//!
//! # TODOs:
//! - Allow finding an Application using its pid.
//! - Allow retrieving a list of window keys for a given application
//! - Implement window sub-surfaces for more precise pixel management.
//! - Partial refresh
//!
//! [driver]: crate::drivers::rockchip_ebc::RockchipEbc
//! [rendering mode]: crate::types::rockchip_ebc::Hint

use std::collections::{HashMap, HashSet};
use thiserror::Error;

use nix::libc::pid_t;

use crate::types::{
    Rect,
    rockchip_ebc::{Hint, RectHint},
    ztree::{ZSurface, ZTree},
};

/// Application representation.
///
/// This struct represent a running process, and hold the default configuration for any of the
/// process windows for which it wasn't overridden.
#[derive(Debug)]
pub struct Application {
    app_id: String,
    pid: pid_t,
    default_hint: Option<Hint>,
    windows: HashSet<String>,
}

impl Application {
    /// Create a new [Application] using global default hint.
    pub fn new(app_id: impl Into<String>, pid: pid_t) -> Self {
        Self {
            app_id: app_id.into(),
            pid,
            default_hint: None,
            windows: Default::default(),
        }
    }

    /// Create a new Application, setting its default hint configuration
    pub fn with_hint(app_id: impl Into<String>, pid: pid_t, default_hint: Option<Hint>) -> Self {
        Self {
            app_id: app_id.into(),
            pid,
            default_hint,
            windows: Default::default(),
        }
    }

    /// Return the application unique Key.
    fn key(&self) -> String {
        format!("{}:{}", self.app_id, self.pid)
    }

    /// Add a window to the Application
    fn window_add(&mut self, win_uid: String) {
        self.windows.insert(win_uid);
    }

    /// Delete an Application window
    fn window_remove(&mut self, win_uid: &String) {
        self.windows.remove(win_uid);
    }
}

#[derive(Clone, Debug)]
pub struct WindowData {
    pub title: String,
    pub area: Rect,
    pub hint: Option<Hint>,
    pub visible: bool,
    pub z_index: i32,
}

/// Represent an on-screen window
///
// TODO: Implement subsurfaces
// TODO: Implement per title search
#[derive(Debug)]
pub struct Window {
    uid: String,
    app_key: String,
    pub data: WindowData, //sub_surface: Vec<Surface>
}

impl Window {
    pub fn new(
        app_key: impl Into<String>,
        title: impl Into<String>,
        area: Rect,
        hint: Option<Hint>,
        visible: bool,
        z_index: i32,
    ) -> Self {
        let uid = uuid::Uuid::new_v4().to_string();
        let app_key = app_key.into();
        let title = title.into();

        Self {
            uid,
            app_key,
            data: WindowData {
                title,
                area,
                hint,
                visible,
                z_index,
            },
        }
    }

    pub fn zsurface(&self, screen_area: &Rect) -> Option<ZSurface> {
        if self.data.visible {
            self.data
                .area
                .intersection(screen_area)
                .map(|rect| ZSurface::new(self.data.z_index, self.uid.clone(), rect))
        } else {
            None
        }
    }

    pub fn update(&mut self, data: WindowData) {
        self.data = data
    }
}

#[derive(Debug, PartialEq, Default)]
pub struct ComputedHints {
    pub default_hint: Option<Hint>,
    pub rect_hints: Vec<RectHint>,
}

impl ComputedHints {
    pub fn new() -> Self {
        Self {
            default_hint: None,
            rect_hints: Default::default(),
        }
    }

    pub fn with_hint(hint: Hint) -> Self {
        Self {
            default_hint: Some(hint),
            ..Default::default()
        }
    }
}

/// Manage per pixel hints
#[derive(Debug)]
pub struct PixelManager {
    /// Default Hints to use for uncovered pixels
    pub default_hint: Hint,
    /// Rectangle representing the full screen.
    screen_area: Rect,

    applications: HashMap<String, Application>,
    windows: HashMap<String, Window>,
}

#[derive(Error, Debug, PartialEq)]
pub enum PixelManagerError {
    #[error("No application with key '{0}' found.")]
    UnknownApp(String),
    #[error("No window with uid '{0}' found")]
    UnknownWindow(String),
}

impl PixelManager {
    pub fn new(default_hint: Hint, screen_area: Rect) -> Self {
        Self {
            default_hint,
            screen_area,
            applications: Default::default(),
            windows: Default::default(),
        }
    }

    pub fn app(&self, app_key: &String) -> Result<&Application, PixelManagerError> {
        self.applications
            .get(app_key)
            .ok_or(PixelManagerError::UnknownApp(app_key.to_owned()))
    }

    pub fn app_mut(&mut self, app_key: &String) -> Result<&mut Application, PixelManagerError> {
        self.applications
            .get_mut(app_key)
            .ok_or(PixelManagerError::UnknownApp(app_key.to_owned()))
    }

    /// Add a new Application to the controller
    pub fn app_add(&mut self, app: Application) -> String {
        let key = app.key();

        if !self.applications.contains_key(&key) {
            self.applications.insert(key.clone(), app);
        }

        key
    }

    /// Remove an Application and its associated Window.
    pub fn app_remove(&mut self, app_key: &String) {
        let Some(app) = self.applications.remove(app_key) else {
            return;
        };

        for win_key in app.windows {
            self.windows.remove(&win_key);
        }
    }

    /// Access default hint for a specif app
    pub fn app_hint(&self, app_key: &String) -> Result<Option<Hint>, PixelManagerError> {
        self.app(app_key).map(|a| a.default_hint)
    }

    /// Update default hint associated with the Application.
    pub fn app_set_hint(&mut self, app_key: &String, hint: Hint) -> Result<(), PixelManagerError> {
        let app = self.app_mut(app_key)?;

        app.default_hint = Some(hint);

        Ok(())
    }

    /// Remove default hint associated with the application.
    pub fn app_unset_hint(&mut self, app_key: &String) -> Result<(), PixelManagerError> {
        let app = self.app_mut(app_key)?;

        app.default_hint = None;

        Ok(())
    }

    pub fn window(&self, win_key: &String) -> Result<&Window, PixelManagerError> {
        self.windows
            .get(win_key)
            .ok_or(PixelManagerError::UnknownWindow(win_key.to_owned()))
    }

    pub fn window_mut(&mut self, win_key: &String) -> Result<&mut Window, PixelManagerError> {
        self.windows
            .get_mut(win_key)
            .ok_or(PixelManagerError::UnknownWindow(win_key.to_owned()))
    }

    /// Add a new window, and link it to an application.
    pub fn window_add(&mut self, window: Window) -> Result<String, PixelManagerError> {
        let app_key = window.app_key.clone();
        let uid = window.uid.clone();

        if !self.applications.contains_key(&app_key) {
            Err(PixelManagerError::UnknownApp(app_key.clone()))?;
        };

        if !self.windows.contains_key(&window.uid) {
            self.windows.insert(uid.clone(), window);

            self.applications
                .entry(app_key.to_owned())
                .and_modify(|a| a.window_add(uid.clone()));
        }

        Ok(uid)
    }

    /// Remove a window using its key.
    pub fn window_remove(&mut self, win_uid: String) {
        let Some(win) = self.windows.remove(&win_uid) else {
            return;
        };
        let app_key = win.app_key;

        self.applications
            .entry(app_key)
            .and_modify(|a| a.window_remove(&win_uid));
    }

    pub fn window_update(
        &mut self,
        win_key: &String,
        data: WindowData,
    ) -> Result<(), PixelManagerError> {
        let window = self.window_mut(win_key)?;

        window.update(data);

        Ok(())
    }

    /// Set a window specific hint.
    pub fn window_set_hint(
        &mut self,
        win_key: &String,
        hint: Hint,
    ) -> Result<(), PixelManagerError> {
        let win = self.window_mut(win_key)?;

        win.data.hint = Some(hint);

        Ok(())
    }

    /// Unset a window specific hint.
    pub fn window_unset_hint(&mut self, win_key: &String) -> Result<(), PixelManagerError> {
        let win = self.window_mut(win_key)?;

        win.data.hint = None;

        Ok(())
    }

    pub fn window_hint(&self, win_key: &String) -> Result<Option<Hint>, PixelManagerError> {
        self.window(win_key).map(|w| w.data.hint)
    }

    pub fn window_hint_fallback(&self, win_key: &String) -> Result<Hint, PixelManagerError> {
        let win = self.window(win_key)?;

        if let Some(hint) = win.data.hint {
            Ok(hint)
        } else {
            let app = self.app(&win.app_key)?;
            Ok(app.default_hint.unwrap_or(self.default_hint))
        }
    }

    /// Compute visible RectHint.
    pub fn compute_hints(&self) -> Result<ComputedHints, PixelManagerError> {
        let mut ret = ComputedHints::with_hint(self.default_hint);

        let ztree = self
            .windows
            .values()
            .filter_map(|w| w.zsurface(&self.screen_area))
            .fold(ZTree::new(), |mut tree, s| {
                tree.insert(s);
                tree
            });

        ret.rect_hints = ztree
            .flatten()
            .into_iter()
            .map(
                |ZSurface {
                     area: rect,
                     reference,
                     ..
                 }| {
                    self.window_hint_fallback(&reference)
                        .map(|hint| RectHint { rect, hint })
                },
            )
            .collect::<Result<_, _>>()?;

        Ok(ret)
    }
}

#[cfg(test)]
#[allow(dead_code)]
mod tests {
    use super::*;
    use crate::types::{
        Rect,
        rockchip_ebc::{Hint, HintBitDepth as BitDepth, HintConvertMode},
    };

    const Y4DITHER_REDRAW: Hint = Hint::new(BitDepth::Y4, HintConvertMode::Dither, true);
    const Y4DITHER: Hint = Hint::new(BitDepth::Y4, HintConvertMode::Dither, false);
    const Y2DITHER_REDRAW: Hint = Hint::new(BitDepth::Y4, HintConvertMode::Dither, true);
    const Y2DITHER: Hint = Hint::new(BitDepth::Y4, HintConvertMode::Dither, false);

    const SCREEN_RECT: Rect = Rect::new(0, 0, 1872, 1404);

    fn setup_manager() -> PixelManager {
        PixelManager::new(Y4DITHER_REDRAW, SCREEN_RECT.clone())
    }

    #[test]
    fn empty_sets_default() {
        let mut mgr = setup_manager();

        let expected = ComputedHints::with_hint(Y4DITHER_REDRAW);

        assert_eq!(expected, mgr.compute_hints().unwrap());

        let expected = ComputedHints::with_hint(Y2DITHER_REDRAW);

        mgr.default_hint = Y2DITHER_REDRAW;

        assert_eq!(expected, mgr.compute_hints().unwrap());
    }

    #[test]
    fn clip_window_to_screen() -> Result<(), PixelManagerError> {
        let mut mgr = setup_manager();

        let app = Application::new("testapp", 1234);
        let app_key = mgr.app_add(app);

        let win_rect = Rect::new(1000, 1000, 2000, 1600);
        let win_hint = Y2DITHER;

        let win = Window::new(
            app_key,
            "TestWindow",
            win_rect.clone(),
            Some(win_hint),
            true,
            0,
        );

        mgr.window_add(win)?;

        let expected = ComputedHints {
            default_hint: Some(mgr.default_hint),
            rect_hints: vec![RectHint {
                hint: win_hint,
                rect: Rect::new(1000, 1000, SCREEN_RECT.x2, SCREEN_RECT.y2),
            }],
        };

        assert_eq!(expected, mgr.compute_hints()?);

        Ok(())
    }

    #[test]
    fn window_add_noapp_fails() -> Result<(), PixelManagerError> {
        let mut mgr = setup_manager();

        let win = Window::new(
            "test_app:1234",
            "test_win",
            Rect::new(100, 100, 200, 200),
            Some(Y2DITHER),
            true,
            0,
        );

        assert_eq!(
            Err(PixelManagerError::UnknownApp("test_app:1234".to_string())),
            mgr.window_add(win)
        );

        Ok(())
    }

    #[test]
    fn one_window() -> Result<(), PixelManagerError> {
        let mut mgr = setup_manager();

        let win_rect = Rect::new(100, 100, 500, 600);
        let win_hint = Y2DITHER;

        let expected = ComputedHints {
            default_hint: Some(mgr.default_hint),
            rect_hints: vec![RectHint {
                rect: win_rect.clone(),
                hint: win_hint,
            }],
        };

        let app = Application::new("testapp", 1234);
        let app_key = mgr.app_add(app);

        let win = Window::new(
            app_key,
            "TestWindow",
            win_rect.clone(),
            Some(win_hint),
            true,
            0,
        );

        mgr.window_add(win)?;

        assert_eq!(expected, mgr.compute_hints()?);

        Ok(())
    }

    #[test]
    fn hidden_window() -> Result<(), PixelManagerError> {
        let mut mgr = setup_manager();

        let win_rect = Rect::new(100, 100, 500, 600);
        let win_hint = Y2DITHER;

        let expected = ComputedHints {
            default_hint: Some(mgr.default_hint),
            rect_hints: vec![],
        };

        let app = Application::new("testapp", 1234);
        let app_key = mgr.app_add(app);

        let win = Window::new(
            app_key,
            "TestWindow",
            win_rect.clone(),
            Some(win_hint),
            false,
            0,
        );

        mgr.window_add(win)?;

        assert_eq!(expected, mgr.compute_hints()?);

        Ok(())
    }

    #[test]
    fn app_hint_fallback() -> Result<(), PixelManagerError> {
        let mut mgr = setup_manager();

        let win_rect = Rect::new(100, 100, 500, 500);

        let app_hint = Y2DITHER;

        let expected = ComputedHints {
            default_hint: Some(mgr.default_hint),
            rect_hints: vec![RectHint {
                rect: win_rect.clone(),
                hint: app_hint,
            }],
        };

        let app_key = mgr.app_add(Application::with_hint("testapp", 1234, Some(app_hint)));

        mgr.window_add(Window::new(
            app_key,
            "TestWindow",
            win_rect.clone(),
            None,
            true,
            0,
        ))?;

        assert_eq!(expected, mgr.compute_hints()?);

        Ok(())
    }

    #[test]
    fn global_hint_fallback() -> Result<(), PixelManagerError> {
        let mut mgr = setup_manager();

        let win_rect = Rect::new(100, 100, 500, 500);

        let expected = ComputedHints {
            default_hint: Some(Y4DITHER_REDRAW),
            rect_hints: vec![RectHint {
                rect: win_rect.clone(),
                hint: Y4DITHER_REDRAW,
            }],
        };

        let app_key = mgr.app_add(Application::new("testapp", 1234));

        mgr.window_add(Window::new(
            app_key,
            "TestWindow",
            win_rect.clone(),
            None,
            true,
            0,
        ))?;

        assert_eq!(expected, mgr.compute_hints()?);

        Ok(())
    }

    #[test]
    fn respect_z_indexing() -> Result<(), PixelManagerError> {
        let mut mgr = setup_manager();

        let app_key = mgr.app_add(Application::new("testapp", 1234));
        let rect_hint1 = RectHint {
            rect: Rect::new(100, 100, 500, 500),
            hint: Y2DITHER,
        };
        let window1 = Window::new(
            app_key,
            "TestWindow1",
            rect_hint1.rect.clone(),
            Some(rect_hint1.hint),
            true,
            5,
        );
        mgr.window_add(window1)?;

        let rect_hint2 = RectHint {
            rect: Rect::new(100, 100, 600, 600),
            hint: Y2DITHER_REDRAW,
        };
        let app_key = mgr.app_add(Application::new("testapp", 1235));
        let window2 = Window::new(
            app_key,
            "TestWindow2",
            rect_hint2.rect.clone(),
            Some(rect_hint2.hint),
            true,
            3,
        );
        mgr.window_add(window2)?;

        let rect_hint3 = RectHint {
            rect: Rect::new(0, 0, 400, 400),
            hint: Y4DITHER,
        };
        let app_key = mgr.app_add(Application::new("testapp", 1236));
        let window3 = Window::new(
            app_key,
            "TestWindow3",
            rect_hint3.rect.clone(),
            Some(rect_hint3.hint),
            true,
            4,
        );
        mgr.window_add(window3)?;

        let expected = ComputedHints {
            default_hint: Some(mgr.default_hint),
            rect_hints: vec![rect_hint2, rect_hint3, rect_hint1],
        };

        assert_eq!(expected, mgr.compute_hints()?);

        Ok(())
    }

    #[test]
    fn remove_hidden_window() -> Result<(), PixelManagerError> {
        let mut mgr = setup_manager();

        let app_key = mgr.app_add(Application::new("testapp", 1234));
        let rect_hint1 = RectHint {
            rect: Rect::new(100, 100, 500, 500),
            hint: Y2DITHER,
        };
        let window1 = Window::new(
            app_key,
            "TestWindow1",
            rect_hint1.rect.clone(),
            Some(rect_hint1.hint),
            true,
            0,
        );
        mgr.window_add(window1)?;

        let rect_hint2 = RectHint {
            rect: Rect::new(100, 100, 600, 600),
            hint: Y2DITHER_REDRAW,
        };
        let app_key = mgr.app_add(Application::new("testapp", 1235));
        let window2 = Window::new(
            app_key,
            "TestWindow2",
            rect_hint2.rect.clone(),
            Some(rect_hint2.hint),
            true,
            1,
        );
        mgr.window_add(window2)?;

        let expected = ComputedHints {
            default_hint: Some(mgr.default_hint),
            rect_hints: vec![rect_hint2],
        };

        assert_eq!(expected, mgr.compute_hints()?);

        Ok(())
    }
}
