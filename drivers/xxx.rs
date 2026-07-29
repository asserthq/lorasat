use core::error::Error;

use sat_core::radio::transciever::{AsyncTransceiver, Transceiver};

use sx126x::SX126x;

use embassy_time::Timer;
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal::spi::SpiDevice;

// ── Delay wrapper (orphan rule: both Delay and DelayNs are foreign) ──

pub struct EmbassyDelay;

impl DelayNs for EmbassyDelay {
    async fn delay_ns(&mut self, ns: u32) {
        Timer::after_nanos(ns as u64).await;
    }
}

// ── Transceiver struct ──

/// SX1262-based LoRa transceiver driver.
///
/// Wraps `lora-phy` async API. `send()`/`receive()` stubbed —
/// only [`AsyncTransceiver`] methods do real work.
///
/// Default parameters: SF7, 125 kHz BW, 4/5 CR, 868.1 MHz,
/// preamble 8, explicit header, CRC on, TX power 14 dBm.
pub struct LoRa1262Transceiver<TSPI: SpiDevice, TNRST, TBUSY, TANT, TDIO1> {
    lora: SX126x<TSPI, TNRST, TBUSY, TANT, TDIO1>,
}

impl<TSPI, TNRST, TBUSY, TANT, TDIO1> LoRa1262Transceiver<TSPI, TNRST, TBUSY, TANT, TDIO1>
where
    TSPI: SpiDevice,
    TNRST: OutputPin,
    TBUSY: InputPin,
    TANT: OutputPin,
    TDIO1: InputPin,
{
    pub async fn new(
        spi: TSPI,
        nrst: TNRST,
        busy: TBUSY,
        ant: TANT,
        dio1: TDIO1,
    ) -> Result<Self, Error> {
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

// ── Sync trait (stubbed — LoRa is async-only) ──

impl<SPI, IV> Transceiver for LoRa1262Transceiver<SPI, IV>
where
    SPI: SpiDevice<u8>,
    IV: InterfaceVariant,
{
    type Error = Error;

    fn send(&mut self, _data: &[u8]) -> Result<usize, Self::Error> {
        Err(Error::SyncSendNotSupported)
    }

    fn receive(&mut self, _buf: &mut [u8]) -> Result<Option<usize>, Self::Error> {
        Ok(None)
    }
}

// ── Async trait ──

impl<SPI, IV> AsyncTransceiver for LoRa1262Transceiver<SPI, IV>
where
    SPI: SpiDevice<u8>,
    IV: InterfaceVariant,
{
    async fn receive_async(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.lora
            .prepare_for_rx(RxMode::Continuous, &self.mod_params, &self.rx_pkt_params)
            .await?;

        let (len, _status) = self.lora.rx(&self.rx_pkt_params, buf).await?;
        Ok(len as usize)
    }

    async fn send_async(&mut self, data: &[u8]) -> Result<usize, Self::Error> {
        let mut tx_pkt_params = self.lora.create_tx_packet_params(
            self.preamble_len,
            false, // explicit header
            true,  // CRC on
            false, // IQ not inverted
            &self.mod_params,
        )?;

        self.lora
            .prepare_for_tx(&self.mod_params, &mut tx_pkt_params, self.tx_power, data)
            .await?;

        self.lora.tx().await?;
        Ok(data.len())
    }
}
