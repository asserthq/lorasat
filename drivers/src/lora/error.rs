#[derive(Debug, PartialEq)]
pub enum Error {
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
