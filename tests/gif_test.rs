use embedded_gif::frame_decoder::LzwEntry;
use embedded_gif::gif_decoder::{OUT_BUF_LEN, REVERSE_BUF_LEN};
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

fn vec_to_boxed_array<T: Copy, const N: usize>(val: T) -> Box<[T; N]> {
    let boxed_slice = vec![val; N].into_boxed_slice();

    let ptr = Box::into_raw(boxed_slice) as *mut [T; N];

    unsafe { Box::from_raw(ptr) }
}

#[test]
fn gif_test() {
    let _ = remove_dir_all("./tests/frames");
    create_dir("./tests/frames").unwrap();

    let bytes = read("./tests/gifs/test_large.gif").unwrap();

    let mut data_source = bytes.into_iter();
    let mut renderer = TestRenderer::new();

    let mut buf_a = vec_to_boxed_array::<u16, 256>(0);
    let mut buf_b = vec_to_boxed_array::<u16, 256>(0);
    let mut buf_c = vec_to_boxed_array::<LzwEntry, 4096>(LzwEntry::default());
    let mut buf_d = vec_to_boxed_array::<u8, REVERSE_BUF_LEN>(0);
    let mut buf_e = vec_to_boxed_array::<u8, OUT_BUF_LEN>(0);

    let mut decoder = GifDecoder::new(
        &mut data_source,
        &mut renderer,
        &mut *buf_a,
        &mut *buf_b,
        &mut *buf_c,
        &mut *buf_d,
        &mut *buf_e,
    );

    decoder.parse_gif_metadata().unwrap();

    loop {
        match decoder.parse_frame_metadata() {
            Ok(()) => decoder.decode_frame_image().unwrap(),
            Err(Error::GifEnded) => break,
            err => err.unwrap(),
        }
    }
}
