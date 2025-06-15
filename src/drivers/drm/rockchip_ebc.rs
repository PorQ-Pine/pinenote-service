//! rockchip_ebc driver support

use std::os::fd::AsRawFd;

use thiserror::Error;

use crate::{
    ioctls::{self, OpenError},
    pixel_manager::ComputedHints,
    sysfs::{
        self,
        attribute::{AttributeBase, Boolean, Int32, RGeneric, RInt32, TypedRead},
    },
    types::{
        Rect,
        rockchip_ebc::{DitheringMethod, FrameBuffers, Hint, Mode},
    },
};

#[derive(Error, Debug)]
pub enum DriverError {
    #[error(transparent)]
    OpenDevice(#[from] OpenError),
    #[error(transparent)]
    IotclError(#[from] nix::Error),
    #[error(transparent)]
    SysFs(#[from] sysfs::attribute::Error),
}

/// Control structure for the RockchipEbc driver
#[allow(unused)]
pub struct RockchipEbc {
    default_hint: RGeneric<Hint>,
    redraw_delay: RInt32,
    early_cancellation_addition: Int32,
    shrink_virtual_window: Boolean,
    // TODO: Make direct_mode optional
    limit_fb_blits: Int32,
    no_off_screen: Boolean,
    refresh_thread_wait_idle: Int32,
    dithering_method: RGeneric<DitheringMethod>,
    bw_threshold: RInt32,
    y2_dt_threshold: Int32,
    y2_th_threshold: Int32,
    temp_override: Int32,
    hskew_override: Int32,
    rect_hint_batch: Int32,
}

impl RockchipEbc {
    const SYSFS_PATH_BASE: &str = "/sys/module/rockchip_ebc/parameters";
    const DEV_PATH: &str = "/dev/dri/by-path/platform-fdec0000.ebc-card";
    const SCREEN_RECT: Rect = Rect::new(0, 0, 1872, 1404);

    pub fn new() -> Self {
        Self {
            default_hint: Self::make_param("default_hint"),
            redraw_delay: Self::make_param("redraw_delay"),
            early_cancellation_addition: Self::make_param("early_cancellation_addition"),
            shrink_virtual_window: Self::make_param("shrink_virtual_window"),
            // TODO: Add direct mode here
            limit_fb_blits: Self::make_param("limit_fb_blits"),
            no_off_screen: Self::make_param("no_off_screen"),
            refresh_thread_wait_idle: Self::make_param("refresh_thread_wait_idle"),
            dithering_method: Self::make_param("dithering_method"),
            bw_threshold: Self::make_param("bw_threshold"),
            y2_dt_threshold: Self::make_param("y2_dt_threshold"),
            y2_th_threshold: Self::make_param("y2_th_threshold"),
            temp_override: Self::make_param("temp_override"),
            hskew_override: Self::make_param("hskew_override"),
            rect_hint_batch: Self::make_param("rect_hint_batch"),
        }
    }

    /// Get the hints applied to uncovered pixels.
    pub fn default_hint(&self) -> Result<Hint, crate::sysfs::attribute::Error> {
        self.default_hint.read()
    }

    /// Get the method used for dithering
    pub fn dithering_method(&self) -> Result<DitheringMethod, crate::sysfs::attribute::Error> {
        self.dithering_method.read()
    }

    /// Trigger a full screen refresh
    pub fn global_refresh(&self) -> Result<(), DriverError> {
        let file = ioctls::open_device(Self::DEV_PATH)?;
        let mut data = ioctls::rockchip_ebc::GlobalRefresh {
            trigger_global_refresh: 1,
        };

        unsafe {
            ioctls::rockchip_ebc::global_refresh_iowr(file.as_raw_fd(), &mut data)?;
        }

        Ok(())
    }

    pub fn upload_rect_hints(&self, rect_hints: ComputedHints) -> Result<(), DriverError> {
        let file = ioctls::open_device(Self::DEV_PATH)?;
        let ComputedHints {
            default_hint,
            rect_hints,
        } = rect_hints;

        let rect_hints: Vec<ioctls::rockchip_ebc::RectHint> =
            rect_hints.into_iter().map(Into::into).collect();

        let data = ioctls::rockchip_ebc::RectHints {
            set_default_hints: default_hint.is_some() as u8,
            default_hints: default_hint.map(|h| h.into()).unwrap_or_default(),
            _padding: Default::default(),
            num_rects: rect_hints.len() as u32,
            ptr_rect_hints: rect_hints.as_ptr() as u64,
        };

        unsafe {
            ioctls::rockchip_ebc::rect_hints_iow(file.as_raw_fd(), &data)?;
        }

        Ok(())
    }

    pub fn screen_area(&self) -> Result<Rect, DriverError> {
        Ok(Self::SCREEN_RECT.clone())
    }

    pub fn extract_framebuffers(&self) -> Result<FrameBuffers, DriverError> {
        let file = ioctls::open_device(Self::DEV_PATH)?;
        let Rect {
            x2: width,
            y2: height,
            ..
        } = Self::SCREEN_RECT.clone();
        let mut fbs = FrameBuffers::new(width, height);

        let mut data = ioctls::rockchip_ebc::ExtractFBs::from(&mut fbs);

        unsafe {
            ioctls::rockchip_ebc::extract_fbs_iowr(file.as_raw_fd(), &mut data)?;
        }

        Ok(fbs)
    }

    pub fn driver_mode(&self) -> Result<Mode, DriverError> {
        let file = ioctls::open_device(Self::DEV_PATH)?;
        let mut data = ioctls::rockchip_ebc::Mode::new();

        unsafe {
            ioctls::rockchip_ebc::mode_iowr(file.as_raw_fd(), &mut data)?;
        }

        Ok(data.into())
    }

    pub fn set_driver_mode(&self, mode: Mode) -> Result<(), DriverError> {
        let file = ioctls::open_device(Self::DEV_PATH)?;
        let mut data = mode.into();

        unsafe {
            ioctls::rockchip_ebc::mode_iowr(file.as_raw_fd(), &mut data)?;
        }

        Ok(())
    }

    fn make_param<T: AttributeBase>(name: &str) -> T {
        T::from_path(format!("{}/{}", Self::SYSFS_PATH_BASE, name))
    }
}

impl Default for RockchipEbc {
    fn default() -> Self {
        Self::new()
    }
}
