/// Persistent storage areas used by platform-level services.
///
/// Each field is an independent logical partition. Queue-like fields normally
/// implement [`super::Storage`]; `configs` normally implements
/// [`super::KeyValueStorage`]. The structure only groups ownership and does not
/// impose one physical flash layout.
pub struct PlatformStorage<TelemetryStorage, EventStorage, DeferredCommandStorage, ConfigStorage> {
    pub telemetry: TelemetryStorage,
    pub events: EventStorage,
    pub deferred_commands: DeferredCommandStorage,
    pub configs: ConfigStorage,
}

impl<TelemetryStorage, EventStorage, DeferredCommandStorage, ConfigStorage>
    PlatformStorage<TelemetryStorage, EventStorage, DeferredCommandStorage, ConfigStorage>
{
    pub const fn new(
        telemetry: TelemetryStorage,
        events: EventStorage,
        deferred_commands: DeferredCommandStorage,
        configs: ConfigStorage,
    ) -> Self {
        Self {
            telemetry,
            events,
            deferred_commands,
            configs,
        }
    }
}
