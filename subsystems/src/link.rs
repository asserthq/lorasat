use embassy_sync::channel::{SendDynamicReceiver, SendDynamicSender};

pub struct ControllerHandle<'a, Cmd, Event> {
    pub cmd_tx: SendDynamicSender<'a, Cmd>,
    pub event_rx: SendDynamicReceiver<'a, Event>,
}

pub struct WorkerHandle<'a, Cmd, Event> {
    pub cmd_rx: SendDynamicReceiver<'a, Cmd>,
    pub event_tx: SendDynamicSender<'a, Event>,
}
