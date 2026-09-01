use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender, TryRecvError},
    },
    thread::JoinHandle,
};

use loreloom_agent::CancellationToken;
use loreloom_core::{
    NoticeKind, RuntimePhase, RuntimeProgressEvent, ShortText, UiNotice, UiSnapshot,
};
use loreloom_runtime::{GameRuntime, RuntimeError};
use loreloom_tui::{RuntimeClient, RuntimeUiEvent, UiClientError};

use crate::error::AppError;

enum RuntimeCommand {
    Submit(String),
    Shutdown,
}

pub struct RuntimeAdapter {
    commands: Sender<RuntimeCommand>,
    events: Receiver<Result<RuntimeUiEvent, UiClientError>>,
    cancellation: CancellationToken,
    accepting: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    shutdown_requested: bool,
}

impl RuntimeAdapter {
    pub fn spawn(mut runtime: GameRuntime) -> Result<Self, AppError> {
        let cancellation = runtime.cancellation_token();
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let accepting = Arc::new(AtomicBool::new(true));
        let worker_accepting = Arc::clone(&accepting);
        let worker = std::thread::Builder::new()
            .name("loreloom-runtime".to_owned())
            .spawn(move || {
                let tokio = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(_) => {
                        worker_accepting.store(false, Ordering::SeqCst);
                        let _ = event_tx.send(Err(UiClientError::new("runtime_start_failed")));
                        return;
                    }
                };
                while let Ok(command) = command_rx.recv() {
                    match command {
                        RuntimeCommand::Submit(input) => {
                            let progress_events = event_tx.clone();
                            let turn =
                                runtime.handle_player_input_with_progress(input, move |progress| {
                                    let event = match progress {
                                        RuntimeProgressEvent::PhaseChanged(phase) => {
                                            RuntimeUiEvent::PhaseChanged(phase)
                                        }
                                        RuntimeProgressEvent::ToolActivityChanged(activity) => {
                                            RuntimeUiEvent::ToolActivityChanged(activity)
                                        }
                                    };
                                    let _ = progress_events.send(Ok(event));
                                });
                            let result = tokio.block_on(turn);
                            let event = match result {
                                Ok(outcome) => RuntimeUiEvent::Snapshot(Box::new(outcome.snapshot)),
                                Err(error) => match tokio.block_on(runtime.initial_snapshot()) {
                                    Ok(snapshot) => RuntimeUiEvent::Snapshot(Box::new(
                                        failed_snapshot(snapshot, &error),
                                    )),
                                    Err(_) => {
                                        worker_accepting.store(true, Ordering::SeqCst);
                                        let _ = event_tx.send(Err(UiClientError::new(
                                            "runtime_snapshot_failed",
                                        )));
                                        continue;
                                    }
                                },
                            };
                            worker_accepting.store(true, Ordering::SeqCst);
                            let _ = event_tx.send(Ok(event));
                        }
                        RuntimeCommand::Shutdown => break,
                    }
                }
                worker_accepting.store(false, Ordering::SeqCst);
            })?;
        Ok(Self {
            commands: command_tx,
            events: event_rx,
            cancellation,
            accepting,
            worker: Some(worker),
            shutdown_requested: false,
        })
    }

    fn request_shutdown(&mut self) -> Result<(), UiClientError> {
        if self.shutdown_requested {
            return Ok(());
        }
        self.shutdown_requested = true;
        self.cancellation.cancel();
        let _ = self.commands.send(RuntimeCommand::Shutdown);
        Ok(())
    }
}

impl RuntimeClient for RuntimeAdapter {
    fn submit(&mut self, input: String) -> Result<(), UiClientError> {
        if self.shutdown_requested {
            return Err(UiClientError::new("runtime_shutdown"));
        }
        if self
            .accepting
            .compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(UiClientError::new("runtime_busy"));
        }
        match self.commands.send(RuntimeCommand::Submit(input)) {
            Ok(()) => Ok(()),
            Err(_) => {
                self.accepting.store(true, Ordering::SeqCst);
                Err(UiClientError::new("runtime_disconnected"))
            }
        }
    }

    fn cancel(&mut self) -> Result<(), UiClientError> {
        self.cancellation.cancel();
        Ok(())
    }

    fn try_recv(&mut self) -> Result<Option<RuntimeUiEvent>, UiClientError> {
        match self.events.try_recv() {
            Ok(Ok(event)) => Ok(Some(event)),
            Ok(Err(error)) => Err(error),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) if self.shutdown_requested => Ok(None),
            Err(TryRecvError::Disconnected) => Err(UiClientError::new("runtime_disconnected")),
        }
    }

    fn shutdown(&mut self) -> Result<(), UiClientError> {
        self.request_shutdown()
    }
}

impl Drop for RuntimeAdapter {
    fn drop(&mut self) {
        let _ = self.request_shutdown();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn failed_snapshot(mut snapshot: UiSnapshot, error: &RuntimeError) -> UiSnapshot {
    let code = error.code();
    snapshot.phase = if code == "cancelled" {
        RuntimePhase::Cancelled
    } else {
        RuntimePhase::Failed
    };
    snapshot.can_submit = true;
    snapshot.can_cancel = false;
    snapshot.waiting = false;
    let message = match error {
        RuntimeError::BridgeUnavailable(diagnostic) => {
            format!("Model request failed · {}", diagnostic.user_summary())
        }
        _ => format!("Turn ended: {code}"),
    };
    if let Ok(message) = ShortText::new(message) {
        snapshot.notices.push(UiNotice {
            kind: if code == "cancelled" {
                NoticeKind::Info
            } else {
                NoticeKind::Error
            },
            message,
        });
    }
    snapshot
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};
    use std::{path::Path, sync::Arc};

    use armillae_core::{
        AssistantContent, CompletionRequest, CompletionResponse, FinishReason, TextContent,
    };
    use armillae_llm::{
        BoxFuture, BridgeCapabilities, BridgeError, CompletionStream, ErrorMetadata, LlmBridge,
        ProjectionReport,
    };
    use loreloom_agent::{ModelFailureDiagnostic, ModelFailureStage, ModelInvocationKind};
    use loreloom_core::{NoticeKind, RuntimePhase};
    use loreloom_runtime::RuntimeError;
    use loreloom_tui::RuntimeClient;

    use super::{RuntimeAdapter, RuntimeCommand, failed_snapshot};
    use crate::{
        config::ConfiguredProviders,
        world::{WorldSetup, build_world_with},
    };

    struct TextBridge;

    impl LlmBridge for TextBridge {
        fn capabilities(&self) -> BridgeCapabilities {
            BridgeCapabilities::all()
        }

        fn project(&self, _request: &CompletionRequest) -> Result<ProjectionReport, BridgeError> {
            Ok(ProjectionReport::exact("loreloom-test"))
        }

        fn complete<'a>(
            &'a self,
            _request: CompletionRequest,
        ) -> BoxFuture<'a, Result<CompletionResponse, BridgeError>> {
            Box::pin(async {
                Ok(CompletionResponse {
                    id: None,
                    model: Some("loreloom-test".to_owned()),
                    content: vec![AssistantContent::Text(TextContent::new("雨声仍在继续。"))],
                    finish_reason: Some(FinishReason::Stop),
                    usage: None,
                    provider_metadata: serde_json::Value::Null,
                })
            })
        }

        fn stream<'a>(
            &'a self,
            _request: CompletionRequest,
        ) -> BoxFuture<'a, Result<CompletionStream, BridgeError>> {
            Box::pin(async {
                Err(BridgeError::InvalidRequest {
                    message: "streaming is not used in tests".to_owned(),
                })
            })
        }
    }

    fn test_setup(io: &tokio::runtime::Runtime, save_path: &Path) -> WorldSetup {
        let bridge: Arc<dyn LlmBridge> = Arc::new(TextBridge);
        io.block_on(build_world_with(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
            save_path,
            &[],
            ConfiguredProviders {
                narrator: Arc::clone(&bridge),
                npc: bridge,
                runtime: Default::default(),
                rules: Default::default(),
            },
        ))
        .expect("test world setup")
    }

    #[test]
    fn submit_queues_without_waiting_for_a_worker_and_reports_busy() {
        let (command_tx, command_rx) = std::sync::mpsc::channel();
        let (_event_tx, event_rx) = std::sync::mpsc::channel();
        let mut adapter = RuntimeAdapter {
            commands: command_tx,
            events: event_rx,
            cancellation: loreloom_agent::CancellationToken::new(),
            accepting: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
            worker: None,
            shutdown_requested: false,
        };

        adapter.submit("first".to_owned()).expect("queue is open");
        assert_eq!(
            adapter
                .submit("second".to_owned())
                .expect_err("only one turn can be queued or running")
                .code,
            "runtime_busy"
        );
        assert!(matches!(
            command_rx.try_recv(),
            Ok(RuntimeCommand::Submit(input)) if input == "first"
        ));
    }

    #[test]
    fn failed_snapshot_exposes_only_safe_model_diagnostics() {
        let temporary = tempfile::tempdir().expect("temporary save root");
        let io = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        let setup = test_setup(&io, &temporary.path().join("save"));
        let bridge_error = BridgeError::ProviderRejected {
            code: Some("must-not-escape".to_owned()),
            message: "secret body must-not-escape".to_owned(),
            metadata: ErrorMetadata::new("openai-compatible")
                .with_http_status(400)
                .with_request_id("req_ui-safe"),
        };
        let diagnostic = ModelFailureDiagnostic::from_bridge_error(
            ModelInvocationKind::Narrator,
            ModelFailureStage::Invocation,
            &bridge_error,
        );
        let correlation_id = diagnostic.correlation_id.to_string();
        let failed = failed_snapshot(
            setup.initial_snapshot,
            &RuntimeError::BridgeUnavailable(diagnostic),
        );

        assert_eq!(failed.phase, RuntimePhase::Failed);
        let notice = failed.notices.last().expect("failure notice");
        assert_eq!(notice.kind, NoticeKind::Error);
        assert!(notice.message.as_str().contains("provider_rejected"));
        assert!(notice.message.as_str().contains("HTTP 400"));
        assert!(notice.message.as_str().contains(&correlation_id));
        assert!(!notice.message.as_str().contains("must-not-escape"));
    }

    #[test]
    fn worker_publishes_phase_events_and_completed_snapshots_and_rearms_cancellation() {
        let temporary = tempfile::tempdir().expect("temporary save root");
        let io = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        let setup = test_setup(&io, &temporary.path().join("save"));
        let initial_revision = setup.initial_snapshot.revision;
        let mut adapter = RuntimeAdapter::spawn(setup.runtime).expect("runtime worker");
        let held_cancellation = adapter.cancellation.clone();

        adapter
            .submit("Ask Mira what she hears in the rain.".to_owned())
            .expect("first turn accepted");
        wait_for_phase(&mut adapter, RuntimePhase::PersistingInput);

        let completed = wait_for_snapshot(&mut adapter, RuntimePhase::Completed);
        assert!(completed.revision > initial_revision);
        assert!(!completed.waiting);
        assert!(completed.can_submit);
        assert!(!completed.transcript.items.is_empty());

        adapter.cancel().expect("idle cancellation is safe");
        assert!(held_cancellation.is_cancelled());
        adapter
            .submit("Continue the conversation.".to_owned())
            .expect("second turn accepted");
        wait_for_phase(&mut adapter, RuntimePhase::PersistingInput);
        assert!(
            !held_cancellation.is_cancelled(),
            "the token must be reset before the cancellable working snapshot is published"
        );
        let second = wait_for_snapshot(&mut adapter, RuntimePhase::Completed);
        assert!(second.revision > completed.revision);
        assert!(
            !held_cancellation.is_cancelled(),
            "the GameRuntime must reset the same shared token for the next turn"
        );

        adapter.shutdown().expect("shutdown requested");
        adapter.shutdown().expect("shutdown is idempotent");
        assert_eq!(
            adapter
                .submit("too late".to_owned())
                .expect_err("a stopped adapter rejects new turns")
                .code,
            "runtime_shutdown"
        );
        adapter
            .worker
            .take()
            .expect("worker handle")
            .join()
            .expect("worker exits after shutdown");
    }

    fn wait_for_snapshot(
        adapter: &mut RuntimeAdapter,
        phase: RuntimePhase,
    ) -> Box<loreloom_core::UiSnapshot> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut last_snapshot = None;
        loop {
            match adapter.try_recv().expect("runtime event") {
                Some(loreloom_tui::RuntimeUiEvent::Snapshot(snapshot))
                    if snapshot.phase == phase =>
                {
                    return snapshot;
                }
                Some(loreloom_tui::RuntimeUiEvent::Snapshot(snapshot)) => {
                    last_snapshot = Some(snapshot);
                }
                Some(_) | None if Instant::now() < deadline => {
                    std::thread::yield_now();
                }
                Some(_) | None => panic!(
                    "timed out waiting for {phase:?} snapshot; last snapshot: {last_snapshot:?}"
                ),
            }
        }
    }

    fn wait_for_phase(adapter: &mut RuntimeAdapter, phase: RuntimePhase) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match adapter.try_recv().expect("runtime event") {
                Some(loreloom_tui::RuntimeUiEvent::PhaseChanged(observed)) if observed == phase => {
                    return;
                }
                Some(_) | None if Instant::now() < deadline => {
                    std::thread::yield_now();
                }
                Some(_) | None => panic!("timed out waiting for {phase:?} phase"),
            }
        }
    }
}
