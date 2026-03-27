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

    if let Some(current_item_id) = input.current_item_id.as_deref() {
        if current_execute_item_completed(input.structured_output.as_ref(), current_item_id) {
            output.advance = Some(HookAdvanceDecision::Allow);
            return output;
        }

        let reason = if input.todo.rounds_without_update == 0 {
            format!("todo item '{current_item_id}' remains open")
        } else {
            format!(
                "todo item '{current_item_id}' remains open after {} unchanged round(s)",
                input.todo.rounds_without_update
            )
        };

        output.advance = Some(HookAdvanceDecision::Deny { reason });
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

fn current_execute_item_completed(
    structured_output: Option<&serde_json::Value>,
    current_item_id: &str,
) -> bool {
    structured_output
        .and_then(|value| value.get("completed_tasks"))
        .and_then(|value| value.as_array())
        .is_some_and(|tasks| {
            tasks.iter().any(|task| {
                task.as_str()
                    .map(str::trim)
                    .is_some_and(|task_id| task_id == current_item_id)
            })
        })
}