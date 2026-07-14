---
content_revision: 174
generation_id: gen_000087_r000174
language: en
last_verified_commit: d8c30e3e9e310ce38cffa965be4688ed55a87787
owner: omega-team
projection_version: 87
source_doc_id: "spec:docs-specs-omega-deterministic-test-seams"
source_path: docs/specs/omega-deterministic-test-seams.md
status: active
version: v0.1
---

# Omega Deterministic Test Seams

## Purpose

This document defines the deterministic test seams for Omega. The rule is strict: mock only true external boundaries, keep workflow/session/core behavior on real implementations, and centralize the mock harnesses so tests do not grow crate-local fake clients or ad hoc temp-root helpers.

## External Boundaries To Mock

Use shared harnesses for these boundaries:

- LLM chat, streaming, mid-stream failure, and count-token requests via `omega-client::test_support::ScriptedLlmClient`
- Runtime envelope capture and ordering assertions via `omega-session::RuntimeEnvelopeRecorder`
- Bash/process execution via the injected `BashCommandRunner` seam in `omega-tools-builtin`
- Temp roots and filesystem-scoped fixtures via `omega-test-support::{test_root, persistent_test_root}`
- TUI key sequence replay via the `EventReplayHarness` in `omega-tui` tests

## Logic That Must Stay Real-Tested

Do not replace these with mocks in normal regression tests:

- `omega-core::Agent` turn loop, tool-result threading, hidden-tool behavior, and todo reminder logic
- `omega-session` workflow routing, structured input/output validation, repair, diagnostics, and todo-managed execute flow
- `omega-app` runtime message policy and stale-turn filtering
- `omega-hooks::HookHost` lifecycle dispatch and builtin hook behavior
- real built-in tool contracts for file search, patching, create/edit, and bash allowlist policy

## Shared Harness Contracts

### LLM

- Queue chat/stream responses independently from `count_tokens` responses.
- Always record chat requests and `count_tokens` requests separately.
- Prefer `ScriptedLlmClient` over crate-local `MockLlmClient` implementations.
- Use local custom clients only for behaviors the shared harness does not model, such as a never-ending pending stream.

### Runtime Envelopes

- Tests should prefer `RuntimeEnvelopeRecorder` over `mpsc::channel` plus handwritten receive loops.
- Recorder-based assertions should wait for `TurnFinished` or the legacy `Idle` status before inspecting the captured sequence.
- Legacy UI assertions should derive envelopes from recorded runtime messages instead of adding a second ad hoc capture path.

### Process And Filesystem

- Use the real system runner by default for integration tests that verify bash allowlist and workspace path safety.
- Use the injected `BashCommandRunner` seam for deterministic timeout and execution-error unit tests.
- Use `test_root` when the test can keep ownership of the temp directory.
- Use `persistent_test_root` when existing test helpers pass around `PathBuf` without owning a temp-dir guard.

### TUI Replay

- Prefer replaying ordered key sequences through a shared harness rather than manually duplicating `handle_key_event(...)` calls across tests.
- Keep reducer and overlay logic real; only the event source should be scripted.

## Migration Rule

When adding or repairing tests in `omega-core`, `omega-session`, `omega-subagent`, `omega-hooks`, `omega-tools-builtin`, or `omega-tui`:

1. Reuse an existing shared seam if one exists.
2. Add a new shared seam only when the behavior is an external boundary and the current harness cannot model it.
3. Do not add a new crate-local mock client or timestamp-based temp-root helper.

## Current Implementations

- `omega-client::test_support::ScriptedLlmClient`
- `omega-session::RuntimeEnvelopeRecorder`
- `omega-test-support`
- `omega-tools-builtin::bash::BashCommandRunner`
- `omega-tui` event replay harness in `src/event_tests.rs`
