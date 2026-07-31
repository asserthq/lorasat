//use sat_core::error::Error;

#[derive(Debug, Clone)]
pub struct Telemetry {
    pub temp: f32,
    pub bat_voltage: f32,
    pub rssi: i32,
}

impl Telemetry {
    const ENCODED_SIZE: usize = 12;

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(12);
        buf.extend_from_slice(&self.temp.to_le_bytes());
        buf.extend_from_slice(&self.bat_voltage.to_le_bytes());
        buf.extend_from_slice(&self.rssi.to_le_bytes());
        buf
    }

    // pub fn try_from_bytes(data: &[u8]) -> Result<Self, Error> {
    //     if data.len() < Self::ENCODED_SIZE {
    //         return Err(Error("telemetry too short"));
    //     }
    //     Ok(Self {
    //         temp: f32::from_le_bytes(data[0..4].try_into().unwrap()),
    //         bat_voltage: f32::from_le_bytes(data[4..8].try_into().unwrap()),
    //         rssi: i32::from_le_bytes(data[8..12].try_into().unwrap()),
    //     })
    // }

    pub fn serialize_batch(tms: &[Self]) -> Vec<u8> {
        let mut buf = Vec::with_capacity(tms.len() * Self::ENCODED_SIZE);
        for tm in tms {
            buf.extend_from_slice(&tm.to_bytes());
        }
        buf
    }

    // pub fn deserialize_batch(data: &[u8]) -> Result<Vec<Self>, Error> {
    //     if data.len() % Self::ENCODED_SIZE != 0 {
    //         return Err(Error("invalid batch length"));
    //     }
    //     let count = data.len() / Self::ENCODED_SIZE;
    //     let mut tms = Vec::with_capacity(count);
    //     for i in 0..count {
    //         let off = i * Self::ENCODED_SIZE;
    //         tms.push(Self::try_from_bytes(&data[off..off + Self::ENCODED_SIZE])?);
    //     }
    //     Ok(tms)
    // }
}
