#[derive(Debug, PartialEq, defmt::Format)]
pub enum Error {
    CreateInterfaceVariant,
    CreateLora,
    InitLora,
    CreateModulationParams,
    CreateTxPacketParams,
    PrepareForTx,
    Tx,
    CreateRxPacketParams,
    PrepareForRx,
    Rx,
}
