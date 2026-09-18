use std::io;
use std::net::{Ipv4Addr, SocketAddr};

use sat_core::comm::address::Address;
use sat_core::comm::transport::{Event, Session, TransportLayer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};

/// Конвенция Address -> 127.0.0.1:(PORT_BASE + addr), как в PhyMockUDP.
/// Не поднимать оба мока на одном адресе в одном процессе: UDP-порт общий.
const PORT_BASE: u16 = 9000;

fn socket_addr_of(addr: Address) -> SocketAddr {
    SocketAddr::new(Ipv4Addr::LOCALHOST.into(), PORT_BASE + addr.0 as u16)
}

fn address_of(sock: SocketAddr) -> Address {
    Address(u32::from(sock.port() - PORT_BASE))
}

/// Mock транспортного уровня поверх реальных TCP/UDP сокетов (PC sandbox).
///
/// Датаграммы — UDP, сессии — TCP с length-prefix (u32 BE) фреймингом.
#[derive(Debug)]
pub struct TransportMockTCP {
    listener: TcpListener,
    udp: UdpSocket,
}

impl TransportMockTCP {
    pub async fn new(local: SocketAddr) -> io::Result<Self> {
        let listener = TcpListener::bind(local).await?;
        let udp = UdpSocket::bind(local).await?;
        Ok(Self { listener, udp })
    }

    pub async fn from_addr(local: Address) -> io::Result<Self> {
        Self::new(socket_addr_of(local)).await
    }
}

impl TransportLayer for TransportMockTCP {
    type Error = io::Error;
    type Session = TcpSession;

    async fn send_datagram(&mut self, dst: Address, data: &[u8]) -> io::Result<()> {
        self.udp.send_to(data, socket_addr_of(dst)).await?;
        Ok(())
    }

    async fn connect<'a>(&mut self, dst: Address) -> io::Result<TcpSession> {
        let stream = TcpStream::connect(socket_addr_of(dst)).await?;
        Ok(TcpSession { stream })
    }

    async fn next<'a>(&mut self, buf: &'a mut [u8]) -> io::Result<Event<'a, TcpSession>> {
        tokio::select! {
            accepted = self.listener.accept() => {
                let (stream, _) = accepted?;
                Ok(Event::Session(TcpSession { stream }))
            }
            recv = self.udp.recv_from(buf) => {
                let (n, from) = recv?;
                Ok(Event::Datagram { from: address_of(from), data: &buf[..n] })
            }
        }
    }
}

/// TCP-сессия с length-prefix фреймингом: u32 BE длина + payload.
#[derive(Debug)]
pub struct TcpSession {
    stream: TcpStream,
}

impl Session for TcpSession {
    type Error = io::Error;

    async fn send(&mut self, data: &[u8]) -> io::Result<()> {
        self.stream
            .write_all(&(data.len() as u32).to_be_bytes())
            .await?;
        self.stream.write_all(data).await
    }

    async fn recv<'a>(&'a mut self, buf: &'a mut [u8]) -> io::Result<&'a [u8]> {
        let mut len = [0u8; 4];
        self.stream.read_exact(&mut len).await?;
        let n = u32::from_be_bytes(len) as usize;
        if n > buf.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "tcp session message larger than buffer",
            ));
        }
        self.stream.read_exact(&mut buf[..n]).await?;
        Ok(&buf[..n])
    }

    fn peer_addr(&self) -> Address {
        address_of(self.stream.peer_addr().unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn datagram_roundtrip() {
        let mut sat = TransportMockTCP::from_addr(Address(2)).await.unwrap();
        let mut gs = TransportMockTCP::from_addr(Address(1)).await.unwrap();

        sat.send_datagram(Address(1), b"ping").await.unwrap();

        let mut buf = [0u8; 256];
        match gs.next(&mut buf).await.unwrap() {
            Event::Datagram { from, data } => {
                assert_eq!(from, Address(2));
                assert_eq!(data, b"ping");
            }
            Event::Session(_) => panic!("expected datagram"),
        }
    }

    #[tokio::test]
    async fn session_roundtrip() {
        let mut server = TransportMockTCP::from_addr(Address(10)).await.unwrap();
        let mut client = TransportMockTCP::from_addr(Address(11)).await.unwrap();

        let (client_session, mut server_session) =
            tokio::join!(client.connect(Address(10)), async {
                let mut ignore = [0u8; 1];
                match server.next(&mut ignore).await {
                    Ok(Event::Session(s)) => s,
                    _ => panic!("expected session"),
                }
            });
        let mut client_session = client_session.unwrap();

        client_session.send(b"hello").await.unwrap();
        let mut buf = [0u8; 64];
        let data = server_session.recv(&mut buf).await.unwrap();
        assert_eq!(data, b"hello");

        server_session.send(b"world").await.unwrap();
        let data = client_session.recv(&mut buf).await.unwrap();
        assert_eq!(data, b"world");
    }
}
