use loreloom_core::{
    ActionId, ActorId, ContentHash, DomainRecord, EventId, ExecutionChangeSet, LockedMod, LongText,
    ModId, ModLock, ModSourceKind, Revision, SAVE_FORMAT_V1, SaveId, SaveManifest, SessionId,
    ShortText, TranscriptItemId, TranscriptItemRecord, TranscriptSpeaker, TranscriptState,
    WorldCommand, WorldCommandKind, WorldEvent, WorldEventKind, WorldId, WorldLock,
    WorldStateRecord, WorldTime,
};
use loreloom_store::{ActionResolution, CommitRequest, CommitResult, CommittedAction, SaveStore};
use tempfile::TempDir;

fn parse<T: std::str::FromStr>(value: &str) -> T
where
    T::Err: std::fmt::Debug,
{
    value.parse().expect("fixture id")
}

fn fixture() -> (SaveManifest, DomainRecord, ActorId) {
    let actor = parse("obj_01890f6a-2b3c-7d4e-8f90-123456789abc");
    let world_id = parse::<WorldId>("wld_01890f6a-2b3d-7d4e-8f90-123456789abc");
    (
        SaveManifest {
            format_version: SAVE_FORMAT_V1,
            save_id: parse::<SaveId>("sav_01890f6a-2b3e-7d4e-8f90-123456789abc"),
            world_id,
            world_lock: WorldLock {
                world_id: parse("games.loreloom.test"),
                version: parse("1.0.0"),
                content_hash: ContentHash::parse("b".repeat(64)).expect("world content hash"),
                manifest_schema: 1,
                content_schema: 1,
            },
            mod_lock: ModLock::default(),
        },
        DomainRecord::WorldState(WorldStateRecord {
            id: world_id,
            player_actor: actor,
            active_scene: parse("obj_01890f6a-2b3f-7d4e-8f90-123456789abc"),
            clock: WorldTime::ZERO,
            rng_seed: [7; 32],
        }),
        actor,
    )
}

fn request(
    action: &str,
    event: &str,
    actor: ActorId,
    expected_revision: Revision,
    ticks: u64,
    state: WorldStateRecord,
) -> CommitRequest {
    let command = WorldCommand {
        action_id: parse::<ActionId>(action),
        actor_id: actor,
        expected_revision,
        kind: WorldCommandKind::AdvanceTime { ticks },
    };
    let revision = expected_revision.next().expect("fixture revision");
    let changes = ExecutionChangeSet {
        action_id: command.action_id,
        expected_revision,
        revision,
        upserts: vec![DomainRecord::WorldState(WorldStateRecord {
            clock: state.clock.checked_add(ticks).expect("fixture world time"),
            ..state
        })],
        deletes: Vec::new(),
        events: vec![WorldEvent {
            id: parse::<EventId>(event),
            action_id: command.action_id,
            actor_id: actor,
            revision,
            kind: WorldEventKind::ClockAdvanced {
                from: expected_revision.get(),
                to: revision.get(),
            },
        }],
        safe_summary: ShortText::new("clock advanced").expect("summary"),
    };
    CommitRequest::from_execution(command, changes).expect("valid commit request")
}

fn candidate_content_locks(manifest: &SaveManifest) -> (WorldLock, ModLock) {
    let mut world_lock = manifest.world_lock.clone();
    world_lock.version = "1.1.0".parse().expect("candidate world version");
    world_lock.content_hash = ContentHash::parse("c".repeat(64)).expect("candidate world hash");
    let mod_id = ModId::parse("games.loreloom.extension").expect("candidate Mod ID");
    let mod_lock = ModLock {
        mods: vec![LockedMod {
            mod_id,
            version: "2.0.0".parse().expect("candidate Mod version"),
            content_hash: ContentHash::parse("d".repeat(64)).expect("candidate Mod hash"),
            manifest_schema: 1,
            content_schema: 1,
            source_kind: ModSourceKind::Directory,
            dependencies: Vec::new(),
            applied_patches: Vec::new(),
        }],
    };
    (world_lock, mod_lock)
}

#[tokio::test]
async fn content_locks_are_adopted_atomically_without_advancing_revision() {
    let directory = TempDir::new().expect("temporary save parent");
    let path = directory.path().join("save");
    let (manifest, initial, _) = fixture();
    let mut first = SaveStore::create(&path, manifest.clone(), vec![initial])
        .await
        .expect("create save");
    let mut stale = first.connect().await.expect("stale connection");
    let (world_lock, mod_lock) = candidate_content_locks(&manifest);

    first
        .adopt_content_locks(world_lock.clone(), mod_lock.clone())
        .await
        .expect("adopt candidate content locks");
    assert_eq!(first.revision(), Revision::ZERO);
    assert_eq!(&first.manifest().world_lock, &world_lock);
    assert_eq!(&first.manifest().mod_lock, &mod_lock);
    let loaded = first.load().await.expect("load adopted manifest");
    assert_eq!(loaded.revision, Revision::ZERO);
    assert_eq!(loaded.manifest, *first.manifest());

    let mut rejected_world = world_lock;
    rejected_world.content_hash =
        ContentHash::parse("e".repeat(64)).expect("rejected candidate hash");
    assert!(matches!(
        stale
            .adopt_content_locks(rejected_world, ModLock::default())
            .await,
        Err(loreloom_store::StoreError::InvalidCommit {
            field: "content_lock_adoption_head"
        })
    ));
    assert_eq!(stale.manifest(), &manifest);

    let connected = first.connect().await.expect("reopen adopted save");
    assert_eq!(connected.revision(), Revision::ZERO);
    assert_eq!(connected.manifest(), first.manifest());
}

#[tokio::test]
async fn create_commit_idempotency_rebuild_and_checkpoint_are_typed() {
    let directory = TempDir::new().expect("temporary save parent");
    let path = directory.path().join("save");
    let (manifest, initial, actor) = fixture();
    let DomainRecord::WorldState(initial_state) = initial.clone() else {
        unreachable!("fixture state")
    };
    let mut store = SaveStore::create(&path, manifest.clone(), vec![initial])
        .await
        .expect("create save");
    let loaded = store.load().await.expect("load revision zero");
    assert_eq!(loaded.manifest, manifest);
    assert_eq!(loaded.revision, Revision::ZERO);
    assert_eq!(loaded.records.len(), 1);

    let commit = request(
        "act_01890f6a-2b40-7d4e-8f90-123456789abc",
        "evt_01890f6a-2b41-7d4e-8f90-123456789abc",
        actor,
        Revision::ZERO,
        1,
        initial_state.clone(),
    );
    assert!(matches!(
        store.commit(&commit).await.expect("commit revision one"),
        CommitResult::Committed(ref outcome) if outcome.revision == Revision::new(1)
    ));
    assert!(matches!(
        store.commit(&commit).await.expect("repeat committed action"),
        CommitResult::AlreadyCommitted(ref outcome) if outcome.revision == Revision::new(1)
    ));
    assert!(matches!(
        store
            .resolve_action(commit.command())
            .await
            .expect("resolve committed action"),
        ActionResolution::Committed(ref outcome) if outcome.revision == Revision::new(1)
    ));

    let loaded = store.load().await.expect("rebuild revision one");
    assert_eq!(loaded.revision, Revision::new(1));
    assert_eq!(loaded.events.len(), 1);
    let DomainRecord::WorldState(state) = &loaded.records[0] else {
        panic!("world state record")
    };
    assert_eq!(state.clock, WorldTime::from_ticks(1));
    let mut inconsistent = loaded.records.clone();
    let DomainRecord::WorldState(inconsistent_state) = &mut inconsistent[0] else {
        panic!("world state record")
    };
    inconsistent_state.clock = WorldTime::from_ticks(99);
    assert!(store.checkpoint(&inconsistent).await.is_err());
    store
        .checkpoint(&loaded.records)
        .await
        .expect("checkpoint revision one");
    store
        .checkpoint(&loaded.records)
        .await
        .expect("identical checkpoint is idempotent");

    let different = request(
        "act_01890f6a-2b40-7d4e-8f90-123456789abc",
        "evt_01890f6a-2b42-7d4e-8f90-123456789abc",
        actor,
        Revision::ZERO,
        2,
        initial_state,
    );
    assert!(matches!(
        store
            .commit(&different)
            .await
            .expect("classify reused action id"),
        CommitResult::ActionIdentityConflict { .. }
    ));
    drop(store);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[tokio::test]
async fn two_connections_competing_from_one_revision_publish_once() {
    let directory = TempDir::new().expect("temporary save parent");
    let path = directory.path().join("save");
    let (manifest, initial, actor) = fixture();
    let DomainRecord::WorldState(initial_state) = initial.clone() else {
        unreachable!("fixture state")
    };
    let mut first = SaveStore::create(&path, manifest, vec![initial])
        .await
        .expect("create save");
    let mut second = first.connect().await.expect("second connection");
    let first_request = request(
        "act_01890f6a-2b50-7d4e-8f90-123456789abc",
        "evt_01890f6a-2b51-7d4e-8f90-123456789abc",
        actor,
        Revision::ZERO,
        1,
        initial_state.clone(),
    );
    let second_request = request(
        "act_01890f6a-2b52-7d4e-8f90-123456789abc",
        "evt_01890f6a-2b53-7d4e-8f90-123456789abc",
        actor,
        Revision::ZERO,
        2,
        initial_state,
    );

    let results = [
        first.commit(&first_request).await.expect("first result"),
        second.commit(&second_request).await.expect("second result"),
    ];
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, CommitResult::Committed(_)))
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, CommitResult::Conflict { .. }))
            .count(),
        1
    );
    let loaded = first.load().await.expect("load winning revision");
    assert_eq!(loaded.revision, Revision::new(1));
    assert_eq!(loaded.events.len(), 1);
    drop((first, second));
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}

#[tokio::test]
async fn transcript_rows_and_record_projection_commit_together() {
    let directory = TempDir::new().expect("temporary save parent");
    let path = directory.path().join("save");
    let (manifest, initial, actor) = fixture();
    let DomainRecord::WorldState(initial_state) = initial.clone() else {
        unreachable!("fixture state")
    };
    let mut store = SaveStore::create(&path, manifest, vec![initial])
        .await
        .expect("create save");
    let action_id = parse::<ActionId>("act_01890f6a-2b60-7d4e-8f90-123456789abc");
    let command = WorldCommand {
        action_id,
        actor_id: actor,
        expected_revision: Revision::ZERO,
        kind: WorldCommandKind::AdvanceTime { ticks: 1 },
    };
    let transcript = TranscriptItemRecord {
        id: parse::<TranscriptItemId>("trn_01890f6a-2b61-7d4e-8f90-123456789abc"),
        session_id: parse::<SessionId>("ses_01890f6a-2b62-7d4e-8f90-123456789abc"),
        revision: Some(Revision::new(1)),
        speaker: TranscriptSpeaker::Narrator,
        text: LongText::new("The harbor clock advances.").expect("transcript text"),
        state: TranscriptState::Committed,
        supporting_events: Vec::new(),
    };
    let changes = ExecutionChangeSet {
        action_id,
        expected_revision: Revision::ZERO,
        revision: Revision::new(1),
        upserts: vec![
            DomainRecord::WorldState(WorldStateRecord {
                clock: WorldTime::from_ticks(1),
                ..initial_state
            }),
            DomainRecord::TranscriptItem(transcript.clone()),
        ],
        deletes: Vec::new(),
        events: Vec::new(),
        safe_summary: ShortText::new("narration committed").expect("summary"),
    };
    let request = CommitRequest::new(
        command,
        changes.record_ops().expect("record operations"),
        changes.events,
        vec![transcript.clone()],
        CommittedAction {
            action_id,
            revision: Revision::new(1),
            event_ids: Vec::new(),
            safe_summary: changes.safe_summary,
        },
    )
    .expect("transcript commit request");
    assert!(matches!(
        store.commit(&request).await.expect("commit transcript"),
        CommitResult::Committed(_)
    ));
    let loaded = store.load().await.expect("load transcript projection");
    assert_eq!(loaded.transcripts, vec![transcript.clone()]);
    assert!(
        loaded
            .records
            .contains(&DomainRecord::TranscriptItem(transcript))
    );
    drop(store);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}
