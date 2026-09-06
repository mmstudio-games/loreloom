use std::{
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use image::DynamicImage;
use loreloom_appearance::{AppearanceCatalog, AppearanceRenderKey};
use loreloom_core::{AppearanceView, UiSnapshot};
use ratatui::layout::Size;
use ratatui_image::{
    Resize,
    picker::{Picker, ProtocolType},
    protocol::Protocol,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ImageProtocolPreference {
    #[default]
    Auto,
    Kitty,
    Iterm2,
    Sixel,
    Halfblocks,
    Disabled,
}

impl ImageProtocolPreference {
    pub(crate) const fn forced(self) -> Option<ProtocolType> {
        match self {
            Self::Auto | Self::Disabled => None,
            Self::Kitty => Some(ProtocolType::Kitty),
            Self::Iterm2 => Some(ProtocolType::Iterm2),
            Self::Sixel => Some(ProtocolType::Sixel),
            Self::Halfblocks => Some(ProtocolType::Halfblocks),
        }
    }
}

pub(crate) struct AppearancePresenter {
    catalog: Arc<AppearanceCatalog>,
    requests: Option<Sender<RenderRequest>>,
    results: Receiver<RenderResult>,
    worker: Option<JoinHandle<()>>,
    requested: Option<(AppearanceRenderKey, Size)>,
    generation: u64,
    prepared: Option<PreparedAppearance>,
    prepared_at: Instant,
    failure: bool,
    protocol_type: ProtocolType,
}

struct RenderRequest {
    generation: u64,
    appearance: AppearanceView,
    target: Size,
}

struct RenderResult {
    generation: u64,
    prepared: Result<PreparedAppearance, ()>,
}

struct PreparedAppearance {
    key: AppearanceRenderKey,
    target: Size,
    frames: Vec<PreparedFrame>,
}

struct PreparedFrame {
    duration: Duration,
    protocol: Protocol,
}

impl AppearancePresenter {
    pub(crate) fn new(catalog: AppearanceCatalog, picker: Picker) -> std::io::Result<Self> {
        let protocol_type = picker.protocol_type();
        let catalog = Arc::new(catalog);
        let worker_catalog = Arc::clone(&catalog);
        let (request_tx, request_rx) = mpsc::channel::<RenderRequest>();
        let (result_tx, result_rx) = mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("loreloom-appearance".to_owned())
            .spawn(move || worker_loop(worker_catalog, picker, request_rx, result_tx))?;
        Ok(Self {
            catalog,
            requests: Some(request_tx),
            results: result_rx,
            worker: Some(worker),
            requested: None,
            generation: 0,
            prepared: None,
            prepared_at: Instant::now(),
            failure: false,
            protocol_type,
        })
    }

    pub(crate) fn sync(&mut self, snapshot: &UiSnapshot, target: Option<Size>) {
        self.sync_view(snapshot.player.appearance.as_ref(), target);
    }

    fn clear(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.requested = None;
        self.prepared = None;
        self.failure = false;
    }

    fn sync_view(&mut self, appearance: Option<&AppearanceView>, target: Option<Size>) {
        while let Ok(result) = self.results.try_recv() {
            self.apply_result(result);
        }

        let Some(appearance) = appearance else {
            self.clear();
            return;
        };
        let Some(target) = target.filter(|target| target.width > 0 && target.height > 0) else {
            self.clear();
            return;
        };
        let Ok(key) = self.catalog.render_key(appearance) else {
            self.clear();
            self.failure = true;
            return;
        };
        if self.requested == Some((key, target)) {
            return;
        }
        self.generation = self.generation.wrapping_add(1);
        self.requested = Some((key, target));
        self.failure = false;
        if self.requests.as_ref().is_none_or(|sender| {
            sender
                .send(RenderRequest {
                    generation: self.generation,
                    appearance: appearance.clone(),
                    target,
                })
                .is_err()
        }) {
            self.failure = true;
        }
    }

    pub(crate) fn protocol(&self) -> Option<&Protocol> {
        let prepared = self.prepared.as_ref()?;
        if self.failure || self.requested != Some((prepared.key, prepared.target)) {
            return None;
        }
        let elapsed = self.prepared_at.elapsed();
        let total = prepared.frames.iter().fold(Duration::ZERO, |total, frame| {
            total.saturating_add(frame.duration)
        });
        if total.is_zero() {
            return prepared.frames.first().map(|frame| &frame.protocol);
        }
        let mut position = Duration::from_millis(
            u64::try_from(elapsed.as_millis() % total.as_millis()).unwrap_or(0),
        );
        for frame in &prepared.frames {
            if position < frame.duration {
                return Some(&frame.protocol);
            }
            position = position.saturating_sub(frame.duration);
        }
        prepared.frames.last().map(|frame| &frame.protocol)
    }

    pub(crate) fn status(&self) -> Option<&'static str> {
        if self.failure {
            Some("portrait unavailable")
        } else if self.requested.is_some()
            && self
                .prepared
                .as_ref()
                .is_none_or(|prepared| self.requested != Some((prepared.key, prepared.target)))
        {
            Some("preparing portrait…")
        } else if self.protocol_type == ProtocolType::Halfblocks && self.protocol().is_some() {
            Some("Portrait: text mode")
        } else {
            None
        }
    }

    fn apply_result(&mut self, result: RenderResult) {
        if result.generation != self.generation {
            return;
        }
        match result.prepared {
            Ok(prepared) => {
                self.prepared = Some(prepared);
                self.prepared_at = Instant::now();
                self.failure = false;
            }
            Err(()) => {
                self.failure = true;
            }
        }
    }
}

impl Drop for AppearancePresenter {
    fn drop(&mut self) {
        drop(self.requests.take());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn worker_loop(
    catalog: Arc<AppearanceCatalog>,
    picker: Picker,
    requests: Receiver<RenderRequest>,
    results: Sender<RenderResult>,
) {
    while let Ok(mut request) = requests.recv() {
        while let Ok(newer) = requests.try_recv() {
            request = newer;
        }
        let generation = request.generation;
        let prepared = prepare(&catalog, &picker, request).map_err(|_| ());
        if results
            .send(RenderResult {
                generation,
                prepared,
            })
            .is_err()
        {
            break;
        }
    }
}

fn prepare(
    catalog: &AppearanceCatalog,
    picker: &Picker,
    request: RenderRequest,
) -> Result<PreparedAppearance, ()> {
    let composed = catalog.compose(&request.appearance).map_err(|_| ())?;
    let frames = composed
        .frames
        .into_iter()
        .map(|frame| {
            let protocol = picker
                .new_protocol(
                    DynamicImage::ImageRgba8(frame.image),
                    request.target,
                    // Scale the full authored canvas, including transparent margins.
                    Resize::Scale(Some(image::imageops::FilterType::Nearest)),
                )
                .map_err(|_| ())?;
            Ok(PreparedFrame {
                duration: Duration::from_millis(u64::from(frame.duration_ms)),
                protocol,
            })
        })
        .collect::<Result<Vec<_>, ()>>()?;
    Ok(PreparedAppearance {
        key: composed.key,
        target: request.target,
        frames,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (AppearanceCatalog, AppearanceView) {
        fixture_with_image(image::RgbaImage::from_pixel(
            8,
            8,
            image::Rgba([255, 0, 0, 255]),
        ))
    }

    fn fixture_with_image(image: image::RgbaImage) -> (AppearanceCatalog, AppearanceView) {
        let namespace = "games.loreloom.portrait-test".parse().expect("namespace");
        let pack = br#"schema_version = 1
pack_id = "games.loreloom.portrait-test:appearance_pack/main"
[[models]]
id = "games.loreloom.portrait-test:appearance_model/player"
canvas_width = 8
canvas_height = 8
[[models.frames]]
name = "idle"
duration_ms = 100
[[models.frames.layers]]
name = "body"
z_index = 0
source = "appearance/images/body.png"
"#;
        let mut png = std::io::Cursor::new(Vec::new());
        image
            .write_to(&mut png, image::ImageFormat::Png)
            .expect("png");
        let catalog = AppearanceCatalog::compile([
            (&namespace, "appearance/pack.toml", pack.as_slice()),
            (
                &namespace,
                "appearance/images/body.png",
                png.get_ref().as_slice(),
            ),
        ])
        .expect("catalog");
        (
            catalog,
            AppearanceView {
                revision: loreloom_core::Revision::new(0),
                model_id: "games.loreloom.portrait-test:appearance_model/player"
                    .parse()
                    .expect("model"),
                parameters: Default::default(),
            },
        )
    }

    #[test]
    fn halfblocks_draw_pixels_and_only_report_status_for_an_actual_portrait() {
        let (catalog, view) = fixture();
        let mut presenter =
            AppearancePresenter::new(catalog, Picker::halfblocks()).expect("worker");
        assert_eq!(presenter.status(), None);
        presenter.sync_view(Some(&view), Some(Size::new(8, 6)));
        assert_eq!(presenter.status(), Some("preparing portrait…"));
        let result = presenter
            .results
            .recv_timeout(Duration::from_secs(5))
            .expect("image result");
        presenter.apply_result(result);
        assert_eq!(presenter.status(), Some("Portrait: text mode"));
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(16, 10)).expect("terminal");
        terminal
            .draw(|frame| {
                frame.render_widget(
                    ratatui_image::Image::new(presenter.protocol().expect("portrait")),
                    frame.area(),
                )
            })
            .expect("draw");
        assert!(
            terminal.backend().buffer().content.iter().any(|cell| {
                matches!(cell.fg, ratatui::style::Color::Rgb(red, 0, 0) if red > 0)
                    || matches!(cell.bg, ratatui::style::Color::Rgb(red, 0, 0) if red > 0)
            }),
            "halfblocks must draw the composed red image: {:?}",
            terminal.backend().buffer()
        );
        presenter.sync_view(None, Some(Size::new(8, 6)));
        assert_eq!(presenter.status(), None);
        assert!(presenter.protocol().is_none());
    }

    #[test]
    fn portrait_scales_with_available_space_and_preserves_transparent_canvas() {
        // A narrow subject in a square canvas must retain its authored margins.
        let mut source = image::RgbaImage::new(8, 8);
        for y in 2..6 {
            for x in 3..5 {
                source.put_pixel(x, y, image::Rgba([255, 0, 0, 255]));
            }
        }
        let (catalog, view) = fixture_with_image(source);
        let picker = Picker::halfblocks(); // 10 x 20 pixels per cell.
        for (target, expected) in [
            (Size::new(8, 6), Size::new(8, 4)),
            (Size::new(16, 10), Size::new(16, 8)),
        ] {
            let prepared = prepare(
                &catalog,
                &picker,
                RenderRequest {
                    generation: 1,
                    appearance: view.clone(),
                    target,
                },
            )
            .expect("prepare");
            let protocol = &prepared.frames[0].protocol;
            assert_eq!(
                protocol.size(),
                expected,
                "square canvas must scale to fit in pixel space"
            );
            let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(
                target.width,
                target.height,
            ))
            .expect("terminal");
            terminal
                .draw(|frame| {
                    frame.render_widget(ratatui_image::Image::new(protocol), frame.area())
                })
                .expect("draw");
            let buffer = terminal.backend().buffer();
            let is_red = |cell: &ratatui::buffer::Cell| {
                matches!(cell.fg, ratatui::style::Color::Rgb(r, 0, 0) if r > 0)
                    || matches!(cell.bg, ratatui::style::Color::Rgb(r, 0, 0) if r > 0)
            };
            assert!(is_red(&buffer[(expected.width / 2, expected.height / 2)]));
            assert!(
                !is_red(&buffer[(0, 0)]),
                "transparent margin must not be cropped"
            );
            assert!(!is_red(&buffer[(expected.width - 1, expected.height - 1)]));
        }
    }

    #[test]
    fn hidden_or_removed_portrait_invalidates_in_flight_results() {
        for missing_view in [false, true] {
            let (catalog, view) = fixture();
            let mut presenter =
                AppearancePresenter::new(catalog, Picker::halfblocks()).expect("worker");
            presenter.sync_view(Some(&view), Some(Size::new(8, 6)));
            let result = presenter
                .results
                .recv_timeout(Duration::from_secs(5))
                .expect("image result");
            if missing_view {
                presenter.sync_view(None, Some(Size::new(8, 6)));
            } else {
                presenter.sync_view(Some(&view), None);
            }
            presenter.apply_result(result);
            assert!(presenter.protocol().is_none());
            assert_eq!(presenter.status(), None);
        }
    }

    #[test]
    fn stale_worker_failure_cannot_replace_the_current_generation() {
        let mut presenter = AppearancePresenter::new(
            AppearanceCatalog::default(),
            ratatui_image::picker::Picker::halfblocks(),
        )
        .expect("start appearance worker");
        presenter.generation = 2;

        presenter.apply_result(RenderResult {
            generation: 1,
            prepared: Err(()),
        });
        assert!(!presenter.failure);

        presenter.apply_result(RenderResult {
            generation: 2,
            prepared: Err(()),
        });
        assert!(presenter.failure);
    }
}
