use serde_json::Value as JsonValue;

#[derive(Debug, toasty::Model)]
pub(crate) struct SaveHeadRow {
    #[key]
    pub(crate) save_id: String,
    pub(crate) revision: i64,
    #[column(type = json)]
    pub(crate) manifest: JsonValue,
    pub(crate) checksum: String,
}

#[derive(Debug, toasty::Model)]
pub(crate) struct CheckpointRow {
    #[key]
    pub(crate) id: String,
    pub(crate) save_id: String,
    pub(crate) revision: i64,
    #[column(type = json)]
    pub(crate) records: JsonValue,
    pub(crate) checksum: String,
}

#[derive(Debug, toasty::Model)]
pub(crate) struct RecordOpRow {
    #[key]
    pub(crate) id: String,
    pub(crate) save_id: String,
    pub(crate) revision: i64,
    pub(crate) op_order: i64,
    pub(crate) action_id: String,
    #[column(type = json)]
    pub(crate) payload: JsonValue,
    pub(crate) checksum: String,
}

#[derive(Debug, toasty::Model)]
pub(crate) struct WorldEventRow {
    #[key]
    pub(crate) id: String,
    pub(crate) save_id: String,
    pub(crate) revision: i64,
    pub(crate) event_id: String,
    #[column(type = json)]
    pub(crate) payload: JsonValue,
    pub(crate) checksum: String,
}

#[derive(Debug, toasty::Model)]
pub(crate) struct TranscriptRow {
    #[key]
    pub(crate) id: String,
    pub(crate) save_id: String,
    pub(crate) revision: i64,
    pub(crate) transcript_id: String,
    #[column(type = json)]
    pub(crate) payload: JsonValue,
    pub(crate) checksum: String,
}

#[derive(Debug, toasty::Model)]
pub(crate) struct ActionCommitRow {
    #[key]
    pub(crate) id: String,
    pub(crate) save_id: String,
    pub(crate) action_id: String,
    pub(crate) revision: i64,
    pub(crate) request_digest: String,
    #[column(type = json)]
    pub(crate) command: JsonValue,
    #[column(type = json)]
    pub(crate) outcome: JsonValue,
    pub(crate) checksum: String,
}

pub(crate) fn model_set() -> toasty::ModelSet {
    toasty::models!(
        SaveHeadRow,
        CheckpointRow,
        RecordOpRow,
        WorldEventRow,
        TranscriptRow,
        ActionCommitRow
    )
}
