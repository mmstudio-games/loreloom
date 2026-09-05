//! Local-only dynamic paper-doll and terminal image spike.
//!
//! The example can consume an unpacked DoL Plus `img/` directory, but no third-party assets are
//! part of the repository. Run without `--terminal` to write visual review PNGs, or with
//! `--terminal` to exercise ratatui-image's native protocol picker in a real TTY.

use std::{
    collections::hash_map::DefaultHasher,
    env,
    error::Error,
    hash::{Hash, Hasher},
    io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use crossterm::event::{self, Event, KeyEventKind};
use image::{DynamicImage, GenericImageView, Rgba, RgbaImage, imageops};
use loreloom_tui::{CrosstermTerminalOps, TerminalSession};
use ratatui::{
    Terminal,
    backend::{CrosstermBackend, TestBackend},
    layout::{Alignment, Constraint, Direction, Layout, Rect, Size},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Paragraph},
};
use ratatui_image::{
    Image, Resize,
    picker::{Picker, ProtocolType},
};

const CANVAS: u32 = 256;
const DARK_BACKGROUND: Rgba<u8> = Rgba([15, 18, 24, 255]);
const DEFAULT_SOURCE: &str = ".local/appearance-spike/source/DoLP_BEEESSS/img";
const DEFAULT_GOOSE_SOURCE: &str = concat!(
    ".local/appearance-spike/source/Goose/",
    "degrees-of-lewdity-plus-master-imagepacks-goosefem/imagepacks/goosefem"
);
const DEFAULT_OUTPUT: &str = ".local/appearance-spike/output";
const ART_CANVAS: u32 = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum PortraitState {
    Calm,
    Worn,
}

impl PortraitState {
    const fn label(self) -> &'static str {
        match self {
            Self::Calm => "calm / intact outfit",
            Self::Worn => "tired / damaged outfit",
        }
    }

    const fn file_stem(self) -> &'static str {
        match self {
            Self::Calm => "calm",
            Self::Worn => "worn",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LayerRecipe {
    name: &'static str,
    z_index: i16,
    relative_path: &'static str,
    frame: u32,
    tint: Option<[u8; 3]>,
    mask_relative_path: Option<&'static str>,
}

#[derive(Default)]
struct AppearanceCache {
    entry: Option<(u64, RgbaImage)>,
    compose_count: usize,
}

impl AppearanceCache {
    fn resolve(
        &mut self,
        source: &Path,
        state: PortraitState,
        layers: &[LayerRecipe],
    ) -> Result<&RgbaImage, Box<dyn Error>> {
        let key = recipe_key(source, state, layers);
        self.resolve_with(key, || compose_layers(source, layers))
    }

    fn resolve_with<F>(&mut self, key: u64, compose: F) -> Result<&RgbaImage, Box<dyn Error>>
    where
        F: FnOnce() -> Result<RgbaImage, Box<dyn Error>>,
    {
        if self.entry.as_ref().is_none_or(|(cached, _)| *cached != key) {
            let image = compose()?;
            self.entry = Some((key, image));
            self.compose_count += 1;
        }
        self.entry
            .as_ref()
            .map(|(_, image)| image)
            .ok_or_else(|| "appearance cache did not retain the composed image".into())
    }
}

fn recipe_key(source: &Path, state: PortraitState, layers: &[LayerRecipe]) -> u64 {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    state.hash(&mut hasher);
    layers.hash(&mut hasher);
    hasher.finish()
}

fn recipes(state: PortraitState) -> Vec<LayerRecipe> {
    let hair = Some([83, 48, 40]);
    let shirt = match state {
        PortraitState::Calm => Some([52, 112, 164]),
        PortraitState::Worn => Some([95, 101, 112]),
    };
    let shorts = match state {
        PortraitState::Calm => Some([51, 67, 94]),
        PortraitState::Worn => Some([82, 67, 61]),
    };
    let integrity = match state {
        PortraitState::Calm => "full",
        PortraitState::Worn => "torn",
    };

    let mut result = vec![
        layer("base head", 5, "body/basehead.png"),
        tinted("hair sides", 10, "hair/sides/pigtails/short.png", hair),
        layer("body", 20, "body/basenoarms-classic.png"),
        layer("face base", 21, "face/default/base.png"),
        layer("left arm", 30, "body/leftarmidle-classic.png"),
        layer("right arm", 30, "body/rightarmidle-classic.png"),
        layer("eyes", 30, "face/default/eyes.png"),
        layer("sclera", 31, "face/default/sclera.png"),
        tinted(
            "left iris",
            32,
            "face/default/iris_left.png",
            Some([63, 151, 104]),
        ),
        tinted(
            "right iris",
            32,
            "face/default/iris_right.png",
            Some([66, 127, 185]),
        ),
        layer("eyelids", 34, "face/default/eyelids.png"),
        tinted(
            "shorts",
            90,
            if integrity == "full" {
                "clothes/lower/shorts/full_gray.png"
            } else {
                "clothes/lower/shorts/torn_gray.png"
            },
            shorts,
        ),
        tinted(
            "left sleeve",
            94,
            "clothes/upper/tshirt/left_gray.png",
            shirt,
        ),
        tinted(
            "right sleeve",
            94,
            "clothes/upper/tshirt/right_gray.png",
            shirt,
        ),
        tinted(
            "shirt",
            95,
            if integrity == "full" {
                "clothes/upper/tshirt/full_gray.png"
            } else {
                "clothes/upper/tshirt/torn_gray.png"
            },
            shirt,
        ),
        tinted("hair fringe", 133, "hair/fringe/split/short.png", hair),
        tinted("brows", 138, "face/default/browmid.png", hair),
    ];

    match state {
        PortraitState::Calm => {
            result.push(layer("smile", 40, "face/default/mouthsmile.png"));
        }
        PortraitState::Worn => {
            result.push(layer("frown", 40, "face/default/mouthfrown.png"));
            result.push(layer("blush", 50, "face/default/blush2.png"));
            result.push(layer("tears", 55, "face/default/tear2.png"));
        }
    }
    result
}

// Live demo adapter for the locally unpacked Goose image pack. The files remain ignored and are
// never required by tests, CI, or release artifacts.
fn goose_recipes(state: PortraitState, animation_frame: u32) -> Vec<LayerRecipe> {
    let hair = Some([98, 54, 39]);
    let shirt = match state {
        PortraitState::Calm => Some([116, 54, 106]),
        PortraitState::Worn => Some([54, 92, 119]),
    };
    let shorts = match state {
        PortraitState::Calm => Some([49, 61, 82]),
        PortraitState::Worn => Some([87, 68, 59]),
    };
    let integrity = match state {
        PortraitState::Calm => "full",
        PortraitState::Worn => "torn",
    };

    vec![
        framed("base head", 5, "body/base-head.png", animation_frame),
        tinted_framed(
            "hair sides",
            10,
            "hair/sides/default/short.png",
            animation_frame,
            hair,
        ),
        framed("body", 20, "body/basenoarms-f.png", animation_frame),
        layer("eyes", 30, "face/default/eyes.png"),
        layer("sclera", 31, "face/default/sclera.png"),
        tinted_framed(
            "left iris",
            32,
            "face/default/iris-left.png",
            animation_frame,
            Some([59, 155, 101]),
        ),
        tinted_framed(
            "right iris",
            32,
            "face/default/iris-right.png",
            animation_frame,
            Some([61, 128, 193]),
        ),
        framed("eyelids", 34, "face/default/eyelids.png", animation_frame),
        framed("lashes", 35, "face/default/lashes.png", animation_frame),
        layer("smile", 40, "face/default/mouthsmile.png"),
        framed("left arm", 50, "body/leftarmidle-f.png", animation_frame),
        framed("right arm", 50, "body/rightarmidle-f.png", animation_frame),
        tinted(
            "shorts",
            90,
            if integrity == "full" {
                "clothes/lower/shorts/full.png"
            } else {
                "clothes/lower/shorts/torn.png"
            },
            shorts,
        ),
        tinted_framed(
            "left sleeve",
            94,
            "clothes/upper/tshirt/left-idle.png",
            animation_frame,
            shirt,
        ),
        tinted_framed(
            "right sleeve",
            94,
            "clothes/upper/tshirt/right-idle.png",
            animation_frame,
            shirt,
        ),
        tinted_framed(
            "shirt",
            95,
            if integrity == "full" {
                "clothes/upper/tshirt/full.png"
            } else {
                "clothes/upper/tshirt/torn.png"
            },
            animation_frame,
            shirt,
        ),
        tinted_framed(
            "hair fringe",
            133,
            "hair/fringe/default/short.png",
            animation_frame,
            hair,
        ),
        layer("brows", 138, "face/default/brow-mid.png"),
    ]
}

const fn layer(name: &'static str, z_index: i16, relative_path: &'static str) -> LayerRecipe {
    LayerRecipe {
        name,
        z_index,
        relative_path,
        frame: 0,
        tint: None,
        mask_relative_path: None,
    }
}

const fn tinted(
    name: &'static str,
    z_index: i16,
    relative_path: &'static str,
    tint: Option<[u8; 3]>,
) -> LayerRecipe {
    LayerRecipe {
        name,
        z_index,
        relative_path,
        frame: 0,
        tint,
        mask_relative_path: None,
    }
}

const fn framed(
    name: &'static str,
    z_index: i16,
    relative_path: &'static str,
    frame: u32,
) -> LayerRecipe {
    LayerRecipe {
        name,
        z_index,
        relative_path,
        frame,
        tint: None,
        mask_relative_path: None,
    }
}

const fn tinted_framed(
    name: &'static str,
    z_index: i16,
    relative_path: &'static str,
    frame: u32,
    tint: Option<[u8; 3]>,
) -> LayerRecipe {
    LayerRecipe {
        name,
        z_index,
        relative_path,
        frame,
        tint,
        mask_relative_path: None,
    }
}

fn compose_layers(source: &Path, layers: &[LayerRecipe]) -> Result<RgbaImage, Box<dyn Error>> {
    let mut ordered = layers.to_vec();
    ordered.sort_by_key(|layer| layer.z_index);
    let mut canvas = RgbaImage::from_pixel(CANVAS, CANVAS, Rgba([0, 0, 0, 0]));

    for layer in ordered {
        let path = source.join(layer.relative_path);
        let decoded = image::open(&path).map_err(|error| {
            format!(
                "failed to decode layer {} at {}: {error}",
                layer.name,
                path.display()
            )
        })?;
        let mut frame = select_frame(&decoded, layer.frame, &path)?;
        if let Some(tint) = layer.tint {
            tint_mask(&mut frame, tint);
        }
        if let Some(mask_relative_path) = layer.mask_relative_path {
            let mask_path = source.join(mask_relative_path);
            let decoded_mask = image::open(&mask_path).map_err(|error| {
                format!(
                    "failed to decode mask for {} at {}: {error}",
                    layer.name,
                    mask_path.display()
                )
            })?;
            let mask = select_frame(&decoded_mask, layer.frame, &mask_path)?;
            apply_alpha_mask(&mut frame, &mask)?;
        }
        imageops::overlay(&mut canvas, &frame, 0, 0);
    }
    Ok(canvas)
}

fn select_frame(
    image: &DynamicImage,
    frame: u32,
    path: &Path,
) -> Result<RgbaImage, Box<dyn Error>> {
    let (width, height) = image.dimensions();
    if height != CANVAS || width < CANVAS {
        return Err(format!(
            "layer {} is {width}x{height}; the DoL sidebar adapter expects a 256px-high frame",
            path.display()
        )
        .into());
    }
    let frame_count = width / CANVAS;
    if frame >= frame_count {
        return Err(format!(
            "layer {} has {frame_count} frames but frame {frame} was requested",
            path.display()
        )
        .into());
    }
    Ok(image.crop_imm(frame * CANVAS, 0, CANVAS, CANVAS).to_rgba8())
}

fn tint_mask(image: &mut RgbaImage, tint: [u8; 3]) {
    for pixel in image.pixels_mut() {
        if pixel[3] == 0 {
            continue;
        }
        let shade = pixel[0].max(pixel[1]).max(pixel[2]);
        pixel[0] = multiply_channel(tint[0], shade);
        pixel[1] = multiply_channel(tint[1], shade);
        pixel[2] = multiply_channel(tint[2], shade);
    }
}

const fn multiply_channel(channel: u8, shade: u8) -> u8 {
    ((channel as u16 * shade as u16 + 127) / 255) as u8
}

fn apply_alpha_mask(image: &mut RgbaImage, mask: &RgbaImage) -> Result<(), Box<dyn Error>> {
    if image.dimensions() != mask.dimensions() {
        return Err(format!(
            "mask is {}x{} but layer is {}x{}",
            mask.width(),
            mask.height(),
            image.width(),
            image.height()
        )
        .into());
    }
    for (pixel, mask_pixel) in image.pixels_mut().zip(mask.pixels()) {
        pixel[3] = multiply_channel(pixel[3], mask_pixel[3]);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlendMode {
    SourceOver,
    Multiply,
    Screen,
    HardLight,
}

struct GeneratedLayer {
    name: &'static str,
    z_index: i16,
    image: RgbaImage,
    opacity: u8,
    blend_mode: BlendMode,
    tint: Option<[u8; 3]>,
    mask: Option<RgbaImage>,
}

#[derive(Debug, Clone, Copy)]
struct DemoPalette {
    hair: [u8; 3],
    cloth: [u8; 3],
    left_eye: [u8; 3],
    right_eye: [u8; 3],
}

const DAY_PALETTE: DemoPalette = DemoPalette {
    hair: [92, 49, 35],
    cloth: [151, 38, 63],
    left_eye: [61, 163, 96],
    right_eye: [62, 132, 205],
};

const NIGHT_PALETTE: DemoPalette = DemoPalette {
    hair: [45, 72, 105],
    cloth: [39, 121, 119],
    left_eye: [224, 165, 54],
    right_eye: [177, 82, 201],
};

fn generated_layer(name: &'static str, z_index: i16, image: RgbaImage) -> GeneratedLayer {
    GeneratedLayer {
        name,
        z_index,
        image,
        opacity: 255,
        blend_mode: BlendMode::SourceOver,
        tint: None,
        mask: None,
    }
}

fn compose_generated(mut layers: Vec<GeneratedLayer>) -> Result<RgbaImage, Box<dyn Error>> {
    layers.sort_by_key(|layer| layer.z_index);
    let mut canvas = transparent_art_layer();
    for mut layer in layers {
        if layer.image.dimensions() != canvas.dimensions() {
            return Err(format!(
                "generated layer {} is {}x{}, expected {}x{}",
                layer.name,
                layer.image.width(),
                layer.image.height(),
                canvas.width(),
                canvas.height()
            )
            .into());
        }
        if let Some(tint) = layer.tint {
            tint_mask(&mut layer.image, tint);
        }
        if let Some(mask) = layer.mask.as_ref() {
            apply_alpha_mask(&mut layer.image, mask)?;
        }
        blend_image(&mut canvas, &layer.image, layer.opacity, layer.blend_mode);
    }
    Ok(canvas)
}

fn blend_image(
    destination: &mut RgbaImage,
    source: &RgbaImage,
    opacity: u8,
    blend_mode: BlendMode,
) {
    debug_assert_eq!(destination.dimensions(), source.dimensions());
    for (backdrop, foreground) in destination.pixels_mut().zip(source.pixels()) {
        *backdrop = blend_pixel(*backdrop, *foreground, opacity, blend_mode);
    }
}

fn blend_pixel(
    backdrop: Rgba<u8>,
    foreground: Rgba<u8>,
    opacity: u8,
    blend_mode: BlendMode,
) -> Rgba<u8> {
    let source_alpha = f32::from(multiply_channel(foreground[3], opacity)) / 255.0;
    let backdrop_alpha = f32::from(backdrop[3]) / 255.0;
    let output_alpha = source_alpha + backdrop_alpha * (1.0 - source_alpha);
    if output_alpha <= f32::EPSILON {
        return Rgba([0, 0, 0, 0]);
    }

    let mut output = [0_u8; 4];
    for channel in 0..3 {
        let backdrop_channel = f32::from(backdrop[channel]) / 255.0;
        let source_channel = f32::from(foreground[channel]) / 255.0;
        let blended = blend_channel(backdrop_channel, source_channel, blend_mode);
        let premultiplied = source_alpha * (1.0 - backdrop_alpha) * source_channel
            + source_alpha * backdrop_alpha * blended
            + (1.0 - source_alpha) * backdrop_alpha * backdrop_channel;
        output[channel] = ((premultiplied / output_alpha) * 255.0).round() as u8;
    }
    output[3] = (output_alpha * 255.0).round() as u8;
    Rgba(output)
}

fn blend_channel(backdrop: f32, source: f32, blend_mode: BlendMode) -> f32 {
    match blend_mode {
        BlendMode::SourceOver => source,
        BlendMode::Multiply => backdrop * source,
        BlendMode::Screen => backdrop + source - backdrop * source,
        BlendMode::HardLight if source <= 0.5 => 2.0 * backdrop * source,
        BlendMode::HardLight => 1.0 - 2.0 * (1.0 - backdrop) * (1.0 - source),
    }
}

fn transparent_art_layer() -> RgbaImage {
    RgbaImage::from_pixel(ART_CANVAS, ART_CANVAS, Rgba([0, 0, 0, 0]))
}

fn original_frame(palette: DemoPalette, blink: bool) -> Result<RgbaImage, Box<dyn Error>> {
    let mut hair_back = transparent_art_layer();
    paint_hair_back(&mut hair_back);
    let mut hair_back_layer = generated_layer("hair back", 5, hair_back);
    hair_back_layer.tint = Some(palette.hair);

    let mut skin = transparent_art_layer();
    paint_skin(&mut skin);

    let mut clothes = transparent_art_layer();
    paint_clothes(&mut clothes);
    let mut clothes_layer = generated_layer("clothes", 30, clothes);
    clothes_layer.tint = Some(palette.cloth);

    let mut clothing_shadow = transparent_art_layer();
    paint_clothing_shadow(&mut clothing_shadow);
    let mut clothing_shadow_layer = generated_layer("clothing shadows", 31, clothing_shadow);
    clothing_shadow_layer.blend_mode = BlendMode::Multiply;
    clothing_shadow_layer.opacity = 150;

    let mut sclera = transparent_art_layer();
    let mut iris_source = transparent_art_layer();
    let mut left_eye_mask = transparent_art_layer();
    let mut right_eye_mask = transparent_art_layer();
    if !blink {
        paint_sclera(&mut sclera);
        paint_iris_source(&mut iris_source);
        paint_left_eye_mask(&mut left_eye_mask);
        paint_right_eye_mask(&mut right_eye_mask);
    }

    let mut left_iris_layer = generated_layer("left iris", 21, iris_source.clone());
    left_iris_layer.tint = Some(palette.left_eye);
    left_iris_layer.mask = Some(left_eye_mask);
    let mut right_iris_layer = generated_layer("right iris", 21, iris_source);
    right_iris_layer.tint = Some(palette.right_eye);
    right_iris_layer.mask = Some(right_eye_mask);

    let mut eye_lines = transparent_art_layer();
    paint_eye_lines(&mut eye_lines, blink);

    let mut features = transparent_art_layer();
    paint_features(&mut features);

    let mut hair_front = transparent_art_layer();
    paint_hair_front(&mut hair_front);
    let mut hair_front_layer = generated_layer("hair front", 40, hair_front);
    hair_front_layer.tint = Some(palette.hair);

    let mut highlights = transparent_art_layer();
    paint_highlights(&mut highlights);
    let mut highlight_layer = generated_layer("highlights", 45, highlights);
    highlight_layer.blend_mode = BlendMode::Screen;
    highlight_layer.opacity = 135;

    let mut trim = transparent_art_layer();
    paint_trim(&mut trim);
    let mut trim_layer = generated_layer("hard-light trim", 46, trim);
    trim_layer.blend_mode = BlendMode::HardLight;
    trim_layer.opacity = 180;

    let art = compose_generated(vec![
        hair_back_layer,
        generated_layer("skin", 10, skin),
        generated_layer("sclera", 20, sclera),
        left_iris_layer,
        right_iris_layer,
        generated_layer("eye lines", 22, eye_lines),
        generated_layer("features", 23, features),
        clothes_layer,
        clothing_shadow_layer,
        hair_front_layer,
        highlight_layer,
        trim_layer,
    ])?;
    Ok(imageops::resize(
        &art,
        CANVAS,
        CANVAS,
        imageops::FilterType::Nearest,
    ))
}

fn paint_hair_back(image: &mut RgbaImage) {
    let outline = Rgba([28, 25, 29, 255]);
    let gray = Rgba([178, 178, 178, 255]);
    fill_ellipse(image, 26, 28, 14, 19, outline);
    fill_ellipse(image, 26, 28, 12, 17, gray);
    fill_polygon(image, &[(14, 31), (20, 37), (18, 62), (12, 70)], outline);
    fill_polygon(image, &[(16, 32), (20, 38), (18, 59), (14, 66)], gray);
    fill_polygon(image, &[(33, 31), (39, 38), (42, 68), (34, 60)], outline);
    fill_polygon(image, &[(34, 33), (37, 39), (39, 63), (35, 58)], gray);

    fill_ellipse(image, 91, 39, 35, 39, outline);
    fill_ellipse(image, 91, 39, 32, 36, gray);
    fill_polygon(
        image,
        &[(60, 43), (69, 55), (60, 94), (49, 111), (55, 66)],
        outline,
    );
    fill_polygon(
        image,
        &[(62, 44), (70, 56), (62, 92), (53, 105), (58, 67)],
        gray,
    );
    fill_polygon(
        image,
        &[(110, 42), (122, 54), (127, 91), (115, 113), (113, 67)],
        outline,
    );
    fill_polygon(
        image,
        &[(109, 44), (120, 56), (124, 89), (117, 106), (115, 66)],
        gray,
    );
}

fn paint_skin(image: &mut RgbaImage) {
    let outline = Rgba([43, 31, 32, 255]);
    let skin = Rgba([232, 174, 139, 255]);

    fill_art_rect(image, 20, 78, 6, 31, outline);
    fill_art_rect(image, 21, 78, 4, 29, skin);
    fill_art_rect(image, 28, 78, 6, 31, outline);
    fill_art_rect(image, 29, 78, 4, 29, skin);
    draw_thick_line(image, 17, 50, 11, 73, 5, outline);
    draw_thick_line(image, 17, 51, 12, 72, 3, skin);
    draw_thick_line(image, 35, 50, 41, 72, 5, outline);
    draw_thick_line(image, 35, 51, 40, 71, 3, skin);
    fill_ellipse(image, 26, 28, 9, 11, outline);
    fill_ellipse(image, 26, 28, 8, 10, skin);

    fill_polygon(
        image,
        &[
            (55, 97),
            (72, 80),
            (79, 74),
            (80, 61),
            (102, 61),
            (103, 74),
            (110, 80),
            (127, 97),
            (127, 127),
            (55, 127),
        ],
        outline,
    );
    fill_polygon(
        image,
        &[
            (59, 97),
            (74, 82),
            (82, 75),
            (82, 62),
            (100, 62),
            (100, 75),
            (108, 82),
            (124, 97),
            (124, 127),
            (59, 127),
        ],
        skin,
    );
    fill_ellipse(image, 68, 44, 5, 8, outline);
    fill_ellipse(image, 68, 44, 4, 7, skin);
    fill_ellipse(image, 114, 44, 5, 8, outline);
    fill_ellipse(image, 114, 44, 4, 7, skin);
    fill_ellipse(image, 91, 42, 23, 28, outline);
    fill_ellipse(image, 91, 42, 21, 26, skin);
}

fn paint_clothes(image: &mut RgbaImage) {
    let outline = Rgba([35, 31, 38, 255]);
    let gray = Rgba([205, 205, 205, 255]);
    fill_polygon(image, &[(17, 47), (35, 47), (39, 83), (14, 83)], outline);
    fill_polygon(image, &[(19, 49), (33, 49), (36, 80), (17, 80)], gray);
    fill_art_rect(image, 19, 79, 15, 8, outline);
    fill_art_rect(image, 21, 79, 11, 6, gray);
    fill_polygon(
        image,
        &[
            (52, 96),
            (74, 76),
            (91, 86),
            (108, 76),
            (127, 96),
            (127, 127),
            (52, 127),
        ],
        outline,
    );
    fill_polygon(
        image,
        &[
            (57, 97),
            (75, 80),
            (91, 90),
            (107, 80),
            (123, 97),
            (123, 127),
            (57, 127),
        ],
        gray,
    );
}

fn paint_clothing_shadow(image: &mut RgbaImage) {
    let shadow = Rgba([88, 88, 88, 210]);
    fill_polygon(image, &[(17, 72), (36, 72), (36, 80), (15, 80)], shadow);
    fill_polygon(
        image,
        &[(58, 111), (82, 91), (87, 96), (69, 127), (57, 127)],
        shadow,
    );
    fill_polygon(
        image,
        &[(100, 93), (122, 107), (123, 127), (111, 127)],
        shadow,
    );
}

fn paint_sclera(image: &mut RgbaImage) {
    let white = Rgba([246, 240, 218, 255]);
    fill_art_rect(image, 22, 27, 3, 2, white);
    fill_art_rect(image, 28, 27, 3, 2, white);
    fill_ellipse(image, 82, 42, 7, 4, white);
    fill_ellipse(image, 100, 42, 7, 4, white);
}

fn paint_iris_source(image: &mut RgbaImage) {
    let shade = Rgba([220, 220, 220, 255]);
    fill_art_rect(image, 23, 27, 2, 2, shade);
    fill_art_rect(image, 29, 27, 2, 2, shade);
    fill_ellipse(image, 83, 42, 3, 4, shade);
    fill_ellipse(image, 99, 42, 3, 4, shade);
}

fn paint_left_eye_mask(image: &mut RgbaImage) {
    let mask = Rgba([255, 255, 255, 255]);
    fill_art_rect(image, 22, 26, 4, 4, mask);
    fill_ellipse(image, 83, 42, 5, 5, mask);
}

fn paint_right_eye_mask(image: &mut RgbaImage) {
    let mask = Rgba([255, 255, 255, 255]);
    fill_art_rect(image, 28, 26, 4, 4, mask);
    fill_ellipse(image, 99, 42, 5, 5, mask);
}

fn paint_eye_lines(image: &mut RgbaImage, blink: bool) {
    let ink = Rgba([42, 30, 36, 255]);
    if blink {
        draw_thick_line(image, 21, 28, 25, 29, 1, ink);
        draw_thick_line(image, 28, 29, 32, 28, 1, ink);
        draw_thick_line(image, 76, 43, 87, 45, 2, ink);
        draw_thick_line(image, 95, 45, 106, 43, 2, ink);
    } else {
        draw_thick_line(image, 21, 26, 25, 26, 1, ink);
        draw_thick_line(image, 28, 26, 32, 26, 1, ink);
        draw_thick_line(image, 76, 39, 87, 38, 2, ink);
        draw_thick_line(image, 95, 38, 106, 39, 2, ink);
        fill_art_rect(image, 83, 41, 2, 3, ink);
        fill_art_rect(image, 98, 41, 2, 3, ink);
        put_pixel(image, 83, 41, Rgba([236, 236, 230, 255]));
        put_pixel(image, 98, 41, Rgba([236, 236, 230, 255]));
    }
}

fn paint_features(image: &mut RgbaImage) {
    let ink = Rgba([75, 43, 43, 255]);
    draw_thick_line(image, 21, 23, 25, 22, 1, ink);
    draw_thick_line(image, 28, 22, 32, 23, 1, ink);
    put_pixel(image, 26, 31, ink);
    draw_thick_line(image, 24, 34, 28, 34, 1, ink);
    draw_thick_line(image, 76, 34, 87, 32, 2, ink);
    draw_thick_line(image, 96, 32, 107, 34, 2, ink);
    draw_thick_line(image, 91, 43, 89, 51, 1, ink);
    draw_thick_line(image, 86, 57, 96, 57, 2, Rgba([139, 58, 68, 255]));
    put_pixel(image, 90, 56, Rgba([226, 155, 145, 255]));
}

fn paint_hair_front(image: &mut RgbaImage) {
    let outline = Rgba([28, 25, 29, 255]);
    let gray = Rgba([180, 180, 180, 255]);
    fill_polygon(
        image,
        &[
            (16, 19),
            (24, 11),
            (36, 17),
            (31, 23),
            (28, 18),
            (25, 25),
            (21, 18),
            (19, 29),
        ],
        outline,
    );
    fill_polygon(
        image,
        &[
            (18, 19),
            (24, 13),
            (34, 17),
            (30, 21),
            (28, 16),
            (25, 23),
            (21, 16),
            (19, 25),
        ],
        gray,
    );
    fill_polygon(
        image,
        &[
            (67, 25),
            (78, 7),
            (99, 6),
            (116, 19),
            (105, 29),
            (100, 20),
            (94, 34),
            (87, 17),
            (78, 34),
            (77, 19),
        ],
        outline,
    );
    fill_polygon(
        image,
        &[
            (70, 24),
            (79, 9),
            (98, 8),
            (113, 19),
            (106, 26),
            (100, 16),
            (94, 30),
            (87, 13),
            (79, 29),
            (78, 15),
        ],
        gray,
    );
}

fn paint_highlights(image: &mut RgbaImage) {
    let light = Rgba([185, 185, 185, 210]);
    draw_thick_line(image, 19, 18, 23, 14, 1, light);
    draw_thick_line(image, 72, 27, 82, 12, 2, light);
    draw_thick_line(image, 62, 98, 72, 88, 2, Rgba([115, 115, 115, 150]));
    fill_ellipse(image, 79, 50, 4, 2, Rgba([125, 125, 125, 95]));
    fill_ellipse(image, 103, 50, 4, 2, Rgba([125, 125, 125, 95]));
}

fn paint_trim(image: &mut RgbaImage) {
    let light = Rgba([210, 210, 210, 220]);
    draw_thick_line(image, 19, 52, 33, 52, 1, light);
    draw_thick_line(image, 18, 75, 35, 75, 1, light);
    draw_thick_line(image, 75, 80, 91, 91, 2, light);
    draw_thick_line(image, 91, 91, 107, 80, 2, light);
}

fn fill_art_rect(image: &mut RgbaImage, x: i32, y: i32, width: i32, height: i32, color: Rgba<u8>) {
    for py in y..y + height {
        for px in x..x + width {
            put_pixel(image, px, py, color);
        }
    }
}

fn fill_ellipse(
    image: &mut RgbaImage,
    center_x: i32,
    center_y: i32,
    radius_x: i32,
    radius_y: i32,
    color: Rgba<u8>,
) {
    for y in center_y - radius_y..=center_y + radius_y {
        for x in center_x - radius_x..=center_x + radius_x {
            let dx = f64::from(x - center_x) / f64::from(radius_x.max(1));
            let dy = f64::from(y - center_y) / f64::from(radius_y.max(1));
            if dx * dx + dy * dy <= 1.0 {
                put_pixel(image, x, y, color);
            }
        }
    }
}

fn fill_polygon(image: &mut RgbaImage, points: &[(i32, i32)], color: Rgba<u8>) {
    let min_x = points.iter().map(|point| point.0).min().unwrap_or(0);
    let max_x = points.iter().map(|point| point.0).max().unwrap_or(-1);
    let min_y = points.iter().map(|point| point.1).min().unwrap_or(0);
    let max_y = points.iter().map(|point| point.1).max().unwrap_or(-1);
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            if point_in_polygon(x, y, points) {
                put_pixel(image, x, y, color);
            }
        }
    }
}

fn point_in_polygon(x: i32, y: i32, points: &[(i32, i32)]) -> bool {
    if points.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut previous = points.len() - 1;
    for current in 0..points.len() {
        let (current_x, current_y) = points[current];
        let (previous_x, previous_y) = points[previous];
        if (current_y > y) != (previous_y > y) {
            let edge_x = f64::from(previous_x - current_x) * f64::from(y - current_y)
                / f64::from(previous_y - current_y)
                + f64::from(current_x);
            if f64::from(x) < edge_x {
                inside = !inside;
            }
        }
        previous = current;
    }
    inside
}

fn draw_thick_line(
    image: &mut RgbaImage,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    thickness: i32,
    color: Rgba<u8>,
) {
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut error = dx + dy;
    let (mut x, mut y) = (x0, y0);
    loop {
        let radius = thickness.saturating_sub(1) / 2;
        fill_art_rect(image, x - radius, y - radius, thickness, thickness, color);
        if x == x1 && y == y1 {
            break;
        }
        let doubled = 2 * error;
        if doubled >= dy {
            error += dy;
            x += sx;
        }
        if doubled <= dx {
            error += dx;
            y += sy;
        }
    }
}

fn put_pixel(image: &mut RgbaImage, x: i32, y: i32, color: Rgba<u8>) {
    if x >= 0 && y >= 0 && x < image.width() as i32 && y < image.height() as i32 {
        image.put_pixel(x as u32, y as u32, color);
    }
}

fn trim_transparent(image: &RgbaImage, margin: u32) -> RgbaImage {
    let mut bounds = None::<(u32, u32, u32, u32)>;
    for (x, y, pixel) in image.enumerate_pixels() {
        if pixel[3] == 0 {
            continue;
        }
        bounds = Some(bounds.map_or((x, y, x, y), |(min_x, min_y, max_x, max_y)| {
            (min_x.min(x), min_y.min(y), max_x.max(x), max_y.max(y))
        }));
    }

    let Some((min_x, min_y, max_x, max_y)) = bounds else {
        return RgbaImage::new(1, 1);
    };
    let x = min_x.saturating_sub(margin);
    let y = min_y.saturating_sub(margin);
    let right = (max_x + margin + 1).min(image.width());
    let bottom = (max_y + margin + 1).min(image.height());
    imageops::crop_imm(image, x, y, right - x, bottom - y).to_image()
}

fn flattened(image: &RgbaImage) -> RgbaImage {
    let mut result = RgbaImage::from_pixel(image.width(), image.height(), DARK_BACKGROUND);
    imageops::overlay(&mut result, image, 0, 0);
    result
}

fn halfblock_preview(
    image: &RgbaImage,
    columns: u16,
    rows: u16,
) -> Result<(RgbaImage, Size, usize), Box<dyn Error>> {
    let pixel_art = imageops::resize(
        image,
        image.width() * 2,
        image.height() * 2,
        imageops::FilterType::Nearest,
    );
    let mut picker = Picker::halfblocks();
    picker.set_background_color(Some(DARK_BACKGROUND));
    let protocol = picker.new_protocol(
        DynamicImage::ImageRgba8(pixel_art),
        Size::new(columns, rows),
        Resize::Fit(None),
    )?;
    let encoded_size = protocol.size();
    let mut terminal = Terminal::new(TestBackend::new(columns, rows))?;
    terminal.draw(|frame| {
        frame.render_widget(Image::new(&protocol), frame.area());
    })?;
    let buffer = terminal.backend().buffer();
    let colored_cells = (0..rows)
        .flat_map(|y| (0..columns).map(move |x| (x, y)))
        .filter(|&(x, y)| buffer.cell((x, y)).is_some_and(|cell| cell.symbol() == "▀"))
        .count();
    Ok((
        rasterize_halfblocks(buffer, 8, 16),
        encoded_size,
        colored_cells,
    ))
}

fn rasterize_halfblocks(buffer: &ratatui::buffer::Buffer, cell_w: u32, cell_h: u32) -> RgbaImage {
    let area = buffer.area;
    let mut output = RgbaImage::from_pixel(
        area.width as u32 * cell_w,
        area.height as u32 * cell_h,
        DARK_BACKGROUND,
    );
    for y in 0..area.height {
        for x in 0..area.width {
            let Some(cell) = buffer.cell((area.x + x, area.y + y)) else {
                continue;
            };
            let upper = terminal_color(cell.fg);
            let lower = terminal_color(cell.bg);
            let (top, bottom) = match cell.symbol() {
                "▀" => (upper, lower),
                "▄" => (lower, upper),
                "█" => (upper, upper),
                _ => (lower, lower),
            };
            fill_rect(
                &mut output,
                x as u32 * cell_w,
                y as u32 * cell_h,
                cell_w,
                cell_h / 2,
                top,
            );
            fill_rect(
                &mut output,
                x as u32 * cell_w,
                y as u32 * cell_h + cell_h / 2,
                cell_w,
                cell_h - cell_h / 2,
                bottom,
            );
        }
    }
    output
}

fn terminal_color(color: Color) -> Rgba<u8> {
    match color {
        Color::Rgb(r, g, b) => Rgba([r, g, b, 255]),
        Color::Black => Rgba([0, 0, 0, 255]),
        Color::White => Rgba([255, 255, 255, 255]),
        Color::Gray => Rgba([128, 128, 128, 255]),
        Color::DarkGray => Rgba([64, 64, 64, 255]),
        Color::Red | Color::LightRed => Rgba([220, 70, 70, 255]),
        Color::Green | Color::LightGreen => Rgba([80, 190, 110, 255]),
        Color::Blue | Color::LightBlue => Rgba([80, 120, 220, 255]),
        Color::Yellow | Color::LightYellow => Rgba([220, 190, 70, 255]),
        Color::Magenta | Color::LightMagenta => Rgba([190, 80, 190, 255]),
        Color::Cyan | Color::LightCyan => Rgba([70, 190, 190, 255]),
        Color::Indexed(value) => Rgba([value, value, value, 255]),
        Color::Reset => DARK_BACKGROUND,
    }
}

fn fill_rect(image: &mut RgbaImage, x: u32, y: u32, width: u32, height: u32, color: Rgba<u8>) {
    for py in y..y + height {
        for px in x..x + width {
            image.put_pixel(px, py, color);
        }
    }
}

fn write_visual_review(source: &Path, output: &Path) -> Result<(), Box<dyn Error>> {
    std::fs::create_dir_all(output)?;
    let mut cache = AppearanceCache::default();
    for state in [PortraitState::Calm, PortraitState::Worn] {
        let layers = recipes(state);
        let composed = cache.resolve(source, state, &layers)?.clone();
        let cropped = trim_transparent(&composed, 8);
        let original_path = output.join(format!("{}-composite.png", state.file_stem()));
        flattened(&cropped).save(&original_path)?;

        let (terminal, encoded_size, colored_cells) = halfblock_preview(&cropped, 24, 20)?;
        let terminal_path = output.join(format!("{}-halfblocks.png", state.file_stem()));
        terminal.save(&terminal_path)?;
        println!(
            "{}: {} layers, composite={}x{}, terminal={}x{} cells ({} rendered cells)",
            state.label(),
            layers.len(),
            cropped.width(),
            cropped.height(),
            encoded_size.width,
            encoded_size.height,
            colored_cells
        );
    }
    println!("visual review written to {}", output.display());
    Ok(())
}

fn write_original_review(output: &Path) -> Result<(), Box<dyn Error>> {
    std::fs::create_dir_all(output)?;
    let started = Instant::now();
    let awake = original_frame(DAY_PALETTE, false)?;
    let blink = original_frame(DAY_PALETTE, true)?;
    let alternate = original_frame(NIGHT_PALETTE, false)?;

    let awake_path = output.join("original-awake.png");
    let blink_path = output.join("original-blink.png");
    let alternate_path = output.join("original-alt-palette.png");
    flattened(&awake).save(&awake_path)?;
    flattened(&blink).save(&blink_path)?;
    flattened(&alternate).save(&alternate_path)?;

    let gap = 8_u32;
    let mut review = RgbaImage::from_pixel(CANVAS * 3 + gap * 4, CANVAS + gap * 2, DARK_BACKGROUND);
    imageops::overlay(&mut review, &awake, gap.into(), gap.into());
    imageops::overlay(&mut review, &blink, (CANVAS + gap * 2).into(), gap.into());
    imageops::overlay(
        &mut review,
        &alternate,
        (CANVAS * 2 + gap * 3).into(),
        gap.into(),
    );
    let review_path = output.join("original-review.png");
    review.save(&review_path)?;

    let mut animation_strip = RgbaImage::from_pixel(CANVAS * 2, CANVAS, Rgba([0, 0, 0, 0]));
    imageops::overlay(&mut animation_strip, &awake, 0, 0);
    imageops::overlay(&mut animation_strip, &blink, CANVAS.into(), 0);
    animation_strip.save(output.join("original-animation-strip.png"))?;

    let (terminal, encoded_size, colored_cells) = halfblock_preview(&awake, 42, 24)?;
    terminal.save(output.join("original-halfblocks.png"))?;
    println!(
        "original dual-view: 12 layers, 3 states, review={}x{}, terminal={}x{} cells ({} rendered cells), {} ms",
        review.width(),
        review.height(),
        encoded_size.width,
        encoded_size.height,
        colored_cells,
        started.elapsed().as_millis()
    );
    println!("visual review written to {}", review_path.display());
    Ok(())
}

fn write_goose_review(source: &Path, output: &Path) -> Result<(), Box<dyn Error>> {
    std::fs::create_dir_all(output)?;
    let started = Instant::now();
    let awake_layers = goose_recipes(PortraitState::Calm, 0);
    let blink_layers = goose_recipes(PortraitState::Calm, 1);
    let worn_layers = goose_recipes(PortraitState::Worn, 0);
    let awake = compose_layers(source, &awake_layers)?;
    let blink = compose_layers(source, &blink_layers)?;
    let worn = compose_layers(source, &worn_layers)?;

    flattened(&awake).save(output.join("goose-awake.png"))?;
    flattened(&blink).save(output.join("goose-blink.png"))?;
    flattened(&worn).save(output.join("goose-worn.png"))?;

    let gap = 8_u32;
    let mut review = RgbaImage::from_pixel(CANVAS * 3 + gap * 4, CANVAS + gap * 2, DARK_BACKGROUND);
    imageops::overlay(&mut review, &awake, gap.into(), gap.into());
    imageops::overlay(&mut review, &blink, (CANVAS + gap * 2).into(), gap.into());
    imageops::overlay(
        &mut review,
        &worn,
        (CANVAS * 2 + gap * 3).into(),
        gap.into(),
    );
    let review_path = output.join("goose-review.png");
    review.save(&review_path)?;

    let mut animation_strip = RgbaImage::from_pixel(CANVAS * 2, CANVAS, Rgba([0, 0, 0, 0]));
    imageops::overlay(&mut animation_strip, &awake, 0, 0);
    imageops::overlay(&mut animation_strip, &blink, CANVAS.into(), 0);
    animation_strip.save(output.join("goose-animation-strip.png"))?;

    let (terminal, encoded_size, colored_cells) = halfblock_preview(&awake, 52, 26)?;
    terminal.save(output.join("goose-halfblocks.png"))?;
    println!(
        "Goose local demo: {} layers, 3 states, review={}x{}, terminal={}x{} cells ({} rendered cells), {} ms",
        awake_layers.len(),
        review.width(),
        review.height(),
        encoded_size.width,
        encoded_size.height,
        colored_cells,
        started.elapsed().as_millis()
    );
    println!("visual review written to {}", review_path.display());
    Ok(())
}

fn run_terminal(
    source: &Path,
    state: PortraitState,
    forced_protocol: Option<ProtocolType>,
) -> Result<(), Box<dyn Error>> {
    let layers = recipes(state);
    let composed = trim_transparent(&compose_layers(source, &layers)?, 8);
    let pixel_art = imageops::resize(
        &composed,
        composed.width() * 2,
        composed.height() * 2,
        imageops::FilterType::Nearest,
    );
    let _session = TerminalSession::open(CrosstermTerminalOps)?;
    let mut picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());
    if let Some(protocol) = forced_protocol {
        picker.set_protocol_type(protocol);
    }
    let selected_protocol = picker.protocol_type();
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let area = terminal.size()?;
    let max_image = Size::new(
        area.width.saturating_sub(6).min(42),
        area.height.saturating_sub(8),
    );
    let protocol = picker.new_protocol(
        DynamicImage::ImageRgba8(pixel_art),
        max_image,
        Resize::Fit(None),
    )?;
    terminal.clear()?;
    loop {
        terminal.draw(|frame| {
            let root = Block::default()
                .title(" Loreloom appearance spike ")
                .borders(Borders::ALL)
                .style(Style::default().fg(Color::Gray));
            let inner = root.inner(frame.area());
            frame.render_widget(root, frame.area());
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(2),
                    Constraint::Min(1),
                    Constraint::Length(2),
                ])
                .split(inner);
            frame.render_widget(
                Paragraph::new(format!("{} · {:?}", state.label(), selected_protocol))
                    .alignment(Alignment::Center)
                    .style(Style::default().add_modifier(Modifier::BOLD)),
                rows[0],
            );
            let image_size = protocol.size();
            let image_area = centered_rect(image_size, rows[1]);
            frame.render_widget(Image::new(&protocol).allow_clipping(true), image_area);
            frame.render_widget(
                Paragraph::new(if selected_protocol == ProtocolType::Halfblocks {
                    "Halfblocks is low fidelity; rerun with --protocol kitty|iterm2|sixel"
                } else {
                    "native pixel protocol · press any key to exit"
                })
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::DarkGray)),
                rows[2],
            );
        })?;
        if let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            break;
        }
    }
    Ok(())
}

fn run_original_terminal(forced_protocol: Option<ProtocolType>) -> Result<(), Box<dyn Error>> {
    let awake = original_frame(DAY_PALETTE, false)?;
    let blink = original_frame(DAY_PALETTE, true)?;
    run_animated_terminal(
        awake,
        blink,
        "original full-body + close-up",
        forced_protocol,
    )
}

fn run_goose_terminal(
    source: &Path,
    state: PortraitState,
    forced_protocol: Option<ProtocolType>,
) -> Result<(), Box<dyn Error>> {
    let awake = compose_layers(source, &goose_recipes(state, 0))?;
    let blink = compose_layers(source, &goose_recipes(state, 1))?;
    run_animated_terminal(awake, blink, "Goose full-body + close-up", forced_protocol)
}

fn run_animated_terminal(
    awake: RgbaImage,
    blink: RgbaImage,
    label: &str,
    forced_protocol: Option<ProtocolType>,
) -> Result<(), Box<dyn Error>> {
    let awake = imageops::resize(
        &awake,
        awake.width() * 2,
        awake.height() * 2,
        imageops::FilterType::Nearest,
    );
    let blink = imageops::resize(
        &blink,
        blink.width() * 2,
        blink.height() * 2,
        imageops::FilterType::Nearest,
    );
    let _session = TerminalSession::open(CrosstermTerminalOps)?;
    let mut picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());
    if let Some(protocol) = forced_protocol {
        picker.set_protocol_type(protocol);
    }
    let selected_protocol = picker.protocol_type();
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let area = terminal.size()?;
    let max_image = Size::new(
        area.width.saturating_sub(6).min(52),
        area.height.saturating_sub(8),
    );
    let awake_protocol = picker.new_protocol(
        DynamicImage::ImageRgba8(awake),
        max_image,
        Resize::Fit(None),
    )?;
    let blink_protocol = picker.new_protocol(
        DynamicImage::ImageRgba8(blink),
        max_image,
        Resize::Fit(None),
    )?;
    let animation_started = Instant::now();
    terminal.clear()?;
    loop {
        let animation_position = animation_started.elapsed().as_millis() % 1_100;
        let is_blinking = animation_position >= 950;
        let protocol = if is_blinking {
            &blink_protocol
        } else {
            &awake_protocol
        };
        terminal.draw(|frame| {
            let root = Block::default()
                .title(" Loreloom DoL-semantics spike ")
                .borders(Borders::ALL)
                .style(Style::default().fg(Color::Gray));
            let inner = root.inner(frame.area());
            frame.render_widget(root, frame.area());
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(2),
                    Constraint::Min(1),
                    Constraint::Length(2),
                ])
                .split(inner);
            frame.render_widget(
                Paragraph::new(format!(
                    "{} · {} · {:?}",
                    label,
                    if is_blinking { "blink" } else { "awake" },
                    selected_protocol
                ))
                .alignment(Alignment::Center)
                .style(Style::default().add_modifier(Modifier::BOLD)),
                rows[0],
            );
            let image_size = protocol.size();
            frame.render_widget(
                Image::new(protocol).allow_clipping(true),
                centered_rect(image_size, rows[1]),
            );
            frame.render_widget(
                Paragraph::new(if selected_protocol == ProtocolType::Halfblocks {
                    "Halfblocks is low fidelity; rerun with --protocol kitty|iterm2|sixel"
                } else {
                    "native pixel protocol · press any key to exit"
                })
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::DarkGray)),
                rows[2],
            );
        })?;
        if event::poll(Duration::from_millis(50))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            break;
        }
    }
    Ok(())
}

fn centered_rect(size: Size, area: Rect) -> Rect {
    Rect::new(
        area.x + area.width.saturating_sub(size.width) / 2,
        area.y + area.height.saturating_sub(size.height) / 2,
        size.width.min(area.width),
        size.height.min(area.height),
    )
}

fn parse_protocol(value: &str) -> Result<Option<ProtocolType>, String> {
    match value {
        "auto" => Ok(None),
        "kitty" => Ok(Some(ProtocolType::Kitty)),
        "iterm2" => Ok(Some(ProtocolType::Iterm2)),
        "sixel" => Ok(Some(ProtocolType::Sixel)),
        "halfblocks" => Ok(Some(ProtocolType::Halfblocks)),
        _ => Err("--protocol must be auto, kitty, iterm2, sixel, or halfblocks".to_owned()),
    }
}

struct Args {
    source: PathBuf,
    goose_source: PathBuf,
    output: PathBuf,
    terminal: bool,
    original: bool,
    goose: bool,
    protocol: Option<ProtocolType>,
    state: PortraitState,
}

fn parse_args() -> Result<Args, Box<dyn Error>> {
    let mut source = PathBuf::from(DEFAULT_SOURCE);
    let mut goose_source = PathBuf::from(DEFAULT_GOOSE_SOURCE);
    let mut output = PathBuf::from(DEFAULT_OUTPUT);
    let mut terminal = false;
    let mut original = false;
    let mut goose = false;
    let mut protocol = None;
    let mut state = PortraitState::Calm;
    let mut args = env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--source" => {
                source = args.next().ok_or("--source requires a path")?.into();
            }
            "--goose-source" => {
                goose_source = args.next().ok_or("--goose-source requires a path")?.into();
            }
            "--output" => {
                output = args.next().ok_or("--output requires a path")?.into();
            }
            "--terminal" => terminal = true,
            "--original" => original = true,
            "--goose" => goose = true,
            "--protocol" => {
                protocol =
                    parse_protocol(&args.next().ok_or("--protocol requires a protocol name")?)?;
            }
            "--state" => {
                state = match args.next().as_deref() {
                    Some("calm") => PortraitState::Calm,
                    Some("worn") => PortraitState::Worn,
                    _ => return Err("--state must be calm or worn".into()),
                };
            }
            "--help" | "-h" => {
                println!(
                    "appearance_spike [--original | --goose] [--source DOL_IMG] [--goose-source GOOSE_IMG] [--output DIR] [--terminal] [--protocol auto|kitty|iterm2|sixel|halfblocks] [--state calm|worn]"
                );
                std::process::exit(0);
            }
            _ => return Err(format!("unknown argument: {argument}").into()),
        }
    }
    if original && goose {
        return Err("--original and --goose are mutually exclusive".into());
    }
    Ok(Args {
        source,
        goose_source,
        output,
        terminal,
        original,
        goose,
        protocol,
        state,
    })
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = parse_args()?;
    if args.original {
        return if args.terminal {
            run_original_terminal(args.protocol)
        } else {
            write_original_review(&args.output)
        };
    }
    if args.goose {
        if !args.goose_source.is_dir() {
            return Err(format!(
                "Goose image directory not found at {}; unpack it locally or pass --goose-source",
                args.goose_source.display()
            )
            .into());
        }
        return if args.terminal {
            run_goose_terminal(&args.goose_source, args.state, args.protocol)
        } else {
            write_goose_review(&args.goose_source, &args.output)
        };
    }
    if !args.source.is_dir() {
        return Err(format!(
            "DoL image directory not found at {}; unpack a local image pack or pass --source",
            args.source.display()
        )
        .into());
    }
    if args.terminal {
        run_terminal(&args.source, args.state, args.protocol)
    } else {
        write_visual_review(&args.source, &args.output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::widgets::Widget;
    use ratatui_image::protocol::Protocol;

    #[test]
    fn layers_are_stably_sorted_and_alpha_composited() {
        let mut bottom = RgbaImage::from_pixel(2, 2, Rgba([200, 20, 10, 255]));
        let top = RgbaImage::from_pixel(1, 1, Rgba([20, 40, 220, 128]));
        imageops::overlay(&mut bottom, &top, 0, 0);
        let mixed = bottom.get_pixel(0, 0);
        assert!(mixed[0] > 100 && mixed[0] < 120);
        assert!(mixed[2] > 110 && mixed[2] < 120);
        assert_eq!(bottom.get_pixel(1, 1), &Rgba([200, 20, 10, 255]));

        let mut unordered = recipes(PortraitState::Calm);
        unordered.reverse();
        unordered.sort_by_key(|layer| layer.z_index);
        assert_eq!(unordered.first().map(|layer| layer.name), Some("base head"));
        assert_eq!(unordered.last().map(|layer| layer.name), Some("brows"));
    }

    #[test]
    fn cache_key_changes_with_appearance_state_and_recipe() {
        let source = Path::new("local-assets");
        let calm = recipes(PortraitState::Calm);
        let worn = recipes(PortraitState::Worn);
        assert_eq!(
            recipe_key(source, PortraitState::Calm, &calm),
            recipe_key(source, PortraitState::Calm, &calm)
        );
        assert_ne!(
            recipe_key(source, PortraitState::Calm, &calm),
            recipe_key(source, PortraitState::Worn, &worn)
        );

        let mut cache = AppearanceCache::default();
        cache
            .resolve_with(7, || Ok(RgbaImage::from_pixel(1, 1, Rgba([1, 2, 3, 255]))))
            .expect("first composition succeeds");
        cache
            .resolve_with(7, || Err("same cache key must not compose again".into()))
            .expect("same key is a cache hit");
        assert_eq!(cache.compose_count, 1);
        cache
            .resolve_with(8, || Ok(RgbaImage::from_pixel(1, 1, Rgba([4, 5, 6, 255]))))
            .expect("changed appearance composes again");
        assert_eq!(cache.compose_count, 2);
        assert_eq!(
            cache.entry.as_ref().map(|(_, image)| image.get_pixel(0, 0)),
            Some(&Rgba([4, 5, 6, 255]))
        );
    }

    #[test]
    fn alpha_masks_keep_left_and_right_eye_tints_independent() {
        let mut source = transparent_art_layer();
        put_pixel(&mut source, 10, 10, Rgba([255, 255, 255, 255]));
        put_pixel(&mut source, 20, 10, Rgba([255, 255, 255, 255]));
        let mut left_mask = transparent_art_layer();
        put_pixel(&mut left_mask, 10, 10, Rgba([255, 255, 255, 255]));
        let mut right_mask = transparent_art_layer();
        put_pixel(&mut right_mask, 20, 10, Rgba([255, 255, 255, 255]));

        let mut left = generated_layer("left", 1, source.clone());
        left.tint = Some([20, 180, 80]);
        left.mask = Some(left_mask);
        let mut right = generated_layer("right", 1, source);
        right.tint = Some([50, 110, 220]);
        right.mask = Some(right_mask);
        let composed = compose_generated(vec![left, right]).expect("masked eyes compose");

        assert_eq!(composed.get_pixel(10, 10), &Rgba([20, 180, 80, 255]));
        assert_eq!(composed.get_pixel(20, 10), &Rgba([50, 110, 220, 255]));
        assert_eq!(composed.get_pixel(15, 10), &Rgba([0, 0, 0, 0]));
    }

    #[test]
    fn representative_canvas_blend_modes_match_expected_pixels() {
        let backdrop = Rgba([100, 100, 100, 255]);
        let foreground = Rgba([200, 200, 200, 255]);
        let channel = |mode| blend_pixel(backdrop, foreground, 255, mode)[0];

        assert_eq!(channel(BlendMode::SourceOver), 200);
        assert_eq!(channel(BlendMode::Multiply), 78);
        assert_eq!(channel(BlendMode::Screen), 222);
        assert_eq!(channel(BlendMode::HardLight), 188);
    }

    #[test]
    fn blink_frame_changes_both_full_body_and_close_up_eyes() {
        let awake = original_frame(DAY_PALETTE, false).expect("awake frame composes");
        let blink = original_frame(DAY_PALETTE, true).expect("blink frame composes");

        let full_body_eye = (46, 54);
        let close_up_eye = (164, 86);
        assert_ne!(
            awake.get_pixel(full_body_eye.0, full_body_eye.1),
            blink.get_pixel(full_body_eye.0, full_body_eye.1)
        );
        assert_ne!(
            awake.get_pixel(close_up_eye.0, close_up_eye.1),
            blink.get_pixel(close_up_eye.0, close_up_eye.1)
        );
    }

    #[test]
    fn halfblocks_render_deterministically_without_a_real_tty() {
        let source = RgbaImage::from_fn(4, 4, |x, y| {
            if (x + y) % 2 == 0 {
                Rgba([240, 80, 60, 255])
            } else {
                Rgba([40, 100, 220, 255])
            }
        });
        let protocol = Protocol::Halfblocks(
            ratatui_image::protocol::halfblocks::Halfblocks::new(
                DynamicImage::ImageRgba8(source),
                Size::new(2, 2),
            )
            .expect("generated image encodes"),
        );
        let mut buffer = ratatui::buffer::Buffer::empty(Rect::new(0, 0, 2, 2));
        Image::new(&protocol).render(buffer.area, &mut buffer);
        assert_eq!(buffer.cell((0, 0)).map(|cell| cell.symbol()), Some("▀"));
        assert!(matches!(
            buffer.cell((0, 0)).map(|cell| cell.fg),
            Some(Color::Rgb(_, _, _))
        ));
    }

    #[test]
    fn terminal_protocol_override_is_explicit_and_validated() {
        assert_eq!(parse_protocol("auto").expect("auto is valid"), None);
        assert_eq!(
            parse_protocol("kitty").expect("kitty is valid"),
            Some(ProtocolType::Kitty)
        );
        assert_eq!(
            parse_protocol("iterm2").expect("iterm2 is valid"),
            Some(ProtocolType::Iterm2)
        );
        assert!(parse_protocol("ansi").is_err());

        let mut picker = Picker::halfblocks();
        picker.set_protocol_type(ProtocolType::Kitty);
        let protocol = picker
            .new_protocol(
                DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 2, Rgba([10, 20, 30, 255]))),
                Size::new(2, 2),
                Resize::Fit(None),
            )
            .expect("forced Kitty protocol encodes");
        assert!(matches!(protocol, Protocol::Kitty(_)));
    }
}
