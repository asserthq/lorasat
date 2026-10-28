use sat_csp::{Link, RunnerBuilder, StackResources};
use tokio::sync::mpsc;

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

impl Link for MemoryLink {
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
    let (link_a, link_b) = MemoryLink::pair(4);
    let resources_a = StackResources::<255, 2, 1, 2>::new();
    let resources_b = StackResources::<255, 2, 1, 2>::new();

    let mut builder_a = RunnerBuilder::new(1, link_a, &resources_a).unwrap();
    let mut socket_a = builder_a.bind(10).unwrap();
    let mut runner_a = builder_a.build();

    let mut builder_b = RunnerBuilder::new(2, link_b, &resources_b).unwrap();
    let mut socket_b = builder_b.bind(20).unwrap();
    let mut runner_b = builder_b.build();

    let application = async {
        socket_a.send_to(2, 20, b"hello").await.unwrap();

        let request = socket_b.recv_from().await;
        println!(
            "node B received {:?} from {}:{}",
            request.payload(),
            request.source(),
            request.source_port()
        );

        socket_b
            .send_to(request.source(), request.source_port(), b"world")
            .await
            .unwrap();

        let reply = socket_a.recv_from().await;
        println!(
            "node A received {:?} from {}:{}",
            reply.payload(),
            reply.source(),
            reply.source_port()
        );
    };

    let runners = async {
        tokio::select! {
            result = runner_a.run() => ("A", result),
            result = runner_b.run() => ("B", result),
        }
    };

    tokio::select! {
        () = application => {}
        (node, result) = runners => panic!("runner {node} stopped unexpectedly: {result:?}"),
    }
}
