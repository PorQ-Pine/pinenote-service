//! hrdl's rockchip_ebc ioctl support
//!
//! Provides binding and objects to call ioctl on hrdl's flavor of rockchip_ebc

use nix::{
    ioctl_readwrite,
    ioctl_write_ptr
};
use crate::ioctls::drm;

const GLOBAL_REFRESH_NR: u8 = drm::COMMAND_BASE + 0x00;
const OFF_SCREEN_NR: u8 = drm::COMMAND_BASE + 0x01;
const EXTRACT_FB_NR: u8 = drm::COMMAND_BASE + 0x02;
const RECT_HINTS_NR: u8 = drm::COMMAND_BASE + 0x03;
const MODE_NR: u8 = drm::COMMAND_BASE + 0x04;
const ZERO_WAVEFORM_NR: u8 = drm::COMMAND_BASE + 0x05;

/// [global_refresh_iowr] parameter type
#[repr(C)]
pub struct GlobalRefresh {
    /// Set to 1 to trigger a screen refresh
    pub trigger_global_refresh: u8,
}
ioctl_readwrite!(
    /// Triggers a global screen refresh
    global_refresh_iowr,
    drm::IOCTL_MAGIC, GLOBAL_REFRESH_NR, GlobalRefresh);

/// [off_screen_iow] parameter type
#[repr(C)]
pub struct OffScreen {
    pub _info: u64,
    pub ptr_screen_content: u64,
}
ioctl_write_ptr!(
    /// Set the off screen image
    off_screen_iow,
    drm::IOCTL_MAGIC, OFF_SCREEN_NR, OffScreen);

/// [extract_fbs_iowr] parameter type
#[repr(C)]
pub struct ExtractFBs {
    pub ptr_packed_inner_outer_nextprev: u64,
    pub ptr_hints: u64,
    pub ptr_prelim_target: u64,
    pub ptr_phase1: u64,
    pub ptr_phase2: u64,
}
ioctl_readwrite!(
    /// Extract framebuffers related data from the kernel
    extract_fbs_iowr, drm::IOCTL_MAGIC, EXTRACT_FB_NR, ExtractFBs);

/// Rectangular screen region with associated pixel hints
#[repr(C)]
pub struct RectHint {
    /// Hint to apply to every pixel of this region
    pub pixel_hints: u8,
    pub _padding: [u8; 7],
    /// Rectangular region of pixel, in screen coordinate
    pub rect: drm::Rect
}

/// [rect_hints_iow] parameter type.
#[repr(C)]
pub struct RectHints {
    /// Whether to apply or ignore default hints
    pub set_default_hints: u8,
    /// Hint to apply to non-covered pixels
    pub default_hints: u8,
    pub _padding: [u8; 2],
    /// Number of rectangle [RectHints::ptr_rect_hints] points to
    pub num_rects: u32,
    /// Pointer to an array of [RectHint]
    pub ptr_rect_hints: u64,
}
ioctl_write_ptr!(
    /// Configure screen regions with specific rendering modes (hints).
    rect_hints_iow,
    drm::IOCTL_MAGIC, RECT_HINTS_NR, RectHints);

/// [mode_iowr] parameter type
#[repr(C)]
pub struct Mode {
	pub set_driver_mode: u8,
	pub driver_mode: u8,
	pub set_dither_mode: u8,
	pub dither_mode: u8,
	pub redraw_delay: u16,
	pub set_redraw_delay: u8,
	pub _pad: u8,
}
ioctl_readwrite!(
    /// Query or set driver mode, dither mode and redraw delays
    mode_iowr,
    drm::IOCTL_MAGIC, MODE_NR, Mode);

#[repr(C)]
pub struct ZeroWaveform {
    pub set_zero_waveform_mode: u8,
    pub zero_waveform_mode: u8,
    pub _pad: [u8; 6],
}
ioctl_readwrite!(
    /// Query or enable ZeroWaveform mode.
    zero_waveform_iowr, drm::IOCTL_MAGIC, ZERO_WAVEFORM_NR, ZeroWaveform);
