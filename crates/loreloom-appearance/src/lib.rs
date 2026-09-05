//! Terminal-independent appearance packs, validation, and deterministic RGBA composition.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::Cursor,
    sync::Arc,
};

use image::{ImageFormat, ImageReader, Limits, Rgba, RgbaImage, imageops};
use loreloom_core::{AppearanceValue, AppearanceView, ContentDefinitionId, ModId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const APPEARANCE_PACK_SCHEMA_V1: u32 = 1;
pub const APPEARANCE_PACK_PATH: &str = "appearance/pack.toml";
pub const MAX_CANVAS_EDGE: u32 = 512;
pub const MAX_MODELS_PER_PACK: usize = 64;
pub const MAX_PARAMETERS_PER_MODEL: usize = 64;
pub const MAX_FRAMES_PER_MODEL: usize = 16;
pub const MAX_LAYERS_PER_FRAME: usize = 128;
pub const MAX_SPRITE_FRAMES: u32 = 16;
const MAX_DECODED_IMAGE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppearancePack {
    pub schema_version: u32,
    pub pack_id: ContentDefinitionId,
    pub models: Vec<AppearanceModel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppearanceModel {
    pub id: ContentDefinitionId,
    pub canvas_width: u32,
    pub canvas_height: u32,
    #[serde(default)]
    pub parameters: Vec<AppearanceParameterDefault>,
    pub frames: Vec<AppearanceFrame>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppearanceParameterDefault {
    pub id: ContentDefinitionId,
    pub value: AppearanceValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppearanceFrame {
    pub name: String,
    pub duration_ms: u32,
    pub layers: Vec<AppearanceLayer>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppearanceLayer {
    pub name: String,
    pub z_index: i16,
    pub source: String,
    #[serde(default)]
    pub source_frame: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask_frame: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tint: Option<AppearanceTint>,
    #[serde(default = "opaque")]
    pub opacity: u8,
    #[serde(default)]
    pub blend: BlendMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<AppearancePredicate>,
}

const fn opaque() -> u8 {
    u8::MAX
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum AppearanceTint {
    Fixed { rgb: [u8; 3] },
    Parameter { id: ContentDefinitionId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppearancePredicate {
    pub parameter: ContentDefinitionId,
    pub equals: AppearanceValue,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlendMode {
    #[default]
    SourceOver,
    Multiply,
    Screen,
    HardLight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AppearanceRenderKey([u8; 32]);

impl AppearanceRenderKey {
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct ComposedAppearance {
    pub key: AppearanceRenderKey,
    pub width: u32,
    pub height: u32,
    pub frames: Vec<ComposedFrame>,
}

#[derive(Debug, Clone)]
pub struct ComposedFrame {
    pub name: String,
    pub duration_ms: u32,
    pub image: RgbaImage,
}

#[derive(Debug, Clone, Default)]
pub struct AppearanceCatalog {
    models: BTreeMap<ContentDefinitionId, CompiledModel>,
}

impl PartialEq for AppearanceCatalog {
    fn eq(&self, other: &Self) -> bool {
        self.models.len() == other.models.len()
            && self.models.iter().all(|(id, model)| {
                other.models.get(id).is_some_and(|candidate| {
                    model.width == candidate.width
                        && model.height == candidate.height
                        && model.digest == candidate.digest
                })
            })
    }
}

impl Eq for AppearanceCatalog {}

#[derive(Debug, Clone)]
struct CompiledModel {
    width: u32,
    height: u32,
    defaults: BTreeMap<ContentDefinitionId, AppearanceValue>,
    frames: Vec<CompiledFrame>,
    digest: [u8; 32],
}

#[derive(Debug, Clone)]
struct CompiledFrame {
    name: String,
    duration_ms: u32,
    layers: Vec<CompiledLayer>,
}

#[derive(Debug, Clone)]
struct CompiledLayer {
    z_index: i16,
    declaration_order: usize,
    source: Arc<RgbaImage>,
    source_frame: u32,
    mask: Option<Arc<RgbaImage>>,
    mask_frame: u32,
    tint: Option<AppearanceTint>,
    opacity: u8,
    blend: BlendMode,
    when: Option<AppearancePredicate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AppearanceError {
    #[error("appearance pack TOML is invalid")]
    InvalidPack,
    #[error("appearance pack uses an unsupported schema")]
    UnsupportedSchema,
    #[error("appearance pack namespace or definition kind is invalid")]
    InvalidIdentity,
    #[error("appearance pack exceeds a resource limit")]
    ResourceLimit,
    #[error("appearance pack contains a duplicate model, parameter, frame, or layer")]
    Duplicate,
    #[error("appearance resource path is unsafe or unsupported")]
    UnsafePath,
    #[error("appearance image is missing")]
    MissingImage,
    #[error("appearance PNG cannot be decoded within the configured limits")]
    InvalidImage,
    #[error("appearance image dimensions or sprite frame are invalid")]
    InvalidDimensions,
    #[error("appearance parameter declaration or value is invalid")]
    InvalidParameter,
    #[error("appearance model is unavailable")]
    ModelUnavailable,
}

impl AppearanceCatalog {
    /// Compile every fixed `appearance/pack.toml` entry from validated package resources.
    pub fn compile<'a>(
        resources: impl IntoIterator<Item = (&'a ModId, &'a str, &'a [u8])>,
    ) -> Result<Self, AppearanceError> {
        let resources = resources
            .into_iter()
            .map(|(namespace, path, bytes)| ((namespace.clone(), path.to_owned()), bytes))
            .collect::<BTreeMap<_, _>>();
        let mut decoded = BTreeMap::<(ModId, String), Arc<RgbaImage>>::new();
        let mut models = BTreeMap::new();

        for ((namespace, path), pack_bytes) in &resources {
            if path != APPEARANCE_PACK_PATH {
                continue;
            }
            let source =
                std::str::from_utf8(pack_bytes).map_err(|_| AppearanceError::InvalidPack)?;
            let pack: AppearancePack =
                toml::from_str(source).map_err(|_| AppearanceError::InvalidPack)?;
            validate_pack_identity(&pack, namespace)?;
            if pack.models.is_empty() || pack.models.len() > MAX_MODELS_PER_PACK {
                return Err(AppearanceError::ResourceLimit);
            }
            for model in pack.models {
                let (id, compiled) =
                    compile_model(namespace, model, pack_bytes, &resources, &mut decoded)?;
                if models.insert(id, compiled).is_some() {
                    return Err(AppearanceError::Duplicate);
                }
            }
        }

        Ok(Self { models })
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.models.is_empty()
    }

    #[must_use]
    pub fn contains_model(&self, id: &ContentDefinitionId) -> bool {
        self.models.contains_key(id)
    }

    pub fn render_key(
        &self,
        appearance: &AppearanceView,
    ) -> Result<AppearanceRenderKey, AppearanceError> {
        let model = self
            .models
            .get(&appearance.model_id)
            .ok_or(AppearanceError::ModelUnavailable)?;
        let parameters = resolve_parameters(model, &appearance.parameters)?;
        let mut digest = Sha256::new();
        digest.update(model.digest);
        digest.update(appearance.model_id.to_string().as_bytes());
        for (id, value) in parameters {
            digest.update(id.to_string().as_bytes());
            hash_value(&mut digest, &value);
        }
        Ok(AppearanceRenderKey(digest.finalize().into()))
    }

    pub fn compose(
        &self,
        appearance: &AppearanceView,
    ) -> Result<ComposedAppearance, AppearanceError> {
        let model = self
            .models
            .get(&appearance.model_id)
            .ok_or(AppearanceError::ModelUnavailable)?;
        let parameters = resolve_parameters(model, &appearance.parameters)?;
        let key = self.render_key(appearance)?;
        let mut frames = Vec::with_capacity(model.frames.len());
        for frame in &model.frames {
            let mut canvas = RgbaImage::from_pixel(model.width, model.height, Rgba([0, 0, 0, 0]));
            let mut layers = frame.layers.iter().collect::<Vec<_>>();
            layers.sort_by_key(|layer| (layer.z_index, layer.declaration_order));
            for layer in layers {
                if layer.when.as_ref().is_some_and(|predicate| {
                    parameters.get(&predicate.parameter) != Some(&predicate.equals)
                }) {
                    continue;
                }
                let mut source =
                    select_frame(&layer.source, model.width, model.height, layer.source_frame)?;
                if let Some(tint) = &layer.tint {
                    let rgb = match tint {
                        AppearanceTint::Fixed { rgb } => *rgb,
                        AppearanceTint::Parameter { id } => match parameters.get(id) {
                            Some(AppearanceValue::Color { rgb }) => *rgb,
                            _ => return Err(AppearanceError::InvalidParameter),
                        },
                    };
                    tint_mask(&mut source, rgb);
                }
                if let Some(mask) = &layer.mask {
                    let mask = select_frame(mask, model.width, model.height, layer.mask_frame)?;
                    apply_alpha_mask(&mut source, &mask);
                }
                blend_image(&mut canvas, &source, layer.opacity, layer.blend);
            }
            frames.push(ComposedFrame {
                name: frame.name.clone(),
                duration_ms: frame.duration_ms,
                image: canvas,
            });
        }
        Ok(ComposedAppearance {
            key,
            width: model.width,
            height: model.height,
            frames,
        })
    }
}

fn validate_pack_identity(pack: &AppearancePack, namespace: &ModId) -> Result<(), AppearanceError> {
    if pack.schema_version != APPEARANCE_PACK_SCHEMA_V1 {
        return Err(AppearanceError::UnsupportedSchema);
    }
    if pack.pack_id.mod_id().ok().as_ref() != Some(namespace)
        || pack.pack_id.kind().ok() != Some("appearance_pack")
    {
        return Err(AppearanceError::InvalidIdentity);
    }
    Ok(())
}

fn compile_model(
    namespace: &ModId,
    model: AppearanceModel,
    pack_bytes: &[u8],
    resources: &BTreeMap<(ModId, String), &[u8]>,
    decoded: &mut BTreeMap<(ModId, String), Arc<RgbaImage>>,
) -> Result<(ContentDefinitionId, CompiledModel), AppearanceError> {
    if model.id.mod_id().ok().as_ref() != Some(namespace)
        || model.id.kind().ok() != Some("appearance_model")
    {
        return Err(AppearanceError::InvalidIdentity);
    }
    if model.canvas_width == 0
        || model.canvas_height == 0
        || model.canvas_width > MAX_CANVAS_EDGE
        || model.canvas_height > MAX_CANVAS_EDGE
        || model.parameters.len() > MAX_PARAMETERS_PER_MODEL
        || model.frames.is_empty()
        || model.frames.len() > MAX_FRAMES_PER_MODEL
    {
        return Err(AppearanceError::ResourceLimit);
    }

    let mut defaults = BTreeMap::new();
    for parameter in model.parameters {
        if parameter.id.mod_id().ok().as_ref() != Some(namespace)
            || parameter.id.kind().ok() != Some("appearance_parameter")
            || !valid_appearance_value(&parameter.value)
            || defaults.insert(parameter.id, parameter.value).is_some()
        {
            return Err(AppearanceError::InvalidParameter);
        }
    }

    let mut digest = Sha256::new();
    digest.update(pack_bytes);
    let mut used_paths = BTreeSet::new();
    let mut frame_names = BTreeSet::new();
    let mut frames = Vec::with_capacity(model.frames.len());
    for frame in model.frames {
        if !valid_local_name(&frame.name)
            || !frame_names.insert(frame.name.clone())
            || !(50..=60_000).contains(&frame.duration_ms)
            || frame.layers.is_empty()
            || frame.layers.len() > MAX_LAYERS_PER_FRAME
        {
            return Err(AppearanceError::ResourceLimit);
        }
        let mut layer_names = BTreeSet::new();
        let mut layers = Vec::with_capacity(frame.layers.len());
        for (declaration_order, layer) in frame.layers.into_iter().enumerate() {
            if !valid_local_name(&layer.name) || !layer_names.insert(layer.name) {
                return Err(AppearanceError::Duplicate);
            }
            validate_image_path(&layer.source)?;
            used_paths.insert(layer.source.clone());
            let source = decode_resource(namespace, &layer.source, resources, decoded)?;
            validate_sprite(
                &source,
                model.canvas_width,
                model.canvas_height,
                layer.source_frame,
            )?;
            let mask = match layer.mask {
                Some(path) => {
                    validate_image_path(&path)?;
                    used_paths.insert(path.clone());
                    let image = decode_resource(namespace, &path, resources, decoded)?;
                    let frame_index = layer.mask_frame.unwrap_or(layer.source_frame);
                    validate_sprite(&image, model.canvas_width, model.canvas_height, frame_index)?;
                    Some((image, frame_index))
                }
                None if layer.mask_frame.is_some() => {
                    return Err(AppearanceError::InvalidDimensions);
                }
                None => None,
            };
            validate_layer_parameters(&defaults, layer.tint.as_ref(), layer.when.as_ref())?;
            layers.push(CompiledLayer {
                z_index: layer.z_index,
                declaration_order,
                source,
                source_frame: layer.source_frame,
                mask: mask.as_ref().map(|(image, _)| Arc::clone(image)),
                mask_frame: mask.map_or(0, |(_, frame)| frame),
                tint: layer.tint,
                opacity: layer.opacity,
                blend: layer.blend,
                when: layer.when,
            });
        }
        frames.push(CompiledFrame {
            name: frame.name,
            duration_ms: frame.duration_ms,
            layers,
        });
    }
    for path in used_paths {
        let bytes = resources
            .get(&(namespace.clone(), path))
            .ok_or(AppearanceError::MissingImage)?;
        digest.update(bytes);
    }
    Ok((
        model.id,
        CompiledModel {
            width: model.canvas_width,
            height: model.canvas_height,
            defaults,
            frames,
            digest: digest.finalize().into(),
        },
    ))
}

fn validate_layer_parameters(
    defaults: &BTreeMap<ContentDefinitionId, AppearanceValue>,
    tint: Option<&AppearanceTint>,
    predicate: Option<&AppearancePredicate>,
) -> Result<(), AppearanceError> {
    if let Some(AppearanceTint::Parameter { id }) = tint
        && !matches!(defaults.get(id), Some(AppearanceValue::Color { .. }))
    {
        return Err(AppearanceError::InvalidParameter);
    }
    if let Some(predicate) = predicate {
        let Some(default) = defaults.get(&predicate.parameter) else {
            return Err(AppearanceError::InvalidParameter);
        };
        if !valid_appearance_value(&predicate.equals)
            || std::mem::discriminant(default) != std::mem::discriminant(&predicate.equals)
        {
            return Err(AppearanceError::InvalidParameter);
        }
    }
    Ok(())
}

fn resolve_parameters(
    model: &CompiledModel,
    overrides: &BTreeMap<ContentDefinitionId, AppearanceValue>,
) -> Result<BTreeMap<ContentDefinitionId, AppearanceValue>, AppearanceError> {
    let mut parameters = model.defaults.clone();
    for (id, value) in overrides {
        let Some(default) = parameters.get(id) else {
            return Err(AppearanceError::InvalidParameter);
        };
        if !valid_appearance_value(value)
            || std::mem::discriminant(default) != std::mem::discriminant(value)
        {
            return Err(AppearanceError::InvalidParameter);
        }
        parameters.insert(id.clone(), value.clone());
    }
    Ok(parameters)
}

fn valid_appearance_value(value: &AppearanceValue) -> bool {
    !matches!(
        value,
        AppearanceValue::Variant { id } if id.kind().ok() != Some("appearance_variant")
    )
}

fn decode_resource(
    namespace: &ModId,
    path: &str,
    resources: &BTreeMap<(ModId, String), &[u8]>,
    decoded: &mut BTreeMap<(ModId, String), Arc<RgbaImage>>,
) -> Result<Arc<RgbaImage>, AppearanceError> {
    let key = (namespace.clone(), path.to_owned());
    if let Some(image) = decoded.get(&key) {
        return Ok(Arc::clone(image));
    }
    let bytes = resources.get(&key).ok_or(AppearanceError::MissingImage)?;
    let mut reader = ImageReader::with_format(Cursor::new(*bytes), ImageFormat::Png);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_CANVAS_EDGE * MAX_SPRITE_FRAMES);
    limits.max_image_height = Some(MAX_CANVAS_EDGE);
    limits.max_alloc = Some(MAX_DECODED_IMAGE_BYTES);
    reader.limits(limits);
    let image = Arc::new(
        reader
            .decode()
            .map_err(|_| AppearanceError::InvalidImage)?
            .to_rgba8(),
    );
    decoded.insert(key, Arc::clone(&image));
    Ok(image)
}

fn validate_sprite(
    image: &RgbaImage,
    width: u32,
    height: u32,
    frame: u32,
) -> Result<(), AppearanceError> {
    if image.height() != height
        || image.width() < width
        || !image.width().is_multiple_of(width)
        || image.width() / width > MAX_SPRITE_FRAMES
        || frame >= image.width() / width
    {
        return Err(AppearanceError::InvalidDimensions);
    }
    Ok(())
}

fn select_frame(
    image: &RgbaImage,
    width: u32,
    height: u32,
    frame: u32,
) -> Result<RgbaImage, AppearanceError> {
    validate_sprite(image, width, height, frame)?;
    Ok(imageops::crop_imm(image, frame * width, 0, width, height).to_image())
}

fn validate_image_path(path: &str) -> Result<(), AppearanceError> {
    if !path.starts_with("appearance/images/")
        || !path.ends_with(".png")
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains('\0')
        || path
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(AppearanceError::UnsafePath);
    }
    Ok(())
}

fn valid_local_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.is_ascii()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn hash_value(digest: &mut Sha256, value: &AppearanceValue) {
    match value {
        AppearanceValue::Color { rgb } => {
            digest.update([0]);
            digest.update(rgb);
        }
        AppearanceValue::Variant { id } => {
            digest.update([1]);
            digest.update(id.to_string().as_bytes());
        }
        AppearanceValue::Boolean { value } => {
            digest.update([2, u8::from(*value)]);
        }
    }
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

fn apply_alpha_mask(image: &mut RgbaImage, mask: &RgbaImage) {
    for (pixel, mask_pixel) in image.pixels_mut().zip(mask.pixels()) {
        pixel[3] = multiply_channel(pixel[3], mask_pixel[3]);
    }
}

fn blend_image(destination: &mut RgbaImage, source: &RgbaImage, opacity: u8, mode: BlendMode) {
    for (backdrop, foreground) in destination.pixels_mut().zip(source.pixels()) {
        *backdrop = blend_pixel(*backdrop, *foreground, opacity, mode);
    }
}

fn blend_pixel(backdrop: Rgba<u8>, foreground: Rgba<u8>, opacity: u8, mode: BlendMode) -> Rgba<u8> {
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
        let blended = match mode {
            BlendMode::SourceOver => source_channel,
            BlendMode::Multiply => backdrop_channel * source_channel,
            BlendMode::Screen => {
                backdrop_channel + source_channel - backdrop_channel * source_channel
            }
            BlendMode::HardLight if source_channel <= 0.5 => {
                2.0 * backdrop_channel * source_channel
            }
            BlendMode::HardLight => 1.0 - 2.0 * (1.0 - backdrop_channel) * (1.0 - source_channel),
        };
        let premultiplied = source_alpha * (1.0 - backdrop_alpha) * source_channel
            + source_alpha * backdrop_alpha * blended
            + (1.0 - source_alpha) * backdrop_alpha * backdrop_channel;
        output[channel] = ((premultiplied / output_alpha) * 255.0).round() as u8;
    }
    output[3] = (output_alpha * 255.0).round() as u8;
    Rgba(output)
}
