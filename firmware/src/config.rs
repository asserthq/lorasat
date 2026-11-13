use sat_core::net::Address;

pub mod network {
    use super::Address;

    pub const MTU: usize = 255;
    pub const TX_QUEUE_DEPTH: usize = 4;
    pub const SOCKET_COUNT: usize = 1;
    pub const RX_QUEUE_DEPTH: usize = 4;

    pub const SATELLITE_ADDRESS: Address = Address::new_unicast(1).unwrap();
    pub const BEACON_PORT: u8 = 10;
    pub const COMMAND_PORT: u8 = 20;
}

pub mod radio {
    pub const SPI_FREQUENCY_KHZ: u32 = 200;

    #[derive(Clone, Copy)]
    pub struct Config {
        pub frequency_hz: u32,
    }

    pub const PLATFORM: Config = Config {
        frequency_hz: 433_000_000,
    };

    pub const SERVICE: Config = Config {
        frequency_hz: 868_000_000,
    };
}

pub mod flash {
    pub const SPI_FREQUENCY_KHZ: u32 = 200;
}

pub mod cdh {
    pub const BEACON_INTERVAL_SECS: u16 = 10;
}

pub mod storage {
    pub const ENTRY_SIZE: usize = 300;
    pub const DEPTH: usize = 16;
}
