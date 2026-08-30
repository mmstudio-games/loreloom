//! P0 evidence for Loreloom's Content Registry and NpcFactory boundary.
//!
//! All domain types are test-only. The spike validates pure content
//! compilation and transactional spawning without freezing public schemas.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::{Deserialize, Serialize};

const SCENE_DEFINITION_ID: &str = "base/scene/old-mill";
const SCENE_OBJECT_ID: &str = "scene/old-mill";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum DefinitionKind {
    AgentProfile,
    Attribute,
    Resource,
    Condition,
    Item,
    Skill,
    Goal,
    Character,
    Scene,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PackLock {
    mod_id: String,
    mod_version: String,
    pack_id: String,
    content_version: u32,
    content_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AgentProfileDefinition {
    id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AttributeDefinition {
    id: String,
    minimum: i32,
    maximum: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ResourceDefinition {
    id: String,
    minimum: i32,
    maximum: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MarkerDefinition {
    id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ResourceInput {
    current: i32,
    base_maximum: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CharacterDefinition {
    id: String,
    display_name: String,
    agent_profile_id: Option<String>,
    scene_definition_id: String,
    attribute_budget: i32,
    attributes: BTreeMap<String, i32>,
    resources: BTreeMap<String, ResourceInput>,
    conditions: Vec<String>,
    inventory: Vec<String>,
    skills: Vec<String>,
    goals: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RelationshipTemplate {
    kind: String,
    target_local_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SceneCharacterDefinition {
    local_key: String,
    character_definition_id: String,
    relationships: Vec<RelationshipTemplate>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SceneDefinition {
    id: String,
    initial_characters: Vec<SceneCharacterDefinition>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ContentPack {
    lock: PackLock,
    agent_profiles: Vec<AgentProfileDefinition>,
    attributes: Vec<AttributeDefinition>,
    resources: Vec<ResourceDefinition>,
    conditions: Vec<MarkerDefinition>,
    items: Vec<MarkerDefinition>,
    skills: Vec<MarkerDefinition>,
    goals: Vec<MarkerDefinition>,
    characters: Vec<CharacterDefinition>,
    scenes: Vec<SceneDefinition>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ContentOrigin {
    mod_id: String,
    mod_version: String,
    pack_id: String,
    definition_id: String,
    content_version: u32,
    content_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct GeneratedOrigin {
    generation_id: String,
    generator_version: String,
    source_event: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CharacterOrigin {
    Content(ContentOrigin),
    Generated(GeneratedOrigin),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SceneTarget {
    Definition { definition_id: String },
    Object { object_id: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct CharacterSpawnSpec {
    origin: CharacterOrigin,
    display_name: String,
    agent_profile_id: Option<String>,
    placement: SceneTarget,
    attribute_budget: i32,
    attributes: BTreeMap<String, i32>,
    resources: BTreeMap<String, ResourceInput>,
    conditions: Vec<String>,
    inventory: Vec<String>,
    skills: Vec<String>,
    goals: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NpcDraft {
    display_name: String,
    agent_profile_id: Option<String>,
    scene_object_id: String,
    attributes: BTreeMap<String, i32>,
    resources: BTreeMap<String, ResourceInput>,
    conditions: Vec<String>,
    inventory: Vec<String>,
    skills: Vec<String>,
    goals: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GenerationPolicy {
    attribute_budget: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ContentError {
    InvalidId(String),
    DuplicateDefinition(String),
    DuplicatePack(String),
    MissingReference {
        from: String,
        target: String,
        expected: DefinitionKind,
    },
    WrongReferenceKind {
        from: String,
        target: String,
        expected: DefinitionKind,
        actual: DefinitionKind,
    },
    DuplicateLocalKey(String),
    MissingLocalReference {
        scene: String,
        key: String,
    },
    InvalidAttributeRange(String),
    AttributeBudgetExceeded {
        used: i32,
        maximum: i32,
    },
    InvalidResourceRange(String),
    UnknownScene(String),
    UnknownObject(String),
    SelfRelationship(String),
    ContentLockMismatch,
    InvalidSave(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct DefinitionRegistry {
    kinds: BTreeMap<String, DefinitionKind>,
    pack_locks: BTreeMap<String, PackLock>,
    character_origins: BTreeMap<String, ContentOrigin>,
    agent_profiles: BTreeMap<String, AgentProfileDefinition>,
    attributes: BTreeMap<String, AttributeDefinition>,
    resources: BTreeMap<String, ResourceDefinition>,
    conditions: BTreeMap<String, MarkerDefinition>,
    items: BTreeMap<String, MarkerDefinition>,
    skills: BTreeMap<String, MarkerDefinition>,
    goals: BTreeMap<String, MarkerDefinition>,
    characters: BTreeMap<String, CharacterDefinition>,
    scenes: BTreeMap<String, SceneDefinition>,
}

impl DefinitionRegistry {
    fn canonical_ids(&self) -> Vec<(String, DefinitionKind)> {
        self.kinds
            .iter()
            .map(|(id, kind)| (id.clone(), *kind))
            .collect()
    }

    fn reference(
        &self,
        from: &str,
        target: &str,
        expected: DefinitionKind,
    ) -> Result<(), ContentError> {
        match self.kinds.get(target) {
            Some(actual) if *actual == expected => Ok(()),
            Some(actual) => Err(ContentError::WrongReferenceKind {
                from: from.to_owned(),
                target: target.to_owned(),
                expected,
                actual: *actual,
            }),
            None => Err(ContentError::MissingReference {
                from: from.to_owned(),
                target: target.to_owned(),
                expected,
            }),
        }
    }

    fn validate_all_references(&self) -> Result<(), ContentError> {
        for character in self.characters.values() {
            if let Some(profile) = &character.agent_profile_id {
                self.reference(&character.id, profile, DefinitionKind::AgentProfile)?;
            }
            self.reference(
                &character.id,
                &character.scene_definition_id,
                DefinitionKind::Scene,
            )?;
            for id in character.attributes.keys() {
                self.reference(&character.id, id, DefinitionKind::Attribute)?;
            }
            for id in character.resources.keys() {
                self.reference(&character.id, id, DefinitionKind::Resource)?;
            }
            for id in &character.conditions {
                self.reference(&character.id, id, DefinitionKind::Condition)?;
            }
            for id in &character.inventory {
                self.reference(&character.id, id, DefinitionKind::Item)?;
            }
            for id in &character.skills {
                self.reference(&character.id, id, DefinitionKind::Skill)?;
            }
            for id in &character.goals {
                self.reference(&character.id, id, DefinitionKind::Goal)?;
            }
        }

        for scene in self.scenes.values() {
            let mut local_keys = BTreeSet::new();
            for entry in &scene.initial_characters {
                if !local_keys.insert(entry.local_key.clone()) {
                    return Err(ContentError::DuplicateLocalKey(entry.local_key.clone()));
                }
                self.reference(
                    &scene.id,
                    &entry.character_definition_id,
                    DefinitionKind::Character,
                )?;
            }
            for entry in &scene.initial_characters {
                for relation in &entry.relationships {
                    if !local_keys.contains(&relation.target_local_key) {
                        return Err(ContentError::MissingLocalReference {
                            scene: scene.id.clone(),
                            key: relation.target_local_key.clone(),
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
struct RegistryLoader {
    published: DefinitionRegistry,
}

impl RegistryLoader {
    fn published(&self) -> &DefinitionRegistry {
        &self.published
    }

    fn import(&mut self, pack: ContentPack) -> Result<(), ContentError> {
        let mut candidate = self.published.clone();
        if candidate.pack_locks.contains_key(&pack.lock.mod_id) {
            return Err(ContentError::DuplicatePack(pack.lock.mod_id));
        }

        let declarations = pack
            .agent_profiles
            .iter()
            .map(|definition| (definition.id.as_str(), DefinitionKind::AgentProfile))
            .chain(
                pack.attributes
                    .iter()
                    .map(|definition| (definition.id.as_str(), DefinitionKind::Attribute)),
            )
            .chain(
                pack.resources
                    .iter()
                    .map(|definition| (definition.id.as_str(), DefinitionKind::Resource)),
            )
            .chain(
                pack.conditions
                    .iter()
                    .map(|definition| (definition.id.as_str(), DefinitionKind::Condition)),
            )
            .chain(
                pack.items
                    .iter()
                    .map(|definition| (definition.id.as_str(), DefinitionKind::Item)),
            )
            .chain(
                pack.skills
                    .iter()
                    .map(|definition| (definition.id.as_str(), DefinitionKind::Skill)),
            )
            .chain(
                pack.goals
                    .iter()
                    .map(|definition| (definition.id.as_str(), DefinitionKind::Goal)),
            )
            .chain(
                pack.characters
                    .iter()
                    .map(|definition| (definition.id.as_str(), DefinitionKind::Character)),
            )
            .chain(
                pack.scenes
                    .iter()
                    .map(|definition| (definition.id.as_str(), DefinitionKind::Scene)),
            )
            .collect::<Vec<_>>();
        for (id, kind) in declarations {
            if id.is_empty() || !id.contains('/') {
                return Err(ContentError::InvalidId(id.to_owned()));
            }
            if candidate.kinds.insert(id.to_owned(), kind).is_some() {
                return Err(ContentError::DuplicateDefinition(id.to_owned()));
            }
        }

        for definition in pack.agent_profiles {
            candidate
                .agent_profiles
                .insert(definition.id.clone(), definition);
        }
        for definition in pack.attributes {
            candidate
                .attributes
                .insert(definition.id.clone(), definition);
        }
        for definition in pack.resources {
            candidate
                .resources
                .insert(definition.id.clone(), definition);
        }
        for definition in pack.conditions {
            candidate
                .conditions
                .insert(definition.id.clone(), definition);
        }
        for definition in pack.items {
            candidate.items.insert(definition.id.clone(), definition);
        }
        for definition in pack.skills {
            candidate.skills.insert(definition.id.clone(), definition);
        }
        for definition in pack.goals {
            candidate.goals.insert(definition.id.clone(), definition);
        }
        for definition in pack.characters {
            candidate.character_origins.insert(
                definition.id.clone(),
                ContentOrigin {
                    mod_id: pack.lock.mod_id.clone(),
                    mod_version: pack.lock.mod_version.clone(),
                    pack_id: pack.lock.pack_id.clone(),
                    definition_id: definition.id.clone(),
                    content_version: pack.lock.content_version,
                    content_hash: pack.lock.content_hash.clone(),
                },
            );
            candidate
                .characters
                .insert(definition.id.clone(), definition);
        }
        for definition in pack.scenes {
            candidate.scenes.insert(definition.id.clone(), definition);
        }
        candidate.validate_all_references()?;
        candidate
            .pack_locks
            .insert(pack.lock.mod_id.clone(), pack.lock);
        self.published = candidate;
        Ok(())
    }
}

fn compile_character_definition(
    registry: &DefinitionRegistry,
    definition_id: &str,
) -> Result<CharacterSpawnSpec, ContentError> {
    let definition =
        registry
            .characters
            .get(definition_id)
            .ok_or_else(|| ContentError::MissingReference {
                from: "spawn".to_owned(),
                target: definition_id.to_owned(),
                expected: DefinitionKind::Character,
            })?;
    let origin = registry
        .character_origins
        .get(definition_id)
        .expect("validated Character Definition has an origin")
        .clone();
    Ok(CharacterSpawnSpec {
        origin: CharacterOrigin::Content(origin),
        display_name: definition.display_name.clone(),
        agent_profile_id: definition.agent_profile_id.clone(),
        placement: SceneTarget::Definition {
            definition_id: definition.scene_definition_id.clone(),
        },
        attribute_budget: definition.attribute_budget,
        attributes: definition.attributes.clone(),
        resources: definition.resources.clone(),
        conditions: definition.conditions.clone(),
        inventory: definition.inventory.clone(),
        skills: definition.skills.clone(),
        goals: definition.goals.clone(),
    })
}

fn compile_generated_draft(
    draft: NpcDraft,
    origin: GeneratedOrigin,
    policy: GenerationPolicy,
) -> CharacterSpawnSpec {
    CharacterSpawnSpec {
        origin: CharacterOrigin::Generated(origin),
        display_name: draft.display_name,
        agent_profile_id: draft.agent_profile_id,
        placement: SceneTarget::Object {
            object_id: draft.scene_object_id,
        },
        attribute_budget: policy.attribute_budget,
        attributes: draft.attributes,
        resources: draft.resources,
        conditions: draft.conditions,
        inventory: draft.inventory,
        skills: draft.skills,
        goals: draft.goals,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ObjectReference {
    LocalKey(String),
    Existing(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RelationshipInput {
    kind: String,
    target: ObjectReference,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SpawnEntry {
    local_key: String,
    character: CharacterSpawnSpec,
    relationships: Vec<RelationshipInput>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SceneSpawnPlan {
    entries: Vec<SpawnEntry>,
}

fn compile_scene_plan(
    registry: &DefinitionRegistry,
    scene_definition_id: &str,
) -> Result<SceneSpawnPlan, ContentError> {
    let scene =
        registry
            .scenes
            .get(scene_definition_id)
            .ok_or_else(|| ContentError::MissingReference {
                from: "scene_spawn".to_owned(),
                target: scene_definition_id.to_owned(),
                expected: DefinitionKind::Scene,
            })?;
    let mut entries = scene
        .initial_characters
        .iter()
        .map(|entry| {
            Ok(SpawnEntry {
                local_key: entry.local_key.clone(),
                character: compile_character_definition(registry, &entry.character_definition_id)?,
                relationships: entry
                    .relationships
                    .iter()
                    .map(|relationship| RelationshipInput {
                        kind: relationship.kind.clone(),
                        target: ObjectReference::LocalKey(relationship.target_local_key.clone()),
                    })
                    .collect(),
            })
        })
        .collect::<Result<Vec<_>, ContentError>>()?;
    entries.sort_by(|left, right| left.local_key.cmp(&right.local_key));
    Ok(SceneSpawnPlan { entries })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SceneInstance {
    object_id: String,
    definition_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct CharacterInstance {
    object_id: String,
    origin: CharacterOrigin,
    display_name: String,
    agent_profile_id: Option<String>,
    scene_object_id: String,
    attributes: BTreeMap<String, i32>,
    resources: BTreeMap<String, ResourceInput>,
    conditions: Vec<String>,
    inventory: Vec<String>,
    skills: Vec<String>,
    goals: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct RelationshipInstance {
    source_id: String,
    target_id: String,
    kind: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct WorldEvent {
    kind: String,
    object_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct TestWorld {
    next_character_id: u64,
    scenes: BTreeMap<String, SceneInstance>,
    characters: BTreeMap<String, CharacterInstance>,
    relationships: Vec<RelationshipInstance>,
    events: Vec<WorldEvent>,
}

impl TestWorld {
    fn seeded() -> Self {
        Self {
            next_character_id: 1,
            scenes: BTreeMap::from([(
                SCENE_OBJECT_ID.to_owned(),
                SceneInstance {
                    object_id: SCENE_OBJECT_ID.to_owned(),
                    definition_id: SCENE_DEFINITION_ID.to_owned(),
                },
            )]),
            characters: BTreeMap::new(),
            relationships: Vec::new(),
            events: Vec::new(),
        }
    }
}

struct NpcFactory<'a> {
    registry: &'a DefinitionRegistry,
}

impl NpcFactory<'_> {
    fn resolve_scene(
        &self,
        world: &TestWorld,
        target: &SceneTarget,
    ) -> Result<String, ContentError> {
        match target {
            SceneTarget::Object { object_id } => world
                .scenes
                .contains_key(object_id)
                .then(|| object_id.clone())
                .ok_or_else(|| ContentError::UnknownScene(object_id.clone())),
            SceneTarget::Definition { definition_id } => world
                .scenes
                .values()
                .find(|scene| &scene.definition_id == definition_id)
                .map(|scene| scene.object_id.clone())
                .ok_or_else(|| ContentError::UnknownScene(definition_id.clone())),
        }
    }

    fn validate_spec(
        &self,
        world: &TestWorld,
        spec: &CharacterSpawnSpec,
    ) -> Result<String, ContentError> {
        match &spec.origin {
            CharacterOrigin::Content(origin) => {
                let expected = self
                    .registry
                    .character_origins
                    .get(&origin.definition_id)
                    .ok_or_else(|| ContentError::MissingReference {
                        from: "content_origin".to_owned(),
                        target: origin.definition_id.clone(),
                        expected: DefinitionKind::Character,
                    })?;
                if expected != origin {
                    return Err(ContentError::ContentLockMismatch);
                }
            }
            CharacterOrigin::Generated(origin) => {
                if origin.generation_id.is_empty()
                    || origin.generator_version.is_empty()
                    || origin.source_event.is_empty()
                {
                    return Err(ContentError::InvalidId("generated_origin".to_owned()));
                }
            }
        }
        if let Some(profile) = &spec.agent_profile_id {
            self.registry
                .reference("spawn", profile, DefinitionKind::AgentProfile)?;
        }
        let mut used = 0_i32;
        for (id, value) in &spec.attributes {
            self.registry
                .reference("spawn", id, DefinitionKind::Attribute)?;
            let definition = self
                .registry
                .attributes
                .get(id)
                .expect("validated Attribute reference exists");
            if *value < definition.minimum || *value > definition.maximum {
                return Err(ContentError::InvalidAttributeRange(id.clone()));
            }
            used = used.saturating_add(*value);
        }
        if used > spec.attribute_budget {
            return Err(ContentError::AttributeBudgetExceeded {
                used,
                maximum: spec.attribute_budget,
            });
        }
        for (id, value) in &spec.resources {
            self.registry
                .reference("spawn", id, DefinitionKind::Resource)?;
            let definition = self
                .registry
                .resources
                .get(id)
                .expect("validated Resource reference exists");
            if value.base_maximum < definition.minimum
                || value.base_maximum > definition.maximum
                || value.current < definition.minimum
                || value.current > value.base_maximum
            {
                return Err(ContentError::InvalidResourceRange(id.clone()));
            }
        }
        for id in &spec.conditions {
            self.registry
                .reference("spawn", id, DefinitionKind::Condition)?;
        }
        for id in &spec.inventory {
            self.registry.reference("spawn", id, DefinitionKind::Item)?;
        }
        for id in &spec.skills {
            self.registry
                .reference("spawn", id, DefinitionKind::Skill)?;
        }
        for id in &spec.goals {
            self.registry.reference("spawn", id, DefinitionKind::Goal)?;
        }
        self.resolve_scene(world, &spec.placement)
    }

    fn spawn(
        &self,
        world: &mut TestWorld,
        mut plan: SceneSpawnPlan,
    ) -> Result<BTreeMap<String, String>, ContentError> {
        plan.entries
            .sort_by(|left, right| left.local_key.cmp(&right.local_key));
        let mut candidate = world.clone();
        let mut allocated = BTreeMap::new();

        for entry in &plan.entries {
            if allocated.contains_key(&entry.local_key) {
                return Err(ContentError::DuplicateLocalKey(entry.local_key.clone()));
            }
            let scene_object_id = self.validate_spec(&candidate, &entry.character)?;
            let object_id = format!("character/{:04}", candidate.next_character_id);
            candidate.next_character_id += 1;
            allocated.insert(entry.local_key.clone(), object_id.clone());
            candidate.characters.insert(
                object_id.clone(),
                CharacterInstance {
                    object_id: object_id.clone(),
                    origin: entry.character.origin.clone(),
                    display_name: entry.character.display_name.clone(),
                    agent_profile_id: entry.character.agent_profile_id.clone(),
                    scene_object_id,
                    attributes: entry.character.attributes.clone(),
                    resources: entry.character.resources.clone(),
                    conditions: entry.character.conditions.clone(),
                    inventory: entry.character.inventory.clone(),
                    skills: entry.character.skills.clone(),
                    goals: entry.character.goals.clone(),
                },
            );
        }

        for entry in &plan.entries {
            let source_id = allocated
                .get(&entry.local_key)
                .expect("first phase allocated every entry")
                .clone();
            for relationship in &entry.relationships {
                let target_id = match &relationship.target {
                    ObjectReference::LocalKey(key) => {
                        allocated.get(key).cloned().ok_or_else(|| {
                            ContentError::MissingLocalReference {
                                scene: "spawn_plan".to_owned(),
                                key: key.clone(),
                            }
                        })?
                    }
                    ObjectReference::Existing(object_id) => candidate
                        .characters
                        .contains_key(object_id)
                        .then(|| object_id.clone())
                        .ok_or_else(|| ContentError::UnknownObject(object_id.clone()))?,
                };
                if source_id == target_id {
                    return Err(ContentError::SelfRelationship(source_id));
                }
                candidate.relationships.push(RelationshipInstance {
                    source_id: source_id.clone(),
                    target_id,
                    kind: relationship.kind.clone(),
                });
            }
            candidate.events.push(WorldEvent {
                kind: "character_spawned".to_owned(),
                object_id: source_id,
            });
        }

        *world = candidate;
        Ok(allocated)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SaveImage {
    mod_lock: BTreeMap<String, PackLock>,
    world: TestWorld,
}

fn save_world(world: &TestWorld, registry: &DefinitionRegistry) -> String {
    serde_json::to_string(&SaveImage {
        mod_lock: registry.pack_locks.clone(),
        world: world.clone(),
    })
    .expect("test world serializes")
}

fn load_world(serialized: &str, registry: &DefinitionRegistry) -> Result<TestWorld, ContentError> {
    let image: SaveImage = serde_json::from_str(serialized)
        .map_err(|error| ContentError::InvalidSave(error.to_string()))?;
    if image.mod_lock != registry.pack_locks {
        return Err(ContentError::ContentLockMismatch);
    }
    for character in image.world.characters.values() {
        if let CharacterOrigin::Content(origin) = &character.origin {
            let expected = registry
                .character_origins
                .get(&origin.definition_id)
                .ok_or(ContentError::ContentLockMismatch)?;
            if expected != origin {
                return Err(ContentError::ContentLockMismatch);
            }
        }
    }
    Ok(image.world)
}

#[derive(Clone, Debug)]
struct NpcGenerationRequest {
    scene_object_id: String,
    generation_id: String,
    source_event: String,
}

struct CountingGenerator {
    calls: AtomicUsize,
}

impl CountingGenerator {
    fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
        }
    }

    fn generate(&self, request: &NpcGenerationRequest) -> (NpcDraft, GeneratedOrigin) {
        self.calls.fetch_add(1, Ordering::SeqCst);
        (
            NpcDraft {
                display_name: "Mira".to_owned(),
                agent_profile_id: Some("base/agent/villager".to_owned()),
                scene_object_id: request.scene_object_id.clone(),
                attributes: BTreeMap::from([
                    ("base/attribute/wits".to_owned(), 4),
                    ("base/attribute/will".to_owned(), 3),
                ]),
                resources: BTreeMap::from([(
                    "base/resource/stamina".to_owned(),
                    ResourceInput {
                        current: 6,
                        base_maximum: 8,
                    },
                )]),
                conditions: vec!["base/condition/alert".to_owned()],
                inventory: vec!["base/item/bell-key".to_owned()],
                skills: vec!["base/skill/listen".to_owned()],
                goals: vec!["base/goal/protect-mill".to_owned()],
            },
            GeneratedOrigin {
                generation_id: request.generation_id.clone(),
                generator_version: "fixture-generator/1".to_owned(),
                source_event: request.source_event.clone(),
            },
        )
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

fn marker(id: &str) -> MarkerDefinition {
    MarkerDefinition { id: id.to_owned() }
}

fn character(id: &str, name: &str) -> CharacterDefinition {
    CharacterDefinition {
        id: id.to_owned(),
        display_name: name.to_owned(),
        agent_profile_id: Some("base/agent/villager".to_owned()),
        scene_definition_id: SCENE_DEFINITION_ID.to_owned(),
        attribute_budget: 10,
        attributes: BTreeMap::from([
            ("base/attribute/wits".to_owned(), 4),
            ("base/attribute/will".to_owned(), 3),
        ]),
        resources: BTreeMap::from([(
            "base/resource/stamina".to_owned(),
            ResourceInput {
                current: 6,
                base_maximum: 8,
            },
        )]),
        conditions: vec!["base/condition/alert".to_owned()],
        inventory: vec!["base/item/bell-key".to_owned()],
        skills: vec!["base/skill/listen".to_owned()],
        goals: vec!["base/goal/protect-mill".to_owned()],
    }
}

fn base_pack() -> ContentPack {
    ContentPack {
        lock: PackLock {
            mod_id: "games.loreloom.base".to_owned(),
            mod_version: "1.0.0".to_owned(),
            pack_id: "base/content".to_owned(),
            content_version: 1,
            content_hash: "sha256:base-fixture".to_owned(),
        },
        agent_profiles: vec![AgentProfileDefinition {
            id: "base/agent/villager".to_owned(),
        }],
        attributes: vec![
            AttributeDefinition {
                id: "base/attribute/will".to_owned(),
                minimum: 0,
                maximum: 10,
            },
            AttributeDefinition {
                id: "base/attribute/wits".to_owned(),
                minimum: 0,
                maximum: 10,
            },
        ],
        resources: vec![ResourceDefinition {
            id: "base/resource/stamina".to_owned(),
            minimum: 0,
            maximum: 20,
        }],
        conditions: vec![marker("base/condition/alert")],
        items: vec![marker("base/item/bell-key")],
        skills: vec![marker("base/skill/listen")],
        goals: vec![marker("base/goal/protect-mill")],
        characters: vec![
            character("base/character/tomas", "Tomas"),
            character("base/character/mira", "Mira"),
        ],
        scenes: vec![SceneDefinition {
            id: SCENE_DEFINITION_ID.to_owned(),
            initial_characters: vec![
                SceneCharacterDefinition {
                    local_key: "tomas".to_owned(),
                    character_definition_id: "base/character/tomas".to_owned(),
                    relationships: Vec::new(),
                },
                SceneCharacterDefinition {
                    local_key: "mira".to_owned(),
                    character_definition_id: "base/character/mira".to_owned(),
                    relationships: vec![RelationshipTemplate {
                        kind: "trusts".to_owned(),
                        target_local_key: "tomas".to_owned(),
                    }],
                },
            ],
        }],
    }
}

fn loaded_registry() -> DefinitionRegistry {
    let mut loader = RegistryLoader::default();
    loader.import(base_pack()).expect("base pack validates");
    loader.published
}

#[test]
fn pack_load_is_two_phase_deterministic_and_atomic_on_reference_failure() {
    let mut loader = RegistryLoader::default();
    let mut pack = base_pack();
    pack.characters.reverse();
    pack.attributes.reverse();
    loader
        .import(pack)
        .expect("forward references resolve after collection");
    let published = loader.published().clone();

    let mut other_loader = RegistryLoader::default();
    other_loader
        .import(base_pack())
        .expect("original order also validates");
    assert_eq!(
        published.canonical_ids(),
        other_loader.published().canonical_ids()
    );
    assert_eq!(
        compile_scene_plan(&published, SCENE_DEFINITION_ID).expect("reordered pack compiles"),
        compile_scene_plan(other_loader.published(), SCENE_DEFINITION_ID)
            .expect("original pack compiles")
    );

    let mut invalid = base_pack();
    invalid.lock.mod_id = "games.loreloom.invalid".to_owned();
    invalid.lock.pack_id = "invalid/content".to_owned();
    invalid.lock.content_hash = "sha256:invalid".to_owned();
    invalid.characters = vec![character("invalid/character/ghost", "Ghost")];
    invalid.characters[0].skills = vec!["invalid/skill/missing".to_owned()];
    invalid.agent_profiles.clear();
    invalid.attributes.clear();
    invalid.resources.clear();
    invalid.conditions.clear();
    invalid.items.clear();
    invalid.skills.clear();
    invalid.goals.clear();
    invalid.scenes.clear();
    let before = loader.published().clone();
    assert!(matches!(
        loader.import(invalid),
        Err(ContentError::MissingReference {
            expected: DefinitionKind::Skill,
            ..
        })
    ));
    assert_eq!(loader.published(), &before);

    let mut wrong_kind = base_pack();
    wrong_kind.lock.mod_id = "games.loreloom.wrong-kind".to_owned();
    wrong_kind.lock.pack_id = "wrong-kind/content".to_owned();
    wrong_kind.lock.content_hash = "sha256:wrong-kind".to_owned();
    wrong_kind.characters = vec![character("wrong-kind/character/ghost", "Ghost")];
    wrong_kind.characters[0].skills = vec!["base/item/bell-key".to_owned()];
    wrong_kind.agent_profiles.clear();
    wrong_kind.attributes.clear();
    wrong_kind.resources.clear();
    wrong_kind.conditions.clear();
    wrong_kind.items.clear();
    wrong_kind.skills.clear();
    wrong_kind.goals.clear();
    wrong_kind.scenes.clear();
    assert!(matches!(
        loader.import(wrong_kind),
        Err(ContentError::WrongReferenceKind {
            expected: DefinitionKind::Skill,
            actual: DefinitionKind::Item,
            ..
        })
    ));
    assert_eq!(loader.published(), &before);

    let mut duplicate = base_pack();
    duplicate.lock.mod_id = "games.loreloom.duplicate".to_owned();
    duplicate.lock.pack_id = "duplicate/content".to_owned();
    duplicate.characters.clear();
    duplicate.scenes.clear();
    duplicate.attributes.clear();
    duplicate.resources.clear();
    duplicate.conditions.clear();
    duplicate.items.clear();
    duplicate.skills.clear();
    duplicate.goals.clear();
    duplicate.agent_profiles = vec![AgentProfileDefinition {
        id: "base/item/bell-key".to_owned(),
    }];
    assert!(matches!(
        loader.import(duplicate),
        Err(ContentError::DuplicateDefinition(id)) if id == "base/item/bell-key"
    ));
    assert_eq!(loader.published(), &before);
}

#[test]
fn preset_and_generated_inputs_share_spawn_spec_and_factory_validation() {
    let registry = loaded_registry();
    let preset =
        compile_character_definition(&registry, "base/character/mira").expect("preset compiles");
    let generator = CountingGenerator::new();
    let request = NpcGenerationRequest {
        scene_object_id: SCENE_OBJECT_ID.to_owned(),
        generation_id: "generation/1".to_owned(),
        source_event: "event/player-arrived".to_owned(),
    };
    let (draft, origin) = generator.generate(&request);
    let generated = compile_generated_draft(
        draft,
        origin,
        GenerationPolicy {
            attribute_budget: 10,
        },
    );

    assert_eq!(preset.display_name, generated.display_name);
    assert_eq!(preset.agent_profile_id, generated.agent_profile_id);
    assert_eq!(preset.attributes, generated.attributes);
    assert_eq!(preset.resources, generated.resources);
    assert_eq!(preset.conditions, generated.conditions);
    assert_eq!(preset.inventory, generated.inventory);
    assert_eq!(preset.skills, generated.skills);
    assert_eq!(preset.goals, generated.goals);

    let factory = NpcFactory {
        registry: &registry,
    };
    let mut world = TestWorld::seeded();
    let ids = factory
        .spawn(
            &mut world,
            SceneSpawnPlan {
                entries: vec![
                    SpawnEntry {
                        local_key: "preset".to_owned(),
                        character: preset,
                        relationships: Vec::new(),
                    },
                    SpawnEntry {
                        local_key: "generated".to_owned(),
                        character: generated,
                        relationships: Vec::new(),
                    },
                ],
            },
        )
        .expect("both origins pass one Factory path");
    assert_eq!(ids.len(), 2);
    assert_eq!(world.characters.len(), 2);
    assert_eq!(world.events.len(), 2);
    assert!(
        world
            .events
            .iter()
            .all(|event| event.kind == "character_spawned")
    );
    let origins = world
        .characters
        .values()
        .map(|character| &character.origin)
        .collect::<Vec<_>>();
    assert!(
        origins
            .iter()
            .any(|origin| matches!(origin, CharacterOrigin::Content(_)))
    );
    assert!(
        origins
            .iter()
            .any(|origin| matches!(origin, CharacterOrigin::Generated(_)))
    );

    let mut invalid =
        compile_character_definition(&registry, "base/character/mira").expect("preset compiles");
    invalid.origin = CharacterOrigin::Generated(GeneratedOrigin {
        generation_id: "generation/invalid".to_owned(),
        generator_version: "fixture-generator/1".to_owned(),
        source_event: "event/invalid".to_owned(),
    });
    invalid.attribute_budget = 2;
    let before = world.clone();
    assert!(matches!(
        factory.spawn(
            &mut world,
            SceneSpawnPlan {
                entries: vec![SpawnEntry {
                    local_key: "invalid".to_owned(),
                    character: invalid,
                    relationships: Vec::new(),
                }],
            },
        ),
        Err(ContentError::AttributeBudgetExceeded { .. })
    ));
    assert_eq!(world, before);
}

#[test]
fn factory_allocates_all_ids_before_relationship_resolution_and_rolls_back() {
    let registry = loaded_registry();
    let factory = NpcFactory {
        registry: &registry,
    };
    let plan = compile_scene_plan(&registry, SCENE_DEFINITION_ID).expect("scene plan compiles");
    assert_eq!(
        plan.entries
            .iter()
            .map(|entry| entry.local_key.as_str())
            .collect::<Vec<_>>(),
        vec!["mira", "tomas"]
    );
    let mut world = TestWorld::seeded();
    let allocated = factory
        .spawn(&mut world, plan)
        .expect("forward local relationship resolves after allocation");
    assert_eq!(allocated["mira"], "character/0001");
    assert_eq!(allocated["tomas"], "character/0002");
    assert_eq!(world.relationships.len(), 1);
    assert_eq!(world.relationships[0].source_id, "character/0001");
    assert_eq!(world.relationships[0].target_id, "character/0002");

    let mut broken =
        compile_character_definition(&registry, "base/character/mira").expect("character compiles");
    broken.display_name = "Broken".to_owned();
    let bad_plan = SceneSpawnPlan {
        entries: vec![SpawnEntry {
            local_key: "broken".to_owned(),
            character: broken,
            relationships: vec![RelationshipInput {
                kind: "knows".to_owned(),
                target: ObjectReference::LocalKey("absent".to_owned()),
            }],
        }],
    };
    let before = world.clone();
    assert!(matches!(
        factory.spawn(&mut world, bad_plan),
        Err(ContentError::MissingLocalReference { key, .. }) if key == "absent"
    ));
    assert_eq!(world, before);
    assert_eq!(world.next_character_id, 3);
}

#[test]
fn existing_object_relationship_is_checked_inside_the_same_candidate() {
    let registry = loaded_registry();
    let factory = NpcFactory {
        registry: &registry,
    };
    let mut world = TestWorld::seeded();
    let first = compile_character_definition(&registry, "base/character/tomas")
        .expect("first preset compiles");
    factory
        .spawn(
            &mut world,
            SceneSpawnPlan {
                entries: vec![SpawnEntry {
                    local_key: "existing".to_owned(),
                    character: first,
                    relationships: Vec::new(),
                }],
            },
        )
        .expect("seed character spawns");
    let second = compile_character_definition(&registry, "base/character/mira")
        .expect("second preset compiles");
    factory
        .spawn(
            &mut world,
            SceneSpawnPlan {
                entries: vec![SpawnEntry {
                    local_key: "new".to_owned(),
                    character: second,
                    relationships: vec![RelationshipInput {
                        kind: "knows".to_owned(),
                        target: ObjectReference::Existing("character/0001".to_owned()),
                    }],
                }],
            },
        )
        .expect("existing Stable ObjectId resolves");
    assert_eq!(world.relationships[0].target_id, "character/0001");

    let third = compile_character_definition(&registry, "base/character/mira")
        .expect("third preset compiles");
    let before = world.clone();
    assert!(matches!(
        factory.spawn(
            &mut world,
            SceneSpawnPlan {
                entries: vec![SpawnEntry {
                    local_key: "missing-target".to_owned(),
                    character: third,
                    relationships: vec![RelationshipInput {
                        kind: "knows".to_owned(),
                        target: ObjectReference::Existing("character/missing".to_owned()),
                    }],
                }],
            },
        ),
        Err(ContentError::UnknownObject(id)) if id == "character/missing"
    ));
    assert_eq!(world, before);
}

#[test]
fn generated_origin_and_complete_state_restore_without_generator_call() {
    let registry = loaded_registry();
    let factory = NpcFactory {
        registry: &registry,
    };
    let generator = CountingGenerator::new();
    let request = NpcGenerationRequest {
        scene_object_id: SCENE_OBJECT_ID.to_owned(),
        generation_id: "generation/persisted".to_owned(),
        source_event: "event/runtime-generation".to_owned(),
    };
    let (draft, origin) = generator.generate(&request);
    let spec = compile_generated_draft(
        draft,
        origin.clone(),
        GenerationPolicy {
            attribute_budget: 10,
        },
    );
    let mut world = TestWorld::seeded();
    factory
        .spawn(
            &mut world,
            SceneSpawnPlan {
                entries: vec![SpawnEntry {
                    local_key: "generated".to_owned(),
                    character: spec,
                    relationships: Vec::new(),
                }],
            },
        )
        .expect("generated NPC spawns");
    assert_eq!(generator.calls(), 1);

    let serialized = save_world(&world, &registry);
    let restored = load_world(&serialized, &registry).expect("save lock matches");
    assert_eq!(generator.calls(), 1);
    assert_eq!(restored, world);
    assert_eq!(
        restored.characters["character/0001"].origin,
        CharacterOrigin::Generated(origin)
    );
    assert_eq!(
        restored.characters["character/0001"].inventory,
        vec!["base/item/bell-key"]
    );
    assert_eq!(
        restored.characters["character/0001"].skills,
        vec!["base/skill/listen"]
    );
}

#[test]
fn save_content_lock_mismatch_is_rejected_instead_of_using_new_definitions() {
    let registry = loaded_registry();
    let factory = NpcFactory {
        registry: &registry,
    };
    let mut world = TestWorld::seeded();
    factory
        .spawn(
            &mut world,
            SceneSpawnPlan {
                entries: vec![SpawnEntry {
                    local_key: "mira".to_owned(),
                    character: compile_character_definition(&registry, "base/character/mira")
                        .expect("preset compiles"),
                    relationships: Vec::new(),
                }],
            },
        )
        .expect("preset spawns");
    let serialized = save_world(&world, &registry);

    let mut changed_registry = registry.clone();
    changed_registry
        .pack_locks
        .get_mut("games.loreloom.base")
        .expect("base lock exists")
        .content_hash = "sha256:different-content".to_owned();
    assert_eq!(
        load_world(&serialized, &changed_registry),
        Err(ContentError::ContentLockMismatch)
    );
    assert_eq!(
        load_world(&serialized, &registry).expect("unchanged lock restores"),
        world
    );
}
