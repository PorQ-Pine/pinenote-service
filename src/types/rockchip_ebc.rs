//! Type safe representation of rockchip_ebc parameters

use std::{
    fmt::{Debug, Display},
    num::ParseIntError,
    str::FromStr,
};

use num_enum::{IntoPrimitive, TryFromPrimitive, TryFromPrimitiveError};
use thiserror::Error;
use zbus::zvariant::{Type, Value};
use log::warn;

use crate::ioctls::{self, drm};

use super::Rect;

#[derive(TryFromPrimitive, IntoPrimitive, Clone, Copy, Type, Value)]
#[repr(u8)]
pub enum HintBitDepth {
    Y1 = 0,
    Y2 = 1,
    Y4 = 2,
}

#[derive(TryFromPrimitive, IntoPrimitive, Clone, Copy, Type, Value)]
#[repr(u8)]
pub enum HintConvertMode {
    Threshold = 0,
    Dither = 1,
}

#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Parse(#[from] ParseIntError),
    #[error("Unsupported bit depth")]
    BitDepth(#[from] TryFromPrimitiveError<HintBitDepth>),
    #[error("Unsupported convert mode")]
    ConvertMode(#[from] TryFromPrimitiveError<HintConvertMode>),
    #[error("Unsupported dithering method")]
    Method(#[from] TryFromPrimitiveError<DitherMode>),
    #[error("Unsupported value")]
    DclkSelect(#[from] TryFromPrimitiveError<DclkSelect>),
    #[error("Invalid value.")]
    Invalid,
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub struct Hint {
    repr: u8,
}

impl Hint {
    const BIT_DEPTH_SHIFT: u8 = 4;
    const BIT_DEPTH_MASK: u8 = 3 << Self::BIT_DEPTH_SHIFT;
    const CONVERT_SHIFT: u8 = 6;
    const CONVERT_MASK: u8 = 1 << Self::CONVERT_SHIFT;
    const REDRAW_SHIFT: u8 = 7;
    const REDRAW_MASK: u8 = 1 << Self::REDRAW_SHIFT;

    pub const fn new(bit_depth: HintBitDepth, convert_mode: HintConvertMode, redraw: bool) -> Self {
        let bit_depth = (bit_depth as u8) << Self::BIT_DEPTH_SHIFT;
        let convert_mode = (convert_mode as u8) << Self::CONVERT_SHIFT;
        let redraw = (redraw as u8) << Self::REDRAW_SHIFT;

        Self {
            repr: bit_depth | convert_mode | redraw,
        }
    }

    pub fn try_from_human_readable(str: &str) -> Result<Self, Error> {
        let mut bitdepth: Option<HintBitDepth> = None;
        let mut convert = HintConvertMode::Threshold;
        let mut redraw: bool = false;

        for token in str.split("|") {
            match token {
                "Y4" => bitdepth = Some(HintBitDepth::Y4),
                "Y2" => bitdepth = Some(HintBitDepth::Y2),
                "Y1" => bitdepth = Some(HintBitDepth::Y1),
                "T" => convert = HintConvertMode::Threshold,
                "D" => convert = HintConvertMode::Dither,
                "R" => redraw = true,
                "r" => redraw = false,
                _ => return Err(Error::Invalid),
            }
        }

        Ok(Self::new(bitdepth.ok_or(Error::Invalid)?, convert, redraw))
    }

    pub fn try_from_part(bit_depth: u8, convert_mode: u8, redraw: bool) -> Result<Self, Error> {
        let bit_depth = HintBitDepth::try_from_primitive(bit_depth)?;
        let convert_mode = HintConvertMode::try_from_primitive(convert_mode)?;

        Ok(Self::new(bit_depth, convert_mode, redraw))
    }

    fn extract_bit_depth(repr: u8) -> u8 {
        (repr & Self::BIT_DEPTH_MASK) >> Self::BIT_DEPTH_SHIFT
    }

    fn extract_convert_mode(repr: u8) -> u8 {
        (repr & Self::CONVERT_MASK) >> Self::CONVERT_SHIFT
    }

    fn extract_redraw(repr: u8) -> bool {
        let redraw = (repr & Self::REDRAW_MASK) >> Self::REDRAW_SHIFT;
        redraw != 0
    }

    pub fn bit_depth(&self) -> HintBitDepth {
        let val = Self::extract_bit_depth(self.repr);
        HintBitDepth::try_from_primitive(val).expect("BitDepth invariants were not maintained.")
    }

    pub fn convert_mode(&self) -> HintConvertMode {
        let val = Self::extract_convert_mode(self.repr);
        HintConvertMode::try_from_primitive(val).unwrap()
    }

    pub fn redraw(&self) -> bool {
        Self::extract_redraw(self.repr)
    }
}

impl FromStr for Hint {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let repr: u8 = s.parse()?;

        let mask = !(Self::BIT_DEPTH_MASK | Self::CONVERT_MASK | Self::REDRAW_MASK);
        if (repr & mask) != 0 {
            return Err(Error::Invalid);
        }

        let bit_depth = Self::extract_bit_depth(repr);
        let convert_mode = Self::extract_convert_mode(repr);
        let redraw = Self::extract_redraw(repr);

        Self::try_from_part(bit_depth, convert_mode, redraw)
    }
}

impl From<Hint> for u8 {
    fn from(value: Hint) -> Self {
        value.repr
    }
}

impl Display for Hint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let depth = match self.bit_depth() {
            HintBitDepth::Y4 => "Y4",
            HintBitDepth::Y2 => "Y2",
            HintBitDepth::Y1 => "Y1",
        };

        let convert = match self.convert_mode() {
            HintConvertMode::Threshold => "T",
            HintConvertMode::Dither => "D",
        };

        let redraw = if self.redraw() { "R" } else { "r" };
        write!(f, "{depth}|{convert}|{redraw}")
    }
}

impl Debug for Hint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hint")
            .field("repr", &format_args!("{:X}", self.repr))
            .field("str", &format_args!("{}", self))
            .finish()
    }
}

#[derive(TryFromPrimitive, IntoPrimitive, Clone, Copy, PartialEq, Eq, Type, Value)]
#[repr(u8)]
pub enum DitherMode {
    Bayer = 0,
    BlueNoise16 = 1,
    BlueNoise32 = 2,
}

impl DitherMode {
    pub fn cycle_next(&self) -> Self {
        match self {
            Self::Bayer => Self::BlueNoise16,
            Self::BlueNoise16 => Self::BlueNoise32,
            Self::BlueNoise32 => Self::Bayer,
        }
    }
}

impl FromStr for DitherMode {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let repr: u8 = s.parse()?;
        Self::try_from_primitive(repr).map_err(Error::from)
    }
}

#[derive(TryFromPrimitive, IntoPrimitive, Clone, Copy, PartialEq, Eq, Type, Value)]
#[repr(u8)]
pub enum DriverMode {
    Normal = 0,
    Fast = 1,
    ZeroWaveform = 8,
}

impl DriverMode {
    pub fn cycle_next(&self) -> Self {
        match self {
            Self::Normal => Self::Fast,
            Self::Fast => Self::Normal,
            _ => *self,
        }
    }
}

#[derive(Default)]
pub struct Mode {
    pub driver_mode: Option<DriverMode>,
    pub dither_mode: Option<DitherMode>,
    pub redraw_delay: Option<u16>,
}

impl From<ioctls::rockchip_ebc::Mode> for Mode {
    fn from(value: ioctls::rockchip_ebc::Mode) -> Self {
        let driver_mode = match DriverMode::try_from_primitive(value.driver_mode) {
            Ok(driver) => Some(driver),
            Err(e) => {
                warn!("Bad driver mode '{}': {:?}", value.driver_mode, e);
                None
            }
        };

        let dither_mode = match DitherMode::try_from_primitive(value.dither_mode) {
            Ok(dither) => Some(dither),
            Err(e) => {
                warn!("Bad dithering mode '{}': {:?}", value.dither_mode, e);
                None
            }
        };

        let redraw_delay = Some(value.redraw_delay);

        Self {
            driver_mode,
            dither_mode,
            redraw_delay,
        }
    }
}

impl From<Mode> for ioctls::rockchip_ebc::Mode {
    fn from(value: Mode) -> Self {
        let mut ret = Self::new();

        if let Some(driver) = value.driver_mode {
            ret.set_driver_mode = true as u8;
            ret.driver_mode = driver.into();
        }

        if let Some(dither_mode) = value.dither_mode {
            ret.set_dither_mode = true as u8;
            ret.dither_mode = dither_mode.into();
        }

        if let Some(delay) = value.redraw_delay {
            ret.set_redraw_delay = true as u8;
            ret.redraw_delay = delay;
        }

        ret
    }
}

#[derive(TryFromPrimitive, IntoPrimitive, Clone, Copy)]
#[repr(i32)]
pub enum DclkSelect {
    Mode = -1,
    Mhz200 = 0,
    Mhz250 = 1,
}

impl FromStr for DclkSelect {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let repr: i32 = s.parse()?;
        Self::try_from_primitive(repr).map_err(Error::from)
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct RectHint {
    pub rect: Rect,
    pub hint: Hint,
}

impl From<RectHint> for ioctls::rockchip_ebc::RectHint {
    fn from(value: RectHint) -> Self {
        let RectHint { rect, hint } = value;

        let Rect { x1, y1, x2, y2 } = rect;
        let rect = drm::Rect { x1, y1, x2, y2 };

        Self {
            pixel_hints: hint.into(),
            _padding: Default::default(),
            rect,
        }
    }
}

pub struct FrameBuffers {
    inner_outer_nextprev: Vec<u8>,
    hints: Vec<u8>,
    prelim_target: Vec<u8>,
    phase1: Vec<u8>,
    phase2: Vec<u8>,
}

impl FrameBuffers {
    pub fn new(width: i32, height: i32) -> Self {
        let num_pixels: usize = width as usize * height as usize;

        let inner_outer_nextprev: Vec<u8> = vec![0; 3 * num_pixels];
        let hints: Vec<u8> = vec![0; num_pixels];
        let prelim_target: Vec<u8> = vec![0; num_pixels];
        let phase1 = vec![0; num_pixels >> 2];
        let phase2 = phase1.clone();

        Self {
            inner_outer_nextprev,
            hints,
            prelim_target,
            phase1,
            phase2,
        }
    }

    pub fn inner_outer_nextprev(&self) -> &Vec<u8> {
        &self.inner_outer_nextprev
    }

    pub fn hints(&self) -> &Vec<u8> {
        &self.hints
    }

    pub fn prelim_target(&self) -> &Vec<u8> {
        &self.prelim_target
    }

    pub fn phase1(&self) -> &Vec<u8> {
        &self.phase1
    }

    pub fn phase2(&self) -> &Vec<u8> {
        &self.phase2
    }
}

impl From<&mut FrameBuffers> for ioctls::rockchip_ebc::ExtractFBs {
    fn from(value: &mut FrameBuffers) -> Self {
        Self {
            ptr_packed_inner_outer_nextprev: value.inner_outer_nextprev.as_mut_ptr() as u64,
            ptr_hints: value.hints.as_mut_ptr() as u64,
            ptr_prelim_target: value.prelim_target.as_mut_ptr() as u64,
            ptr_phase1: value.phase1.as_mut_ptr() as u64,
            ptr_phase2: value.phase2.as_mut_ptr() as u64,
        }
    }
}
