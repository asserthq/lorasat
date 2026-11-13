#[derive(Debug, PartialEq, defmt::Format)]
pub enum RadioError {
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
