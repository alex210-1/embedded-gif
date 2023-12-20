use crate::gif_decoder::{OUT_BUF_LEN, REVERSE_BUF_LEN};
use crate::gif_error::Error;
use crate::renderer::ImageRenderer;

#[derive(Clone, Copy)]
pub struct ImageArea {
    pub xpos: u16,
    pub ypos: u16,
    pub width: u16,
    pub height: u16,
}

pub struct GraphicsControlExtension {
    pub millis_delay: u32,
    pub has_transparency: bool,
    pub transparency_index: u8,
}

pub struct GifFrameMetadata {
    pub frame_area: ImageArea,
    pub local_color_table_size: usize,
    pub has_local_color_table: bool,
    pub extension: Option<GraphicsControlExtension>,
}

// TODO is speedup due to aligned access significant enough to justify this much padding?
#[derive(Default, Clone, Copy)]
#[repr(packed(4))] // TODO does this work as intended?
pub struct LzwEntry {
    first: u16,
    last: u8,
}

/// Decodes a single frame of a GIF file using LZW compression
// a 2-12 bit input token is reffered to as a symbol,
// an lzw table entry containing a pair of symbols is caled an entry
pub struct FrameDecoder<'a, DS, R> {
    // initial state
    data_source: &'a mut DS,
    frame_metadata: &'a GifFrameMetadata,
    color_table: &'a mut [u16; 256],
    lzw_table: &'a mut [LzwEntry; 4096],
    reverse_buffer: &'a mut [u8],
    output_buffer: &'a mut [u8],
    renderer: &'a mut R,
    initial_symbol_size: u8,
    clear_code: u16,
    stop_code: u16,
    transparency_index: Option<u8>,
    output_section_height: u16,

    // mutable state
    current_symbol_size: u8,
    table_index: u16,
    bit_buffer: u32,
    bit_count: u8,
    last_symbol: Option<u16>,
    output_line: u16,
    output_index: usize,
    finished: bool,
}

impl<'a, DS, R> FrameDecoder<'a, DS, R>
where
    DS: Iterator<Item = u8>,
    R: ImageRenderer,
{
    pub(crate) fn new(
        data_source: &'a mut DS,
        frame_metadata: &'a GifFrameMetadata,
        color_table: &'a mut [u16; 256],
        lzw_table: &'a mut [LzwEntry; 4096],
        reverse_buffer: &'a mut [u8],
        output_buffer: &'a mut [u8],
        renderer: &'a mut R,
        initial_lzw_size: u8,
    ) -> Self {
        let clear_code = 1 << initial_lzw_size;

        let transparency_index = match frame_metadata.extension {
            Some(GraphicsControlExtension {
                has_transparency: true,
                transparency_index,
                ..
            }) => Some(transparency_index),
            _ => None,
        };

        let output_section_height = (OUT_BUF_LEN / frame_metadata.frame_area.width as usize) as u16;

        Self {
            data_source,
            frame_metadata,
            color_table,
            lzw_table,
            reverse_buffer,
            output_buffer,
            renderer,
            initial_symbol_size: initial_lzw_size + 1,
            clear_code,
            stop_code: clear_code + 1,
            transparency_index,
            output_section_height,

            current_symbol_size: initial_lzw_size + 1,
            table_index: clear_code + 1,
            bit_buffer: 0,
            bit_count: 0,
            last_symbol: None,
            output_line: 0,
            output_index: 0,
            finished: false,
        }
    }

    fn next_byte(&mut self) -> Result<u8, Error> {
        self.data_source.next().ok_or(Error::FileEnded)
    }

    /// consumes and decoded all blocks of image data in input stream
    pub(crate) fn decode_frame(&mut self) -> Result<(), Error> {
        let mut block_size = self.next_byte()?;

        while block_size != 0 {
            for _ in 0..block_size {
                let data = self.next_byte()?;
                self.process_byte(data)?;
            }
            block_size = self.next_byte()?;
        }
        Ok(())
    }

    /// takes a single byte from the file and extracts the varibale-width symbols
    fn process_byte(&mut self, byte: u8) -> Result<(), Error> {
        self.bit_buffer = self.bit_buffer >> 8 | (byte as u32) << 24;
        self.bit_count += 8;

        while self.current_symbol_size <= self.bit_count {
            let shift = 32 - self.bit_count;
            let mask = ((1u32 << self.current_symbol_size) - 1) << shift;
            let symbol = ((self.bit_buffer & mask) >> shift) as u16;
            self.bit_count -= self.current_symbol_size;

            self.process_symbol(symbol)?;
        }
        Ok(())
    }

    /// decodes a single LZW input symbol
    /// see https://de.wikipedia.org/wiki/Lempel-Ziv-Welch-Algorithmus
    fn process_symbol(&mut self, symbol: u16) -> Result<(), Error> {
        if self.finished {
            return Err(Error::DecoderAlreadyFinished);
        }

        if symbol == self.clear_code {
            return self.on_clear_code();
        } else if symbol == self.stop_code {
            return self.on_stop_code();
        };

        // first iteration
        if self.last_symbol == None {
            self.last_symbol = Some(symbol);

            return self.process_pixel(symbol as u8);
        }

        if symbol > self.table_index + 1 {
            return Err(Error::InvalidSymbol);
        }

        // space in table
        if self.table_index < 4096 - 1 {
            // handle lzw special case
            let current_symbol = if symbol <= self.table_index {
                symbol
            } else {
                self.last_symbol.unwrap()
            };

            let first_symbol = self.find_first_symbol_in_chain(current_symbol);
            let new_entry = LzwEntry {
                first: self.last_symbol.unwrap(),
                last: first_symbol,
            };

            self.table_index += 1;
            self.lzw_table[self.table_index as usize] = new_entry;

            // check for new sybol size
            if self.table_index + 1 == 1 << self.current_symbol_size {
                if self.current_symbol_size < 12 {
                    self.current_symbol_size += 1;
                }
            }
        }

        self.emit_entry_chain(symbol)?;

        self.last_symbol = Some(symbol);
        Ok(())
    }

    /// resets the decoding tables to achieve higher compression ratios
    fn on_clear_code(&mut self) -> Result<(), Error> {
        // reset table
        self.current_symbol_size = self.initial_symbol_size;
        self.table_index = self.stop_code;

        // The spec is not clear about this. I assume, the lastSymbol
        // should be refetched on a clear symbol. This seems to work
        self.last_symbol = None;
        Ok(())
    }

    /// end of image. Write rest of data and flush renderer
    fn on_stop_code(&mut self) -> Result<(), Error> {
        if self.output_line < self.frame_metadata.frame_area.height {
            let remaining_height = self.frame_metadata.frame_area.height - self.output_line;
            self.render_buffer(remaining_height)?;
        }
        self.finished = true;

        self.renderer.flush_frame()?;
        Ok(())
    }

    /// puts a pixel into the output buffer and renders it when full
    /// TODO refactoring the pixel processing into a different module might be a good idea
    fn process_pixel(&mut self, pixel: u8) -> Result<(), Error> {
        self.output_buffer[self.output_index] = pixel;
        self.output_index += 1;

        let max_size =
            self.frame_metadata.frame_area.width as usize * self.output_section_height as usize;

        if self.output_index >= max_size {
            self.render_buffer(self.output_section_height)?;
        }
        Ok(())
    }

    /// follows a chain of LZW table entries until it finds a literal
    /// TODO deadlock theoretically possible here
    fn find_first_symbol_in_chain(&mut self, start: u16) -> u8 {
        let mut current_symbol = start;

        while current_symbol >= self.clear_code {
            current_symbol = self.lzw_table[current_symbol as usize].first;
        }

        current_symbol as u8
    }

    /// reverses chain of LZW table entries and outputs them
    fn emit_entry_chain(&mut self, start: u16) -> Result<(), Error> {
        let mut current_symbol = start;
        let mut reverse_index = 0;

        // shortcut for hot path
        if start < self.clear_code {
            return self.process_pixel(start as u8);
        }

        // follow chain
        loop {
            let entry = self.lzw_table[current_symbol as usize];
            current_symbol = entry.first;

            self.reverse_buffer[reverse_index] = entry.last;
            reverse_index += 1;

            if reverse_index >= REVERSE_BUF_LEN {
                return Err(Error::ReverseBufferOverflow);
            }

            if entry.first < self.clear_code {
                self.reverse_buffer[reverse_index] = entry.first as u8;
                reverse_index += 1;
                break;
            }
        }

        // unwind reverse buffer
        while reverse_index > 0 {
            reverse_index -= 1;
            self.process_pixel(self.reverse_buffer[reverse_index])?;
        }
        Ok(())
    }

    fn render_buffer(&mut self, height: u16) -> Result<(), Error> {
        let output_area = ImageArea {
            xpos: self.frame_metadata.frame_area.xpos,
            ypos: self.frame_metadata.frame_area.ypos + self.output_line,
            width: self.frame_metadata.frame_area.width,
            height,
        };

        self.renderer.write_area(
            output_area,
            self.output_buffer,
            self.color_table,
            self.transparency_index,
        )?;

        self.output_index = 0;
        self.output_line += height;

        Ok(())
    }
}
