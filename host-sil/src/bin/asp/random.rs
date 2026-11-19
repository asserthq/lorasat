use std::{env, time::SystemTime};

use host_sil::config::CLIENT_ADDR;

pub struct Random {
    state: u32,
}

impl Random {
    pub fn from_host() -> Result<Self, String> {
        let seed = match env::var("ASP_SEED") {
            Ok(value) => value
                .parse::<u32>()
                .map_err(|_| "ASP_SEED must be a decimal u32".to_owned())?,
            Err(env::VarError::NotPresent) => {
                let ticks = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos();
                ticks as u32
                    ^ (ticks >> 32) as u32
                    ^ std::process::id().rotate_left(11)
                    ^ u32::from(CLIENT_ADDR.get()).wrapping_mul(0x9e37_79b9)
            }
            Err(error) => return Err(format!("invalid ASP_SEED: {error}")),
        };
        let seed = if seed == 0 { 1 } else { seed };
        println!("asp: ALOHA RNG seed={seed}; not cryptographic");
        Ok(Self { state: seed })
    }

    pub fn next_u32(&mut self) -> u32 {
        let mut value = self.state;
        value ^= value << 13;
        value ^= value >> 17;
        value ^= value << 5;
        self.state = value;
        value
    }
}
