#[derive(Debug)]
pub enum Error {
    FileEnded,
    WrongFiletype,
    ImageTooBig,
    MissingBlockterminator,
    InvalidBlockintroducer,
    InterlacingNotSupported,
    GifEnded,
    InvalidSymbol,
    DecoderAlreadyFinished,
    ReverseBufferOverflow,
    RenderError,
    RewindError,
}
