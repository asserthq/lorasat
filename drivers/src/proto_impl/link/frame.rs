//! Канальный PDU (Protocol Data Unit) — тип `Frame`.
//!
//! На канальном уровне PDU принято называть **frame** (кадр).
//! В литературе: L2-PDU, link-layer frame, MAC frame.
//! Общий термин для единицы данных любого уровня — PDU.
//!
//! # Wire format (postcard)
//!
//! Кодирование через `postcard` (serde). Enum-вариант сериализуется
//! как variant index (varint) + поля варианта.
//!
//! ```text
//! Beacon:
//! ┌──────────────┬───────────────────┐
//! │ variant: 0   │ payload: &[u8]    │
//! │   (varint)   │  (varint len +    │
//! │              │   raw bytes)      │
//! └──────────────┴───────────────────┘
//!
//! Data:
//! ┌──────────────┬──────────┬──────────┬───────────────────┐
//! │ variant: 1   │ src: u32 │ dst: u32 │ payload: &[u8]    │
//! │   (varint)   │ (varint) │ (varint) │  (varint len +    │
//! │              │          │          │   raw bytes)      │
//! └──────────────┴──────────┴──────────┴───────────────────┘
//! ```
//!
//! Все целые поля (`u32`, длина среза) кодируются как varint
//! (compact variable-length integer). Порядок байт — little-endian.
//!
//! Отказ от явного tag byte в пользу serde enum — осознанный:
//! postcard гарантирует однозначное отображение variant index.
//! Если потребуется ручной контроль кодирования (например,
//! для совместимости с C-реализациями), заменить на:
//!
//! ```ignore
//! const TAG_BEACON: u8 = 0x00;
//! const TAG_DATA: u8   = 0x01;
//!
//! impl Frame<'_> {
//!     fn encode(&self, buf: &mut [u8]) -> Result<usize, Error> {
//!         match self {
//!             Frame::Beacon(b) => {
//!                 buf[0] = TAG_BEACON;
//!                 // ... тело маяка ...
//!             }
//!             Frame::Data(d) => {
//!                 buf[0] = TAG_DATA;
//!                 // ... тело данных ...
//!             }
//!         }
//!     }
//! }
//! ```

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, defmt::Format)]
pub enum Frame<'a> {
    #[serde(borrow)]
    Beacon(BeaconFrame<'a>),
    #[serde(borrow)]
    Data(DataFrame<'a>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, defmt::Format)]
pub struct DataFrame<'a> {
    pub src: u32,
    pub dst: u32,
    #[serde(borrow)]
    pub payload: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, defmt::Format)]
pub struct BeaconFrame<'a> {
    #[serde(borrow)]
    pub payload: &'a [u8],
}
