//! P0 evidence for Loreloom's durable commit boundary.
//!
//! The models and helpers here are test-only. They exercise candidate backend
//! behavior without freezing a production Store schema or public API.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use std::time::Instant;

use serde_json::{Value as JsonValue, json};
use tempfile::TempDir;
use toasty::{Db, Executor};
use toasty_core::driver::{ConnectContext, Driver};
use toasty_core::schema::db::Migration;
use toasty_driver_sqlite::Sqlite;
use toasty_driver_surreal::SurrealDb;

const SAVE_ID: &str = "save/main";

#[derive(Debug, toasty::Model)]
struct SaveHead {
    #[key]
    save_id: String,
    revision: i64,
}

#[derive(Debug, toasty::Model)]
struct RecordOp {
    #[key]
    id: String,
    save_id: String,
    revision: i64,
    object_id: String,
    #[column(type = json)]
    payload: JsonValue,
    #[column(type = json)]
    optional_payload: Option<JsonValue>,
}

#[derive(Debug, toasty::Model)]
struct WorldEvent {
    #[key]
    id: String,
    save_id: String,
    revision: i64,
    kind: String,
}

#[derive(Debug, toasty::Model)]
struct TranscriptItem {
    #[key]
    id: String,
    save_id: String,
    revision: i64,
    text: String,
}

#[derive(Debug, toasty::Model)]
struct ActionCommit {
    #[key]
    action_id: String,
    save_id: String,
    revision: i64,
    #[column(type = json)]
    result: JsonValue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InjectAfter {
    Record,
    Event,
    Transcript,
    Action,
    Head,
}

#[derive(Debug, PartialEq, Eq)]
enum CommitOutcome {
    Applied { revision: i64 },
    AlreadyCommitted { revision: i64 },
    Conflict { actual_revision: i64 },
    Injected,
}

fn surreal_models() -> toasty::ModelSet {
    toasty::models!(SaveHead, RecordOp, WorldEvent, TranscriptItem, ActionCommit)
}

async fn open_surreal(driver: SurrealDb) -> toasty::Result<Db> {
    let db = Db::builder().models(surreal_models()).build(driver).await?;
    db.push_schema().await?;
    Ok(db)
}

async fn seed_head(db: &mut Db) -> toasty::Result<()> {
    match SaveHead::get_by_save_id(db, SAVE_ID).await {
        Ok(_) => Ok(()),
        Err(error) if error.is_record_not_found() => {
            toasty::create!(SaveHead {
                save_id: SAVE_ID,
                revision: 0,
            })
            .exec(db)
            .await?;
            Ok(())
        }
        Err(error) => Err(error),
    }
}

async fn rollback_injected(tx: toasty::Transaction<'_>) -> toasty::Result<CommitOutcome> {
    tx.rollback().await?;
    Ok(CommitOutcome::Injected)
}

async fn commit_durable(
    db: &mut Db,
    action_id: &str,
    expected_revision: i64,
    payload: JsonValue,
    inject_after: Option<InjectAfter>,
) -> toasty::Result<CommitOutcome> {
    let mut tx = db.transaction().await?;

    match ActionCommit::get_by_action_id(&mut tx, action_id).await {
        Ok(existing) => {
            let revision = existing.revision;
            tx.rollback().await?;
            return Ok(CommitOutcome::AlreadyCommitted { revision });
        }
        Err(error) if error.is_record_not_found() => {}
        Err(error) => return Err(error),
    }

    let mut head = SaveHead::get_by_save_id(&mut tx, SAVE_ID).await?;
    if head.revision != expected_revision {
        let actual_revision = head.revision;
        tx.rollback().await?;
        return Ok(CommitOutcome::Conflict { actual_revision });
    }
    let revision = expected_revision
        .checked_add(1)
        .expect("spike revisions remain in the signed database range");

    toasty::create!(RecordOp {
        id: format!("{action_id}/record"),
        save_id: SAVE_ID,
        revision,
        object_id: "character/player",
        payload: payload.clone(),
        optional_payload: None,
    })
    .exec(&mut tx)
    .await?;
    if inject_after == Some(InjectAfter::Record) {
        return rollback_injected(tx).await;
    }

    toasty::create!(WorldEvent {
        id: format!("{action_id}/event"),
        save_id: SAVE_ID,
        revision,
        kind: "resource_changed",
    })
    .exec(&mut tx)
    .await?;
    if inject_after == Some(InjectAfter::Event) {
        return rollback_injected(tx).await;
    }

    toasty::create!(TranscriptItem {
        id: format!("{action_id}/transcript"),
        save_id: SAVE_ID,
        revision,
        text: "The player rests.",
    })
    .exec(&mut tx)
    .await?;
    if inject_after == Some(InjectAfter::Transcript) {
        return rollback_injected(tx).await;
    }

    toasty::create!(ActionCommit {
        action_id,
        save_id: SAVE_ID,
        revision,
        result: json!({ "status": "committed", "revision": revision }),
    })
    .exec(&mut tx)
    .await?;
    if inject_after == Some(InjectAfter::Action) {
        return rollback_injected(tx).await;
    }

    head.update().revision(revision).exec(&mut tx).await?;
    if inject_after == Some(InjectAfter::Head) {
        return rollback_injected(tx).await;
    }

    match tx.commit().await {
        Ok(()) => Ok(CommitOutcome::Applied { revision }),
        Err(error) if error.is_serialization_failure() => Ok(CommitOutcome::Conflict {
            actual_revision: expected_revision,
        }),
        Err(error) => Err(error),
    }
}

#[derive(Debug, PartialEq, Eq)]
struct DurableCounts {
    records: usize,
    events: usize,
    transcripts: usize,
    actions: usize,
}

async fn durable_counts(db: &mut Db) -> toasty::Result<DurableCounts> {
    let records: Vec<RecordOp> = RecordOp::all().exec(db).await?;
    let events: Vec<WorldEvent> = WorldEvent::all().exec(db).await?;
    let transcripts: Vec<TranscriptItem> = TranscriptItem::all().exec(db).await?;
    let actions: Vec<ActionCommit> = ActionCommit::all().exec(db).await?;
    Ok(DurableCounts {
        records: records.len(),
        events: events.len(),
        transcripts: transcripts.len(),
        actions: actions.len(),
    })
}

async fn assert_empty_revision_zero(db: &mut Db) {
    assert_eq!(
        SaveHead::get_by_save_id(db, SAVE_ID)
            .await
            .expect("save head exists")
            .revision,
        0
    );
    assert_eq!(
        durable_counts(db)
            .await
            .expect("durable rows can be counted"),
        DurableCounts {
            records: 0,
            events: 0,
            transcripts: 0,
            actions: 0,
        }
    );
}

#[tokio::test]
async fn explicit_transaction_commits_one_durable_unit_and_action_is_idempotent() {
    let mut db = open_surreal(SurrealDb::mem())
        .await
        .expect("open in-memory SurrealDB");
    seed_head(&mut db).await.expect("seed save head");
    let payload = json!({
        "schema_version": 1,
        "component": "resource_pool",
        "data": { "stamina": 9 },
    });

    assert_eq!(
        commit_durable(&mut db, "action/rest-1", 0, payload.clone(), None)
            .await
            .expect("durable commit succeeds"),
        CommitOutcome::Applied { revision: 1 }
    );
    assert_eq!(
        commit_durable(&mut db, "action/rest-1", 0, payload, None)
            .await
            .expect("duplicate action is classified"),
        CommitOutcome::AlreadyCommitted { revision: 1 }
    );
    assert_eq!(
        durable_counts(&mut db)
            .await
            .expect("durable rows can be counted"),
        DurableCounts {
            records: 1,
            events: 1,
            transcripts: 1,
            actions: 1,
        }
    );
    assert_eq!(
        SaveHead::get_by_save_id(&mut db, SAVE_ID)
            .await
            .expect("head can be loaded")
            .revision,
        1
    );
}

#[tokio::test]
async fn every_injected_stage_rolls_back_the_entire_durable_unit() {
    for (index, stage) in [
        InjectAfter::Record,
        InjectAfter::Event,
        InjectAfter::Transcript,
        InjectAfter::Action,
        InjectAfter::Head,
    ]
    .into_iter()
    .enumerate()
    {
        let mut db = open_surreal(SurrealDb::mem())
            .await
            .expect("open isolated in-memory SurrealDB");
        seed_head(&mut db).await.expect("seed save head");
        assert_eq!(
            commit_durable(
                &mut db,
                &format!("action/injected-{index}"),
                0,
                json!({ "stage": format!("{stage:?}") }),
                Some(stage),
            )
            .await
            .expect("injected transaction rolls back"),
            CommitOutcome::Injected
        );
        assert_empty_revision_zero(&mut db).await;
    }
}

async fn stage_competing_unit<E: Executor>(
    executor: &mut E,
    action_id: &str,
    revision: i64,
) -> toasty::Result<()> {
    toasty::create!(RecordOp {
        id: format!("{action_id}/record"),
        save_id: SAVE_ID,
        revision,
        object_id: "character/player",
        payload: json!({ "winner": action_id }),
        optional_payload: None,
    })
    .exec(&mut *executor)
    .await?;
    toasty::create!(WorldEvent {
        id: format!("{action_id}/event"),
        save_id: SAVE_ID,
        revision,
        kind: "competing_action",
    })
    .exec(&mut *executor)
    .await?;
    toasty::create!(TranscriptItem {
        id: format!("{action_id}/transcript"),
        save_id: SAVE_ID,
        revision,
        text: action_id,
    })
    .exec(&mut *executor)
    .await?;
    toasty::create!(ActionCommit {
        action_id,
        save_id: SAVE_ID,
        revision,
        result: json!({ "status": "committed", "revision": revision }),
    })
    .exec(executor)
    .await?;
    Ok(())
}

#[tokio::test]
async fn two_connections_competing_from_one_revision_commit_exactly_once() {
    let directory = TempDir::new().expect("create temporary SurrealKV parent");
    let path = directory.path().join("conflict");
    let driver = SurrealDb::surrealkv(&path);
    driver.reset_db().await.expect("reset conflict database");
    let mut first_db = open_surreal(driver.clone())
        .await
        .expect("open first database handle");
    let mut second_db = open_surreal(driver)
        .await
        .expect("open second database handle");
    seed_head(&mut first_db).await.expect("seed save head");

    let mut first_tx = first_db
        .transaction()
        .await
        .expect("start first transaction");
    let mut second_tx = second_db
        .transaction()
        .await
        .expect("start second transaction");
    let mut first_head = SaveHead::get_by_save_id(&mut first_tx, SAVE_ID)
        .await
        .expect("first transaction reads revision zero");
    let mut second_head = SaveHead::get_by_save_id(&mut second_tx, SAVE_ID)
        .await
        .expect("second transaction reads revision zero");
    assert_eq!(first_head.revision, 0);
    assert_eq!(second_head.revision, 0);

    stage_competing_unit(&mut first_tx, "action/first", 1)
        .await
        .expect("first durable unit stages");
    stage_competing_unit(&mut second_tx, "action/second", 1)
        .await
        .expect("second durable unit stages");
    first_head
        .update()
        .revision(1)
        .exec(&mut first_tx)
        .await
        .expect("first head update stages");
    second_head
        .update()
        .revision(1)
        .exec(&mut second_tx)
        .await
        .expect("second head update stages");

    first_tx.commit().await.expect("first transaction commits");
    let conflict = second_tx
        .commit()
        .await
        .expect_err("second transaction must conflict");
    assert!(conflict.is_serialization_failure());
    assert_eq!(
        SaveHead::get_by_save_id(&mut first_db, SAVE_ID)
            .await
            .expect("committed head exists")
            .revision,
        1
    );
    assert_eq!(
        durable_counts(&mut first_db)
            .await
            .expect("only the winner is visible"),
        DurableCounts {
            records: 1,
            events: 1,
            transcripts: 1,
            actions: 1,
        }
    );
}

#[tokio::test]
async fn native_json_preserves_versioned_payload_and_null_distinction() {
    let mut db = open_surreal(SurrealDb::mem())
        .await
        .expect("open JSON test database");
    seed_head(&mut db).await.expect("seed save head");
    let payload = json!({
        "schema_version": 1,
        "known": "value",
        "future_unknown": { "array": [1, true, null, "日本語"] },
        "largest_u64": u64::MAX,
        "json_null": null,
    });

    commit_durable(&mut db, "action/json", 0, payload.clone(), None)
        .await
        .expect("JSON durable unit commits");
    let loaded = RecordOp::get_by_id(&mut db, "action/json/record")
        .await
        .expect("JSON record reloads");
    assert_eq!(loaded.payload, payload);
    assert_eq!(loaded.payload["json_null"], JsonValue::Null);
    assert_eq!(loaded.optional_payload, None);
}

async fn reopen_surreal(path: &Path) -> Db {
    let mut last_error = None;
    for _ in 0..50 {
        match open_surreal(SurrealDb::surrealkv(path)).await {
            Ok(db) => return db,
            Err(error) => {
                last_error = Some(error);
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
    panic!("failed to reopen SurrealKV database: {last_error:?}");
}

fn copy_directory(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_directory(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[tokio::test]
async fn surrealkv_reopen_observes_only_the_complete_revision() {
    let directory = TempDir::new().expect("create temporary SurrealKV parent");
    let path = directory.path().join("reopen");
    {
        let driver = SurrealDb::surrealkv(&path);
        driver.reset_db().await.expect("reset reopen database");
        let mut db = open_surreal(driver)
            .await
            .expect("open initial SurrealKV database");
        seed_head(&mut db).await.expect("seed save head");
        assert_eq!(
            commit_durable(
                &mut db,
                "action/persisted",
                0,
                json!({ "persisted": true }),
                None,
            )
            .await
            .expect("persistent durable unit commits"),
            CommitOutcome::Applied { revision: 1 }
        );
    }

    let mut reopened = reopen_surreal(&path).await;
    assert_eq!(
        SaveHead::get_by_save_id(&mut reopened, SAVE_ID)
            .await
            .expect("reopened head exists")
            .revision,
        1
    );
    assert_eq!(
        durable_counts(&mut reopened)
            .await
            .expect("reopened durable rows are complete"),
        DurableCounts {
            records: 1,
            events: 1,
            transcripts: 1,
            actions: 1,
        }
    );
}

#[tokio::test]
async fn closed_surrealkv_backup_restores_and_save_paths_do_not_cross_talk() {
    let directory = TempDir::new().expect("create temporary backup parent");
    let primary_path = directory.path().join("primary");
    let other_path = directory.path().join("other");
    let backup_path = directory.path().join("backup");

    let primary_driver = SurrealDb::surrealkv(&primary_path);
    primary_driver.reset_db().await.expect("reset primary save");
    let mut primary = open_surreal(primary_driver)
        .await
        .expect("open primary save");
    seed_head(&mut primary).await.expect("seed primary head");
    commit_durable(
        &mut primary,
        "action/backup",
        0,
        json!({ "backup": "revision-one" }),
        None,
    )
    .await
    .expect("commit primary durable unit");
    drop(primary);
    tokio::time::sleep(Duration::from_millis(100)).await;

    copy_directory(&primary_path, &backup_path).expect("copy closed SurrealKV save directory");

    let other_driver = SurrealDb::surrealkv(&other_path);
    other_driver.reset_db().await.expect("reset other save");
    let mut other = open_surreal(other_driver).await.expect("open other save");
    seed_head(&mut other).await.expect("seed other head");
    assert_empty_revision_zero(&mut other).await;

    let mut restored = reopen_surreal(&backup_path).await;
    assert_eq!(
        SaveHead::get_by_save_id(&mut restored, SAVE_ID)
            .await
            .expect("restored head exists")
            .revision,
        1
    );
    assert_eq!(
        durable_counts(&mut restored)
            .await
            .expect("restored durable rows are complete"),
        DurableCounts {
            records: 1,
            events: 1,
            transcripts: 1,
            actions: 1,
        }
    );
}

async fn prepare_crash_store(path: &Path) {
    let driver = SurrealDb::surrealkv(path);
    driver.reset_db().await.expect("reset crash test save");
    let mut db = open_surreal(driver).await.expect("open crash test save");
    seed_head(&mut db).await.expect("seed crash test head");
    drop(db);
    tokio::time::sleep(Duration::from_millis(100)).await;
}

fn run_crash_child(path: &Path, mode: &str, expected_code: i32) {
    let executable = std::env::current_exe().expect("resolve current test executable");
    let status = Command::new(executable)
        .arg("--exact")
        .arg("crash_child_process")
        .arg("--nocapture")
        .env("LORELOOM_SPIKE_CRASH_MODE", mode)
        .env("LORELOOM_SPIKE_CRASH_PATH", path)
        .status()
        .expect("spawn crash test child");
    assert_eq!(status.code(), Some(expected_code));
}

#[tokio::test]
async fn crash_child_process() {
    let Ok(mode) = std::env::var("LORELOOM_SPIKE_CRASH_MODE") else {
        return;
    };
    let path = PathBuf::from(
        std::env::var_os("LORELOOM_SPIKE_CRASH_PATH")
            .expect("crash child receives a database path"),
    );
    if mode == "before" {
        std::process::exit(70);
    }

    let mut db = reopen_surreal(&path).await;
    if mode == "during" {
        let mut tx = db.transaction().await.expect("start crash transaction");
        let mut head = SaveHead::get_by_save_id(&mut tx, SAVE_ID)
            .await
            .expect("crash transaction reads head");
        stage_competing_unit(&mut tx, "action/crash-during", 1)
            .await
            .expect("stage crash durable unit");
        head.update()
            .revision(1)
            .exec(&mut tx)
            .await
            .expect("stage crash head update");
        std::process::exit(71);
    }
    if mode == "after" {
        assert_eq!(
            commit_durable(
                &mut db,
                "action/crash-after",
                0,
                json!({ "crash": "after-commit" }),
                None,
            )
            .await
            .expect("commit before simulated crash"),
            CommitOutcome::Applied { revision: 1 }
        );
        std::process::exit(72);
    }
    panic!("unknown crash child mode: {mode}");
}

#[tokio::test]
async fn process_exit_before_during_and_after_commit_recovers_complete_revisions() {
    let directory = TempDir::new().expect("create temporary crash parent");
    for (mode, code, expected_revision, expected_rows) in [
        ("before", 70, 0, 0),
        ("during", 71, 0, 0),
        ("after", 72, 1, 1),
    ] {
        let path = directory.path().join(mode);
        prepare_crash_store(&path).await;
        run_crash_child(&path, mode, code);

        let mut reopened = reopen_surreal(&path).await;
        assert_eq!(
            SaveHead::get_by_save_id(&mut reopened, SAVE_ID)
                .await
                .expect("recovered head exists")
                .revision,
            expected_revision
        );
        let counts = durable_counts(&mut reopened)
            .await
            .expect("recovered durable rows can be counted");
        assert_eq!(counts.records, expected_rows);
        assert_eq!(counts.events, expected_rows);
        assert_eq!(counts.transcripts, expected_rows);
        assert_eq!(counts.actions, expected_rows);
    }
}

#[tokio::test]
async fn ten_thousand_records_commit_reopen_and_load() {
    const RECORD_COUNT: usize = 10_000;

    let directory = TempDir::new().expect("create temporary scale parent");
    let path = directory.path().join("scale");
    let driver = SurrealDb::surrealkv(&path);
    driver.reset_db().await.expect("reset scale database");
    let mut db = open_surreal(driver).await.expect("open scale database");
    seed_head(&mut db).await.expect("seed scale head");

    let commit_started = Instant::now();
    let mut tx = db.transaction().await.expect("start scale transaction");
    for index in 0..RECORD_COUNT {
        toasty::create!(RecordOp {
            id: format!("bulk/{index:05}"),
            save_id: SAVE_ID,
            revision: 1,
            object_id: format!("object/{index:05}"),
            payload: json!({ "schema_version": 1, "index": index }),
            optional_payload: None,
        })
        .exec(&mut tx)
        .await
        .expect("stage scale record");
    }
    let mut head = SaveHead::get_by_save_id(&mut tx, SAVE_ID)
        .await
        .expect("load scale head");
    head.update()
        .revision(1)
        .exec(&mut tx)
        .await
        .expect("stage scale head");
    tx.commit().await.expect("commit scale transaction");
    let commit_elapsed = commit_started.elapsed();
    drop(db);

    let reopen_started = Instant::now();
    let mut reopened = reopen_surreal(&path).await;
    let reopen_elapsed = reopen_started.elapsed();
    let load_started = Instant::now();
    let records: Vec<RecordOp> = RecordOp::all()
        .exec(&mut reopened)
        .await
        .expect("load scale records");
    let load_elapsed = load_started.elapsed();
    assert_eq!(records.len(), RECORD_COUNT);
    assert_eq!(
        SaveHead::get_by_save_id(&mut reopened, SAVE_ID)
            .await
            .expect("scale head persists")
            .revision,
        1
    );

    eprintln!(
        "store-spike records={RECORD_COUNT} commit_ms={} reopen_ms={} load_ms={}",
        commit_elapsed.as_millis(),
        reopen_elapsed.as_millis(),
        load_elapsed.as_millis()
    );
}

#[tokio::test]
async fn migration_ids_are_tracked_by_the_public_driver_contract() {
    let driver = SurrealDb::mem();
    let mut connection = driver
        .connect(&ConnectContext::default())
        .await
        .expect("open direct migration connection");
    connection
        .apply_migration(
            2026083001,
            "loreloom-spike",
            &Migration::new_sql("RETURN NONE".to_owned()),
        )
        .await
        .expect("apply tracked migration");
    let applied = connection
        .applied_migrations()
        .await
        .expect("load tracked migrations");
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].id(), 2026083001);
}

#[derive(Debug, toasty::Model)]
struct ComparisonHead {
    #[key]
    save_id: String,
    revision: i64,
}

#[derive(Debug, toasty::Model)]
struct ComparisonRecord {
    #[key]
    id: String,
    revision: i64,
    canonical_json: String,
}

#[tokio::test]
async fn sqlite_explicit_transaction_satisfies_the_same_atomic_unit_shape() {
    let mut db = Db::builder()
        .models(toasty::models!(ComparisonHead, ComparisonRecord))
        .build(Sqlite::in_memory())
        .await
        .expect("open SQLite comparison database");
    db.push_schema().await.expect("push SQLite schema");
    toasty::create!(ComparisonHead {
        save_id: SAVE_ID,
        revision: 0,
    })
    .exec(&mut db)
    .await
    .expect("seed SQLite head");

    let mut tx = db.transaction().await.expect("start SQLite transaction");
    let mut head = ComparisonHead::get_by_save_id(&mut tx, SAVE_ID)
        .await
        .expect("read SQLite head");
    toasty::create!(ComparisonRecord {
        id: "action/sqlite/record",
        revision: 1,
        canonical_json: "{\"schema_version\":1}",
    })
    .exec(&mut tx)
    .await
    .expect("stage SQLite record");
    head.update()
        .revision(1)
        .exec(&mut tx)
        .await
        .expect("stage SQLite head");
    tx.commit().await.expect("commit SQLite durable unit");

    let mut rollback = db
        .transaction()
        .await
        .expect("start SQLite rollback transaction");
    toasty::create!(ComparisonRecord {
        id: "action/sqlite/rolled-back",
        revision: 2,
        canonical_json: "{\"must\":\"disappear\"}",
    })
    .exec(&mut rollback)
    .await
    .expect("stage rolled-back SQLite record");
    rollback
        .rollback()
        .await
        .expect("roll back SQLite transaction");

    let records: Vec<ComparisonRecord> = ComparisonRecord::all()
        .exec(&mut db)
        .await
        .expect("scan SQLite records");
    assert_eq!(records.len(), 1);
    assert_eq!(
        ComparisonHead::get_by_save_id(&mut db, SAVE_ID)
            .await
            .expect("load committed SQLite head")
            .revision,
        1
    );
}
