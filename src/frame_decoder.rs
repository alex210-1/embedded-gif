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
    pub local_color_table_size: u8,
    pub has_local_color_table: bool,
    pub extension: Option<GraphicsControlExtension>,
}

#[derive(Default, Clone, Copy)]
#[repr(packed)]
pub struct LzwEntry {
    first: u16,
    last: u16,
}

pub struct FrameDecoder<'a, DS, R> {
    // constant state
    data_source: &'a mut DS,
    frame_metadata: &'a GifFrameMetadata,
    color_table: &'a mut [u16; 256],
    lzw_table: &'a mut [LzwEntry; 4096],
    reverse_buffer: &'a mut [u16],
    output_buffer: &'a mut [u8],
    renderer: &'a R,
    initial_symbol_size: u8,
    clear_code: u16,
    stop_code: u16,
    transparency_index: Option<u8>,

    // mutable state
    current_symbol_size: u8,
    table_index: u16,
    bit_buffer: u32,
    bit_count: u8,
    last_symbol: Option<u16>,
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
        reverse_buffer: &'a mut [u16],
        output_buffer: &'a mut [u8],
        renderer: &'a R,
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

            current_symbol_size: initial_lzw_size + 1,
            table_index: clear_code + 1,
            bit_buffer: 0,
            bit_count: 0,
            last_symbol: None,
        }
    }

    fn next_byte(&mut self) -> Result<u8, Error> {
        self.data_source.next().ok_or(Error::FileEnded)
    }

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

    fn process_byte(&mut self, byte: u8) -> Result<(), Error> {
        self.bit_buffer = self.bit_buffer >> 8 | (byte as u32) << 24;

        while self.current_symbol_size <= self.bit_count {
            let shift = 32 - self.bit_count;
            let mask = ((1u32 << self.current_symbol_size) - 1) << shift;
            let symbol = ((self.bit_buffer & mask) >> shift) as u16;
            self.bit_count -= self.current_symbol_size;

            self.process_symbol(symbol)?;
        }
        Ok(())
    }

    /// see https://de.wikipedia.org/wiki/Lempel-Ziv-Welch-Algorithmus
    fn process_symbol(&mut self, symbol: u16) -> Result<(), Error> {
        if symbol == self.clear_code {
            self.on_clear_code();
        } else if symbol == self.stop_code {
            self.on_stop_code()?;
        };

        // first iteration
        if self.last_symbol == None {
            self.last_symbol = Some(symbol);

            return self.process_pixel(symbol as u8);
        }

        // space in table
        if self.table_index < 4096 - 1 {
            // handle lzw special case
            let current_symbol = if symbol <= self.table_index {
                symbol
            } else {
                self.last_symbol.unwrap()
            };

            let first_symbol = 
        }

        todo!()
    }

    fn on_clear_code(&mut self) {
        // reset table
        self.current_symbol_size = self.initial_symbol_size;
        self.table_index = self.stop_code;

        // The spec is not clear about this. I assume, the lastSymbol
        // should be refetched on a clear symbol. This seems to work
        self.last_symbol = None;
    }

    fn on_stop_code(&mut self) -> Result<(), Error> {
        todo!()
    }

    fn process_pixel(&mut self, pixel: u8) -> Result<(), Error> {
        todo!()
    }

    fn find_first_symbol_in_chain(&mut self, start: u16) -> u16 {
        todo!()
    }
}
