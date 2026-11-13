use super::mode::AdcsMode;

#[derive(Debug, defmt::Format)]
pub enum AdcsCommand {
    SetMode(AdcsMode),
}
