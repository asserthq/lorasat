use embedded_hal::digital::OutputPin;
use embedded_hal_async::{digital::Wait, spi::SpiDevice};
use lora_phy::{
    DelayNs, LoRa,
    iv::GenericSx126xInterfaceVariant,
    mod_params::{Bandwidth, CodingRate, ModulationParams, PacketParams, RxMode, SpreadingFactor},
    sx126x::{self, Sx126x, Sx1262, TcxoCtrlVoltage},
};
use sat_csp::CspInterface;

pub const LORA_MAX_FRAME_SIZE: u8 = u8::MAX;

/// Board pins owned by one SX1262-class radio.
///
/// SX1268 uses the same command interface in `lora-phy` and is represented by
/// `Sx1262`, but its RF frequency range is different. Board wiring and RF
/// parameters still have to match the actual module.
pub struct Sx1262Control<ControlPin, WaitPin> {
    pub reset: ControlPin,
    pub dio1: WaitPin,
    pub busy: WaitPin,
    pub rf_switch_rx: Option<ControlPin>,
    pub rf_switch_tx: Option<ControlPin>,
}

/// Radio and packet parameters used by [`Sx1262CspInterface`].
///
/// Values returned by [`Self::new`] are bring-up defaults, not universal board
/// defaults. In particular, verify TCXO voltage, regulator mode, RF switch
/// wiring, frequency, and transmit power against the module schematic.
pub struct Sx1262CspConfig {
    pub frequency_hz: u32,
    pub max_frame_size: u8,
    pub spreading_factor: SpreadingFactor,
    pub bandwidth: Bandwidth,
    pub coding_rate: CodingRate,
    pub preamble_length: u16,
    pub tx_power_dbm: i32,
    pub enable_public_network: bool,
    pub tcxo_ctrl: Option<TcxoCtrlVoltage>,
    pub use_dcdc: bool,
    pub rx_boost: bool,
    pub invert_iq: bool,
}

impl Sx1262CspConfig {
    pub const fn new(frequency_hz: u32, max_frame_size: u8) -> Self {
        Self {
            frequency_hz,
            max_frame_size,
            spreading_factor: SpreadingFactor::_9,
            bandwidth: Bandwidth::_125KHz,
            coding_rate: CodingRate::_4_5,
            preamble_length: 8,
            tx_power_dbm: 2,
            enable_public_network: false,
            tcxo_ctrl: Some(TcxoCtrlVoltage::Ctrl1V8),
            use_dcdc: true,
            rx_boost: true,
            invert_iq: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, defmt::Format)]
pub enum Sx1262CspError {
    CreateInterfaceVariant,
    InitializeRadio,
    CreateModulationParameters,
    CreateRxPacketParameters,
    CreateTxPacketParameters,
    InvalidMaxFrameSize { length: usize, minimum: usize },
    FrameTooLong { length: usize, maximum: usize },
    ReceiveBufferTooSmall { length: usize, minimum: usize },
    PrepareForTransmit,
    Transmit,
    PrepareForReceive,
    Receive,
}

type LoraSx1262<Spi, ControlPin, WaitPin, Delay> =
    LoRa<Sx126x<Spi, GenericSx126xInterfaceVariant<ControlPin, WaitPin>, Sx1262>, Delay>;

/// Direct packet-oriented CSP interface over one `lora-phy` SX1262 radio.
///
/// Each LoRa packet contains exactly one complete CSP frame. This type performs
/// no CSP parsing, routing, retries, fragmentation, or MAC scheduling.
pub struct Sx1262CspInterface<Spi, ControlPin, WaitPin, Delay>
where
    Spi: SpiDevice<u8>,
    ControlPin: OutputPin,
    WaitPin: Wait,
    Delay: DelayNs,
{
    lora: LoraSx1262<Spi, ControlPin, WaitPin, Delay>,
    modulation: ModulationParams,
    rx_packet: PacketParams,
    tx_packet: PacketParams,
    max_frame_size: usize,
    tx_power_dbm: i32,
}

impl<Spi, ControlPin, WaitPin, Delay> Sx1262CspInterface<Spi, ControlPin, WaitPin, Delay>
where
    Spi: SpiDevice<u8>,
    ControlPin: OutputPin,
    WaitPin: Wait,
    Delay: DelayNs,
{
    pub async fn new(
        spi: Spi,
        control: Sx1262Control<ControlPin, WaitPin>,
        delay: Delay,
        config: Sx1262CspConfig,
    ) -> Result<Self, Sx1262CspError> {
        if (config.max_frame_size as usize) < sat_csp::HEADER_SIZE {
            return Err(Sx1262CspError::InvalidMaxFrameSize {
                length: config.max_frame_size as usize,
                minimum: sat_csp::HEADER_SIZE,
            });
        }

        let interface = GenericSx126xInterfaceVariant::new(
            control.reset,
            control.dio1,
            control.busy,
            control.rf_switch_rx,
            control.rf_switch_tx,
        )
        .map_err(|_| Sx1262CspError::CreateInterfaceVariant)?;

        let radio_config = sx126x::Config {
            chip: Sx1262,
            tcxo_ctrl: config.tcxo_ctrl,
            use_dcdc: config.use_dcdc,
            rx_boost: config.rx_boost,
        };
        let radio = Sx126x::new(spi, interface, radio_config);
        let mut lora = LoRa::new(radio, config.enable_public_network, delay)
            .await
            .map_err(|_| Sx1262CspError::InitializeRadio)?;

        let modulation = lora
            .create_modulation_params(
                config.spreading_factor,
                config.bandwidth,
                config.coding_rate,
                config.frequency_hz,
            )
            .map_err(|_| Sx1262CspError::CreateModulationParameters)?;

        let rx_packet = lora
            .create_rx_packet_params(
                config.preamble_length,
                false,
                config.max_frame_size,
                true,
                config.invert_iq,
                &modulation,
            )
            .map_err(|_| Sx1262CspError::CreateRxPacketParameters)?;

        let tx_packet = lora
            .create_tx_packet_params(
                config.preamble_length,
                false,
                true,
                config.invert_iq,
                &modulation,
            )
            .map_err(|_| Sx1262CspError::CreateTxPacketParameters)?;

        Ok(Self {
            lora,
            modulation,
            rx_packet,
            tx_packet,
            max_frame_size: config.max_frame_size as usize,
            tx_power_dbm: config.tx_power_dbm,
        })
    }

    pub const fn max_frame_size(&self) -> usize {
        self.max_frame_size
    }
}

impl<Spi, ControlPin, WaitPin, Delay> CspInterface
    for Sx1262CspInterface<Spi, ControlPin, WaitPin, Delay>
where
    Spi: SpiDevice<u8>,
    ControlPin: OutputPin,
    WaitPin: Wait,
    Delay: DelayNs,
{
    type Error = Sx1262CspError;

    async fn send(&mut self, frame: &[u8]) -> Result<(), Self::Error> {
        if frame.len() > self.max_frame_size {
            return Err(Sx1262CspError::FrameTooLong {
                length: frame.len(),
                maximum: self.max_frame_size,
            });
        }

        self.lora
            .prepare_for_tx(
                &self.modulation,
                &mut self.tx_packet,
                self.tx_power_dbm,
                frame,
            )
            .await
            .map_err(|_| Sx1262CspError::PrepareForTransmit)?;

        self.lora.tx().await.map_err(|_| Sx1262CspError::Transmit)
    }

    async fn receive(&mut self, frame: &mut [u8]) -> Result<usize, Self::Error> {
        if frame.len() < self.max_frame_size {
            return Err(Sx1262CspError::ReceiveBufferTooSmall {
                length: frame.len(),
                minimum: self.max_frame_size,
            });
        }

        self.lora
            .prepare_for_rx(RxMode::Continuous, &self.modulation, &self.rx_packet)
            .await
            .map_err(|_| Sx1262CspError::PrepareForReceive)?;

        let (length, _packet_status) = self
            .lora
            .rx(&self.rx_packet, &mut frame[..self.max_frame_size])
            .await
            .map_err(|_| Sx1262CspError::Receive)?;

        Ok(length as usize)
    }
}
