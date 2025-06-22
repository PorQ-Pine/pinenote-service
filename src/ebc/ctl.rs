use std::{io::Write, path::PathBuf, time::SystemTime};

use anyhow::{Context, Result, anyhow};
use pinenote_service::{
    drivers::rockchip_ebc::RockchipEbc,
    pixel_manager as pm,
    types::rockchip_ebc::{FrameBuffers, Mode},
};
use tokio::{
    io::AsyncWriteExt,
    sync::{mpsc, oneshot},
};

use super::command::{self as cmd, CommandStr};

pub struct Ctl {
    driver: RockchipEbc,
    pixel_manager: pm::PixelManager,
    display_width: u32,
    display_height: u32,
    offscreen_override: String,
}

pub enum OffScreenError {
    LoadFailed,
    DecodeFailed,
    UploadFailed,
}

mod utils {
    use anyhow::Result;
    use image::{DynamicImage, ImageReader, imageops::FilterType, metadata::Orientation};

    use super::OffScreenError;

    pub fn load_image(path: &String) -> Result<DynamicImage, OffScreenError> {
        Ok(ImageReader::open(path)
            .map_err(|_| OffScreenError::LoadFailed)?
            .decode()
            .map_err(|_| OffScreenError::DecodeFailed)?
            .to_luma8()
            .into())
    }

    pub fn transform_off_screen(mut img: DynamicImage, width: u32, height: u32) -> DynamicImage {
        if img.height() > img.width() {
            img.apply_orientation(Orientation::Rotate90FlipH);
        } else {
            img.apply_orientation(Orientation::FlipHorizontal);
        }

        if (img.width(), img.height()) != (width, height) {
            img = img.resize_to_fill(width, height, FilterType::Nearest);
        }

        img
    }
}

impl Ctl {
    pub fn new() -> Result<Ctl> {
        let driver = RockchipEbc::new();

        let default_hint = driver.default_hint()?;
        let screen_area = driver.screen_area()?;
        let display_width = screen_area.x2 as u32;
        let display_height = screen_area.y2 as u32;

        Ok(Ctl {
            driver: RockchipEbc::new(),
            pixel_manager: pm::PixelManager::new(default_hint, screen_area),
            display_width,
            display_height,
            offscreen_override: "unknown".into(),
        })
    }

    fn load_offscreen(
        &mut self,
        path: String,
        reply: oneshot::Sender<Result<(), OffScreenError>>,
    ) -> Result<()> {
        let img = match utils::load_image(&path) {
            Ok(img) => img,
            Err(e) => {
                reply
                    .send(Err(e))
                    .map_err(|_| anyhow!("Failed to send error"))?;
                return Ok(());
            }
        };

        let img = utils::transform_off_screen(img, self.display_width, self.display_height);

        let bytes: Vec<u8> = img.into_bytes().iter().map(|p| p >> 4).collect();

        match self.driver.upload_off_screen(bytes) {
            Ok(_) => {
                self.offscreen_override = path;
                reply
                    .send(Ok(()))
                    .map_err(|_| anyhow!("Failed to send Ok reply to SetOffScreen"))?;
            }
            Err(e) => {
                self.offscreen_override = "error".into();

                reply
                    .send(Err(OffScreenError::UploadFailed))
                    .map_err(|_| anyhow!("Failed to send error"))?;
                Err(e)?;
            }
        }

        Ok(())
    }

    fn recompute_hints(&self) -> Result<()> {
        let hints = self
            .pixel_manager
            .compute_hints()
            .context("Failed to compute new hints")?;

        self.driver
            .upload_rect_hints(hints)
            .context("Failed to upload hints")
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

        tokio::fs::create_dir_all(&path)
            .await
            .with_context(|| format!("Failed to create '{:?}'", path))?;

        let mut fopt = tokio::fs::OpenOptions::new();
        fopt.create(true).mode(0o644).write(true).truncate(true);

        let dump = async |filename: &str, vec: &Vec<u8>| -> Result<()> {
            let path = path.join(filename);
            fopt.open(&path)
                .await
                .with_context(|| format!("Failed to open '{:?}'", path))?
                .write_all(vec)
                .await
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
                reply
                    .send(app_key)
                    .map_err(|e| anyhow!("Failed to send response: {e}"))?;
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
                tx.send(h)
                    .map_err(|_| anyhow!("Failed to send back default hint"))?;
            }
            SetDefaultHint(h) => {
                self.pixel_manager.default_hint = h;

                self.recompute_hints()?;
            }
            DriverMode(tx) => {
                let Mode { driver_mode, .. } = self.driver.mode()?;

                let dm = driver_mode.ok_or(anyhow!("No DriverMode found."))?;
                tx.send(dm)
                    .map_err(|_| anyhow!("Failed to send back driver mode"))?;
            }
            SetDriverMode(mode) => {
                self.driver.set_mode(Mode {
                    driver_mode: Some(mode),
                    ..Default::default()
                })?;
            }
            DitherMode(tx) => {
                let Mode { dither_mode, .. } = self.driver.mode()?;

                let dm = dither_mode.ok_or(anyhow!("No DitherMode found"))?;

                tx.send(dm)
                    .map_err(|_| anyhow!("Failed to send dither mode back"))?;
            }
            SetDitherMode(dith) => {
                self.driver.set_mode(Mode {
                    dither_mode: Some(dith),
                    ..Default::default()
                })?;
            }
            RedrawDelay(tx) => {
                let Mode { redraw_delay, .. } = self.driver.mode()?;

                let rd = redraw_delay.ok_or(anyhow!("No redraw delay found"))?;

                tx.send(rd)
                    .map_err(|_| anyhow!("Failed to send redraw delay back"))?;
            }
            SetRedrawDelay(rd) => {
                self.driver.set_mode(Mode {
                    redraw_delay: Some(rd),
                    ..Default::default()
                })?;
            }
            OffScreenDisable(tx) => {
                let v = self.driver.no_off_screen()?;

                tx.send(v)
                    .map_err(|_| anyhow!("Failed to send OffScreenDisable value"))?;
            }
            SetOffScreenDisable(val) => {
                self.driver.set_no_off_screen(val)?;
            }
            OffScreenOverride(tx) => {
                tx.send(self.offscreen_override.clone())
                    .map_err(|_| anyhow!("Failed to send OffScreen override path"))?;
            }
        }

        Ok(())
    }

    async fn dispatch_window(&mut self, win_cmd: cmd::Window) -> Result<()> {
        use cmd::Window::*;

        match win_cmd {
            Add {
                app_key,
                title,
                area,
                hint,
                visible,
                fullscreen,
                z_index,
                reply,
            } => {
                let win_key = self
                    .pixel_manager
                    .window_add(pm::Window::new(
                        app_key, title, area, hint, visible, fullscreen, z_index,
                    ))
                    .context("PixelManager::window_add failed")?;

                reply
                    .send(win_key)
                    .map_err(|e| anyhow!("Failed to send response: {e:?}"))?;

                self.recompute_hints()?;
            }
            Update {
                win_key,
                update:
                    cmd::WindowUpdate {
                        title,
                        area,
                        hint,
                        visible,
                        fullscreen,
                        z_index,
                    },
            } => {
                let win = self
                    .pixel_manager
                    .window(&win_key)
                    .context("Failed to get window {win_key}")?;

                let update = pm::WindowData {
                    title: title.unwrap_or(win.data.title.clone()),
                    area: area.unwrap_or(win.data.area.clone()),
                    hint: hint.unwrap_or(win.data.hint),
                    visible: visible.unwrap_or(win.data.visible),
                    fullscreen: fullscreen.unwrap_or(win.data.fullscreen),
                    z_index: z_index.unwrap_or(win.data.z_index),
                };

                self.pixel_manager
                    .window_update(&win_key, update)
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
                let fbs = self
                    .driver
                    .extract_framebuffers()
                    .context("Could not retrieve framebuffers")?;

                let now = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .context("Failed to get timestamp")?
                    .as_secs();

                tokio::spawn(async move {
                    if let Err(e) = Self::fb_dump_dir(fbs, path, now)
                        .await
                        .context("Failed to dump framebuffers")
                    {
                        eprintln!("{:#?}", e);
                    }
                });
            }
            GlobalRefresh => {
                self.driver
                    .global_refresh()
                    .context("RockchipEbc::global_refresh failed")?;
            }
            Property(p) => {
                self.dispatch_props(p).await?;
            }
            SetMode(dr, di, rd) => {
                self.driver.set_mode(Mode {
                    driver_mode: Some(dr),
                    dither_mode: Some(di),
                    redraw_delay: Some(rd),
                })?;
            }
            Window(w) => self.dispatch_window(w).await?,
            OffScreen(p, reply) => self.load_offscreen(p, reply)?,
        };

        Ok(())
    }

    pub async fn serve(&mut self, mut rx: mpsc::Receiver<cmd::Command>) {
        while let Some(cmd) = rx.recv().await {
            let ctx = cmd.get_command_str();

            if let Err(e) = self
                .dispatch(cmd)
                .await
                .with_context(|| format!("While handling {ctx}"))
            {
                eprintln!("{e:?}")
            }
        }
    }
}
