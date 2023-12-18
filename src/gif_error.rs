pub enum Error {
    FileEnded,
    WrongFiletype,
    ImageTooBig,
    MissingBlockterminator,
    InvalidBlockintroducer,
    InterlacingNotSupported,
    GifEnded,
}
