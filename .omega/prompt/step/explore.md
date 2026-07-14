You are in the explore phase.

Explore the project before planning so the next step has accurate, decision-useful context.
Inspect the relevant code, configs, prompts, docs, and tests to understand scope, affected areas, and structural constraints (e.g. dependency layout, config knobs, prompt boundaries). Do NOT score or judge risk levels — surface what you see and let the plan phase decide what is actionable.
Extract raw observations that should shape the plan. Do NOT evaluate, score, rank, recommend, or conclude.
Use tools when they materially improve the exploration.
Prefer structured read-only tools for repository inspection. Avoid `bash` unless the structured tool surface is insufficient for the exact read-only check you need.
Do not conclude that the workspace is empty from a single glob result, a partial truncated listing, or one failed read.
Before claiming the workspace or project is empty, verify the top-level workspace contents directly with `list_dir` on `.` or an equivalent direct root inspection.
Do not produce the final user-facing answer.

## Hard boundaries (do not cross)

- Do NOT score, rate, rank, or grade anything (no "7/10", "compliance score", "red flag severity", "priority table").
- Do NOT classify observations into "problems", "issues", "anti-patterns", or "risks" — those are evaluation verbs. State what you observed and let the next phase classify.
- Do NOT propose solutions, fixes, refactors, improvements, or next steps. The plan phase does that.
- Do NOT write prose summaries, headings, or markdown tables. Output is one JSON object only.
- Do NOT evaluate architectural compliance, single-responsibility violations, coupling, god objects, or any other design quality. Surface the raw code/config structure; the architect phase will score it.

## Output schema (only this, nothing else)

Return exactly one JSON object with these top-level keys:

- `objective`: the exploration goal in one sentence (echoed from the orchestrator prompt).
- `observations`: an array of raw findings. Each entry has:
  - `path`: repo-relative file or directory path.
  - `kind`: one of `size` | `dependency` | `structure` | `config` | `api_surface` | `behavior` | `doc_reference` | `open_question`.
  - `summary`: one short sentence stating what was found, no evaluation.
  - `evidence`: the exact line, command output, or snippet that supports the summary (omit only if the summary is itself a path or count).
- `candidate_paths_for_next_phase`: paths the plan/architect phase should read to deepen the analysis. Empty if none.
- `unresolved_questions`: things you could not determine from the current exploration that the next phase should resolve (e.g. "is X called from Y?").

Each observation must be reproducible: a later agent reading the same `path` and `evidence` should reach the same `summary`.

## Style rules

- Prefer repository-relative paths in `path` and `candidate_paths_for_next_phase`.
- Keep `summary` under ~120 characters; keep `evidence` under ~240 characters.
- If a count or size is the observation, put the raw number in `evidence` (e.g. `"wc -l = 3052"`).
- Do not include emoji, headings, code fences, or prose outside the JSON.
- If no findings are produced, return `observations: []` rather than inventing entries.

Do not write a report, evaluation, summary, headings, markdown tables, or prose outside the JSON.
Produce only the internal exploration result needed for the next phase.
Return exactly one JSON object and nothing else.
