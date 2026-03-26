use crate::host::{HookAdvanceDecision, HookDispatchInput, HookDispatchOutput, HookEventKind};

const TODO_MANAGED_EXECUTE_HOOK_ID: &str = "todo_managed_execute";

pub(crate) fn invoke_builtin(
    hook_id: &str,
    input: &HookDispatchInput,
) -> Option<HookDispatchOutput> {
    match hook_id {
        TODO_MANAGED_EXECUTE_HOOK_ID => Some(todo_managed_execute(input)),
        _ => None,
    }
}

fn todo_managed_execute(input: &HookDispatchInput) -> HookDispatchOutput {
    let mut output = HookDispatchOutput {
        storage: input.storage.clone(),
        ..HookDispatchOutput::default()
    };

    if !matches!(input.event, HookEventKind::BeforeAdvance) {
        return output;
    }

    if input.step_id != "execute" || !input.todo.has_open_items {
        output.advance = Some(HookAdvanceDecision::Allow);
        return output;
    }

    let reason = if input.todo.rounds_without_update == 0 {
        "todo items remain open".to_string()
    } else {
        format!(
            "todo items remain open after {} unchanged round(s)",
            input.todo.rounds_without_update
        )
    };

    output.advance = Some(HookAdvanceDecision::Deny { reason });
    output
}