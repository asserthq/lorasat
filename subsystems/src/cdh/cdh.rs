use sat_core::{net::DatagramSocket, storage::QueueStorage};

pub struct CdhSystem<BeaconS, PlatformQ, IotQ>
where
    BeaconS: DatagramSocket,
    PlatformQ: QueueStorage,
    IotQ: QueueStorage,
{
    beacon_socket: BeaconS,
    platform_storage: PlatformQ,
    iot_storage: IotQ,
}

impl<BeaconS, PlatformQ, IotQ> CdhSystem<BeaconS, PlatformQ, IotQ>
where
    BeaconS: DatagramSocket,
    PlatformQ: QueueStorage,
    IotQ: QueueStorage,
{
    pub fn new(beacon_socket: BeaconS, platform_storage: PlatformQ, iot_storage: IotQ) -> Self {
        Self {
            beacon_socket,
            platform_storage,
            iot_storage,
        }
    }
}
