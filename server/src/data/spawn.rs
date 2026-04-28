// SpawnRecord and SpawnType now live in the common crate.
// Re-exported here so server-internal modules can keep `crate::data::spawn::SpawnRecord`
// imports unchanged.
pub use common::{SpawnRecord, SpawnType};
