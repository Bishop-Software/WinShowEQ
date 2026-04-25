/// Internal world-time representation (mirrors worldBuffer_t in World.h).
/// World data is packed into a SpawnRecord via Spawn::packNetBufferWorld before
/// transmission; this struct is not sent directly over the wire.
#[derive(Debug, Clone, Copy, Default)]
pub struct WorldTime {
    pub hour: u8,
    pub minute: u8,
    pub day: u8,
    pub month: u8,
    pub year: u32,
}