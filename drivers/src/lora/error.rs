#[derive(Debug, PartialEq)]
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
