use std::collections::BTreeMap;

use image::{ImageEncoder, Rgba, RgbaImage};
use loreloom_appearance::{APPEARANCE_PACK_PATH, AppearanceCatalog, AppearanceError};
use loreloom_core::{AppearanceValue, AppearanceView, ContentDefinitionId, ModId, Revision};

fn id(kind: &str, name: &str) -> ContentDefinitionId {
    ContentDefinitionId::parse(format!("games.test.appearance:{kind}/{name}")).expect("test ID")
}

fn png(image: &RgbaImage) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            image::ExtendedColorType::Rgba8,
        )
        .expect("encode PNG");
    bytes
}

fn pack_source() -> String {
    r#"
schema_version = 1
pack_id = "games.test.appearance:appearance_pack/main"

[[models]]
id = "games.test.appearance:appearance_model/player"
canvas_width = 2
canvas_height = 1

[[models.parameters]]
id = "games.test.appearance:appearance_parameter/eye"
value = { type = "color", rgb = [20, 40, 60] }

[[models.parameters]]
id = "games.test.appearance:appearance_parameter/blush"
value = { type = "boolean", value = false }

[[models.frames]]
name = "awake"
duration_ms = 950

[[models.frames.layers]]
name = "iris"
z_index = 10
source = "appearance/images/iris.png"
mask = "appearance/images/iris-mask.png"
tint = { type = "parameter", id = "games.test.appearance:appearance_parameter/eye" }

[[models.frames.layers]]
name = "blush"
z_index = 20
source = "appearance/images/blush.png"
blend = "screen"
when = { parameter = "games.test.appearance:appearance_parameter/blush", equals = { type = "boolean", value = true } }

[[models.frames]]
name = "blink"
duration_ms = 150

[[models.frames.layers]]
name = "closed"
z_index = 10
source = "appearance/images/closed.png"
"#
    .to_owned()
}

fn catalog() -> AppearanceCatalog {
    let namespace = ModId::parse("games.test.appearance").expect("namespace");
    let iris = png(&RgbaImage::from_pixel(2, 1, Rgba([255, 255, 255, 255])));
    let mut iris_mask = RgbaImage::from_pixel(2, 1, Rgba([0, 0, 0, 255]));
    iris_mask.put_pixel(1, 0, Rgba([0, 0, 0, 0]));
    let iris_mask = png(&iris_mask);
    let blush = png(&RgbaImage::from_pixel(2, 1, Rgba([255, 0, 0, 128])));
    let closed = png(&RgbaImage::from_pixel(2, 1, Rgba([2, 3, 4, 255])));
    let pack = pack_source().into_bytes();
    AppearanceCatalog::compile([
        (&namespace, APPEARANCE_PACK_PATH, pack.as_slice()),
        (&namespace, "appearance/images/iris.png", iris.as_slice()),
        (
            &namespace,
            "appearance/images/iris-mask.png",
            iris_mask.as_slice(),
        ),
        (&namespace, "appearance/images/blush.png", blush.as_slice()),
        (
            &namespace,
            "appearance/images/closed.png",
            closed.as_slice(),
        ),
    ])
    .expect("compile catalog")
}

#[test]
fn composes_tint_predicate_and_synchronized_frames() {
    let catalog = catalog();
    let appearance = AppearanceView {
        revision: Revision::ZERO,
        model_id: id("appearance_model", "player"),
        parameters: BTreeMap::from([
            (
                id("appearance_parameter", "eye"),
                AppearanceValue::Color { rgb: [40, 80, 120] },
            ),
            (
                id("appearance_parameter", "blush"),
                AppearanceValue::Boolean { value: true },
            ),
        ]),
    };
    let composed = catalog.compose(&appearance).expect("compose");
    assert_eq!(composed.frames.len(), 2);
    assert_eq!(composed.frames[0].duration_ms, 950);
    assert_ne!(composed.frames[0].image, composed.frames[1].image);
    assert_eq!(
        composed.frames[0].image.get_pixel(0, 0).0,
        [148, 80, 120, 255]
    );
    assert_eq!(composed.frames[0].image.get_pixel(1, 0).0, [255, 0, 0, 128]);
    assert_eq!(composed.frames[1].image.get_pixel(0, 0).0, [2, 3, 4, 255]);
    assert_eq!(catalog.render_key(&appearance).expect("key"), composed.key);
}

#[test]
fn render_key_ignores_world_revision_but_changes_with_parameters() {
    let catalog = catalog();
    let mut appearance = AppearanceView {
        revision: Revision::ZERO,
        model_id: id("appearance_model", "player"),
        parameters: BTreeMap::new(),
    };
    let first = catalog.render_key(&appearance).expect("first key");
    appearance.revision = Revision::new(99);
    assert_eq!(first, catalog.render_key(&appearance).expect("same key"));
    appearance.parameters.insert(
        id("appearance_parameter", "blush"),
        AppearanceValue::Boolean { value: true },
    );
    assert_ne!(first, catalog.render_key(&appearance).expect("changed key"));
}

#[test]
fn rejects_unsafe_images_and_unknown_parameters() {
    let namespace = ModId::parse("games.test.appearance").expect("namespace");
    let source = pack_source().replace("appearance/images/iris.png", "../iris.png");
    assert_eq!(
        AppearanceCatalog::compile([(&namespace, APPEARANCE_PACK_PATH, source.as_bytes())])
            .expect_err("unsafe path"),
        AppearanceError::UnsafePath
    );

    let source = pack_source();
    let iris = png(&RgbaImage::from_pixel(2, 1, Rgba([255; 4])));
    assert_eq!(
        AppearanceCatalog::compile([
            (&namespace, APPEARANCE_PACK_PATH, source.as_bytes()),
            (&namespace, "appearance/images/iris.png", iris.as_slice()),
        ])
        .expect_err("missing mask"),
        AppearanceError::MissingImage
    );

    let too_tall = png(&RgbaImage::from_pixel(2, 513, Rgba([255; 4])));
    let oversized = source.replace("appearance/images/iris.png", "appearance/images/tall.png");
    assert_eq!(
        AppearanceCatalog::compile([
            (&namespace, APPEARANCE_PACK_PATH, oversized.as_bytes()),
            (
                &namespace,
                "appearance/images/tall.png",
                too_tall.as_slice(),
            ),
        ])
        .expect_err("oversized decoded image"),
        AppearanceError::InvalidImage
    );

    let catalog = catalog();
    let appearance = AppearanceView {
        revision: Revision::ZERO,
        model_id: id("appearance_model", "player"),
        parameters: BTreeMap::from([(
            id("appearance_parameter", "unknown"),
            AppearanceValue::Boolean { value: true },
        )]),
    };
    assert_eq!(
        catalog.compose(&appearance).expect_err("unknown parameter"),
        AppearanceError::InvalidParameter
    );
}
