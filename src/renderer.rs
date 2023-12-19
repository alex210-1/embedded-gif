use crate::frame_decoder::ImageArea;
use crate::gif_error::Error;

pub trait ImageRenderer {
    fn write_area(
        &mut self,
        area: ImageArea,
        buffer: &[u8],
        color_table: &[u16; 256],
        transparency_index: Option<u8>,
    ) -> Result<(), Error>;

    fn flush_frame(&mut self) -> Result<(), Error>;
}
