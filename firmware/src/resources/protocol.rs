use sat_csp::{RdpResources, StackResources};

use crate::config::network::{MTU, RX_QUEUE_DEPTH, SOCKET_COUNT, TX_QUEUE_DEPTH};

pub(crate) type CspStackResources =
    StackResources<MTU, TX_QUEUE_DEPTH, SOCKET_COUNT, RX_QUEUE_DEPTH>;
pub(crate) type CspRdpResources = RdpResources<MTU>;

pub(crate) struct ProtocolStackResources {
    pub stack: CspStackResources,
    pub rdp: CspRdpResources,
}

impl ProtocolStackResources {
    pub const fn new() -> Self {
        Self {
            stack: CspStackResources::new(),
            rdp: CspRdpResources::new(),
        }
    }
}
