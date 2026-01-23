use std::sync::Mutex;

use crate::ebc;
use anyhow::{Context, Result, bail};
use niri_ipc::{Event, Request, Response, socket::Socket};
use tokio::sync::{
    mpsc::{self, Sender},
    oneshot,
};

const QUILL_NIRI_BRIDGE: &str = "Quill niri";

pub struct QuillNiriBridge {}

impl QuillNiriBridge {
    pub async fn new() -> Result<Self> {
        let bridge = Self {};
        Ok(bridge)
    }

    pub async fn main_manage(&mut self, tx: &Sender<ebc::Command>) {
        let mut socket = get_socket().await;
        println!("Requesting windows");
        let reply = socket.send(Request::Windows).unwrap();
        let windows = match reply {
            Ok(Response::Windows(w)) => w,
            Ok(_) => {
                eprintln!("Error: Received unexpected response variant.");
                return;
            }
            Err(e) => {
                eprintln!("Error: Failed to get reply: {:?}", e);
                return;
            }
        };

        println!("Windows are: {:#?}", windows);

        println!("Main manage exit");
    }

    pub async fn run(mut self, tx: Sender<ebc::Command>) -> Result<()> {
        println!("Bridge niri started");
        let mut socket = get_socket().await;

        let reply = socket.send(Request::EventStream).unwrap();
        if matches!(reply, Ok(Response::Handled)) {
            let mut read_event = socket.read_events();

            while let Ok(event) = read_event() {
                match event {
                    Event::WindowsChanged { .. } => {
                        println!("Trigger: WindowsChanged");
                        self.main_manage(&tx).await;
                    }
                    Event::WindowOpenedOrChanged { .. } => {
                        println!("Trigger: WindowOpenedOrChanged");
                        self.main_manage(&tx).await;
                    }
                    Event::WindowClosed { .. } => {
                        println!("Trigger: WindowClosed");
                        self.main_manage(&tx).await;
                    }
                    Event::WindowLayoutsChanged { .. } => {
                        println!("Trigger: WindowLayoutsChanged");
                        self.main_manage(&tx).await;
                    }
                    Event::WindowFocusChanged { .. } => {
                        println!("Trigger: WindowFocusChanged");
                        self.main_manage(&tx).await;
                    }
                    Event::WorkspaceActivated { .. } => {
                        println!("Trigger: WorkspaceActivated");
                        self.main_manage(&tx).await;
                    }
                    _ => {
                        println!("Received event: {:?}", event);
                    }
                }
            }
        }

        eprintln!("Quill niri bridge ended");
        Ok(())
    }
}

pub async fn start(tx: mpsc::Sender<ebc::Command>) -> Result<String> {
    let quill_niri_bridge = QuillNiriBridge::new()
        .await
        .context("While trying to start Quill niri bridge")?;

    tokio::spawn(async move {
        let _ = quill_niri_bridge.run(tx).await;
    });

    Ok(QUILL_NIRI_BRIDGE.into())
}

async fn get_socket() -> Socket {
    let niri_socket_env = std::env::var("NIRI_SOCKET");
    let socket = if let Ok(niri_socket) = niri_socket_env.clone() {
        Socket::connect_to(niri_socket).unwrap()
    } else {
        Socket::connect().unwrap()
    };
    socket
}
