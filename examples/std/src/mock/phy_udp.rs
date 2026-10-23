use std::io;
use std::net::{Ipv4Addr, SocketAddr};

use sat_core::comm::address::Address;
use sat_core::comm::phy::PhyLayer;
use tokio::net::UdpSocket;

/// Конвенция Address → 127.0.0.1:(PORT_BASE + addr).
/// Каждый узел занимает свой UDP-порт.
const PORT_BASE: u16 = 9000;

fn socket_addr_of(addr: Address) -> SocketAddr {
    SocketAddr::new(Ipv4Addr::LOCALHOST.into(), PORT_BASE + addr.0 as u16)
}

/// Заглушка PhyLayer на UDP, эмулирующая радиоэфир.
///
/// `send_bytes` отправляет данные всем узлам из списка `peers`
/// (точка-многоточка, как в радиоэфире).
/// `recv_bytes` принимает данные от любого отправителя.
///
/// Каждый экземпляр должен иметь уникальный `local` адрес,
/// иначе порты пересекутся.
#[derive(Debug)]
pub struct PhyRadioUDP {
    socket: UdpSocket,
    peers: Vec<SocketAddr>,
}

impl PhyRadioUDP {
    /// Создать радио-узел.
    ///
    /// `local` — собственный адрес (уникальный среди всех узлов).
    /// `peers` — адреса всех остальных узлов в зоне радиовидимости.
    pub async fn new(local: Address, peers: &[Address]) -> io::Result<Self> {
        let local_sock = socket_addr_of(local);
        let socket = UdpSocket::bind(local_sock).await?;
        let peers = peers.iter().map(|a| socket_addr_of(*a)).collect();
        Ok(Self { socket, peers })
    }
}

impl PhyLayer for PhyRadioUDP {
    type Error = io::Error;

    /// Отправить пакет всем узлам в зоне (как радио).
    async fn send_bytes(&mut self, payload: &[u8]) -> Result<(), Self::Error> {
        for peer in &self.peers {
            self.socket.send_to(payload, *peer).await?;
        }
        Ok(())
    }

    /// Принять пакет от любого отправителя.
    ///
    /// Возвращает количество прочитанных байт.
    /// Адрес отправителя не возвращается — фильтрация
    /// на канальном уровне (LinkLayer).
    async fn recv_bytes(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let (len, _from) = self.socket.recv_from(buf).await?;
        Ok(len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn broadcast_to_all_peers() {
        let mut a = PhyRadioUDP::new(Address(1), &[Address(2), Address(3)])
            .await
            .unwrap();
        let mut b = PhyRadioUDP::new(Address(2), &[Address(1)]).await.unwrap();
        let mut c = PhyRadioUDP::new(Address(3), &[Address(1)]).await.unwrap();

        a.send_bytes(b"hello radio").await.unwrap();

        let mut buf = [0u8; 64];
        let n = b.recv_bytes(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"hello radio");

        let n = c.recv_bytes(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"hello radio");
    }

    #[tokio::test]
    async fn does_not_receive_own_send() {
        let mut a = PhyRadioUDP::new(Address(10), &[Address(20)]).await.unwrap();
        let mut b = PhyRadioUDP::new(Address(20), &[Address(10)]).await.unwrap();

        a.send_bytes(b"ping").await.unwrap();

        // a не должен получить свой же пакет (peers не включает self)
        let mut buf = [0u8; 64];
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            a.recv_bytes(&mut buf),
        )
        .await;
        assert!(result.is_err(), "should not receive own transmission");

        // b получает
        let n = b.recv_bytes(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"ping");
    }

    #[tokio::test]
    async fn empty_peers_send_is_noop() {
        let mut a = PhyRadioUDP::new(Address(50), &[]).await.unwrap();
        // Отправка без пиров — не падает, просто никому не шлёт
        a.send_bytes(b"nobody").await.unwrap();
    }
}
