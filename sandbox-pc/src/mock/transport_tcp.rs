use std::io;
use std::net::{Ipv4Addr, SocketAddr};

use sat_core::comm::{Address, TransportEvent, TransportLayer, TransportSession};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};

/// Конвенция Address -> 127.0.0.1:(PORT_BASE + addr).
/// Не поднимать оба мока на одном адресе в одном процессе: UDP-порт общий.
const PORT_BASE: u16 = 9000;

fn socket_addr_of(addr: Address) -> SocketAddr {
    SocketAddr::new(Ipv4Addr::LOCALHOST.into(), PORT_BASE + addr.0 as u16)
}

fn address_of(sock: SocketAddr) -> Address {
    Address(u32::from(sock.port() - PORT_BASE))
}

/// Tag-байты для различения датаграмм и маяков в UDP.
const TAG_DATAGRAM: u8 = 0x00;
const TAG_BEACON: u8 = 0x01;

/// Максимальный размер UDP-пакета (заголовок + payload).
const MAX_UDP: usize = 512;

/// Mock транспортного уровня поверх реальных TCP/UDP сокетов (PC sandbox).
///
/// Датаграммы — UDP с tag `0x00`.
/// Маяки — UDP с tag `0x01`, отправляются всем адресам из `beacon_targets`.
/// Сессии — TCP с length-prefix (u32 BE) фреймингом.
#[derive(Debug)]
pub struct TransportMockTCP {
    listener: TcpListener,
    udp: UdpSocket,
    beacon_targets: Vec<SocketAddr>,
}

impl TransportMockTCP {
    pub async fn new(local: Address, beacon_targets: &[Address]) -> io::Result<Self> {
        let local_sock = socket_addr_of(local);
        let listener = TcpListener::bind(local_sock).await?;
        let udp = UdpSocket::bind(local_sock).await?;
        let targets = beacon_targets.iter().map(|a| socket_addr_of(*a)).collect();
        Ok(Self {
            listener,
            udp,
            beacon_targets: targets,
        })
    }
}

impl TransportLayer for TransportMockTCP {
    type Error = io::Error;
    type Session = TcpSession;

    async fn send_beacon(&mut self, beacon: &[u8]) -> io::Result<()> {
        if self.beacon_targets.is_empty() {
            return Ok(());
        }
        let mut buf = [0u8; MAX_UDP];
        buf[0] = TAG_BEACON;
        let len = beacon.len().min(MAX_UDP - 1);
        buf[1..1 + len].copy_from_slice(&beacon[..len]);
        let total = 1 + len;
        for dst in &self.beacon_targets {
            self.udp.send_to(&buf[..total], *dst).await?;
        }
        Ok(())
    }

    async fn send_datagram(&mut self, dst: Address, data: &[u8]) -> io::Result<()> {
        let mut buf = [0u8; MAX_UDP];
        buf[0] = TAG_DATAGRAM;
        let len = data.len().min(MAX_UDP - 1);
        buf[1..1 + len].copy_from_slice(&data[..len]);
        self.udp
            .send_to(&buf[..1 + len], socket_addr_of(dst))
            .await?;
        Ok(())
    }

    async fn connect<'a>(&mut self, dst: Address) -> io::Result<TcpSession> {
        let stream = TcpStream::connect(socket_addr_of(dst)).await?;
        Ok(TcpSession { stream })
    }

    async fn next<'a>(&mut self, buf: &'a mut [u8]) -> io::Result<TransportEvent<'a, TcpSession>> {
        tokio::select! {
            accepted = self.listener.accept() => {
                let (stream, _) = accepted?;
                Ok(TransportEvent::Session(TcpSession { stream }))
            }
            recv = self.udp.recv_from(buf) => {
                let (n, from) = recv?;
                if n < 1 {
                    return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "empty udp packet"));
                }
                match buf[0] {
                    TAG_DATAGRAM => Ok(TransportEvent::Datagram {
                        from: address_of(from),
                        data: &buf[1..n],
                    }),
                    TAG_BEACON => Ok(TransportEvent::Beacon {
                        payload: &buf[1..n],
                    }),
                    _ => Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "unknown udp tag byte",
                    )),
                }
            }
        }
    }
}

/// TCP-сессия с length-prefix фреймингом: u32 BE длина + payload.
#[derive(Debug)]
pub struct TcpSession {
    stream: TcpStream,
}

impl TransportSession for TcpSession {
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
        let mut sat = TransportMockTCP::new(Address(2), &[]).await.unwrap();
        let mut gs = TransportMockTCP::new(Address(1), &[]).await.unwrap();

        sat.send_datagram(Address(1), b"ping").await.unwrap();

        let mut buf = [0u8; 512];
        match gs.next(&mut buf).await.unwrap() {
            TransportEvent::Datagram { from, data } => {
                assert_eq!(from, Address(2));
                assert_eq!(data, b"ping");
            }
            _ => panic!("expected Datagram"),
        }
    }

    #[tokio::test]
    async fn beacon_broadcast() {
        let mut sat = TransportMockTCP::new(Address(101), &[Address(102), Address(103)])
            .await
            .unwrap();
        let mut gs1 = TransportMockTCP::new(Address(102), &[]).await.unwrap();
        let mut gs2 = TransportMockTCP::new(Address(103), &[]).await.unwrap();

        sat.send_beacon(b"hello").await.unwrap();

        let mut buf = [0u8; 512];
        match gs1.next(&mut buf).await.unwrap() {
            TransportEvent::Beacon { payload } => {
                assert_eq!(payload, b"hello");
            }
            _ => panic!("expected Beacon"),
        }
        match gs2.next(&mut buf).await.unwrap() {
            TransportEvent::Beacon { payload } => {
                assert_eq!(payload, b"hello");
            }
            _ => panic!("expected Beacon"),
        }
    }

    #[tokio::test]
    async fn beacon_only_sent_to_targets() {
        // GS не в beacon_targets — не получает маяк.
        let mut sat = TransportMockTCP::new(Address(30), &[Address(22)])
            .await
            .unwrap();
        let mut gs = TransportMockTCP::new(Address(21), &[]).await.unwrap();

        sat.send_beacon(b"secret").await.unwrap();

        // GS не должен получить — используем timeout
        let mut buf = [0u8; 512];
        let result =
            tokio::time::timeout(std::time::Duration::from_millis(200), gs.next(&mut buf)).await;
        assert!(result.is_err(), "gs should NOT receive beacon");
    }

    #[tokio::test]
    async fn session_roundtrip() {
        let mut server = TransportMockTCP::new(Address(201), &[]).await.unwrap();
        let mut client = TransportMockTCP::new(Address(202), &[]).await.unwrap();

        let (client_session, mut server_session) =
            tokio::join!(client.connect(Address(201)), async {
                let mut ignore = [0u8; 1];
                match server.next(&mut ignore).await {
                    Ok(TransportEvent::Session(s)) => s,
                    _ => panic!("expected Session"),
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
