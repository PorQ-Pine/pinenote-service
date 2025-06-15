use std::{io::Write, path::PathBuf, time::SystemTime};

use anyhow::{anyhow, Context, Result};
use pinenote_service::{
    drivers::rockchip_ebc::RockchipEbc,
    pixel_manager as pm,
    types::rockchip_ebc::FrameBuffers
};
use tokio::{io::AsyncWriteExt, sync::mpsc};

use super::command::{self as cmd, CommandStr};

pub struct Ctl {
    driver: RockchipEbc,
    pixel_manager: pm::PixelManager,
}

impl Ctl {
    pub fn new() -> Result<Ctl> {
        let driver = RockchipEbc::new();

        let default_hint = driver.default_hint()?;
        let screen_area = driver.screen_area()?;

        Ok(Ctl {
            driver: RockchipEbc::new(),
            pixel_manager: pm::PixelManager::new(default_hint, screen_area)
        })
    }

    fn recompute_hints(&self) -> Result<()> {
        let hints = self.pixel_manager.compute_hints().context("Failed to compute new hints")?;

        self.driver.upload_rect_hints(hints).context("Failed to upload hints")
    }

    fn dump(&self, mut output: impl Write) {
        let _ = writeln!(output, "=========== EBC_CTL DUMP ===========");
        let _ = writeln!(output, "PixelManager: ");
        let _ = writeln!(output, "{:#?}", self.pixel_manager);
        let _ = writeln!(output, "=========== ! EBC_CTL DUMP ===========");
    }


    /// Dump Framebuffer data to a specific directory
    async fn fb_dump_dir(fbs: FrameBuffers, path: String, stamp: u64) -> Result<()> {
        let mut path = PathBuf::from(&path);
        path.push(format!("dump_{}", stamp));

        tokio::fs::create_dir_all(&path).await
            .with_context(|| format!("Failed to create '{:?}'", path))?;

        let mut fopt = tokio::fs::OpenOptions::new();
        fopt
            .create(true)
            .mode(0o644)
            .write(true)
            .truncate(true);

        let dump = async |filename: &str, vec: &Vec<u8>| -> Result<()> {
            let path = path.join(filename);
            fopt.open(&path).await
                .with_context(|| format!("Failed to open '{:?}'", path))?
                .write_all(vec).await
                .with_context(|| format!("Failed to write '{:?}'", path))?;
            Ok(())
        };

        dump("buf_inner_outer_nextprev.bin", fbs.inner_outer_nextprev()).await?;
        dump("buf_hints.bin", fbs.hints()).await?;
        dump("buf_prelim_target.bin", fbs.prelim_target()).await?;
        dump("buf_phase1.bin", fbs.phase1()).await?;
        dump("buf_phase2.bin", fbs.phase2()).await?;

        Ok(())
    }

    async fn dispatch_app(&mut self, app_cmd: cmd::Application) -> Result<()> {
        use cmd::Application::*;

        match app_cmd {
            Add(pid, reply) => {
                let app_key = self.pixel_manager.app_add(pm::Application::new("", pid));
                reply.send(app_key).map_err(|e| anyhow!("Failed to send response: {e}"))?;
            }
            Remove(app_id) => {
                self.pixel_manager.app_remove(&app_id);
                self.recompute_hints()?;
            }
        }

        Ok(())
    }

    async fn dispatch_props(&mut self, prop_cmd: cmd::Property) -> Result<()> {
        use cmd::Property::*;

        match prop_cmd {
            DefaultHint(tx) => {
                let h = self.pixel_manager.default_hint;
                tx.send(h).map_err(|_| anyhow!("Failed to send back default hint"))?;
            },
            SetDefaultHint(h) => {
                self.pixel_manager.default_hint = h;

                self.recompute_hints()?;
            }
        }

        Ok(())
    }

    async fn dispatch_window(&mut self, win_cmd: cmd::Window) -> Result<()> {
        use cmd::Window::*;

        match win_cmd {
            Add { app_key, title, area, hint, visible, z_index, reply } => {
                let win_key = self.pixel_manager.window_add(pm::Window::new(app_key, title, area, hint, visible, z_index))
                    .context("PixelManager::window_add failed")?;

                reply.send(win_key).map_err(|e| anyhow!("Failed to send response: {e:?}"))?;

                self.recompute_hints()?;
            },
            Update { win_key, title, area, hint, visible, z_index } => {
                let win = self.pixel_manager.window(&win_key)
                    .context("Failed to get window {win_key}")?;

                let update = pm::WindowData {
                    title: title.unwrap_or(win.data.title.clone()),
                    area: area.unwrap_or(win.data.area.clone()),
                    hint: hint.unwrap_or(win.data.hint),
                    visible: visible.unwrap_or(win.data.visible),
                    z_index: z_index.unwrap_or(win.data.z_index),
                };

                self.pixel_manager.window_update(&win_key, update)
                    .context("Failed to update window {win_key}")?;

                self.recompute_hints()?;
            }
            Remove(win_id) => {
                self.pixel_manager.window_remove(win_id);
                self.recompute_hints()?;
            }
        }

        Ok(())
    }

    async fn dispatch(&mut self, cmd: cmd::Command) -> Result<()> {
        use cmd::Command::*;
        match cmd {
            Application(a) => self.dispatch_app(a).await?,
            Dump(path) => {
                if path == "-" {
                    self.dump(std::io::stderr())
                } else if let Ok(f) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)
                {
                    self.dump(f);
                } else {
                    self.dump(std::io::stderr());
                }
            }
            FbDumpToDir(path) => {
                let fbs = self.driver.extract_framebuffers()
                    .context("Could not retrieve framebuffers")?;

                let now = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .context("Failed to get timestamp")?
                    .as_secs();

                tokio::spawn(async move {
                    if let Err(e) = Self::fb_dump_dir(fbs, path, now).await
                        .context("Failed to dump framebuffers") {
                        eprintln!("{:#?}", e);
                    }
                });
            }
            GlobalRefresh => {
                self.driver.global_refresh()
                    .context("RockchipEbc::global_refresh failed")?;
            }
            Property(p) => {
                self.dispatch_props(p).await?;
            }
            Window(w) => self.dispatch_window(w).await?,
        };

        Ok(())
    }

    pub async fn serve(&mut self, mut rx: mpsc::Receiver<cmd::Command>) {
        while let Some(cmd) = rx.recv().await {
            let ctx = cmd.get_command_str();

            if let Err(e) = self.dispatch(cmd).await
                .with_context(|| format!("While handling {ctx}"))
            {
                eprintln!("{e:?}")
            }
        }
    }
}
