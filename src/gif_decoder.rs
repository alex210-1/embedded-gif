use crate::frame_decoder::{
    FrameDecoder, GifFrameMetadata, GraphicsControlExtension, ImageArea, LzwEntry,
};
use crate::renderer::ImageRenderer;
use crate::{gif_error::Error, util::color565_from_rgb};
use core::{str::from_utf8, usize};

pub const MAX_SIZE: u16 = 360;
pub const REVERSE_BUF_LEN: usize = 512; // depends on MaxSize
pub const OUT_BUF_LEN: usize = 240 * 20; // 20 lines

pub trait Rewindable {
    fn rewind(&mut self) -> Result<(), Error>;
}

#[derive(Clone)]
pub struct GifFileMetadata {
    width: u16,
    height: u16,
    global_color_table_size: usize,
    // background_color_index: u8, // TODO implement
    has_global_color_table: bool,
}

/// Streaming GIF Decoder.
/// Takes an iterator that serves the bytes of a GIF file as intput,
/// emits the resulting image in bursts of lines.
/// Works completely allocationless, needs about 20kiB for the decoding tables.
///
/// Usage: Construct with a data source and a renderer. Call parse_gif_metadata().
/// Then for each frame call parse_frame_metadata() followed by decode_frame_image().
pub struct GifDecoder<'a, DS, R> {
    data_source: &'a mut DS,
    file_metadata: Option<GifFileMetadata>,
    current_frame_metadata: Option<GifFrameMetadata>,
    renderer: &'a mut R,
    global_color_table: &'a mut [u16; 256],
    current_local_color_table: &'a mut [u16; 256],
    lzw_table: &'a mut [LzwEntry; 4096],
    reverse_buffer: &'a mut [u8; REVERSE_BUF_LEN],
    output_buffer: &'a mut [u8; OUT_BUF_LEN],
}

// TODO the proper way to implement this would be with seperate typestes
// but that seems overkill fo now, because it is tricky to do allocationless
impl<'a, DS, R> GifDecoder<'a, DS, R>
where
    DS: Iterator<Item = u8>,
    R: ImageRenderer,
{
    /// buffers need to be passed in from outside so that this object still fits on the stack
    pub fn new(
        data_source: &'a mut DS,
        renderer: &'a mut R,
        buf_a: &'a mut [u16; 256],
        buf_b: &'a mut [u16; 256],
        buf_c: &'a mut [LzwEntry; 4096],
        buf_d: &'a mut [u8; REVERSE_BUF_LEN],
        buf_e: &'a mut [u8; OUT_BUF_LEN],
    ) -> Self {
        GifDecoder {
            data_source,
            file_metadata: None,
            current_frame_metadata: None,
            renderer,
            global_color_table: buf_a,
            current_local_color_table: buf_b,
            lzw_table: buf_c,
            reverse_buffer: buf_d,
            output_buffer: buf_e,
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
        let _background_color_index = self.next_byte()?;
        self.next_byte()?; // ignore aspect ratio

        let has_global_color_table = (packed_fields & 1 << 7) != 0;
        let table_bits = (packed_fields & 0b00000111) + 1;
        let global_color_table_size = 1 << table_bits;

        Ok(GifFileMetadata {
            width,
            height,
            // background_color_index,
            has_global_color_table,
            global_color_table_size,
        })
    }

    fn parse_global_color_table(&mut self, size: usize) -> Result<(), Error> {
        for i in 0..size {
            let r = self.next_byte()?;
            let g = self.next_byte()?;
            let b = self.next_byte()?;

            self.global_color_table[i as usize] = color565_from_rgb(r, g, b);
        }
        Ok(())
    }

    fn parse_local_color_table(&mut self, size: usize) -> Result<(), Error> {
        for i in 0..size {
            let r = self.next_byte()?;
            let g = self.next_byte()?;
            let b = self.next_byte()?;

            self.current_local_color_table[i as usize] = color565_from_rgb(r, g, b);
        }
        Ok(())
    }

    /// Parses and consumes the initial metadata section of a GIF file
    pub fn parse_gif_metadata(&mut self) -> Result<(), Error> {
        self.validate_header()?;
        let metadata = self.parse_logical_screen_descriptor()?;

        if metadata.width > MAX_SIZE || metadata.height > MAX_SIZE {
            return Err(Error::ImageTooBig);
        };
        if metadata.has_global_color_table {
            self.parse_global_color_table(metadata.global_color_table_size)?;
        }
        self.file_metadata = Some(metadata);

        Ok(())
    }

    pub fn get_gif_metadata(&self) -> Option<&GifFileMetadata> {
        self.file_metadata.as_ref()
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

    /// Parses and consumes the metadata section of the next frame, including all
    /// GIF extensions up until the actual image data.
    /// Resturns Err(Error::GifEnded) when there is no frame left
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

    /// Decodes and consumes the image data of the frame.
    /// Calls renderer.write_area() whenever the output buffer is full.
    /// Calls renderer.flush_frame() when all images data has been written.
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
            *color_table,
            self.lzw_table,
            self.reverse_buffer,
            self.output_buffer,
            self.renderer,
            initial_lzw_size,
        );

        frame_decoder.decode_frame()
    }

    pub fn get_data_source(&mut self) -> &mut DS {
        &mut self.data_source
    }
}

// optional rewind capability of datasource
impl<'a, DS, R> GifDecoder<'a, DS, R>
where
    DS: Rewindable,
{
    pub fn rewind(&mut self) -> Result<(), Error> {
        self.data_source.rewind()
    }
}
