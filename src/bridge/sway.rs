use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use futures_lite::stream::StreamExt;
use nalgebra::Matrix3;
use nix::libc::pid_t;
use pinenote_service::types::{Rect, rockchip_ebc::Hint};
use swayipc_async::{
    Connection, Event, EventStream, EventType, Node, NodeBorder, NodeType, Rect as SwayRect,
};
use tokio::sync::{mpsc::Sender, oneshot};

use crate::ebc;

mod utils;

#[derive(Debug, PartialEq)]
struct SwayWindow {
    id: i64,
    pid: pid_t,
    title: String,
    area: Rect,
    visible: bool,
    floating: bool,
    fullscreen: bool,
    hint: Option<Hint>,
    z_index: i32,
}

impl SwayWindow {
    fn diff(&self, other: &Self) -> Option<ebc::WindowUpdate> {
        if self != other {
            let &Self {
                ref title,
                ref area,
                visible,
                fullscreen,
                hint,
                z_index,
                ..
            } = other;

            Some(ebc::WindowUpdate {
                title: if &self.title != title {
                    Some(title.clone())
                } else {
                    None
                },
                area: if &self.area != area {
                    Some(area.clone())
                } else {
                    None
                },
                visible: if self.visible != visible {
                    Some(visible)
                } else {
                    None
                },
                fullscreen: if self.fullscreen != fullscreen {
                    Some(fullscreen)
                } else {
                    None
                },
                hint: if self.hint != hint { Some(hint) } else { None },
                z_index: if self.z_index != z_index {
                    Some(z_index)
                } else {
                    None
                },
            })
        } else {
            None
        }
    }
}

pub struct SwayWindowError;

impl TryFrom<&Node> for SwayWindow {
    type Error = SwayWindowError;

    fn try_from(node: &Node) -> std::result::Result<Self, Self::Error> {
        let Some(_shell) = node.shell else {
            return Err(SwayWindowError);
        };
        let Some(visible) = node.visible else {
            return Err(SwayWindowError);
        };
        let Some(pid) = node.pid else {
            return Err(SwayWindowError);
        };

        let SwayRect {
            x,
            mut y,
            width,
            mut height,
            ..
        } = node.rect;

        if node.node_type == NodeType::FloatingCon && node.border == NodeBorder::Normal {
            y -= node.deco_rect.height;
            height += node.deco_rect.height;
        }

        let title = node.name.as_deref().unwrap_or("NO_TITLE").to_owned();

        let area = Rect::from_xywh(x, y, width, height);

        let hint = node.marks.iter().find_map(|m| {
            if m.starts_with("ebchint:") || m.starts_with("_ebchint:") {
                m.split(':')
                    .nth(2)
                    .and_then(|s| Hint::try_from_human_readable(s).ok())
            } else {
                None
            }
        });

        Ok(Self {
            id: node.id,
            pid,
            title,
            area,
            visible,
            floating: node.node_type == NodeType::FloatingCon,
            fullscreen: node.fullscreen_mode.unwrap_or_default() != 0,
            hint,
            z_index: 0,
            //_data
        })
    }
}

pub struct SwayBridge {
    swayipc: Connection,
    swayevents: EventStream,
    transform: Matrix3<f64>,
    app_meta: HashMap<pid_t, (String, HashSet<i64>)>,
    window_meta: HashMap<i64, (String, SwayWindow)>,
}

impl SwayBridge {
    const OUTPUT_NAME: &str = "DPI-1";

    pub async fn new() -> Result<Self> {
        let mut swayipc = Connection::new()
            .await
            .context("Failed to connect to Sway IPC")?;

        let transform = utils::get_output(&mut swayipc, Self::OUTPUT_NAME)
            .await
            .and_then(|o| utils::output_to_transform(&o))?;

        let events = vec![
            EventType::Output,
            EventType::Window,
            EventType::Workspace,
            EventType::Shutdown,
        ];

        let swayevents = Connection::new()
            .await
            .context("Failed to connect to Sway IPC")?
            .subscribe(events)
            .await
            .context("Failed to subscibe to Sway Event")?;

        Ok(Self {
            swayipc,
            swayevents,
            transform,
            app_meta: Default::default(),
            window_meta: Default::default(),
        })
    }

    /// Add an application
    async fn add_app(&mut self, pid: pid_t, tx: &mut ebc::CommandSender) -> Result<()> {
        let (ret_tx, ret_rx) = oneshot::channel::<String>();
        let app_key = tx
            .with_reply(ebc::command::Application::Add(pid, ret_tx), ret_rx)
            .await
            .context("Failed to add application '{pid}'")?;

        self.app_meta
            .insert(pid, (app_key.clone(), Default::default()));

        Ok(())
    }

    /// Remove stale app from the app_meta map, notifying the EbcService in the process.
    async fn remove_stale_apps(
        &mut self,
        stale_pid: Vec<pid_t>,
        tx: &mut ebc::CommandSender,
    ) -> Result<()> {
        for p in stale_pid {
            let Some((app_key, win_ids)) = self.app_meta.remove(&p) else {
                continue;
            };

            tx.send(ebc::command::Application::Remove(app_key))
                .await
                .context("Failed to send remove '{app_key}'")?;

            for id in win_ids {
                self.window_meta.remove(&id);
            }
        }

        Ok(())
    }

    /// Add a new window
    async fn add_window(&mut self, win: SwayWindow, tx: &mut ebc::CommandSender) -> Result<()> {
        let (rtx, rx) = oneshot::channel::<String>();

        let app_meta = self
            .app_meta
            .get_mut(&win.pid)
            .expect("Window should be added after apps");
        let app_key = app_meta.0.clone();

        let cmd = ebc::command::Window::Add {
            app_key,
            title: win.title.clone(),
            area: win.area.clone(),
            hint: win.hint,
            visible: win.visible,
            fullscreen: win.fullscreen,
            z_index: win.z_index,
            reply: rtx,
        };

        let win_key = tx
            .with_reply(cmd, rx)
            .await
            .with_context(|| "Failed to add window '{title}")?;

        let id = win.id;
        self.window_meta.insert(id, (win_key, win));
        app_meta.1.insert(id);

        Ok(())
    }

    /// Update Window
    async fn update_window(
        &mut self,
        up_win: SwayWindow,
        tx: &mut ebc::CommandSender,
    ) -> Result<()> {
        let &mut (ref win_key, ref mut win) = self.window_meta.get_mut(&up_win.id).unwrap();

        if let Some(update) = win.diff(&up_win) {
            tx.send(ebc::command::Window::Update {
                win_key: win_key.clone(),
                update,
            })
            .await
            .context("Failed to update window '{win_key}'")?;

            self.window_meta
                .entry(up_win.id)
                .and_modify(|e| e.1 = up_win);
        }

        Ok(())
    }

    async fn remove_stale_windows(
        &mut self,
        stale_id: Vec<i64>,
        tx: &mut ebc::CommandSender,
    ) -> Result<()> {
        for wid in stale_id {
            let Some((win_key, win)) = self.window_meta.remove(&wid) else {
                continue;
            };

            tx.send(ebc::command::Window::Remove(win_key))
                .await
                .context("Failed to remove window '{win_key}'")?;

            self.app_meta.entry(win.pid).and_modify(|e| {
                e.1.remove(&wid);
            });
        }

        Ok(())
    }

    async fn process_tree(&mut self, tx: &mut ebc::CommandSender) -> Result<()> {
        let swaytree = self
            .swayipc
            .get_tree()
            .await
            .context("Failed to get Sway Tree")?;

        let Some(output) = swaytree.find_as_ref(|n| {
            n.node_type == NodeType::Output && n.name.as_deref().unwrap_or("") == Self::OUTPUT_NAME
        }) else {
            bail!("Could not find output '{}'", Self::OUTPUT_NAME)
        };
        let Some(workspace) = output.find_focused_as_ref(|n| n.node_type == NodeType::Workspace)
        else {
            bail!("No focused workspace for output '{}", Self::OUTPUT_NAME)
        };

        let (pid_set, windows) = utils::get_all_windows_and_app(workspace, &self.transform);

        let stale_pid: Vec<pid_t> = self
            .app_meta
            .keys()
            .filter(|k| !pid_set.contains(k))
            .copied()
            .collect();

        self.remove_stale_apps(stale_pid, tx)
            .await
            .context("SwayBridge::remove_stale_apps failed")?;

        for pid in pid_set {
            if !self.app_meta.contains_key(&pid) {
                self.add_app(pid, tx)
                    .await
                    .context("SwayBridge::add_app failed")?;
            }
        }

        let mut win_set: HashSet<i64> = HashSet::new();
        for w in windows {
            win_set.insert(w.id);

            if !self.window_meta.contains_key(&w.id) {
                self.add_window(w, tx)
                    .await
                    .context("SwayBridge::add_window failed")?;
            } else {
                self.update_window(w, tx)
                    .await
                    .context("SwayBridge::update_window failed")?;
            }
        }

        let stale_window = self
            .window_meta
            .keys()
            .filter(|k| !win_set.contains(k))
            .copied()
            .collect();

        self.remove_stale_windows(stale_window, tx)
            .await
            .context("SwayBridge::remove_stale_window failed")?;

        Ok(())
    }

    pub async fn run(&mut self, tx: Sender<ebc::Command>) -> Result<()> {
        let mut tx: ebc::CommandSender = tx.into();
        let mut process_tree = true;

        loop {
            if process_tree {
                if let Err(e) = self
                    .process_tree(&mut tx)
                    .await
                    .context("Failed to process_tree")
                {
                    eprintln!("{e:?}");
                };
                process_tree = false;
            }

            match tokio::time::timeout(Duration::from_millis(100), self.swayevents.next()).await {
                Err(_) => process_tree = true,
                Ok(Some(evt)) => {
                    let event = evt?;

                    match event {
                        Event::Shutdown(_) => break,
                        Event::Window(_) => {
                            process_tree = true;
                        }
                        Event::Output(_) => {
                            match utils::get_output(&mut self.swayipc, Self::OUTPUT_NAME)
                                .await
                                .and_then(|o| utils::output_to_transform(&o))
                            {
                                Ok(t) => {
                                    self.transform = t;
                                }
                                Err(e) => {
                                    self.transform = Matrix3::identity();
                                    eprintln!("{e:#?}");
                                }
                            }
                            process_tree = true;
                        }
                        Event::Workspace(_) => {
                            process_tree = true;
                        }
                        _ => {}
                    }
                }
                Ok(None) => {
                    eprintln!("SwayIPC EventStream is done");
                    break;
                }
            }
        }

        eprintln!("Implement proper exit");

        Ok(())
    }
}
