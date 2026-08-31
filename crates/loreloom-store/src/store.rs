use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use loreloom_core::{
    ActionId, DomainRecord, EventId, RecordEnvelope, Revision, SaveManifest, TranscriptItemId,
    TranscriptItemRecord, VersionedRecordOp, WorldCommand, WorldEvent, decode_domain_records,
    rebuild_records,
};
use serde::Serialize;
use serde_json::{Value as JsonValue, json};
use sha2::{Digest, Sha256};
use toasty::{Db, Executor};
use toasty_driver_surreal::SurrealDb;

use crate::{
    CommitRequest, CommitResult, CommittedAction, StoreError,
    models::{
        ActionCommitRow, CheckpointRow, RecordOpRow, SaveHeadRow, TranscriptRow, WorldEventRow,
        model_set,
    },
};

const COMMAND_DIGEST_DOMAIN: &[u8] = b"loreloom.world-command.v1\0";
const HEAD_CHECKSUM_DOMAIN: &[u8] = b"loreloom.save-head.v1\0";
const CHECKPOINT_CHECKSUM_DOMAIN: &[u8] = b"loreloom.checkpoint.v1\0";
const RECORD_OP_CHECKSUM_DOMAIN: &[u8] = b"loreloom.record-op.v1\0";
const EVENT_CHECKSUM_DOMAIN: &[u8] = b"loreloom.world-event.v1\0";
const TRANSCRIPT_CHECKSUM_DOMAIN: &[u8] = b"loreloom.transcript.v1\0";
const ACTION_CHECKSUM_DOMAIN: &[u8] = b"loreloom.action-commit.v1\0";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionResolution {
    NotCommitted { head_revision: Revision },
    Committed(CommittedAction),
    ActionIdentityConflict { action_id: ActionId },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedSave {
    pub manifest: SaveManifest,
    pub revision: Revision,
    pub records: Vec<DomainRecord>,
    pub events: Vec<WorldEvent>,
    pub transcripts: Vec<TranscriptItemRecord>,
}

pub struct SaveStore {
    db: Db,
    driver: SurrealDb,
    manifest: SaveManifest,
    revision: Revision,
}

impl std::fmt::Debug for SaveStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SaveStore")
            .field("save_id", &self.manifest.save_id)
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}

impl SaveStore {
    pub async fn create(
        path: impl AsRef<Path>,
        manifest: SaveManifest,
        initial_records: Vec<DomainRecord>,
    ) -> Result<Self, StoreError> {
        if path.as_ref().exists() {
            return Err(StoreError::SaveAlreadyExists);
        }
        manifest.validate()?;
        let (records, transcripts) = checkpoint_records(&initial_records, Revision::ZERO)?;
        validate_world_identity(&manifest, &initial_records)?;
        let records_json = to_json(&records, "encode initial checkpoint")?;
        let checkpoint_checksum = checkpoint_checksum(Revision::ZERO, &records_json)?;
        let manifest_json = to_json(&manifest, "encode save manifest")?;
        let head_checksum = head_checksum(Revision::ZERO, &manifest_json)?;
        let prepared_transcripts = prepare_transcripts(
            &manifest.save_id.to_string(),
            Revision::ZERO,
            &transcripts,
            true,
        )?;

        let driver = SurrealDb::surrealkv(path.as_ref());
        let mut db = open_database(driver.clone()).await?;
        let mut tx = db
            .transaction()
            .await
            .map_err(|error| StoreError::backend("begin initialize transaction", error))?;
        toasty::create!(SaveHeadRow {
            save_id: manifest.save_id.to_string(),
            revision: 0_i64,
            manifest: manifest_json,
            checksum: head_checksum,
        })
        .exec(&mut tx)
        .await
        .map_err(|error| StoreError::backend("write initial save head", error))?;
        toasty::create!(CheckpointRow {
            id: checkpoint_row_id(Revision::ZERO),
            save_id: manifest.save_id.to_string(),
            revision: 0_i64,
            records: records_json,
            checksum: checkpoint_checksum,
        })
        .exec(&mut tx)
        .await
        .map_err(|error| StoreError::backend("write initial checkpoint", error))?;
        for transcript in prepared_transcripts {
            create_transcript_row(&mut tx, transcript).await?;
        }
        tx.commit()
            .await
            .map_err(|error| StoreError::backend("commit save initialization", error))?;
        Ok(Self {
            db,
            driver,
            manifest,
            revision: Revision::ZERO,
        })
    }

    pub async fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        if !path.as_ref().exists() {
            return Err(StoreError::SaveNotFound);
        }
        Self::from_driver(SurrealDb::surrealkv(path.as_ref())).await
    }

    pub async fn connect(&self) -> Result<Self, StoreError> {
        let connected = Self::from_driver(self.driver.clone()).await?;
        if connected.manifest != self.manifest {
            return integrity("connected_manifest");
        }
        Ok(connected)
    }

    async fn from_driver(driver: SurrealDb) -> Result<Self, StoreError> {
        let mut db = open_database(driver.clone()).await?;
        let (manifest, revision) = load_single_head(&mut db).await?;
        Ok(Self {
            db,
            driver,
            manifest,
            revision,
        })
    }

    #[must_use]
    pub fn manifest(&self) -> &SaveManifest {
        &self.manifest
    }

    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    pub async fn commit(&mut self, request: &CommitRequest) -> Result<CommitResult, StoreError> {
        request.validate()?;
        let prepared = PreparedCommit::new(&self.manifest, request)?;
        let mut tx = self
            .db
            .transaction()
            .await
            .map_err(|error| StoreError::backend("begin durable transaction", error))?;

        match ActionCommitRow::get_by_id(&mut tx, &prepared.action_id).await {
            Ok(existing) => {
                let outcome = validate_action_row(&existing, &self.manifest.save_id.to_string())?;
                tx.rollback()
                    .await
                    .map_err(|error| StoreError::backend("rollback duplicate action", error))?;
                return if existing.request_digest == prepared.request_digest {
                    Ok(CommitResult::AlreadyCommitted(outcome))
                } else {
                    Ok(CommitResult::ActionIdentityConflict {
                        action_id: request.command().action_id,
                    })
                };
            }
            Err(error) if error.is_record_not_found() => {}
            Err(error) => return Err(StoreError::backend("read action identity", error)),
        }

        let mut head = SaveHeadRow::get_by_save_id(&mut tx, &prepared.save_id)
            .await
            .map_err(|error| StoreError::backend("read save head for commit", error))?;
        let (_, actual) = validate_head_row(&head)?;
        if actual != request.command().expected_revision {
            tx.rollback()
                .await
                .map_err(|error| StoreError::backend("rollback revision conflict", error))?;
            return Ok(CommitResult::Conflict {
                expected: request.command().expected_revision,
                actual,
            });
        }

        for row in prepared.record_ops {
            toasty::create!(RecordOpRow {
                id: row.id,
                save_id: prepared.save_id.clone(),
                revision: row.revision,
                op_order: row.order,
                action_id: prepared.action_id.clone(),
                payload: row.payload,
                checksum: row.checksum,
            })
            .exec(&mut tx)
            .await
            .map_err(|error| StoreError::backend("write record operation", error))?;
        }
        for row in prepared.events {
            toasty::create!(WorldEventRow {
                id: row.id,
                save_id: prepared.save_id.clone(),
                revision: row.revision,
                event_id: row.event_id,
                payload: row.payload,
                checksum: row.checksum,
            })
            .exec(&mut tx)
            .await
            .map_err(|error| StoreError::backend("write world event", error))?;
        }
        for row in prepared.transcripts {
            create_transcript_row(&mut tx, row).await?;
        }
        toasty::create!(ActionCommitRow {
            id: prepared.action_id.clone(),
            save_id: prepared.save_id.clone(),
            action_id: prepared.action_id,
            revision: prepared.revision,
            request_digest: prepared.request_digest,
            command: prepared.command,
            outcome: prepared.outcome,
            checksum: prepared.action_checksum,
        })
        .exec(&mut tx)
        .await
        .map_err(|error| StoreError::backend("write action commit", error))?;
        head.update()
            .revision(prepared.revision)
            .checksum(prepared.head_checksum)
            .exec(&mut tx)
            .await
            .map_err(|error| StoreError::backend("advance save head", error))?;

        let outcome = request.safe_outcome().clone();
        match tx.commit().await {
            Ok(()) => {
                self.revision = outcome.revision;
                Ok(CommitResult::Committed(outcome))
            }
            Err(error) if error.is_serialization_failure() => {
                let (_, actual) =
                    load_head(&mut self.db, &self.manifest.save_id.to_string()).await?;
                self.revision = actual;
                Ok(CommitResult::Conflict {
                    expected: request.command().expected_revision,
                    actual,
                })
            }
            Err(error) => Err(StoreError::backend("commit durable transaction", error)),
        }
    }

    pub async fn resolve_action(
        &mut self,
        command: &WorldCommand,
    ) -> Result<ActionResolution, StoreError> {
        let digest = request_digest(command)?;
        match ActionCommitRow::get_by_id(&mut self.db, command.action_id.to_string()).await {
            Ok(row) => {
                let outcome = validate_action_row(&row, &self.manifest.save_id.to_string())?;
                if row.request_digest == digest {
                    Ok(ActionResolution::Committed(outcome))
                } else {
                    Ok(ActionResolution::ActionIdentityConflict {
                        action_id: command.action_id,
                    })
                }
            }
            Err(error) if error.is_record_not_found() => {
                let (_, revision) =
                    load_head(&mut self.db, &self.manifest.save_id.to_string()).await?;
                self.revision = revision;
                Ok(ActionResolution::NotCommitted {
                    head_revision: revision,
                })
            }
            Err(error) => Err(StoreError::backend("resolve action identity", error)),
        }
    }

    pub async fn load(&mut self) -> Result<LoadedSave, StoreError> {
        let (manifest, revision) =
            load_head(&mut self.db, &self.manifest.save_id.to_string()).await?;
        if manifest != self.manifest {
            return integrity("manifest_changed");
        }
        let checkpoint = load_checkpoint(&mut self.db, &manifest, revision).await?;
        let operations =
            load_record_ops(&mut self.db, &manifest, checkpoint.revision, revision).await?;
        let actions = load_actions(&mut self.db, &manifest, revision).await?;
        for operation in &operations {
            let Some(action) = actions.get(&operation.action_id) else {
                return integrity("record_op_action_missing");
            };
            if action.revision != operation.revision {
                return integrity("record_op_action_revision");
            }
        }
        let (rebuilt_revision, records) =
            rebuild_records(checkpoint.revision, checkpoint.records, operations)?;
        if rebuilt_revision != revision {
            return integrity("rebuilt_revision");
        }
        let typed = decode_domain_records(&records)?;
        let records = typed.values().cloned().collect::<Vec<_>>();
        validate_world_identity(&manifest, &records)?;
        let events = load_events(&mut self.db, &manifest, revision).await?;
        for event in &events {
            if actions
                .get(&event.action_id)
                .is_none_or(|action| action.revision != event.revision)
            {
                return integrity("event_action_projection");
            }
        }
        let transcripts = load_transcripts(&mut self.db, &manifest, revision).await?;
        validate_transcript_projection(&typed, &transcripts)?;
        self.revision = revision;
        Ok(LoadedSave {
            manifest,
            revision,
            records,
            events,
            transcripts,
        })
    }

    pub async fn checkpoint(&mut self, records: &[DomainRecord]) -> Result<(), StoreError> {
        validate_world_identity(&self.manifest, records)?;
        let (envelopes, _) = checkpoint_records(records, self.revision)?;
        let loaded = self.load().await?;
        let (durable_envelopes, _) = checkpoint_records(&loaded.records, self.revision)?;
        if envelopes != durable_envelopes {
            return integrity("checkpoint_projection");
        }
        let records_json = to_json(&envelopes, "encode checkpoint")?;
        let checksum = checkpoint_checksum(self.revision, &records_json)?;
        let id = checkpoint_row_id(self.revision);
        let revision_i64 = revision_to_i64(self.revision)?;
        let mut tx = self
            .db
            .transaction()
            .await
            .map_err(|error| StoreError::backend("begin checkpoint transaction", error))?;
        let head = SaveHeadRow::get_by_save_id(&mut tx, self.manifest.save_id.to_string())
            .await
            .map_err(|error| StoreError::backend("read checkpoint save head", error))?;
        let (_, actual) = validate_head_row(&head)?;
        if actual != self.revision {
            tx.rollback()
                .await
                .map_err(|error| StoreError::backend("rollback stale checkpoint", error))?;
            self.revision = actual;
            return Err(StoreError::InvalidCommit {
                field: "checkpoint_revision",
            });
        }
        match CheckpointRow::get_by_id(&mut tx, &id).await {
            Ok(existing) => {
                let same = existing.save_id == self.manifest.save_id.to_string()
                    && existing.revision == revision_i64
                    && existing.records == records_json
                    && existing.checksum == checksum;
                tx.rollback()
                    .await
                    .map_err(|error| StoreError::backend("rollback existing checkpoint", error))?;
                return if same {
                    Ok(())
                } else {
                    integrity("checkpoint_identity")
                };
            }
            Err(error) if error.is_record_not_found() => {}
            Err(error) => return Err(StoreError::backend("read checkpoint identity", error)),
        }
        toasty::create!(CheckpointRow {
            id,
            save_id: self.manifest.save_id.to_string(),
            revision: revision_i64,
            records: records_json,
            checksum,
        })
        .exec(&mut tx)
        .await
        .map_err(|error| StoreError::backend("write checkpoint", error))?;
        tx.commit()
            .await
            .map_err(|error| StoreError::backend("commit checkpoint", error))
    }
}

struct LoadedCheckpoint {
    revision: Revision,
    records: Vec<RecordEnvelope>,
}

struct PreparedRecordOp {
    id: String,
    revision: i64,
    order: i64,
    payload: JsonValue,
    checksum: String,
}

struct PreparedEvent {
    id: String,
    revision: i64,
    event_id: String,
    payload: JsonValue,
    checksum: String,
}

struct PreparedTranscript {
    id: String,
    save_id: String,
    revision: i64,
    transcript_id: String,
    payload: JsonValue,
    checksum: String,
}

struct PreparedCommit {
    save_id: String,
    action_id: String,
    revision: i64,
    request_digest: String,
    command: JsonValue,
    outcome: JsonValue,
    action_checksum: String,
    head_checksum: String,
    record_ops: Vec<PreparedRecordOp>,
    events: Vec<PreparedEvent>,
    transcripts: Vec<PreparedTranscript>,
}

impl PreparedCommit {
    fn new(manifest: &SaveManifest, request: &CommitRequest) -> Result<Self, StoreError> {
        let revision = request.safe_outcome().revision;
        let revision_i64 = revision_to_i64(revision)?;
        let save_id = manifest.save_id.to_string();
        let action_id = request.command().action_id.to_string();
        let request_digest = request_digest(request.command())?;
        let command = to_json(request.command(), "encode committed command")?;
        let outcome = to_json(request.safe_outcome(), "encode committed outcome")?;
        let action_checksum = action_checksum(
            &save_id,
            &action_id,
            revision,
            &request_digest,
            &command,
            &outcome,
        )?;
        let manifest_json = to_json(manifest, "encode committed manifest")?;
        let head_checksum = head_checksum(revision, &manifest_json)?;
        let record_ops = request
            .record_ops()
            .iter()
            .map(|operation| {
                let payload = to_json(operation, "encode record operation")?;
                let checksum = record_op_checksum(&save_id, operation, &payload)?;
                Ok(PreparedRecordOp {
                    id: record_op_row_id(operation.revision, operation.order),
                    revision: revision_i64,
                    order: i64::from(operation.order),
                    payload,
                    checksum,
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        let events = request
            .events()
            .iter()
            .map(|event| {
                let payload = to_json(event, "encode world event")?;
                let checksum = event_checksum(&save_id, event, &payload)?;
                Ok(PreparedEvent {
                    id: event_row_id(event.revision, event.id),
                    revision: revision_i64,
                    event_id: event.id.to_string(),
                    payload,
                    checksum,
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        let transcripts = prepare_transcripts(&save_id, revision, request.transcripts(), false)?;
        Ok(Self {
            save_id,
            action_id,
            revision: revision_i64,
            request_digest,
            command,
            outcome,
            action_checksum,
            head_checksum,
            record_ops,
            events,
            transcripts,
        })
    }
}

async fn open_database(driver: SurrealDb) -> Result<Db, StoreError> {
    let db = Db::builder()
        .models(model_set())
        .build(driver)
        .await
        .map_err(|error| StoreError::backend("open SurrealKV", error))?;
    db.push_schema()
        .await
        .map_err(|error| StoreError::backend("apply store schema", error))?;
    Ok(db)
}

async fn load_single_head(db: &mut Db) -> Result<(SaveManifest, Revision), StoreError> {
    let heads: Vec<SaveHeadRow> = SaveHeadRow::all()
        .exec(db)
        .await
        .map_err(|error| StoreError::backend("scan save heads", error))?;
    match heads.len() {
        0 => Err(StoreError::SaveNotInitialized),
        1 => validate_head_row(&heads[0]),
        _ => Err(StoreError::InvalidSaveHeadCount),
    }
}

async fn load_head(db: &mut Db, save_id: &str) -> Result<(SaveManifest, Revision), StoreError> {
    let head = SaveHeadRow::get_by_save_id(db, save_id)
        .await
        .map_err(|error| StoreError::backend("load save head", error))?;
    validate_head_row(&head)
}

fn validate_head_row(row: &SaveHeadRow) -> Result<(SaveManifest, Revision), StoreError> {
    let revision = revision_from_i64(row.revision)?;
    if head_checksum(revision, &row.manifest)? != row.checksum {
        return integrity("save_head_checksum");
    }
    let manifest: SaveManifest = from_json(&row.manifest, "decode save manifest")?;
    manifest.validate()?;
    if row.save_id != manifest.save_id.to_string() {
        return integrity("save_head_identity");
    }
    Ok((manifest, revision))
}

async fn load_checkpoint(
    db: &mut Db,
    manifest: &SaveManifest,
    head_revision: Revision,
) -> Result<LoadedCheckpoint, StoreError> {
    let rows: Vec<CheckpointRow> = CheckpointRow::all()
        .exec(db)
        .await
        .map_err(|error| StoreError::backend("scan checkpoints", error))?;
    let mut selected = None;
    for row in rows {
        if row.save_id != manifest.save_id.to_string() {
            return integrity("checkpoint_save_identity");
        }
        let revision = revision_from_i64(row.revision)?;
        if revision > head_revision {
            return integrity("checkpoint_future_revision");
        }
        if row.id != checkpoint_row_id(revision)
            || checkpoint_checksum(revision, &row.records)? != row.checksum
        {
            return integrity("checkpoint_checksum");
        }
        if selected
            .as_ref()
            .is_none_or(|(selected_revision, _): &(Revision, JsonValue)| {
                revision > *selected_revision
            })
        {
            selected = Some((revision, row.records));
        }
    }
    let (revision, payload) = selected.ok_or(StoreError::Integrity {
        item: "checkpoint_missing",
    })?;
    let records: Vec<RecordEnvelope> = from_json(&payload, "decode checkpoint")?;
    let mut previous = None;
    for record in &records {
        if record.revision() > revision || previous.as_ref().is_some_and(|key| key >= &record.key())
        {
            return integrity("checkpoint_record_order");
        }
        previous = Some(record.key());
    }
    Ok(LoadedCheckpoint { revision, records })
}

async fn load_record_ops(
    db: &mut Db,
    manifest: &SaveManifest,
    checkpoint_revision: Revision,
    head_revision: Revision,
) -> Result<Vec<VersionedRecordOp>, StoreError> {
    let rows: Vec<RecordOpRow> = RecordOpRow::all()
        .exec(db)
        .await
        .map_err(|error| StoreError::backend("scan record operations", error))?;
    let mut operations = Vec::new();
    for row in rows {
        if row.save_id != manifest.save_id.to_string() {
            return integrity("record_op_save_identity");
        }
        let revision = revision_from_i64(row.revision)?;
        let order = u32::try_from(row.op_order).map_err(|_| StoreError::Integrity {
            item: "record_op_order_range",
        })?;
        if revision > head_revision
            || row.id != record_op_row_id(revision, order)
            || record_op_checksum_parts(
                &row.save_id,
                revision,
                order,
                &row.action_id,
                &row.payload,
            )? != row.checksum
        {
            return integrity("record_op_checksum");
        }
        let operation: VersionedRecordOp = from_json(&row.payload, "decode record operation")?;
        if operation.revision != revision
            || operation.order != order
            || operation.action_id.to_string() != row.action_id
        {
            return integrity("record_op_projection");
        }
        if revision > checkpoint_revision {
            operations.push(operation);
        }
    }
    operations.sort_by_key(|operation| (operation.revision, operation.order));
    Ok(operations)
}

async fn load_events(
    db: &mut Db,
    manifest: &SaveManifest,
    head_revision: Revision,
) -> Result<Vec<WorldEvent>, StoreError> {
    let rows: Vec<WorldEventRow> = WorldEventRow::all()
        .exec(db)
        .await
        .map_err(|error| StoreError::backend("scan world events", error))?;
    let mut events = Vec::new();
    let mut ids = BTreeSet::new();
    for row in rows {
        if row.save_id != manifest.save_id.to_string() {
            return integrity("event_save_identity");
        }
        let revision = revision_from_i64(row.revision)?;
        let event_id: EventId = row.event_id.parse().map_err(|_| StoreError::Integrity {
            item: "event_id_encoding",
        })?;
        if revision > head_revision
            || row.id != event_row_id(revision, event_id)
            || event_checksum_parts(&row.save_id, revision, event_id, &row.payload)? != row.checksum
        {
            return integrity("event_checksum");
        }
        let event: WorldEvent = from_json(&row.payload, "decode world event")?;
        if event.revision != revision || event.id != event_id || !ids.insert(event.id) {
            return integrity("event_projection");
        }
        events.push(event);
    }
    events.sort_by_key(|event| (event.revision, event.id));
    Ok(events)
}

async fn load_actions(
    db: &mut Db,
    manifest: &SaveManifest,
    head_revision: Revision,
) -> Result<BTreeMap<ActionId, CommittedAction>, StoreError> {
    let rows: Vec<ActionCommitRow> = ActionCommitRow::all()
        .exec(db)
        .await
        .map_err(|error| StoreError::backend("scan action commits", error))?;
    let mut actions = BTreeMap::new();
    for row in rows {
        if row.save_id != manifest.save_id.to_string() {
            return integrity("action_save_identity");
        }
        let outcome = validate_action_row(&row, &manifest.save_id.to_string())?;
        if outcome.revision > head_revision || actions.insert(outcome.action_id, outcome).is_some()
        {
            return integrity("action_projection");
        }
    }
    Ok(actions)
}

async fn load_transcripts(
    db: &mut Db,
    manifest: &SaveManifest,
    head_revision: Revision,
) -> Result<Vec<TranscriptItemRecord>, StoreError> {
    let rows: Vec<TranscriptRow> = TranscriptRow::all()
        .exec(db)
        .await
        .map_err(|error| StoreError::backend("scan transcripts", error))?;
    let mut versions = Vec::new();
    for row in rows {
        if row.save_id != manifest.save_id.to_string() {
            return integrity("transcript_save_identity");
        }
        let revision = revision_from_i64(row.revision)?;
        let transcript_id: TranscriptItemId =
            row.transcript_id
                .parse()
                .map_err(|_| StoreError::Integrity {
                    item: "transcript_id_encoding",
                })?;
        if revision > head_revision
            || row.id != transcript_row_id(revision, transcript_id)
            || transcript_checksum_parts(&row.save_id, revision, transcript_id, &row.payload)?
                != row.checksum
        {
            return integrity("transcript_checksum");
        }
        let transcript: TranscriptItemRecord = from_json(&row.payload, "decode transcript")?;
        if transcript.id != transcript_id
            || transcript.revision.is_some_and(|value| value != revision)
        {
            return integrity("transcript_projection");
        }
        versions.push((revision, transcript));
    }
    versions.sort_by_key(|(revision, transcript)| (*revision, transcript.id));
    let mut current = BTreeMap::new();
    for (_, transcript) in versions {
        current.insert(transcript.id, transcript);
    }
    Ok(current.into_values().collect())
}

fn validate_transcript_projection(
    records: &BTreeMap<loreloom_core::RecordKey, DomainRecord>,
    transcripts: &[TranscriptItemRecord],
) -> Result<(), StoreError> {
    let from_records = records
        .values()
        .filter_map(|record| match record {
            DomainRecord::TranscriptItem(transcript) => Some((transcript.id, transcript)),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    if from_records.len() != transcripts.len()
        || transcripts
            .iter()
            .any(|transcript| from_records.get(&transcript.id) != Some(&transcript))
    {
        return integrity("transcript_record_projection");
    }
    Ok(())
}

fn validate_world_identity(
    manifest: &SaveManifest,
    records: &[DomainRecord],
) -> Result<(), StoreError> {
    let mut world_ids = records.iter().filter_map(|record| match record {
        DomainRecord::WorldState(state) => Some(state.id),
        _ => None,
    });
    if world_ids.next() != Some(manifest.world_id) || world_ids.next().is_some() {
        return integrity("world_identity");
    }
    Ok(())
}

fn checkpoint_records(
    records: &[DomainRecord],
    revision: Revision,
) -> Result<(Vec<RecordEnvelope>, Vec<TranscriptItemRecord>), StoreError> {
    let mut envelopes = Vec::with_capacity(records.len());
    let mut transcripts = Vec::new();
    for record in records {
        record.validate()?;
        if let DomainRecord::TranscriptItem(transcript) = record {
            transcripts.push(transcript.clone());
        }
        envelopes.push(record.to_envelope(revision, None)?);
    }
    envelopes.sort_by_key(RecordEnvelope::key);
    if envelopes
        .windows(2)
        .any(|pair| pair[0].key() == pair[1].key())
    {
        return integrity("checkpoint_duplicate_record");
    }
    Ok((envelopes, transcripts))
}

fn prepare_transcripts(
    save_id: &str,
    revision: Revision,
    transcripts: &[TranscriptItemRecord],
    initial: bool,
) -> Result<Vec<PreparedTranscript>, StoreError> {
    transcripts
        .iter()
        .map(|transcript| {
            if (!initial && transcript.revision != Some(revision))
                || (initial && transcript.revision.is_some_and(|value| value != revision))
            {
                return Err(StoreError::InvalidCommit {
                    field: "transcript_revision",
                });
            }
            let payload = to_json(transcript, "encode transcript")?;
            let checksum = transcript_checksum(save_id, revision, transcript, &payload)?;
            Ok(PreparedTranscript {
                id: transcript_row_id(revision, transcript.id),
                save_id: save_id.to_owned(),
                revision: revision_to_i64(revision)?,
                transcript_id: transcript.id.to_string(),
                payload,
                checksum,
            })
        })
        .collect()
}

async fn create_transcript_row<E: Executor>(
    executor: &mut E,
    row: PreparedTranscript,
) -> Result<(), StoreError> {
    toasty::create!(TranscriptRow {
        id: row.id,
        save_id: row.save_id,
        revision: row.revision,
        transcript_id: row.transcript_id,
        payload: row.payload,
        checksum: row.checksum,
    })
    .exec(executor)
    .await
    .map_err(|error| StoreError::backend("write transcript", error))?;
    Ok(())
}

fn request_digest(command: &WorldCommand) -> Result<String, StoreError> {
    let bytes = serde_json::to_vec(command)
        .map_err(|error| StoreError::json("encode command digest", error))?;
    Ok(digest(COMMAND_DIGEST_DOMAIN, &bytes))
}

fn head_checksum(revision: Revision, manifest: &JsonValue) -> Result<String, StoreError> {
    checksum_json(
        HEAD_CHECKSUM_DOMAIN,
        &json!({ "revision": revision, "manifest": manifest }),
    )
}

fn checkpoint_checksum(revision: Revision, records: &JsonValue) -> Result<String, StoreError> {
    checksum_json(
        CHECKPOINT_CHECKSUM_DOMAIN,
        &json!({ "revision": revision, "records": records }),
    )
}

fn record_op_checksum(
    save_id: &str,
    operation: &VersionedRecordOp,
    payload: &JsonValue,
) -> Result<String, StoreError> {
    record_op_checksum_parts(
        save_id,
        operation.revision,
        operation.order,
        &operation.action_id.to_string(),
        payload,
    )
}

fn record_op_checksum_parts(
    save_id: &str,
    revision: Revision,
    order: u32,
    action_id: &str,
    payload: &JsonValue,
) -> Result<String, StoreError> {
    checksum_json(
        RECORD_OP_CHECKSUM_DOMAIN,
        &json!({
            "save_id": save_id,
            "revision": revision,
            "order": order,
            "action_id": action_id,
            "payload": payload,
        }),
    )
}

fn event_checksum(
    save_id: &str,
    event: &WorldEvent,
    payload: &JsonValue,
) -> Result<String, StoreError> {
    event_checksum_parts(save_id, event.revision, event.id, payload)
}

fn event_checksum_parts(
    save_id: &str,
    revision: Revision,
    event_id: EventId,
    payload: &JsonValue,
) -> Result<String, StoreError> {
    checksum_json(
        EVENT_CHECKSUM_DOMAIN,
        &json!({
            "save_id": save_id,
            "revision": revision,
            "event_id": event_id,
            "payload": payload,
        }),
    )
}

fn transcript_checksum(
    save_id: &str,
    revision: Revision,
    transcript: &TranscriptItemRecord,
    payload: &JsonValue,
) -> Result<String, StoreError> {
    transcript_checksum_parts(save_id, revision, transcript.id, payload)
}

fn transcript_checksum_parts(
    save_id: &str,
    revision: Revision,
    transcript_id: TranscriptItemId,
    payload: &JsonValue,
) -> Result<String, StoreError> {
    checksum_json(
        TRANSCRIPT_CHECKSUM_DOMAIN,
        &json!({
            "save_id": save_id,
            "revision": revision,
            "transcript_id": transcript_id,
            "payload": payload,
        }),
    )
}

fn action_checksum(
    save_id: &str,
    action_id: &str,
    revision: Revision,
    request_digest: &str,
    command: &JsonValue,
    outcome: &JsonValue,
) -> Result<String, StoreError> {
    checksum_json(
        ACTION_CHECKSUM_DOMAIN,
        &json!({
            "save_id": save_id,
            "action_id": action_id,
            "revision": revision,
            "request_digest": request_digest,
            "command": command,
            "outcome": outcome,
        }),
    )
}

fn validate_action_row(
    row: &ActionCommitRow,
    save_id: &str,
) -> Result<CommittedAction, StoreError> {
    let revision = revision_from_i64(row.revision)?;
    if row.save_id != save_id
        || row.id != row.action_id
        || action_checksum(
            save_id,
            &row.action_id,
            revision,
            &row.request_digest,
            &row.command,
            &row.outcome,
        )? != row.checksum
    {
        return integrity("action_commit_checksum");
    }
    let command: WorldCommand = from_json(&row.command, "decode committed command")?;
    let outcome: CommittedAction = from_json(&row.outcome, "decode committed outcome")?;
    if command.action_id.to_string() != row.action_id
        || outcome.action_id != command.action_id
        || outcome.revision != revision
        || request_digest(&command)? != row.request_digest
    {
        return integrity("action_commit_projection");
    }
    Ok(outcome)
}

fn checksum_json(domain: &[u8], value: &JsonValue) -> Result<String, StoreError> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| StoreError::json("encode checksum payload", error))?;
    Ok(digest(domain, &bytes))
}

fn digest(domain: &[u8], bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(bytes);
    let output = hasher.finalize();
    let mut encoded = String::with_capacity(output.len() * 2);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in output {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn to_json<T: Serialize>(value: &T, operation: &'static str) -> Result<JsonValue, StoreError> {
    serde_json::to_value(value).map_err(|error| StoreError::json(operation, error))
}

fn from_json<T: serde::de::DeserializeOwned>(
    value: &JsonValue,
    operation: &'static str,
) -> Result<T, StoreError> {
    serde_json::from_value(value.clone()).map_err(|error| StoreError::json(operation, error))
}

fn revision_to_i64(revision: Revision) -> Result<i64, StoreError> {
    i64::try_from(revision.get()).map_err(|_| StoreError::RevisionOutOfRange { revision })
}

fn revision_from_i64(revision: i64) -> Result<Revision, StoreError> {
    u64::try_from(revision)
        .map(Revision::new)
        .map_err(|_| StoreError::Integrity {
            item: "negative_revision",
        })
}

fn checkpoint_row_id(revision: Revision) -> String {
    format!("{:020}", revision.get())
}

fn record_op_row_id(revision: Revision, order: u32) -> String {
    format!("{:020}/{order:010}", revision.get())
}

fn event_row_id(revision: Revision, event_id: EventId) -> String {
    format!("{:020}/{event_id}", revision.get())
}

fn transcript_row_id(revision: Revision, transcript_id: TranscriptItemId) -> String {
    format!("{:020}/{transcript_id}", revision.get())
}

fn integrity<T>(item: &'static str) -> Result<T, StoreError> {
    Err(StoreError::Integrity { item })
}
