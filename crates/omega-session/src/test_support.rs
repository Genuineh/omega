use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::runtime_message::legacy_runtime_ui_envelopes;
use crate::{
    RuntimeMessage, RuntimeMessageBridge, RuntimeMessageEnvelope, RuntimeUiBridge, RuntimeUiEffect,
    RuntimeUiEnvelope, SharedRuntimeMessageBridge, StateMessage, StatusSlot, StatusValue,
};

#[derive(Debug, Default)]
struct RecorderState {
    runtime_messages: Vec<RuntimeMessageEnvelope>,
    ui_envelopes: Vec<RuntimeUiEnvelope>,
}

#[derive(Debug, Default)]
struct RecorderInner {
    state: Mutex<RecorderState>,
    updated: Condvar,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeEnvelopeRecorder {
    inner: Arc<RecorderInner>,
}

impl RuntimeEnvelopeRecorder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn runtime_messages(&self) -> Vec<RuntimeMessageEnvelope> {
        self.inner.state.lock().unwrap().runtime_messages.clone()
    }

    pub fn runtime_bridge(&self) -> SharedRuntimeMessageBridge {
        Arc::new(self.clone())
    }

    pub fn ui_envelopes(&self) -> Vec<RuntimeUiEnvelope> {
        self.inner.state.lock().unwrap().ui_envelopes.clone()
    }

    pub fn legacy_ui_envelopes(&self) -> Vec<RuntimeUiEnvelope> {
        self.runtime_messages()
            .into_iter()
            .flat_map(legacy_runtime_ui_envelopes)
            .collect()
    }

    pub fn wait_for_turn_finished_messages(
        &self,
        turn_id: u64,
        timeout: Duration,
    ) -> Vec<RuntimeMessageEnvelope> {
        self.wait_for_runtime(timeout, |messages| {
            messages.iter().any(|envelope| {
                envelope.turn_id == turn_id
                    && matches!(
                        envelope.message,
                        RuntimeMessage::State(StateMessage::TurnFinished)
                    )
            })
        })
    }

    pub fn wait_for_idle_ui(&self, turn_id: u64, timeout: Duration) -> Vec<RuntimeUiEnvelope> {
        self.wait_for_runtime(timeout, |messages| {
            messages
                .iter()
                .flat_map(|envelope| legacy_runtime_ui_envelopes(envelope.clone()))
                .any(|envelope| {
                    matches!(
                        envelope,
                        RuntimeUiEnvelope::Effect {
                            turn_id: envelope_turn_id,
                            effect: RuntimeUiEffect::SetStatusSlot {
                                slot: StatusSlot::Agent,
                                value: StatusValue::Label(label),
                            },
                        } if envelope_turn_id == turn_id && label == "Idle"
                    )
                })
        })
        .into_iter()
        .flat_map(legacy_runtime_ui_envelopes)
        .collect()
    }

    pub fn direct_ui_envelopes(&self) -> Vec<RuntimeUiEnvelope> {
        self.ui_envelopes()
    }

    pub fn wait_for_direct_idle_ui(
        &self,
        turn_id: u64,
        timeout: Duration,
    ) -> Vec<RuntimeUiEnvelope> {
        self.wait_for_ui(timeout, |envelopes| {
            envelopes.iter().any(|envelope| {
                matches!(
                    envelope,
                    RuntimeUiEnvelope::Effect {
                        turn_id: envelope_turn_id,
                        effect: RuntimeUiEffect::SetStatusSlot {
                            slot: StatusSlot::Agent,
                            value: StatusValue::Label(label),
                        },
                    } if *envelope_turn_id == turn_id && label == "Idle"
                )
            })
        })
    }

    fn wait_for_runtime<F>(&self, timeout: Duration, predicate: F) -> Vec<RuntimeMessageEnvelope>
    where
        F: Fn(&[RuntimeMessageEnvelope]) -> bool,
    {
        let deadline = Instant::now() + timeout;
        let mut guard = self.inner.state.lock().unwrap();
        while !predicate(&guard.runtime_messages) {
            let now = Instant::now();
            assert!(now < deadline, "timed out waiting for runtime envelopes");
            let remaining = deadline.saturating_duration_since(now);
            let (next_guard, wait_result) =
                self.inner.updated.wait_timeout(guard, remaining).unwrap();
            guard = next_guard;
            assert!(
                !wait_result.timed_out(),
                "timed out waiting for runtime envelopes"
            );
        }
        guard.runtime_messages.clone()
    }

    fn wait_for_ui<F>(&self, timeout: Duration, predicate: F) -> Vec<RuntimeUiEnvelope>
    where
        F: Fn(&[RuntimeUiEnvelope]) -> bool,
    {
        let deadline = Instant::now() + timeout;
        let mut guard = self.inner.state.lock().unwrap();
        while !predicate(&guard.ui_envelopes) {
            let now = Instant::now();
            assert!(now < deadline, "timed out waiting for runtime ui envelopes");
            let remaining = deadline.saturating_duration_since(now);
            let (next_guard, wait_result) =
                self.inner.updated.wait_timeout(guard, remaining).unwrap();
            guard = next_guard;
            assert!(
                !wait_result.timed_out(),
                "timed out waiting for runtime ui envelopes"
            );
        }
        guard.ui_envelopes.clone()
    }
}

impl RuntimeMessageBridge for RuntimeEnvelopeRecorder {
    fn send(&self, envelope: RuntimeMessageEnvelope) {
        let mut guard = self.inner.state.lock().unwrap();
        guard.runtime_messages.push(envelope);
        self.inner.updated.notify_all();
    }
}

impl RuntimeUiBridge for RuntimeEnvelopeRecorder {
    fn send(&self, envelope: RuntimeUiEnvelope) {
        let mut guard = self.inner.state.lock().unwrap();
        guard.ui_envelopes.push(envelope);
        self.inner.updated.notify_all();
    }
}
