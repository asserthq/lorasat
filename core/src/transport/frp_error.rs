use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub enum FrpError {
    InvalidControlByte,
    BufferTooShort,
    InvalidSessionId,
    ChunkIndexOutOfBounds,
    SessionNotInitialized,
}

impl fmt::Display for FrpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidControlByte => write!(f, "Неверный управляющий байт FRP"),
            Self::BufferTooShort => write!(f, "Буфер слишком мал для сообщения FRP"),
            Self::InvalidSessionId => write!(f, "Несовпадение Session ID"),
            Self::ChunkIndexOutOfBounds => write!(f, "Индекс фрагмента выходит за границы"),
            Self::SessionNotInitialized => {
                write!(f, "Сессия получателя не инициализирована (нет START)")
            }
        }
    }
}
impl core::error::Error for FrpError {}
