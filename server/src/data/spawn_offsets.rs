use crate::config::IniReader;

/// Byte-level offsets within EQ's spawn struct in memory.
/// Read from [SpawnInfo Offsets] in myseqserver.ini.
/// Mirrors Spawn::init() in Spawn.cpp.
#[derive(Debug, Clone, Default)]
pub struct SpawnOffsets {
    pub name:      usize,
    pub last_name: usize,
    pub x:         usize,
    pub y:         usize,
    pub z:         usize,
    pub speed:     usize,
    pub heading:   usize,
    pub prev:      usize,
    pub next:      usize,
    pub type_:     usize,
    pub level:     usize,
    pub hidden:    usize,
    pub class:     usize,
    pub id:        usize,
    pub owner:     usize,
    pub race:      usize,
    pub primary:   usize,
    pub offhand:   usize,
    /// When true, race is 8-bit and id/owner/primary/offhand are 16-bit.
    pub race8:     bool,
    /// Bytes to read per spawn node = largest field offset + 30.
    pub buf_size:  usize,
}

impl SpawnOffsets {
    pub fn from_ini(ir: &IniReader) -> Self {
        let o = |name: &str| ir.read_integer_entry("SpawnInfo Offsets", name, false) as usize;
        let race8 = ir.read_integer_entry("SpawnInfo Offsets", "EightBitRace", false) != 0;

        let name      = o("NameOffset");
        let last_name = o("LastNameOffset");
        let x         = o("XOffset");
        let y         = o("YOffset");
        let z         = o("ZOffset");
        let speed     = o("SpeedOffset");
        let heading   = o("HeadingOffset");
        let prev      = o("PrevOffset");
        let next      = o("NextOffset");
        let type_     = o("TypeOffset");
        let level     = o("LevelOffset");
        let hidden    = o("HideOffset");
        let class     = o("ClassOffset");
        let id        = o("SpawnIDOffset");
        let owner     = o("OwnerIDOffset");
        let race      = o("RaceOffset");
        let primary   = o("PrimaryOffset");
        let offhand   = o("OffhandOffset");

        let largest = [name, last_name, x, y, z, speed, heading, prev, next,
                       type_, level, hidden, class, id, owner, race, primary, offhand]
            .iter().copied().max().unwrap_or(0);

        Self {
            name, last_name, x, y, z, speed, heading, prev, next,
            type_, level, hidden, class, id, owner, race, primary, offhand,
            race8,
            buf_size: largest + 30,
        }
    }

    /// Returns (INI key name, current value) for each spawn offset, in index order.
    /// The order matches the `set_by_index` mapping — index 0 = NameOffset, etc.
    pub fn named_values(&self) -> Vec<(&'static str, usize)> {
        vec![
            ("NameOffset",     self.name),
            ("LastNameOffset", self.last_name),
            ("SpawnIDOffset",  self.id),
            ("OwnerIDOffset",  self.owner),
            ("LevelOffset",    self.level),
            ("RaceOffset",     self.race),
            ("ClassOffset",    self.class),
            ("XOffset",        self.x),
            ("YOffset",        self.y),
            ("ZOffset",        self.z),
            ("HeadingOffset",  self.heading),
            ("SpeedOffset",    self.speed),
            ("TypeOffset",     self.type_),
            ("HideOffset",     self.hidden),
            ("PrimaryOffset",  self.primary),
            ("OffhandOffset",  self.offhand),
            ("PrevOffset",     self.prev),
            ("NextOffset",     self.next),
        ]
    }

    /// Set a secondary spawn offset by numeric index. Updates buf_size.
    /// Returns false if the index is out of range.
    pub fn set_by_index(&mut self, i: usize, val: usize) -> bool {
        match i {
            0  => self.name      = val,
            1  => self.last_name = val,
            2  => self.id        = val,
            3  => self.owner     = val,
            4  => self.level     = val,
            5  => self.race      = val,
            6  => self.class     = val,
            7  => self.x         = val,
            8  => self.y         = val,
            9  => self.z         = val,
            10 => self.heading   = val,
            11 => self.speed     = val,
            12 => self.type_     = val,
            13 => self.hidden    = val,
            14 => self.primary   = val,
            15 => self.offhand   = val,
            16 => self.prev      = val,
            17 => self.next      = val,
            _  => return false,
        }
        let largest = self.named_values().iter().map(|(_, v)| *v).max().unwrap_or(0);
        self.buf_size = largest + 30;
        true
    }

    /// Set a secondary spawn offset by name (case-insensitive). Returns the index on success.
    pub fn set_by_name(&mut self, name: &str, val: usize) -> Option<usize> {
        let idx = self.named_values().iter().position(|(n, _)| n.eq_ignore_ascii_case(name))?;
        self.set_by_index(idx, val).then_some(idx)
    }
}

/// Byte-level offsets within EQ's ground item struct in memory.
/// Read from [GroundItem Offsets] in myseqserver.ini.
/// Mirrors Item::init() in Item.cpp.
#[derive(Debug, Clone, Default)]
pub struct ItemOffsets {
    pub prev:     usize,
    pub next:     usize,
    pub id:       usize,
    pub drop_id:  usize,
    pub x:        usize,
    pub y:        usize,
    pub z:        usize,
    pub name:     usize,
    pub buf_size: usize,
}

impl ItemOffsets {
    pub fn from_ini(ir: &IniReader) -> Self {
        let o = |name: &str| ir.read_integer_entry("GroundItem Offsets", name, false) as usize;

        let prev    = o("PrevOffset");
        let next    = o("NextOffset");
        let id      = o("IdOffset");
        let drop_id = o("DropIdOffset");
        let x       = o("XOffset");
        let y       = o("YOffset");
        let z       = o("ZOffset");
        let name    = o("NameOffset");

        let largest = [prev, next, id, drop_id, x, y, z, name]
            .iter().copied().max().unwrap_or(0);

        Self { prev, next, id, drop_id, x, y, z, name, buf_size: largest + 30 }
    }

    pub fn named_values(&self) -> Vec<(&'static str, usize)> {
        vec![
            ("PrevOffset",   self.prev),
            ("NextOffset",   self.next),
            ("IdOffset",     self.id),
            ("DropIdOffset", self.drop_id),
            ("XOffset",      self.x),
            ("YOffset",      self.y),
            ("ZOffset",      self.z),
            ("NameOffset",   self.name),
        ]
    }
}

/// Byte-level offsets within EQ's world info struct in memory.
/// Read from [WorldInfo Offsets] in myseqserver.ini.
/// Mirrors World::init() in World.cpp.
#[derive(Debug, Clone, Default)]
pub struct WorldOffsets {
    pub hour:     usize,
    pub minute:   usize,
    pub day:      usize,
    pub month:    usize,
    pub year:     usize,
    /// When true, year is 16-bit (same EightBitRace flag as spawn).
    pub race8:    bool,
    pub buf_size: usize,
}

impl WorldOffsets {
    pub fn from_ini(ir: &IniReader) -> Self {
        let o = |name: &str| ir.read_integer_entry("WorldInfo Offsets", name, false) as usize;
        let race8 = ir.read_integer_entry("SpawnInfo Offsets", "EightBitRace", false) != 0;

        let hour   = o("WorldHourOffset");
        let minute = o("WorldMinuteOffset");
        let day    = o("WorldDayOffset");
        let month  = o("WorldMonthOffset");
        let year   = o("WorldYearOffset");

        let largest = [hour, minute, day, month, year].iter().copied().max().unwrap_or(0);

        Self { hour, minute, day, month, year, race8, buf_size: largest + 30 }
    }

    pub fn named_values(&self) -> Vec<(&'static str, usize)> {
        vec![
            ("WorldHourOffset",   self.hour),
            ("WorldMinuteOffset", self.minute),
            ("WorldDayOffset",    self.day),
            ("WorldMonthOffset",  self.month),
            ("WorldYearOffset",   self.year),
        ]
    }
}