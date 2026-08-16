#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn with_alpha(self, alpha: u8) -> Self {
        Self { a: alpha, ..self }
    }

    pub const BLACK: Self = Self::rgb(0, 0, 0);
    pub const WHITE: Self = Self::rgb(255, 255, 255);
}

pub const ANNOTATION_COLORS: [Rgba; 8] = [
    Rgba::rgb(0xe8, 0x11, 0x23),
    Rgba::rgb(0xff, 0x8c, 0x00),
    Rgba::rgb(0xff, 0xd7, 0x00),
    Rgba::rgb(0x10, 0x89, 0x3e),
    Rgba::rgb(0x00, 0x78, 0xd4),
    Rgba::rgb(0x88, 0x17, 0x98),
    Rgba::WHITE,
    Rgba::BLACK,
];
