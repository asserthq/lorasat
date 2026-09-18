use core::time::Duration;
use nalgebra::Vector3;

use sat_core::time::Delay;

use sat_core::adcs::{AdcsCommand, AdcsMode, BdotAlgorithm, Coils, Mag};

#[derive(Default)]
pub struct AdcsState {
    mode: AdcsMode,
    outer_b: Vector3<f32>,
    coil_levels: CoilLevelsVec,
}

pub struct AdcSystem<M: Mag, A: Coils, D: Delay> {
    mag: M,
    coils: A,
    delay: D,
    state: AdcsState,
    bdot: BdotAlgorithm,
    pending_cmd: Option<AdcsCommand>,
}

impl<M: Mag, A: Coils, D: Delay> AdcSystem<M, A, D> {
    pub fn new(mag: M, coils: A, delay: D) -> Self {
        Self {
            mag,
            coils,
            delay,
            state: AdcsState::default(),
            bdot: BdotAlgorithm::new(),
            pending_cmd: None,
        }
    }

    pub async fn run(&mut self) {
        loop {
            self.read_sensors().await;
            self.apply_control().await;
            self.handle_commands().await;
        }
    }

    pub fn send_command(&mut self, cmd: AdcsCommand) {
        self.pending_cmd = Some(cmd);
    }

    async fn read_sensors(&mut self) {
        self.state.outer_b = self.mag.read().await;
    }

    async fn apply_control(&mut self) {
        match self.state.mode {
            AdcsMode::Idle => {}
            AdcsMode::Detumbling => {
                let u = self.bdot.calc_control(self.state.outer_b);
                self.state.coil_levels = u;
                self.coils.apply_levels(u).await;
                self.delay.delay(Duration::from_millis(200)).await;
            }
        }
    }

    async fn handle_commands(&mut self) {
        if let Some(cmd) = self.pending_cmd.take() {
            match cmd {
                AdcsCommand::SetMode(mode) => self.set_mode(mode),
            }
            self.pending_cmd = None;
        }
    }

    fn set_mode(&mut self, mode: AdcsMode) {
        if self.state.mode != mode {
            self.reset();
            self.state.mode = mode;
        }
    }

    fn reset(&mut self) {
        self.state = AdcsState::default();
    }
}
