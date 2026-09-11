#[derive(Debug, PartialEq, defmt::Format)]
pub enum Error {
    CreateInterfaceVariant,
    CreateLora,
    CreateModulationParams,
    CreateTxPacketParams,
    PrepareForTx,
    Tx,
    CreateRxPacketParams,
    PrepareForRx,
    Rx,
}
