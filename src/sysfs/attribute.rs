//! Generic sysfs attributes

use std::{
    fs::OpenOptions,
    io::{self, Read, Write}, marker::PhantomData, str::FromStr,
};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    IoError(#[from] io::Error),
    #[error("Error while converting value")]
    ConvError
}

/// Base trait for attribues
pub trait AttributeBase {
    fn path(&self) -> &str;
}

/// Read raw value from sysfs attribute file.
pub trait RawRead: AttributeBase {
    fn read_raw(&self) -> Result<String, Error> {
        let path = self.path();
        let mut file = OpenOptions::new().read(true).open(path)?;
        let mut str = String::new();

        file.read_to_string(&mut str)?;

        Ok(str)
    }
}

/// Read from sysfs file, and perform conversion
pub trait TypedRead: RawRead {
    type Repr;

    fn read(&self) -> Result<Self::Repr, Error>;
}

/// Write a raw string to sysfs attribute file
pub trait RawWrite: AttributeBase {
    fn write_raw(&self, value: impl Into<String>) -> Result<(), Error> {
        let path = self.path();

        OpenOptions::new()
            .write(true)
            .open(path)
            .and_then(|mut f| { write!(f, "{}", value.into()) } )?;

        Ok(())
    }
}

/// Perform Repr to String conversion, and write to sysfs file.
pub trait TypedWrite: RawWrite {
    type Repr;

    fn write(&self, value: Self::Repr) -> Result<(), Error>;
}

/// Wrapper type to prevent an attribute from using any of the write* functions.
pub struct ReadOnly<T> where T: RawRead {
    attribute: T
}

impl<T: RawRead> AttributeBase for ReadOnly<T> {
    fn path(&self) -> &str { self.attribute.path() }
}

impl<T: RawRead> RawRead for ReadOnly<T> {
    fn read_raw(&self) -> Result<String, Error> {
        self.attribute.read_raw()
    }
}

impl<T: TypedRead> TypedRead for ReadOnly<T> {
    type Repr = T::Repr;

    fn read(&self) -> Result<Self::Repr, Error> {
        self.attribute.read()
    }
}

impl<T: RawRead> From<T> for ReadOnly<T> {
    fn from(attribute: T) -> Self {
        Self { attribute }
    }
}

/// Wrapper restricting an attribute from using any of the read* functions.
pub struct WriteOnly<T> where T: RawWrite {
    attribute: T,
}

impl<T: RawWrite> AttributeBase for WriteOnly<T> {
    fn path(&self) -> &str { self.attribute.path() }
}

impl<T: RawWrite> RawWrite for WriteOnly<T> {
    fn write_raw(&self, value: impl Into<String>) -> Result<(), Error> {
        self.attribute.write_raw(value)
    }
}

impl<T: TypedWrite> TypedWrite for WriteOnly<T> {
    type Repr = T::Repr;

    fn write(&self, value: Self::Repr) -> Result<(), Error> {
        self.attribute.write(value)
    }
}

impl<T: RawWrite> From<T> for WriteOnly<T> {
    fn from(attribute: T) -> Self {
        Self { attribute }
    }
}

/// Boolean attribute
///
/// These are handled as a special case because boolean can both be represented
/// as "Y/N" or "0/1" by the kernel, so a special parsing is needed.
///
struct Boolean {
    pub path: String
}

impl AttributeBase for Boolean {
    fn path(&self) -> &str {
        self.path.as_str()
    }
}

impl RawRead for Boolean {}
impl RawWrite for Boolean {}

impl TypedRead for Boolean {
    type Repr = bool;

    fn read(&self) -> Result<Self::Repr, Error> {
        let repr = self.read_raw()?;

        match repr.trim() {
            "Y" | "1" => Ok(true),
            "N" | "0" => Ok(false),
            _ => Err(Error::ConvError)
        }
    }
}

impl TypedWrite for Boolean {
    type Repr = bool;

    fn write(&self, value: Self::Repr) -> Result<(), Error> {
        self.write_raw(format!("{}", value as u8))
    }
}

/// Attribute with arbitrary implementation
struct Generic<T> {
    pub path: String,
    _phantom: PhantomData<T>
}

impl<T> AttributeBase for Generic<T> {
    fn path(&self) -> &str {
        self.path.as_str()
    }
}

impl<T> RawRead for Generic<T> {}
impl<T> RawWrite for Generic<T> {}

impl<T> TypedRead for Generic<T>
where T: FromStr {
    type Repr = T;

    fn read(&self) -> Result<Self::Repr, Error> {
        self.read_raw()?.parse().map_err(|_| {
            Error::ConvError
        })
    }
}

impl<T> TypedWrite for Generic<T>
where T: ToString {
    type Repr = T;
    fn write(&self, value: Self::Repr) -> Result<(), Error> {
        self.write_raw(value.to_string())
    }
}

#[allow(dead_code)]
type RBoolean = ReadOnly<Boolean>;

#[allow(dead_code)]
type WBoolean = WriteOnly<Boolean>;
