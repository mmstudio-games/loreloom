use std::collections::BTreeSet;

use loreloom_core::{
    ActionId, DomainRecord, EventId, ExecutionChangeSet, RecordMutation, Revision, ShortText,
    TranscriptItemRecord, VersionedRecordOp, WorldCommand, WorldEvent,
};
use serde::{Deserialize, Serialize};

use crate::StoreError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommittedAction {
    pub action_id: ActionId,
    pub revision: Revision,
    pub event_ids: Vec<EventId>,
    pub safe_summary: ShortText,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitResult {
    Committed(CommittedAction),
    AlreadyCommitted(CommittedAction),
    Conflict {
        expected: Revision,
        actual: Revision,
    },
    ActionIdentityConflict {
        action_id: ActionId,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommitRequest {
    command: WorldCommand,
    record_ops: Vec<VersionedRecordOp>,
    events: Vec<WorldEvent>,
    transcripts: Vec<TranscriptItemRecord>,
    safe_outcome: CommittedAction,
}

impl CommitRequest {
    pub fn new(
        command: WorldCommand,
        record_ops: Vec<VersionedRecordOp>,
        events: Vec<WorldEvent>,
        transcripts: Vec<TranscriptItemRecord>,
        safe_outcome: CommittedAction,
    ) -> Result<Self, StoreError> {
        let request = Self {
            command,
            record_ops,
            events,
            transcripts,
            safe_outcome,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn from_execution(
        command: WorldCommand,
        changes: ExecutionChangeSet,
    ) -> Result<Self, StoreError> {
        let event_ids = changes.events.iter().map(|event| event.id).collect();
        let transcripts = changes
            .upserts
            .iter()
            .filter_map(|record| match record {
                DomainRecord::TranscriptItem(transcript) => Some(transcript.clone()),
                _ => None,
            })
            .collect();
        let safe_outcome = CommittedAction {
            action_id: changes.action_id,
            revision: changes.revision,
            event_ids,
            safe_summary: changes.safe_summary.clone(),
        };
        let record_ops = changes.record_ops()?;
        Self::new(
            command,
            record_ops,
            changes.events,
            transcripts,
            safe_outcome,
        )
    }

    #[must_use]
    pub fn command(&self) -> &WorldCommand {
        &self.command
    }

    #[must_use]
    pub fn record_ops(&self) -> &[VersionedRecordOp] {
        &self.record_ops
    }

    #[must_use]
    pub fn events(&self) -> &[WorldEvent] {
        &self.events
    }

    #[must_use]
    pub fn transcripts(&self) -> &[TranscriptItemRecord] {
        &self.transcripts
    }

    #[must_use]
    pub fn safe_outcome(&self) -> &CommittedAction {
        &self.safe_outcome
    }

    pub fn validate(&self) -> Result<(), StoreError> {
        let revision = self.command.expected_revision.next()?;
        if self.safe_outcome.action_id != self.command.action_id
            || self.safe_outcome.revision != revision
        {
            return invalid("safe_outcome_identity");
        }
        if self.record_ops.is_empty() {
            return invalid("record_ops_empty");
        }
        for (order, operation) in self.record_ops.iter().enumerate() {
            if operation.action_id != self.command.action_id
                || operation.revision != revision
                || usize::try_from(operation.order).ok() != Some(order)
            {
                return invalid("record_op_identity");
            }
            if let RecordMutation::Upsert { record } = &operation.mutation
                && record.revision() != revision
            {
                return invalid("record_op_envelope_revision");
            }
        }

        let event_ids = self.events.iter().map(|event| event.id).collect::<Vec<_>>();
        if event_ids != self.safe_outcome.event_ids
            || event_ids.iter().copied().collect::<BTreeSet<_>>().len() != event_ids.len()
        {
            return invalid("event_ids");
        }
        if self.events.iter().any(|event| {
            event.action_id != self.command.action_id
                || event.actor_id != self.command.actor_id
                || event.revision != revision
        }) {
            return invalid("event_identity");
        }

        let upsert_keys = self
            .record_ops
            .iter()
            .filter_map(|operation| match &operation.mutation {
                RecordMutation::Upsert { record } => Some(record.key()),
                RecordMutation::Delete { .. } => None,
            })
            .collect::<BTreeSet<_>>();
        for transcript in &self.transcripts {
            if transcript.revision != Some(revision) {
                return invalid("transcript_revision");
            }
            let key = DomainRecord::TranscriptItem(transcript.clone()).key()?;
            if !upsert_keys.contains(&key) {
                return invalid("transcript_record_op");
            }
        }
        Ok(())
    }
}

fn invalid<T>(field: &'static str) -> Result<T, StoreError> {
    Err(StoreError::InvalidCommit { field })
}
