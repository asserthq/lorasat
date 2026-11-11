use std::{io, net::SocketAddr};

use sat_core::net::{Address, Endpoint};

use super::MockNetError;

pub(super) const ENCODED_ENDPOINT_SIZE: usize = 3;

const PORT_BASE: u16 = 25_000;

pub(super) fn socket_address(endpoint: Endpoint) -> Result<SocketAddr, MockNetError> {
    let address = u32::from(endpoint.addr().get());
    if endpoint.addr().is_broadcast() {
        return Err(MockNetError::InvalidEndpoint(endpoint));
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

pub(super) fn endpoint_from_socket(address: SocketAddr) -> Result<Endpoint, MockNetError> {
    let SocketAddr::V4(address) = address else {
        return Err(invalid_socket_address(
            "mock network received an IPv6 address",
        ));
    };
    let octets = address.ip().octets();
    if octets[0] != 127 || address.port() < PORT_BASE || address.port() > PORT_BASE + 255 {
        return Err(invalid_socket_address(
            "socket address is outside mock network address space",
        ));
    }

    let node = (u32::from(octets[1]) << 16) | (u32::from(octets[2]) << 8) | u32::from(octets[3]);
    let node = u16::try_from(node)
        .ok()
        .and_then(Address::new_unicast)
        .ok_or_else(|| invalid_socket_address("invalid mock network node address"))?;
    Ok(Endpoint::new(node, (address.port() - PORT_BASE) as u8))
}

pub(super) fn encode_endpoint(endpoint: Endpoint) -> [u8; ENCODED_ENDPOINT_SIZE] {
    let address = endpoint.addr().get().to_be_bytes();
    [address[0], address[1], endpoint.port()]
}

pub(super) fn decode_endpoint(
    encoded: [u8; ENCODED_ENDPOINT_SIZE],
) -> Result<Endpoint, MockNetError> {
    let raw = u16::from_be_bytes([encoded[0], encoded[1]]);
    let address = Address::new_unicast(raw)
        .ok_or_else(|| invalid_socket_address("invalid mock network node address"))?;
    Ok(Endpoint::new(address, encoded[2]))
}

fn invalid_socket_address(message: &'static str) -> MockNetError {
    MockNetError::Io(io::Error::new(io::ErrorKind::InvalidData, message))
}
