use defmt_or_log::debug;
use embedded_hal::digital::OutputPin;
use sat_csp::CspInterface;

use lora_phy::{
    DelayNs, LoRa,
    iv::GenericSx126xInterfaceVariant,
    mod_params::{Bandwidth, CodingRate, ModulationParams, PacketParams, RxMode, SpreadingFactor},
    sx126x::{self, Sx126x, Sx1262, TcxoCtrlVoltage},
};

use embedded_hal_async::{digital::Wait, spi::SpiDevice};

use super::error::RadioError;

pub struct RadioSx1262<SPI, CTRL, WAIT, DLY>
where
    SPI: SpiDevice<u8>,
    CTRL: OutputPin,
    WAIT: Wait,
    DLY: DelayNs,
{
    lora: LoRa<Sx126x<SPI, GenericSx126xInterfaceVariant<CTRL, WAIT>, Sx1262>, DLY>,
    mod_params: ModulationParams,
    rx_pkt_params: PacketParams,
    tx_pkt_params: PacketParams,
}

impl<SPI, CTRL, WAIT, DLY> RadioSx1262<SPI, CTRL, WAIT, DLY>
where
    SPI: SpiDevice<u8>,
    CTRL: OutputPin,
    WAIT: Wait,
    DLY: DelayNs,
{
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        spi: SPI,
        reset: CTRL,
        irq_dio1: WAIT,
        irq_busy: WAIT,
        rf_switch_rx: Option<CTRL>,
        rf_switch_tx: Option<CTRL>,
        delay: DLY,
        freq_hz: u32,
    ) -> Result<Self, RadioError> {
        let iv = GenericSx126xInterfaceVariant::new(
            reset,
            irq_dio1,
            irq_busy,
            rf_switch_rx,
            rf_switch_tx,
        )
        .map_err(|_| RadioError::CreateInterfaceVariant)?;
        let config = sx126x::Config {
            chip: Sx1262,
            tcxo_ctrl: Some(TcxoCtrlVoltage::Ctrl1V8),
            use_dcdc: true,
            rx_boost: true,
        };
        let mut lora = LoRa::new(Sx126x::new(spi, iv, config), false, delay)
            .await
            .map_err(|_| RadioError::CreateLora)?;

        let mod_params = lora
            .create_modulation_params(
                SpreadingFactor::_12,
                Bandwidth::_125KHz,
                CodingRate::_4_5,
                freq_hz,
            )
            .map_err(|_| RadioError::CreateModulationParams)?;

        let preamble_len = 8;

        let rx_pkt_params = lora
            .create_rx_packet_params(preamble_len, false, 255, true, false, &mod_params)
            .map_err(|_| RadioError::CreateRxPacketParams)?;

        let tx_pkt_params = lora
            .create_tx_packet_params(preamble_len, false, true, false, &mod_params)
            .map_err(|_| RadioError::CreateTxPacketParams)?;

        Ok(Self {
            lora,
            mod_params,
            rx_pkt_params,
            tx_pkt_params,
        })
    }
}

impl<SPI, CTRL, WAIT, DLY> CspInterface for RadioSx1262<SPI, CTRL, WAIT, DLY>
where
    SPI: SpiDevice<u8>,
    CTRL: OutputPin,
    WAIT: Wait,
    DLY: DelayNs,
{
    type Error = RadioError;

    async fn send(&mut self, frame: &[u8]) -> Result<(), Self::Error> {
        self.lora
            .prepare_for_tx(&self.mod_params, &mut self.tx_pkt_params, 20, frame)
            .await
            .map_err(|_| RadioError::PrepareForTx)?;

        self.lora.tx().await.map_err(|_| RadioError::Tx)?;
        debug!("radio tx bytes: {:?}", frame);
        Ok(())
    }

    async fn receive(&mut self, frame: &mut [u8]) -> Result<usize, Self::Error> {
        self.lora
            .prepare_for_rx(RxMode::Continuous, &self.mod_params, &self.rx_pkt_params)
            .await
            .map_err(|_| RadioError::PrepareForRx)?;

        let (length, _status) = self
            .lora
            .rx(&self.rx_pkt_params, frame)
            .await
            .map_err(|_| RadioError::Rx)?;

        Ok(length as usize)
    }
}
