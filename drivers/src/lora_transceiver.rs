use sat_core::radio::transciever::HalfDuplexTransceiver;

use lora_phy::{
    DelayNs, LoRa,
    mod_params::{
        Bandwidth, CodingRate, ModulationParams, PacketParams, RadioError, RxMode, SpreadingFactor,
    },
    mod_traits::InterfaceVariant,
    sx126x::{Config, Sx126x, Sx1262, TcxoCtrlVoltage},
};

use embassy_time::Timer;
use embedded_hal_async::spi::SpiDevice;

/// SX1262-based LoRa transceiver driver.
///
/// Wraps `lora-phy` async API. `send()`/`receive()` stubbed —
/// only [`AsyncTransceiver`] methods do real work.
///
/// Default parameters: SF7, 125 kHz BW, 4/5 CR, 868.1 MHz,
/// preamble 8, explicit header, CRC on, TX power 14 dBm.
pub struct LoRa1262Transceiver<SPI, IV>
where
    SPI: SpiDevice<u8>,
    IV: InterfaceVariant,
{
    lora: LoRa<Sx126x<SPI, IV, Sx1262>, EmbassyDelay>,
    mod_params: ModulationParams,
    rx_pkt_params: PacketParams,
    tx_power: i32,
    preamble_len: u16,
}

impl<SPI, IV> LoRa1262Transceiver<SPI, IV>
where
    SPI: SpiDevice<u8>,
    IV: InterfaceVariant,
{
    /// Create and initialise radio.
    ///
    /// Call once from embassy task. Blocks until init done.
    pub async fn new(spi: SPI, iv: IV) -> Result<Self, RadioError> {
        let config = Config {
            chip: Sx1262,
            tcxo_ctrl: Some(TcxoCtrlVoltage::Ctrl1V7),
            use_dcdc: true,
            rx_boost: false,
        };

        let radio_kind = Sx126x::new(spi, iv, config);
        let mut lora = LoRa::new(radio_kind, false, EmbassyDelay).await?;
        lora.init().await?;

        let mod_params = lora.create_modulation_params(
            SpreadingFactor::_7,
            Bandwidth::_125KHz,
            CodingRate::_4_5,
            868_100_000,
        )?;

        let rx_pkt_params = lora.create_rx_packet_params(
            8,     // preamble_length
            false, // implicit_header
            255,   // max_payload_length
            true,  // crc_on
            false, // iq_inverted
            &mod_params,
        )?;

        Ok(Self {
            lora,
            mod_params,
            rx_pkt_params,
            tx_power: 14,
            preamble_len: 8,
        })
    }
}

impl<SPI, IV> HalfDuplexTransceiver for LoRa1262Transceiver<SPI, IV>
where
    SPI: SpiDevice<u8>,
    IV: InterfaceVariant,
{
    type Error = Error;

    async fn transmit(&mut self, payload: &[u8]) -> Result<usize, Self::Error> {
        let mut tx_pkt_params = self.lora.create_tx_packet_params(
            self.preamble_len,
            false, // explicit header
            true,  // CRC on
            false, // IQ not inverted
            &self.mod_params,
        )?;

        self.lora
            .prepare_for_tx(&self.mod_params, &mut tx_pkt_params, self.tx_power, payload)
            .await?;

        self.lora.tx().await?;
        Ok(payload.len())
    }

    async fn receive(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.lora
            .prepare_for_rx(RxMode::Continuous, &self.mod_params, &self.rx_pkt_params)
            .await?;

        let (len, _status) = self.lora.rx(&self.rx_pkt_params, buf).await?;
        Ok(len as usize)
    }
}
