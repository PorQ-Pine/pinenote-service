//! Type safe representation of rockchip_ebc parameters

use std::{num::ParseIntError, str::FromStr};

use num_enum::{IntoPrimitive, TryFromPrimitive, TryFromPrimitiveError};
use thiserror::Error;

#[derive(TryFromPrimitive, IntoPrimitive, Clone, Copy)]
#[repr(u8)]
pub enum HintBitDepth {
    Y1 = 0,
    Y2 = 1,
    Y4 = 2
}

#[derive(TryFromPrimitive, IntoPrimitive, Clone, Copy)]
#[repr(u8)]
pub enum HintConvertMode {
    Threshold = 0,
    Dither = 1
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
    Method(#[from] TryFromPrimitiveError<DitheringMethod>),
    #[error("Unsupported value")]
    DclkSelect(#[from] TryFromPrimitiveError<DclkSelect>),
    #[error("Invalid value.")]
    Invalid

}

pub struct Hint {
    repr: u8
}

impl Hint {
    const BIT_DEPTH_SHIFT : u8 = 4;
    const BIT_DEPTH_MASK : u8 = 3 << Self::BIT_DEPTH_SHIFT;
    const CONVERT_SHIFT : u8 = 6;
    const CONVERT_MASK : u8 = 1 << Self::CONVERT_SHIFT;
    const REDRAW_SHIFT : u8 = 7;
    const REDRAW_MASK : u8 = 1 << Self::REDRAW_SHIFT;

    pub fn new(bit_depth: HintBitDepth, convert_mode: HintConvertMode, redraw: bool) -> Self {
        let bit_depth = (bit_depth as u8) << Self::BIT_DEPTH_SHIFT;
        let convert_mode = (convert_mode as u8) << Self::CONVERT_SHIFT;
        let redraw = (redraw as u8) << Self::REDRAW_SHIFT;

        Self {
            repr: bit_depth | convert_mode | redraw
        }
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
            return Err(Error::Invalid)
        }

        let bit_depth = Self::extract_bit_depth(repr);
        let convert_mode = Self::extract_convert_mode(repr);
        let redraw = Self::extract_redraw(repr);

        Self::try_from_part(bit_depth, convert_mode, redraw)
    }
}

#[derive(TryFromPrimitive, IntoPrimitive, Clone, Copy)]
#[repr(u8)]
pub enum DitheringMethod {
    Bayer = 0,
    BlueNoise16 = 1,
    BlueNoise32 = 2
}

impl FromStr for DitheringMethod {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let repr: u8 = s.parse()?;
        Self::try_from_primitive(repr).map_err(Error::from)
    }
}

#[derive(TryFromPrimitive, IntoPrimitive, Clone, Copy)]
#[repr(i32)]
pub enum DclkSelect {
    Mode = -1,
    Mhz200 = 0,
    Mhz250 = 1
}

impl FromStr for DclkSelect {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let repr: i32 = s.parse()?;
        Self::try_from_primitive(repr).map_err(Error::from)
    }
}
