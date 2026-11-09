#[derive(Debug, PartialEq, defmt::Format)]
pub enum MagError {
    MagInitError,
    MagReadError,
}
