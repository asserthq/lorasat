use sat_core::message::GroundCommand;

pub fn parse(cmd: &[u8]) -> Option<GroundCommand> {
    if cmd == b"ping" {
        Some(GroundCommand::Ping)
    } else if cmd == b"telemetry" {
        Some(GroundCommand::RequestSatTelemetry)
    } else if cmd == b"client" {
        Some(GroundCommand::RequestClientData)
    } else if cmd.starts_with(b"beacon ") {
        Some(GroundCommand::ChangeBeaconInterval(parse_u32(&cmd[7..])?))
    } else if cmd.starts_with(b"time ") {
        Some(GroundCommand::SetTime(parse_u64(&cmd[5..])?))
    } else {
        None
    }
}

fn parse_u32(s: &[u8]) -> Option<u32> {
    core::str::from_utf8(s).ok()?.parse().ok()
}

fn parse_u64(s: &[u8]) -> Option<u64> {
    core::str::from_utf8(s).ok()?.parse().ok()
}
