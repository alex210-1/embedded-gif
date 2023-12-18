use crate::frame_decoder::{
    FrameDecoder, GifFrameMetadata, GraphicsControlExtension, ImageArea, LzwEntry,
};
use crate::renderer::ImageRenderer;
use crate::{gif_error::Error, util::color565_from_rgb};
use core::cell::OnceCell;
use core::{str::from_utf8, usize};

pub const MAX_SIZE: u16 = 360;
pub const REVERSE_BUF_LEN: usize = 512; // depends on MaxSize
pub const OUT_BUF_LEN: usize = 1 << 13;

#[derive(Clone)]
pub struct GifFileMetadata {
    width: u16,
    height: u16,
    global_color_table_size: u8,
    background_color_index: u8,
    has_global_color_table: bool,
}

struct GifDecoder<'a, DS, R> {
    data_source: DS,
    file_metadata: OnceCell<GifFileMetadata>,
    current_frame_metadata: Option<GifFrameMetadata>,
    renderer: &'a R,
    global_color_table: [u16; 256],
    current_local_color_table: [u16; 256],
    lzw_table: [LzwEntry; 4096],
    reverse_buffer: [u16; REVERSE_BUF_LEN],
    output_buffer: [u8; OUT_BUF_LEN],
}

// TODO the proper way to implement this would be with seperate typestes
// but that seems overkill fo now, because it is tricky to do allocationless
impl<'a, DS, R> GifDecoder<'a, DS, R>
where
    DS: Iterator<Item = u8>,
    R: ImageRenderer,
{
    pub fn new(data_source: DS, renderer: &'a R) -> Self {
        GifDecoder {
            data_source,
            file_metadata: OnceCell::new(),
            current_frame_metadata: None,
            renderer,

            global_color_table: [0; 256],
            current_local_color_table: [0; 256],
            lzw_table: [LzwEntry::default(); 4096],
            reverse_buffer: [0; REVERSE_BUF_LEN],
            output_buffer: [0; OUT_BUF_LEN],
        }
    }

    fn next_byte(&mut self) -> Result<u8, Error> {
        self.data_source.next().ok_or(Error::FileEnded)
    }

    fn next_short(&mut self) -> Result<u16, Error> {
        let bytes: [u8; 2] = self.data_source.next_chunk().or(Err(Error::FileEnded))?;
        Ok(u16::from_le_bytes(bytes))
    }

    // === metadata ===

    /// verify that the magic number and version of the gif file are correct
    fn validate_header(&mut self) -> Result<(), Error> {
        let header: [u8; 6] = self.data_source.next_chunk().or(Err(Error::FileEnded))?;

        match from_utf8(&header) {
            Ok("GIF89a") => Ok(()),
            _ => Err(Error::WrongFiletype),
        }
    }

    /// see GIF 89a spec section 18. Parses LogicalScreenDescriptor into gifMetadata
    fn parse_logical_screen_descriptor(&mut self) -> Result<GifFileMetadata, Error> {
        let width = self.next_short()?;
        let height = self.next_short()?;
        let packed_fields = self.next_byte()?;
        let background_color_index = self.next_byte()?;
        self.next_byte()?; // ignore aspect ratio

        let has_global_color_table = (packed_fields & 1 << 7) != 0;
        let table_bits = (packed_fields & 0b00000111) + 1;
        let global_color_table_size = 1 << table_bits;

        Ok(GifFileMetadata {
            width,
            height,
            background_color_index,
            has_global_color_table,
            global_color_table_size,
        })
    }

    fn parse_global_color_table(&mut self, size: u8) -> Result<(), Error> {
        for i in 0..size {
            let r = self.next_byte()?;
            let g = self.next_byte()?;
            let b = self.next_byte()?;

            self.global_color_table[i as usize] = color565_from_rgb(r, g, b);
        }
        Ok(())
    }

    // TODO deduplicate. find way to please borrow checker
    fn parse_local_color_table(&mut self, size: u8) -> Result<(), Error> {
        for i in 0..size {
            let r = self.next_byte()?;
            let g = self.next_byte()?;
            let b = self.next_byte()?;

            self.current_local_color_table[i as usize] = color565_from_rgb(r, g, b);
        }
        Ok(())
    }

    pub fn parse_gif_metadata(&mut self) -> Result<(), Error> {
        self.validate_header()?;
        let metadata = self.parse_logical_screen_descriptor()?;

        if metadata.width > MAX_SIZE || metadata.height > MAX_SIZE {
            return Err(Error::ImageTooBig);
        };
        if metadata.has_global_color_table {
            self.parse_global_color_table(metadata.global_color_table_size)?;
        }
        self.file_metadata.set(metadata);

        Ok(())
    }

    pub fn get_gif_metadata(&self) -> Option<&GifFileMetadata> {
        self.file_metadata.get()
    }

    // === parse frame ===

    /// See GIF 89a spec section 23.
    /// Extension Introducer, label and block size already handled by caller
    fn parse_graphics_control_extension(&mut self) -> Result<GraphicsControlExtension, Error> {
        let packed_fields = self.next_byte()?;
        let hundedths_delay = self.next_short()?;
        let transparency_index = self.next_byte()?;

        let has_transparency = packed_fields & 1 != 0;

        let terminator = self.next_byte()?;
        if terminator != 0 {
            return Err(Error::MissingBlockterminator);
        }

        Ok(GraphicsControlExtension {
            millis_delay: hundedths_delay as u32 * 10,
            has_transparency,
            transparency_index,
        })
    }

    /// See GIF 89a spec section 20.
    /// Image Separator already handled by caller
    fn parse_image_descriptor(
        &mut self,
        extension: Option<GraphicsControlExtension>,
    ) -> Result<GifFrameMetadata, Error> {
        let xpos = self.next_short()?;
        let ypos = self.next_short()?;
        let width = self.next_short()?;
        let height = self.next_short()?;
        let packed_fields = self.next_byte()?;

        let has_local_color_table = (packed_fields & 1 << 7) != 0;
        let interlace = (packed_fields & 1 << 6) != 0;
        let color_table_bits = packed_fields & 0b00000111;
        let local_color_table_size = 1 << (color_table_bits + 1);

        if interlace {
            return Err(Error::InterlacingNotSupported);
        }

        Ok(GifFrameMetadata {
            frame_area: ImageArea {
                xpos,
                ypos,
                width,
                height,
            },
            local_color_table_size,
            has_local_color_table,
            extension,
        })
    }

    pub fn parse_frame_metadata(&mut self) -> Result<(), Error> {
        let mut extension: Option<GraphicsControlExtension> = None;

        loop {
            let block_introducer = self.next_byte()?;

            match block_introducer {
                0x2C => {
                    // Image descriptor
                    let metadata = self.parse_image_descriptor(extension)?;

                    if metadata.has_local_color_table {
                        self.parse_local_color_table(metadata.local_color_table_size)?;
                    }
                    self.current_frame_metadata = Some(metadata);

                    return Ok(()); // image data follows
                }
                0x21 => {
                    // Extension
                    let extension_label = self.next_byte()?;
                    let mut block_size = self.next_byte()?;

                    if extension_label == 0xF9 {
                        // graphics control extension
                        extension = Some(self.parse_graphics_control_extension()?);
                    } else {
                        // ignore all other extensions
                        while block_size != 0 {
                            for _ in 0..block_size {
                                self.next_byte()?;
                            }
                            block_size = self.next_byte()?;
                        }
                    }
                }
                0x3B => {
                    return Err(Error::GifEnded);
                }
                _ => {
                    return Err(Error::InvalidBlockintroducer);
                }
            }
        }
    }

    pub fn decode_frame_image(&mut self) -> Result<(), Error> {
        // == construct frame decoder ==
        let initial_lzw_size = self.next_byte()?;

        let metadata = self.current_frame_metadata.as_ref().unwrap();

        let color_table = match metadata.has_local_color_table {
            true => &mut self.current_local_color_table,
            false => &mut self.global_color_table,
        };

        let mut frame_decoder = FrameDecoder::new(
            &mut self.data_source,
            metadata,
            color_table,
            &mut self.lzw_table,
            &mut self.reverse_buffer,
            &mut self.output_buffer,
            self.renderer,
            initial_lzw_size,
        );

        frame_decoder.decode_frame()
    }
}
