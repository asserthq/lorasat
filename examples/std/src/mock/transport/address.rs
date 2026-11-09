use std::{io, net::SocketAddr};

use sat_core::comm::{Address, Endpoint, address::BROADCAST_ADDRESS};

use super::MockCspError;

const PORT_BASE: u16 = 25_000;
const MAX_ADDRESS: u32 = 0x00FF_FFFF;

pub(super) fn socket_address(endpoint: Endpoint) -> Result<SocketAddr, MockCspError> {
    let address = endpoint.addr().0;
    if endpoint.addr() == BROADCAST_ADDRESS || address > MAX_ADDRESS {
        return Err(MockCspError::InvalidEndpoint(endpoint));
    }

    Ok(SocketAddr::from((
        [
            127,
            ((address >> 16) & 0xFF) as u8,
            ((address >> 8) & 0xFF) as u8,
            (address & 0xFF) as u8,
        ],
        PORT_BASE + u16::from(endpoint.port()),
    )))
}

pub(super) fn endpoint_from_socket(address: SocketAddr) -> Result<Endpoint, MockCspError> {
    let SocketAddr::V4(address) = address else {
        return Err(invalid_socket_address("mock CSP received an IPv6 address"));
    };
    let octets = address.ip().octets();
    if octets[0] != 127 || address.port() < PORT_BASE || address.port() > PORT_BASE + 255 {
        return Err(invalid_socket_address(
            "socket address is outside mock CSP address space",
        ));
    }

    let node = (u32::from(octets[1]) << 16) | (u32::from(octets[2]) << 8) | u32::from(octets[3]);
    Ok(Endpoint::new(
        Address(node),
        (address.port() - PORT_BASE) as u8,
    ))
}

pub(super) fn encode_endpoint(endpoint: Endpoint) -> [u8; 5] {
    let mut encoded = [0_u8; 5];
    encoded[..4].copy_from_slice(&endpoint.addr().0.to_be_bytes());
    encoded[4] = endpoint.port();
    encoded
}

pub(super) fn decode_endpoint(encoded: [u8; 5]) -> Endpoint {
    Endpoint::new(
        Address(u32::from_be_bytes([
            encoded[0], encoded[1], encoded[2], encoded[3],
        ])),
        encoded[4],
    )
}

fn invalid_socket_address(message: &'static str) -> MockCspError {
    MockCspError::Io(io::Error::new(io::ErrorKind::InvalidData, message))
}
