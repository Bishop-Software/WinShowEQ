/// Internal ground-item representation (mirrors itemBuffer_t in Item.h).
/// Items are NOT sent as this struct; they are packed into a SpawnRecord via
/// Spawn::packNetBufferFrom before transmission.
#[derive(Debug, Clone)]
pub struct GroundItem {
    pub id: u32,
    pub drop_id: u32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub name: String,
    pub flags: u32,
}
