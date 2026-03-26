use omega_session::{RuntimeMessage, RuntimeMessageEnvelope};

use crate::engine::TuiSurface;

pub trait RuntimeMessagePolicy: Send + Sync {
    fn apply(&self, surface: &mut dyn TuiSurface, message: RuntimeMessage);
}

pub fn apply_runtime_message_with_policy(
    active_turn_id: u64,
    envelope: RuntimeMessageEnvelope,
    policy: &dyn RuntimeMessagePolicy,
    surface: &mut dyn TuiSurface,
) -> bool {
    if active_turn_id != envelope.turn_id {
        return false;
    }

    policy.apply(surface, envelope.message);
    true
}