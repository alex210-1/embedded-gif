pub fn color565_from_rgb(r: u8, g: u8, b: u8) -> u16 {
    (r as u16 & 0xF8) << 8 | (g as u16 & 0xFC) << 3 | b as u16 >> 3
}
