use embedded_gif::gif_error::Error;
use embedded_gif::renderer::ImageRenderer;
use embedded_gif::{frame_decoder::ImageArea, gif_decoder::GifDecoder};
use image::{ImageBuffer, Rgba};
use std::fs::create_dir;
use std::fs::read;
use std::fs::remove_dir_all;

const SCREEN_SIZE: usize = 240;

struct TestRenderer {
    screen: ImageBuffer<Rgba<u8>, Vec<u8>>,
    current_frame: usize,
}

impl TestRenderer {
    fn new() -> Self {
        let mut screen = ImageBuffer::new(SCREEN_SIZE as u32, SCREEN_SIZE as u32);
        screen.fill(0);

        Self {
            screen,
            current_frame: 0,
        }
    }
}

impl ImageRenderer for TestRenderer {
    fn write_area(
        &mut self,
        area: ImageArea,
        buffer: &[u8],
        color_table: &[u16; 256],
        transparency_index: Option<u8>,
    ) -> Result<(), Error> {
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
                    let pixel = Rgba([r, g, b, a]);

                    self.screen.put_pixel(x as u32, y as u32, pixel);
                }
            }
        }
        Ok(())
    }

    fn flush_frame(&mut self) -> Result<(), Error> {
        self.screen
            .save(format!("./tests/frames/frame_{}.png", self.current_frame))
            .or(Err(Error::RenderError))?;

        self.current_frame += 1;
        self.screen.fill(0);
        Ok(())
    }
}

#[test]
fn gif_test() {
    let _ = remove_dir_all("./tests/frames");
    create_dir("./tests/frames").unwrap();

    let bytes = read("./tests/gifs/test_large.gif").unwrap();

    let data_source = bytes.into_iter();
    let mut renderer = TestRenderer::new();

    let mut decoder = GifDecoder::new(data_source, &mut renderer);

    decoder.parse_gif_metadata().unwrap();

    loop {
        match decoder.parse_frame_metadata() {
            Ok(()) => decoder.decode_frame_image().unwrap(),
            Err(Error::GifEnded) => break,
            err => err.unwrap(),
        }
    }
}
