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
