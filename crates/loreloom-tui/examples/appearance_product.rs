//! Local visual smoke test for the product Appearance catalog and real TUI layout.
//!
//! This adapter reads an already unpacked Goose image pack from `.local/`; it never downloads,
//! copies, or publishes those third-party files.

use std::{
    collections::BTreeMap,
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
};

use loreloom_appearance::{
    APPEARANCE_PACK_PATH, AppearanceCatalog, AppearanceFrame, AppearanceLayer, AppearanceModel,
    AppearancePack, AppearanceParameterDefault, AppearanceTint, BlendMode,
};
use loreloom_core::{
    ActionState, AppearanceValue, AppearanceView, CharacterContext, CharacterProfile, DisplayName,
    LifeState, ModId, PackageCatalogView, Posture, Revision, RuntimePhase, SceneContext, ShortText,
    TranscriptWindow, UiSnapshot, WorldPackageView, WorldTime,
};
use loreloom_tui::{
    ImageProtocolPreference, RuntimeClient, RuntimeUiEvent, TuiConfig, UiClientError,
    run_with_appearance,
};

const DEFAULT_GOOSE_SOURCE: &str = concat!(
    ".local/appearance-spike/source/Goose/",
    "degrees-of-lewdity-plus-master-imagepacks-goosefem/imagepacks/goosefem"
);

fn main() -> Result<(), Box<dyn Error>> {
    let (source, protocol) = arguments()?;
    let catalog = goose_catalog(&source)?;
    let mut client = IdleClient;
    run_with_appearance(
        &mut client,
        snapshot(),
        TuiConfig {
            image_protocol: protocol,
            ..TuiConfig::default()
        },
        catalog,
    )?;
    Ok(())
}

fn arguments() -> Result<(PathBuf, ImageProtocolPreference), Box<dyn Error>> {
    let mut source = PathBuf::from(DEFAULT_GOOSE_SOURCE);
    let mut protocol = ImageProtocolPreference::Auto;
    let mut args = env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--goose-source" => {
                source = args.next().ok_or("--goose-source requires a path")?.into();
            }
            "--protocol" => {
                protocol = match args.next().as_deref() {
                    Some("auto") => ImageProtocolPreference::Auto,
                    Some("kitty") => ImageProtocolPreference::Kitty,
                    Some("iterm2") => ImageProtocolPreference::Iterm2,
                    Some("sixel") => ImageProtocolPreference::Sixel,
                    Some("halfblocks") => ImageProtocolPreference::Halfblocks,
                    Some("disabled") => ImageProtocolPreference::Disabled,
                    _ => {
                        return Err(
                            "--protocol expects auto|kitty|iterm2|sixel|halfblocks|disabled".into(),
                        );
                    }
                };
            }
            "--help" | "-h" => {
                println!(
                    "Usage: cargo run -p loreloom-tui --example appearance_product -- [--goose-source PATH] [--protocol auto|kitty|iterm2|sixel|halfblocks|disabled]"
                );
                std::process::exit(0);
            }
            _ => return Err("unknown appearance product argument".into()),
        }
    }
    if !source.is_dir() {
        return Err(format!(
            "Goose source is unavailable at {}; unpack it under the ignored .local directory or pass --goose-source",
            source.display()
        )
        .into());
    }
    Ok((source, protocol))
}

fn goose_catalog(source: &Path) -> Result<AppearanceCatalog, Box<dyn Error>> {
    let namespace = ModId::parse("games.loreloom.local-goose")?;
    let pack = AppearancePack {
        schema_version: 1,
        pack_id: definition("appearance_pack", "main"),
        models: vec![AppearanceModel {
            id: definition("appearance_model", "player"),
            canvas_width: 256,
            canvas_height: 256,
            parameters: vec![
                color_default("hair", [98, 54, 39]),
                color_default("shirt", [116, 54, 106]),
                color_default("shorts", [49, 61, 82]),
                color_default("left_eye", [59, 155, 101]),
                color_default("right_eye", [61, 128, 193]),
            ],
            frames: vec![goose_frame(0, "awake", 950), goose_frame(1, "blink", 150)],
        }],
    };
    let pack_bytes = toml::to_string(&pack)?.into_bytes();
    let paths = [
        "body/base-head.png",
        "hair/sides/default/short.png",
        "body/basenoarms-f.png",
        "face/default/eyes.png",
        "face/default/sclera.png",
        "face/default/iris-left.png",
        "face/default/iris-right.png",
        "face/default/eyelids.png",
        "face/default/lashes.png",
        "face/default/mouthsmile.png",
        "body/leftarmidle-f.png",
        "body/rightarmidle-f.png",
        "clothes/lower/shorts/full.png",
        "clothes/upper/tshirt/left-idle.png",
        "clothes/upper/tshirt/right-idle.png",
        "clothes/upper/tshirt/full.png",
        "hair/fringe/default/short.png",
        "face/default/brow-mid.png",
    ];
    let mut resources = vec![(
        namespace.clone(),
        APPEARANCE_PACK_PATH.to_owned(),
        pack_bytes,
    )];
    for path in paths {
        resources.push((
            namespace.clone(),
            virtual_path(path),
            fs::read(source.join(path))?,
        ));
    }
    Ok(AppearanceCatalog::compile(resources.iter().map(
        |(namespace, path, bytes)| (namespace, path.as_str(), bytes.as_slice()),
    ))?)
}

fn goose_frame(source_frame: u32, name: &str, duration_ms: u32) -> AppearanceFrame {
    let layers = [
        ("base_head", 5, "body/base-head.png", None, true),
        (
            "hair_sides",
            10,
            "hair/sides/default/short.png",
            Some("hair"),
            true,
        ),
        ("body", 20, "body/basenoarms-f.png", None, true),
        ("eyes", 30, "face/default/eyes.png", None, false),
        ("sclera", 31, "face/default/sclera.png", None, false),
        (
            "left_iris",
            32,
            "face/default/iris-left.png",
            Some("left_eye"),
            true,
        ),
        (
            "right_iris",
            32,
            "face/default/iris-right.png",
            Some("right_eye"),
            true,
        ),
        ("eyelids", 34, "face/default/eyelids.png", None, true),
        ("lashes", 35, "face/default/lashes.png", None, true),
        ("smile", 40, "face/default/mouthsmile.png", None, false),
        ("left_arm", 50, "body/leftarmidle-f.png", None, true),
        ("right_arm", 50, "body/rightarmidle-f.png", None, true),
        (
            "shorts",
            90,
            "clothes/lower/shorts/full.png",
            Some("shorts"),
            false,
        ),
        (
            "left_sleeve",
            94,
            "clothes/upper/tshirt/left-idle.png",
            Some("shirt"),
            true,
        ),
        (
            "right_sleeve",
            94,
            "clothes/upper/tshirt/right-idle.png",
            Some("shirt"),
            true,
        ),
        (
            "shirt",
            95,
            "clothes/upper/tshirt/full.png",
            Some("shirt"),
            true,
        ),
        (
            "hair_fringe",
            133,
            "hair/fringe/default/short.png",
            Some("hair"),
            true,
        ),
        (
            "brows",
            138,
            "face/default/brow-mid.png",
            Some("hair"),
            false,
        ),
    ]
    .into_iter()
    .map(|(name, z_index, source, tint, animated)| AppearanceLayer {
        name: name.to_owned(),
        z_index,
        source: virtual_path(source),
        source_frame: if animated { source_frame } else { 0 },
        mask: None,
        mask_frame: None,
        tint: tint.map(|name| AppearanceTint::Parameter {
            id: definition("appearance_parameter", name),
        }),
        opacity: u8::MAX,
        blend: BlendMode::SourceOver,
        when: None,
    })
    .collect();
    AppearanceFrame {
        name: name.to_owned(),
        duration_ms,
        layers,
    }
}

fn virtual_path(path: &str) -> String {
    format!("appearance/images/{path}")
}

fn color_default(name: &str, rgb: [u8; 3]) -> AppearanceParameterDefault {
    AppearanceParameterDefault {
        id: definition("appearance_parameter", name),
        value: AppearanceValue::Color { rgb },
    }
}

fn definition(kind: &str, name: &str) -> loreloom_core::ContentDefinitionId {
    loreloom_core::ContentDefinitionId::parse(format!("games.loreloom.local-goose:{kind}/{name}"))
        .expect("local demo Definition ID")
}

fn snapshot() -> UiSnapshot {
    let actor = parse("obj_01890f6a-2b3c-7d4e-8f90-123456789abc");
    let scene = parse("obj_01890f6a-2b3d-7d4e-8f90-123456789abc");
    let place = parse("obj_01890f6a-2b3e-7d4e-8f90-123456789abc");
    UiSnapshot {
        revision: Revision::ZERO,
        session_id: parse("ses_01890f6a-2b3f-7d4e-8f90-123456789abc"),
        player: CharacterContext {
            actor_id: actor,
            revision: Revision::ZERO,
            display_name: display("Goose 本地外观演示"),
            profile: CharacterProfile {
                summary: text("正式 Appearance Catalog 与产品 TUI 布局。"),
                values: Vec::new(),
                speaking_style: text("Visual smoke test."),
                narrative_tags: Default::default(),
            },
            appearance: Some(AppearanceView {
                revision: Revision::ZERO,
                model_id: definition("appearance_model", "player"),
                parameters: BTreeMap::new(),
            }),
            location_id: place,
            attributes: Vec::new(),
            resources: Vec::new(),
            conditions: Vec::new(),
            inventory: Vec::new(),
            skills: Vec::new(),
            known_facts: Vec::new(),
            goals: Vec::new(),
            life_state: LifeState::Alive,
            action_state: ActionState::Idle,
            posture: Posture::Standing,
        },
        scene: SceneContext {
            scene_id: scene,
            revision: Revision::ZERO,
            display_name: display("Appearance integration"),
            framing: text("Local visual review without model or world mutation."),
            place_id: place,
            place_name: display("TUI product layout"),
            adjacent_places: Vec::new(),
            clock: WorldTime::ZERO,
            visible_actors: Vec::new(),
            recent_events: Vec::new(),
        },
        parameters: Vec::new(),
        active_events: Vec::new(),
        packages: PackageCatalogView {
            world: WorldPackageView {
                world_id: ModId::parse("games.loreloom.local-goose").expect("world ID"),
                version: "0.1.0".parse().expect("version"),
            },
            mods: Vec::new(),
            unavailable_installed: 0,
        },
        transcript: TranscriptWindow {
            items: Vec::new(),
            before_cursor: None,
        },
        tool_activity: Vec::new(),
        phase: RuntimePhase::Idle,
        can_submit: false,
        can_cancel: false,
        waiting: false,
        notices: Vec::new(),
        supporting_events: Vec::new(),
    }
}

fn parse<T>(value: &str) -> T
where
    T: std::str::FromStr,
    T::Err: std::fmt::Debug,
{
    value.parse().expect("fixture ID")
}

fn display(value: &str) -> DisplayName {
    DisplayName::new(value).expect("display name")
}

fn text(value: &str) -> ShortText {
    ShortText::new(value).expect("short text")
}

struct IdleClient;

impl RuntimeClient for IdleClient {
    fn submit(&mut self, _input: String) -> Result<(), UiClientError> {
        Err(UiClientError::new("demo_read_only"))
    }

    fn cancel(&mut self) -> Result<(), UiClientError> {
        Ok(())
    }

    fn try_recv(&mut self) -> Result<Option<RuntimeUiEvent>, UiClientError> {
        Ok(None)
    }

    fn shutdown(&mut self) -> Result<(), UiClientError> {
        Ok(())
    }
}
