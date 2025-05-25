pub mod ioctls {
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
}
