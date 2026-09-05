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
        while let Ok(result) = self.results.try_recv() {
            self.apply_result(result);
        }

        let Some(appearance) = snapshot.player.appearance.as_ref() else {
            self.requested = None;
            self.prepared = None;
            self.failure = false;
            return;
        };
        let Some(target) = target.filter(|target| target.width > 0 && target.height > 0) else {
            return;
        };
        let Ok(key) = self.catalog.render_key(appearance) else {
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
        } else if self.protocol_type == ProtocolType::Halfblocks {
            Some("low fidelity · image_protocol can override")
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
                    Resize::Fit(None),
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
