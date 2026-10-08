use embassy_sync::channel::{Channel, Receiver, Sender};
use embassy_time::Instant;
use heapless::Vec;
use sat_core::comm::address::Address;
use sat_core::comm::link::{LinkLayer, RecvFrame};
use sat_core::comm::transport::{Socket, TransportEvent, TransportLayer};

use crate::proto_impl::transport::runner::TransportRunner;
use crate::proto_impl::transport::stack::TransportStack;

use super::pdu::{MAX_CHUNKS, MTU, Packet};

use embassy_sync::blocking_mutex::raw::{CriticalSectionRawMutex, NoopRawMutex};
use embassy_sync::channel::{Channel, Receiver, Sender};
use embassy_sync::mutex::Mutex;
use heapless::{LinearMap, Vec};

pub fn init<L: LinkLayer>(link: L) -> (TransportRunner<L>, TransportStack) {
    let sockets_map = Mutex::new(LinearMap::new());
    let tx_channel = Channel::new();
    let (tx_sender, tx_receiver) = tx_channel.split();
    let runner = TransportRunner::new(link, sockets_map, tx_receiver);
    let stack = TransportStack::new(sockets_map, tx_sender);
    (runner, stack)
}
