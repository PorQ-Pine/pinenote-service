/// Driver support
pub mod drivers {
    pub mod drm {
        pub mod rockchip_ebc;
    }

    pub use drm::rockchip_ebc;
}

pub mod ioctls {
    use std::{fs::OpenOptions, os::unix::fs::OpenOptionsExt, path::Path};
    use nix::libc;
    use thiserror::Error;

    pub mod drm {
        pub const IOCTL_MAGIC: u8 = b'd';
        pub const COMMAND_BASE: u8 = 0x40;

        /// DRM UAPI 2D rectangle.
        pub struct Rect {
            /// Starting horizontal coordinate(inclusive)
            pub x1: i32,
            /// Starting vertical coordinate(inclusive)
            pub y1: i32,
            /// Ending horizontal coordinate(exclusive)
            pub x2: i32,
            /// Ending vertical coordinate(exclusive)
            pub y2: i32
        }

        pub mod rockchip_ebc;
    }

    pub use drm::rockchip_ebc;


    #[derive(Error, Debug)]
    #[error("Could not open device at '{path}'")]
    pub struct OpenError {
        path: String,
        source: std::io::Error
    }

    pub fn open_device(path: impl AsRef<Path>) -> Result<std::fs::File, OpenError> {
        OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&path)
            .map_err(|source|  {
                let path = path.as_ref().to_string_lossy().to_string();
                OpenError { path, source }
            })
    }
}

pub mod types {
    pub mod rockchip_ebc;
    pub mod rect;
    pub use rect::Rect;
}

pub mod sysfs {
    pub mod attribute;
}

