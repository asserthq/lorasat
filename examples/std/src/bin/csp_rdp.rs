use sat_csp::{CspInterface, RdpConfig, RdpResources, RunnerBuilder, StackResources};
use tokio::{sync::mpsc, time::Duration};

#[derive(Debug)]
enum MemoryLinkError {
    Disconnected,
    FrameTooLarge,
}

struct MemoryLink {
    outgoing: mpsc::Sender<Vec<u8>>,
    incoming: mpsc::Receiver<Vec<u8>>,
}

impl MemoryLink {
    fn pair(capacity: usize) -> (Self, Self) {
        let (a_to_b_tx, a_to_b_rx) = mpsc::channel(capacity);
        let (b_to_a_tx, b_to_a_rx) = mpsc::channel(capacity);
        (
            Self {
                outgoing: a_to_b_tx,
                incoming: b_to_a_rx,
            },
            Self {
                outgoing: b_to_a_tx,
                incoming: a_to_b_rx,
            },
        )
    }
}

impl CspInterface for MemoryLink {
    type Error = MemoryLinkError;

    async fn send(&mut self, frame: &[u8]) -> Result<(), Self::Error> {
        self.outgoing
            .send(frame.to_vec())
            .await
            .map_err(|_| MemoryLinkError::Disconnected)
    }

    async fn receive(&mut self, frame: &mut [u8]) -> Result<usize, Self::Error> {
        let received = self
            .incoming
            .recv()
            .await
            .ok_or(MemoryLinkError::Disconnected)?;
        if received.len() > frame.len() {
            return Err(MemoryLinkError::FrameTooLarge);
        }
        frame[..received.len()].copy_from_slice(&received);
        Ok(received.len())
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let (link_ground, link_satellite) = MemoryLink::pair(4);
    let stack_ground = StackResources::<255, 2, 1, 1>::new();
    let stack_satellite = StackResources::<255, 2, 1, 1>::new();
    let rdp_ground = RdpResources::<255>::new();
    let rdp_satellite = RdpResources::<255>::new();
    rdp_ground.update_time(0);
    rdp_satellite.update_time(0);

    let mut ground_builder = RunnerBuilder::new(1, link_ground, &stack_ground).unwrap();
    let client = ground_builder
        .rdp_client(&rdp_ground, RdpConfig::new(0x1000))
        .unwrap();
    let mut ground_runner = ground_builder.build();

    let mut satellite_builder = RunnerBuilder::new(2, link_satellite, &stack_satellite).unwrap();
    let listener = satellite_builder
        .listen_rdp(20, &rdp_satellite, RdpConfig::new(0x2000))
        .unwrap();
    let mut satellite_runner = satellite_builder.build();

    let application = async {
        let (ground_connection, satellite_connection) =
            tokio::join!(client.connect(2, 20), listener.accept());
        let mut ground_connection = ground_connection.unwrap();
        let mut satellite_connection = satellite_connection.unwrap();

        ground_connection.send(b"execute command").await.unwrap();
        println!("ground: command delivery acknowledged");

        let command = satellite_connection.recv().await;
        println!(
            "satellite: received {:?} from {}:{}",
            command.payload(),
            command.source(),
            command.source_port()
        );
    };

    let background = async {
        let clock = async {
            let mut now_ms = 0_u32;
            loop {
                tokio::time::sleep(Duration::from_millis(10)).await;
                now_ms = now_ms.wrapping_add(10);
                rdp_ground.update_time(now_ms);
                rdp_satellite.update_time(now_ms);
            }
        };
        tokio::select! {
            result = ground_runner.run() => panic!("ground runner stopped: {result:?}"),
            result = satellite_runner.run() => panic!("satellite runner stopped: {result:?}"),
            () = clock => unreachable!(),
        }
    };

    tokio::select! {
        () = application => {}
        () = background => unreachable!(),
    }
}
