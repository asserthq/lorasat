use sat_core::layer::physical::PhysicalLayer;

use lora_phy::{
    DelayNs, LoRa,
    mod_params::{Bandwidth, CodingRate, ModulationParams, PacketParams, RxMode, SpreadingFactor},
    mod_traits::InterfaceVariant,
    sx126x::{Config, Sx126x, Sx1262, TcxoCtrlVoltage},
};

use embedded_hal_async::spi::SpiDevice;

use super::error::Error;

pub struct Radio<SPI, IV, DLY>
where
    SPI: SpiDevice<u8>,
    IV: InterfaceVariant,
    DLY: DelayNs,
{
    lora: LoRa<Sx126x<SPI, IV, Sx1262>, DLY>,
    mod_params: ModulationParams,
    rx_pkt_params: PacketParams,
    tx_pkt_params: PacketParams,
}

// enum Frequency {
//     _868 = 868_100_000,
//     _435 = 435_100_000,
// }

impl<SPI, IV, DLY> Radio<SPI, IV, DLY>
where
    SPI: SpiDevice<u8>,
    IV: InterfaceVariant,
    DLY: DelayNs,
{
    pub async fn new(spi: SPI, iv: IV, delay: DLY, freq_hz: u32) -> Result<Self, Error> {
        let config = Config {
            chip: Sx1262,
            tcxo_ctrl: Some(TcxoCtrlVoltage::Ctrl1V7),
            use_dcdc: true,
            rx_boost: false,
        };

        let radio_kind = Sx126x::new(spi, iv, config);
        let mut lora = LoRa::new(radio_kind, false, delay)
            .await
            .map_err(|_| Error::CreateLora)?;
        lora.init().await.map_err(|_| Error::InitLora)?;

        let mod_params = lora
            .create_modulation_params(
                SpreadingFactor::_7,
                Bandwidth::_125KHz,
                CodingRate::_4_5,
                freq_hz,
            )
            .map_err(|_| Error::CreateModulationParams)?;

        let preamble_len = 8;

        let rx_pkt_params = lora
            .create_rx_packet_params(preamble_len, false, 255, true, false, &mod_params)
            .map_err(|_| Error::CreateTxPacketParams)?;

        let tx_pkt_params = lora
            .create_tx_packet_params(preamble_len, false, true, false, &mod_params)
            .map_err(|_| Error::CreateTxPacketParams)?;

        Ok(Self {
            lora,
            mod_params,
            rx_pkt_params,
            tx_pkt_params,
        })
    }
}

impl<SPI, IV, DLY> PhysicalLayer for Radio<SPI, IV, DLY>
where
    SPI: SpiDevice<u8>,
    IV: InterfaceVariant,
    DLY: DelayNs,
{
    type Error = super::error::Error;

    async fn try_send_bytes(&mut self, payload: &[u8]) -> Result<(), Self::Error> {
        let power_tx = 14;
        self.lora
            .prepare_for_tx(&self.mod_params, &mut self.tx_pkt_params, power_tx, payload)
            .await
            .map_err(|_| Error::PrepareForTx)?;

        self.lora.tx().await.map_err(|_| Error::Tx)?;
        Ok(())
    }

    async fn try_recv_bytes<'a>(&mut self, buf: &'a mut [u8]) -> Result<&'a mut [u8], Self::Error> {
        self.lora
            .prepare_for_rx(RxMode::Continuous, &self.mod_params, &self.rx_pkt_params)
            .await
            .map_err(|_| Error::PrepareForRx)?;

        let (len, _status) = self
            .lora
            .rx(&self.rx_pkt_params, buf)
            .await
            .map_err(|_| Error::Rx)?;
        Ok(&mut buf[..(len as usize)])
    }
}
