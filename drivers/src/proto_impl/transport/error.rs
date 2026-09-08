use core::fmt::Debug;

#[derive(defmt::Format, Debug, PartialEq, Eq)]
pub enum TransportError {
    Link,
    Timeout,
}
