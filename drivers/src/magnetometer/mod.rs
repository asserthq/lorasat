pub mod error;
pub mod mmc5983ma;

pub use error::Error;
pub use mmc5983ma::{MagneticField, Mmc5983Ma, Temperature};
