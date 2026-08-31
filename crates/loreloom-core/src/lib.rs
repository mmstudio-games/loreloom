//! Backend-independent Loreloom domain protocols.

mod codec;
mod domain;
mod identity;
mod numeric;
mod persistence;
mod protocol;
mod record;
mod revision;
mod text;
mod view;

pub use codec::{DomainError, DomainRecord, decode_domain_records, migrate_domain_records};
pub use domain::{
    ActionState, AgentBinding, AttributeAdjustment, AttributeOperation, AutonomyMode,
    BaseAttributes, CharacterController, CharacterLifetime, CharacterOrigin, CharacterProfile,
    CharacterRecord, CharacterSpawnSpec, ConditionGrantInput, ConditionRecord, ConditionSource,
    ContainerState, ContentHash, ContentOrigin, DomainValueError, Durability, EntityOrigin,
    EquippedState, EventInstanceRecord, EventStatus, FactSource, FactSubject, FactValue,
    GeneratedOrigin, GenerationSource, GoalInput, GoalRecord, GoalSource, GoalStatus,
    IntensityPolicy, ItemGrantInput, ItemRecord, KnowledgeStatus, KnownFactInput, KnownFactRecord,
    LifeState, ParameterSetRecord, ParameterValue, PlaceRecord, PlacementInput, Posture,
    RelationshipRecord, ResourcePool, RuleStateRecord, SceneRecord, SkillGrantInput,
    SkillGrantRecord, SkillSource, SpawnConstraints, StackState, TranscriptItemRecord,
    TranscriptSpeaker, TranscriptState, WorldStateRecord,
};
pub use identity::{
    ActionId, ActorId, ContentDefinitionId, EventId, GenerationId, IdGenerator, IdentityError,
    ModId, NpcTurnRequestId, ObjectId, SaveId, SessionId, SystemIdGenerator, TranscriptItemId,
    WorldId,
};
pub use numeric::{Fixed, FixedError, WorldTime};
pub use persistence::{
    LockedDependency, LockedMod, ModLock, ModSourceKind, PersistenceError, SAVE_FORMAT_V1,
    SaveManifest,
};
pub use protocol::{
    ExecutionChangeSet, SkillTargetRef, WorldCommand, WorldCommandKind, WorldEvent, WorldEventKind,
};
pub use record::{
    MigrationRegistry, MigrationStep, RecordEnvelope, RecordError, RecordId, RecordKey,
    RecordMutation, RecordProvenance, RecordSet, RecordType, SchemaVersion, VersionedRecordOp,
    rebuild_records,
};
pub use revision::{Revision, RevisionError};
pub use text::{BoundedText, DisplayName, LongText, ShortText, TextError};
pub use view::{
    ActiveEventView, AttributeView, CharacterContext, ConditionView,
    DIAGNOSED_CONDITION_PREDICATE_ID, EventOptionView, InventoryView, NoticeKind, ParameterSetView,
    ParameterValueView, ResourceView, RuntimePhase, SceneContext, SceneObservation, SkillView,
    ToolActivity, ToolActivityState, TranscriptWindow, UiNotice, UiSnapshot, VisibleActorView,
};
