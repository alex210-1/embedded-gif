use embedded_gif::gif_decoder::GifDecoder;
use embedded_gif::renderer::ImageRenderer;
use std::{fs::File, io::BufReader};

const SCREEN_SIZE: usize = 240;

#[derive(Clone, Copy)]
struct RGBA {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

impl Default for RGBA {
    fn default() -> Self {
        Self {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        }
    }
}

struct TestRenderer {
    screen: [[RGBA; SCREEN_SIZE]; SCREEN_SIZE],
    current_frame: usize,
}

impl TestRenderer {
    fn new() -> Self {
        Self {
            screen: [[RGBA::default(); SCREEN_SIZE]; SCREEN_SIZE],
            current_frame: 0,
        }
    }
}

impl ImageRenderer for TestRenderer {
    fn write_area(
        &mut self,
        area: embedded_gif::frame_decoder::ImageArea,
        buffer: &[u8],
        color_table: &[u16; 256],
        transparency_index: Option<u8>,
    ) -> Result<(), embedded_gif::gif_error::Error> {
        let mut buf_index = 0;

        for y in area.ypos..(area.ypos + area.height) {
            for x in area.xpos..(area.xpos + area.width) {
                let color_index = buffer[buf_index];
                buf_index += 1;
                let color_16 = color_table[color_index as usize];

                let r = ((color_16 & 0b1111100000000000) >> 8) as u8;
                let g = ((color_16 & 0b0000011111100000) >> 3) as u8;
                let b = ((color_16 & 0b0000000000011111) << 3) as u8;

                let a = match transparency_index {
                    Some(ti) if ti == color_index => 0x00,
                    _ => 0xFF,
                };

                if (x as usize) < SCREEN_SIZE && (y as usize) < SCREEN_SIZE {
                    let rgba = RGBA { r, g, b, a };
                    self.screen[SCREEN_SIZE - y as usize - 1][x as usize] = rgba;
                }
            }
        }
        Ok(())
    }

    fn flush_frame(&mut self) -> Result<(), embedded_gif::gif_error::Error> {
        todo!()
    }
}

#[test]
fn gif_test() {
    let gif_file = File::open("./test/gifs/test_small.gif");
    let reader = BufReader::new(gif_file.unwrap());
}
