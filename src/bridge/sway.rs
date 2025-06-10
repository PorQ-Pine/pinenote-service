use std::collections::{HashMap, HashSet};

use nix::libc::pid_t;
use pinenote_service::types::Rect;
use swayipc_async::{Connection, Event, EventStream, EventType, Node, NodeBorder, NodeType, Rect as SwayRect};
use futures_lite::stream::StreamExt;
use anyhow::{bail, Context, Result};
use tokio::sync::{mpsc::Sender, oneshot};

use crate::EbcCommand;

#[derive(Debug, PartialEq)]
struct SwayWindow {
    id: i64,
    pid: pid_t,
    title: String,
    area: Rect,
    visible: bool,
    floating: bool,
    _fullscreen: bool,
    z_index: i32,
}

struct SwayWindowDiff {
    title: Option<String>,
    area: Option<Rect>,
    visible: Option<bool>,
    z_index: Option<i32>,
}

impl SwayWindow {
    fn diff(&self, other: &Self) -> Option<SwayWindowDiff> {
        if self != other {
            let &Self { ref title, ref area, visible, z_index, .. } = other;

            Some(SwayWindowDiff {
                title: if &self.title != title { Some(title.clone()) } else { None },
                area: if &self.area != area { Some(area.clone()) } else { None },
                visible: if self.visible != visible { Some(visible) } else { None },
                z_index: if self.z_index != z_index { Some(z_index) } else { None }
            })
        } else { None }
    }
}

pub struct SwayWindowError;

impl TryFrom<&Node> for SwayWindow {
    type Error = SwayWindowError;

    fn try_from(node: &Node) -> std::result::Result<Self, Self::Error> {
        let Some(_shell) = node.shell else { return Err(SwayWindowError) };
        let Some(visible) = node.visible else { return Err(SwayWindowError) };
        let Some(pid) = node.pid else { return Err(SwayWindowError) };

        let SwayRect { x, mut y, width, height, .. } = node.rect;

        if node.node_type == NodeType::FloatingCon && node.border == NodeBorder::Normal {
            y -= node.deco_rect.height
        }

        let title = node.name.as_deref().unwrap_or("NO_TITLE").to_owned();

        let area = Rect::from_xywh(x, y, width, height);

        Ok(Self {
            id: node.id,
            title,
            area,
            visible,
            floating: node.node_type == NodeType::FloatingCon,
            _fullscreen: node.fullscreen_mode.unwrap_or_default() != 0,
            z_index: 0,
            pid,
            //_data
        })
    }
}

struct StandardNodeIterator<'a> {
    queue: Vec<&'a Node>,
}

impl<'a> Iterator for StandardNodeIterator<'a> {
    type Item = &'a Node;

    fn next(&mut self) -> Option<&'a Node> {
        match self.queue.pop() {
            None => None,
            Some(node) => {
                self.queue.extend(node.nodes.iter());
                Some(node)
            }
        }
    }
}

fn iter_standard<'a>(node: &'a Node) -> StandardNodeIterator<'a> {
    StandardNodeIterator { queue: vec![node] }
}

fn get_all_windows_and_app(workspace: &Node) -> (HashSet<pid_t>,Vec<SwayWindow>) {
    let mut floating_idx = 1;

    workspace.floating_nodes.iter()
        .chain(iter_standard(workspace))
        .filter_map(|n| SwayWindow::try_from(n).ok().map(|n| {
            if n.floating {
                let z_index = floating_idx;
                floating_idx += 1;

                SwayWindow { z_index, ..n }
            } else { n }
        }))
        .map(|w| (w.pid, w))
        .collect()
}

pub struct SwayBridge {
    swayipc: Connection,
    swayevents: EventStream,
    app_meta: HashMap<pid_t, (String, HashSet<i64>)>,
    window_meta: HashMap<i64, (String, SwayWindow)>
}

impl SwayBridge {
    const OUTPUT_NAME: &str = "DPI-1";

    pub async fn new() -> Result<Self> {
        let swayipc = Connection::new().await
            .context("Failed to connect to Sway IPC")?;

        let events = vec![
            EventType::Output,
            EventType::Window,
            EventType::Workspace,
            EventType::Shutdown
        ];

        let swayevents = Connection::new().await.context("Failed to connect to Sway IPC")?
            .subscribe(events).await.context("Failed to subscibe to Sway Event")?;

        Ok(Self {
            swayipc,
            swayevents,
            app_meta: Default::default(),
            window_meta: Default::default(),
        })
    }

    /// Add an application
    async fn add_app(&mut self, pid: pid_t, tx: &mut Sender<EbcCommand>) -> Result<()> {
        let (ret_tx, ret_rx) = oneshot::channel::<String>();
        tx.send(EbcCommand::AddApplication(pid, ret_tx)).await.context("Failed to add application '{pid}'")?;
        let app_key = ret_rx.await.context("Failed to get application key for '{pid}'")?;

        self.app_meta.insert(pid, (app_key.clone(), Default::default()));

        Ok(())
    }

    /// Remove stale app from the app_meta map, notifying the EbcService in the process.
    async fn remove_stale_apps(&mut self, stale_pid: Vec<pid_t>, tx: &mut Sender<EbcCommand>) -> Result<()> {
        for p in stale_pid {
            let Some((app_key, win_ids)) = self.app_meta.remove(&p) else { continue; };

            tx.send(EbcCommand::RemoveApplication(app_key)).await
                .context("Failed to send RemoveApplication for '{app_key}'")?;

            for id in win_ids {
                self.window_meta.remove(&id);
            }
        }

        Ok(())
    }

    /// Add a new window
    async fn add_window(&mut self, win: SwayWindow, tx: &mut Sender<EbcCommand>) -> Result<()> {
        let (rtx, rx) = oneshot::channel::<String>();

        let app_meta = self.app_meta.get_mut(&win.pid).expect("Window should be added after apps");
        let app_key = app_meta.0.clone();

        let cmd = EbcCommand::AddWindow {
            app_key,
            title: win.title.clone(),
            area: win.area.clone(),
            hint: None,
            visible: win.visible,
            z_index: win.z_index,
            ret: rtx
        };

        tx.send(cmd).await.context("Fail to send window creation command")?;
        let win_key = rx.await.context("Did not receive win_key")?;

        let id = win.id;
        self.window_meta.insert(id, (win_key, win));
        app_meta.1.insert(id);

        Ok(())
    }

    /// Update Window
    ///
    // TODO: Manage Window Hint
    async fn update_window(&mut self, up_win: SwayWindow, tx: &mut Sender<EbcCommand>) -> Result<()> {
        let &mut (ref win_key,ref mut win) = self.window_meta.get_mut(&up_win.id).unwrap();

        if let Some(SwayWindowDiff { title, area, visible, z_index, .. }) = win.diff(&up_win) {
            tx.send(EbcCommand::UpdateWindow {
                win_key: win_key.clone(),
                title,
                area,
                hint: None,
                visible,
                z_index
            }).await.context("Failed to send update for window '{win_key}'")?;

            self.window_meta.entry(up_win.id).and_modify(|e| e.1 = up_win);
        }


        Ok(())
    }

    async fn remove_stale_windows(&mut self, stale_id: Vec<i64>, tx: &mut Sender<EbcCommand>) -> Result<()> {
        for wid in stale_id {
            let Some((win_key, win)) = self.window_meta.remove(&wid) else { continue; };

            tx.send(EbcCommand::RemoveWindow(win_key)).await.context("Failed to remove window '{win_key}'")?;

            self.app_meta.entry(win.pid).and_modify(|e| { e.1.remove(&wid); });
        }

        Ok(())
    }

    async fn process_tree(&mut self, tx: &mut Sender<EbcCommand>) -> Result<()> {
        let swaytree = self.swayipc.get_tree().await.context("Failed to get Sway Tree")?;

        let Some(output) = swaytree.find_as_ref(|n|
            n.node_type == NodeType::Output &&
            n.name.as_deref().unwrap_or("") == Self::OUTPUT_NAME
        ) else {
            bail!("Could not find output '{}'", Self::OUTPUT_NAME)
        };
        let Some(workspace) = output.find_focused_as_ref(|n| n.node_type == NodeType::Workspace) else {
            bail!("No focused workspace for output '{}", Self::OUTPUT_NAME)
        };

        let (pid_set, windows) = get_all_windows_and_app(workspace);

        let stale_pid: Vec<pid_t> = self.app_meta.keys()
            .filter(|k| !pid_set.contains(k))
            .copied()
            .collect();

        self.remove_stale_apps(stale_pid, tx).await.context("SwayBridge::remove_stale_apps failed")?;

        for pid in pid_set {
            if !self.app_meta.contains_key(&pid) {
                self.add_app(pid, tx).await.context("SwayBridge::add_app failed")?;
            }
        }

        let mut win_set: HashSet<i64> = HashSet::new();
        for w in windows {
            win_set.insert(w.id);

            if !self.window_meta.contains_key(&w.id) {
                self.add_window(w, tx).await.context("SwayBridge::add_window failed")?;
            } else {
                self.update_window(w, tx).await.context("SwayBridge::update_window failed")?;
            }
        }

        let stale_window = self.window_meta.keys()
            .filter(|k| !win_set.contains(k))
            .copied()
            .collect();

        self.remove_stale_windows(stale_window, tx).await.context("SwayBridge::remove_stale_window failed")?;

        Ok(())
    }

    pub async fn run(&mut self, mut tx: Sender<EbcCommand>) -> Result<()> {
        let mut process_tree = true;

        loop {
            if process_tree {
                eprintln!("====== Processing Tree ======");
                if let Err(e) = self.process_tree(&mut tx).await.context("Failed to process_tree") {
                    eprintln!("{e:?}");
                };
                process_tree = false;
            }

            if let Some(evt) = self.swayevents.next().await {
                let event = evt?;
                eprintln!("======== New Sway Event =======");
                eprintln!("{:?}", &event);

                match event {
                    Event::Shutdown(_) => { break },
                    Event::Window(_) => {
                        process_tree = true;
                    },
                    Event::Output(_) => {
                        process_tree = true;
                    },
                    Event::Workspace(_) => {
                        process_tree = true;
                    }
                    _ => {}
                }
            }
        }

        eprintln!("Implement proper exit");

        Ok(())
    }
}
