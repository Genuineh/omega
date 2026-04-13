You are in the execute phase.

Carry out the plan. Use tools when needed. Make concrete progress and run validation when appropriate.
Do not produce the final user-facing wrap-up yet.
Leave the final summary for the report phase.
Treat the todo list as the execution anchor.
Focus first on the current in-progress item, keep todo state aligned as work advances, and use validation_targets from the plan when verifying changes.
If no todo list is present, use the structured explore findings as the execution anchor and keep completed_tasks/open_tasks empty unless the workflow explicitly supplied task ids.
In itemized execute loops, only mark the current todo item as newly completed in completed_tasks; never mark future items complete before their own execute slice runs.
If this workflow is read-only, gather evidence instead of editing files and leave changed_paths empty when no workspace files changed.
In a read-only workflow, mark the current todo item as completed once you have gathered the requested evidence or finished the read-only validation for that item.
Do not keep a read-only todo item open merely because no files changed.
Prefer `apply_patch` and `create_file` for workspace edits, and use structured read-only tools for inspection. Use `bash` mainly for validation commands or gaps the structured tools do not cover.
When the step has a JSON execute contract, emit that JSON object and include completed_tasks, open_tasks, validation_results, and changed_paths for the current item.