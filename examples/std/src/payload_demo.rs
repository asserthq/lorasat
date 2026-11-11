use std::{io, io::Write, net::SocketAddr};

use embassy_sync::channel::{DynamicReceiver, DynamicSender};
use sat_core::net::{Address, DatagramSocket, Endpoint};
use sat_subsystems::payload::{PayloadCommand, PayloadEvent};
use tokio::time::{Duration, timeout};

use crate::csp_demo::MTU;

pub const TX_ADDRESS: SocketAddr =
    SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), 25_051);
pub const RX_ADDRESS: SocketAddr =
    SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), 25_052);

pub const TX_NODE: u16 = 2;
pub const RX_NODE: u16 = 1;
pub const COMMAND_PORT: u8 = 20;
pub const TX_ENDPOINT: Endpoint =
    Endpoint::new(Address::new_unicast(TX_NODE).unwrap(), COMMAND_PORT);
pub const RX_ENDPOINT: Endpoint =
    Endpoint::new(Address::new_unicast(RX_NODE).unwrap(), COMMAND_PORT);
pub const SEND_INTERVAL: Duration = Duration::from_secs(1);
pub const RESPONSE_TIMEOUT: Duration = Duration::from_secs(2);
pub const MESSAGE_SIZE: usize = 6;

const VERSION: u8 = 1;
const RECEIVE_BUFFER_SIZE: usize = MTU - sat_csp::HEADER_SIZE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message {
    Ping { request_id: u32 },
    Executed { request_id: u32 },
}

impl Message {
    pub fn encode(self) -> [u8; MESSAGE_SIZE] {
        let (kind, request_id) = match self {
            Self::Ping { request_id } => (1, request_id),
            Self::Executed { request_id } => (2, request_id),
        };
        let mut bytes = [0; MESSAGE_SIZE];
        bytes[0] = VERSION;
        bytes[1] = kind;
        bytes[2..].copy_from_slice(&request_id.to_be_bytes());
        bytes
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() != MESSAGE_SIZE {
            return Err("invalid payload demo message length");
        }
        if bytes[0] != VERSION {
            return Err("unsupported payload demo version");
        }
        let request_id = u32::from_be_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]);
        match bytes[1] {
            1 => Ok(Self::Ping { request_id }),
            2 => Ok(Self::Executed { request_id }),
            _ => Err("unknown payload demo message type"),
        }
    }
}

pub async fn send_ping(
    socket: &mut impl DatagramSocket,
    destination: Endpoint,
    request_id: u32,
) -> io::Result<()> {
    timeout(RESPONSE_TIMEOUT, async {
        socket
            .send_to(destination, &Message::Ping { request_id }.encode())
            .await
            .map_err(|error| io::Error::other(format!("CSP send failed: {error:?}")))?;
        let mut buffer = [0; RECEIVE_BUFFER_SIZE];
        loop {
            let metadata = socket
                .recv_from(&mut buffer)
                .await
                .map_err(|error| io::Error::other(format!("CSP receive failed: {error:?}")))?;
            if metadata.source() == destination
                && Message::decode(&buffer[..metadata.length()])
                    == Ok(Message::Executed { request_id })
            {
                return Ok(());
            }
        }
    })
    .await
    .map_err(|_| {
        io::Error::new(
            io::ErrorKind::TimedOut,
            format!("no execution confirmation for request #{request_id}"),
        )
    })?
}

pub async fn serve_commands(
    socket: &mut impl DatagramSocket,
    commands: DynamicSender<'_, PayloadCommand>,
    events: DynamicReceiver<'_, PayloadEvent>,
) -> io::Result<()> {
    let mut buffer = [0; RECEIVE_BUFFER_SIZE];
    loop {
        let metadata = socket
            .recv_from(&mut buffer)
            .await
            .map_err(|error| io::Error::other(format!("CSP receive failed: {error:?}")))?;
        if metadata.source() != TX_ENDPOINT {
            eprintln!(
                "rx: ignoring command from unexpected source {:?}",
                metadata.source()
            );
            continue;
        }
        let request_id = match Message::decode(&buffer[..metadata.length()]) {
            Ok(Message::Ping { request_id }) => request_id,
            Ok(Message::Executed { .. }) => continue,
            Err(error) => {
                eprintln!("rx: ignoring invalid command: {error}");
                continue;
            }
        };
        println!("rx #{request_id}: received Ping; passing to PayloadSystem");
        io::stdout().flush()?;
        commands.send(PayloadCommand::Ping).await;
        loop {
            match events.receive().await {
                PayloadEvent::Pong => break,
                event => eprintln!("rx: payload event while waiting for Pong: {event:?}"),
            }
        }
        socket
            .send_to(
                metadata.source(),
                &Message::Executed { request_id }.encode(),
            )
            .await
            .map_err(|error| io::Error::other(format!("CSP reply failed: {error:?}")))?;
        println!("rx #{request_id}: Ping executed (Pong); confirmation queued");
        io::stdout().flush()?;
    }
}
