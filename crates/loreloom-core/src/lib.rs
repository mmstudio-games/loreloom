//! Backend-independent Loreloom domain protocols.

mod identity;
mod record;
mod revision;

pub use identity::{
    ActionId, ActorId, ContentDefinitionId, EventId, GenerationId, IdGenerator, IdentityError,
    ModId, NpcTurnRequestId, ObjectId, SaveId, SessionId, SystemIdGenerator, WorldId,
};
pub use record::{
    MigrationRegistry, MigrationStep, RecordEnvelope, RecordError, RecordId, RecordKey,
    RecordMutation, RecordProvenance, RecordSet, RecordType, SchemaVersion, VersionedRecordOp,
    rebuild_records,
};
pub use revision::{Revision, RevisionError};
