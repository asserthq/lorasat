#[derive(Debug, Default, Clone, PartialEq, defmt::Format)]
pub enum AdcsMode {
    #[default]
    Idle,
    Detumbling,
}
