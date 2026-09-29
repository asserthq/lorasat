/// Ошибки транспортного уровня.
#[derive(Debug, PartialEq, defmt::Format)]
pub enum TransportError {
    /// Ошибка нижележащего канального уровня.
    Link,
    /// Таймаут ожидания ACK.
    Timeout,
    /// Ошибка сериализации PDU.
    Encode,
    /// Ошибка десериализации PDU.
    Decode,
    /// Сообщение не влезает в MAX_CHUNKS × MTU.
    PayloadTooLarge,
}
