---
content_revision: 174
generation_id: gen_000087_r000174
language: en
last_verified_commit: uncommitted+bf370bfab2710af857531eedb308cdbfcfb152bf
projection_version: 87
source_doc_id: "adr:docs-decisions-008-tui-component-architecture-refactor"
source_path: docs/decisions/008-tui-component-architecture-refactor.md
---

# ADR-008: TUI Component Architecture Refactor — Component Layer + Visual Regression Harness

## Overview

Status: accepted (2026-06-03)
Owners: omega-tui working group
Related: `docs/specs/omega-tui-visual-refresh.md`, `docs/specs/omega-tui-ui-reference.md`, `docs/TODO.md` (Task 39 ~ 39I)

## Context

The user-visible Agent Response panel and the broader TUI feel cluttered, inconsistent across panels, and resistant to visual change — repeated design tweaks over the past several iterations have not produced a visible effect on the running UI. A focused exploration of `crates/omega-tui/src/render/`, `crates/omega-tui/src/app/`, and `crates/omega-theme/src/lib.rs` (21 raw observations, see PR description for the JSON evidence payload) surfaces five structural problems that together explain the symptom:

1. **Zero visual regression coverage.** `render_tests.rs` (711 lines, 16+ tests) drives `render()` through `TestBackend` but never inspects the rendered buffer — only rect coordinates and focus state. There is no test that would fail if a panel's text changed, a border type changed, or a focus indicator disappeared. Any visual change either is silent (no test notices) or breaks something only the human running the binary notices.

2. **God render function.** `crates/omega-tui/src/render/layout.rs::render()` is 600 lines and orchestrates seven+ concerns in one body: layout split, palette caching, response line list build/wrap/select, sidebar block+chunks+rail+body, input shell, status bar, focus normalization. Every panel's chrome (title, border type, focus marker) is hard-coded inside this function with no central source of truth.

3. **God `App` struct + 12 `*_rect` fields.** `crates/omega-tui/src/app.rs` is 2154 lines and the `App` struct holds response state, sidebar state, input state, todo state, delivery state, focus state, theme cache, and 12 `*_rect` fields that are written by `render()` and read by event handlers. The render path and the input path share a single mutable struct, which makes the rendering contract implicit and any refactor risky.

4. **God response module.** `crates/omega-tui/src/app/response.rs` is 2622 lines and contains the entire response-section lifecycle, message-type classification, line emission, and section merging in one module. Adding a new `MsgKind` variant currently requires edits in `style.rs`, `response.rs`, `delivery.rs`, and `diagnostics.rs` (shotgun surgery).

5. **God `RenderPalette`.** `crates/omega-theme/src/lib.rs::RenderPalette` is a 61-field flat struct that flattens seven themed sub-modules (Surfaces, Input, ContextBar, StatusBar, Report, Messages, Overlay) plus ad-hoc markdown/code/badge/final-answer/thinking color tuples. The `render_palette()` constructor mixes: (a) one-to-one passthroughs from themed sub-modules, (b) deliberate cross-purposing where one source color feeds multiple role fields (e.g. `surfaces.title_fg` is copied into `title_fg`, `heading_1_fg`, AND `user_badge_fg`), and (c) five `Color::Rgb(..., ..., ...)` literal escapes that bypass the configurable theme entirely (`inline_code_bg`, `code_block_bg`, `warning_badge`, `error_badge`, `thinking_body`).

Architectural scoring against the five principles in `.claude/skills/architect/SKILL.md`:

| Principle | Score | Why |
|---|---|---|
| Single Responsibility | 1/2 | `render()` is a god function; `App` is a god struct; `app/response.rs` is a god module |
| Separation of Concerns | 1/2 | response styling split between `layout.rs` and `style.rs`; theme construction flattens abstractions; five `Color::Rgb` literals bypass the theme system |
| Scalability | 1/2 | adding a `MsgKind` requires 4+ file edits; new panel titles require editing the god render function; sidebar section order is implicit in code order |
| Testability | 1/2 | 0 visual-regression tests on the rendered buffer; 16+ tests assert only rect dimensions |
| Maintainability | 1/2 | layout magic numbers (60/100 width thresholds, 30%/34% split, `Constraint::Length(2/9)`) are inlined; theme TOML has 30+ unlabeled hex colors |

Total: **5/10 (Fair, significant refactoring required).**

Five of nine red flags from the architect skill table apply: God Objects (multiple), Big Ball of Mud (`RenderPalette` cross-mapping), Layer Violations (`*_rect` shared between render and event paths), Shotgun Surgery (`MsgKind` change), Hidden Globals (`App.cached_palette` with no invalidation hook).

## Decision

We will refactor the TUI rendering and theme layers along four architectural axes, ordered so that each later step can be validated against the regression net built in the first step:

**A. Component abstraction layer.** Introduce named widget builders (`Panel`, `Section`, `Card`) in a new `crates/omega-tui/src/render/component.rs` module that capture the repeated pattern of `Block::default().borders(Borders::ALL).border_type(...).title(...)` + content. Every existing inline `Block::default()` in `render/` is migrated to one of these builders. The builders take a `PaletteView` (not a raw `RenderPalette`) so they only see the colors they need.

**B. View model separation.** Split `App` into `ViewModel` (data the renderer reads) and `Controller` (event handlers mutate). The 12 `*_rect` fields move from `App` to a per-frame `Frame` struct that lives only for the duration of `render()`. Event handlers request layout via the controller and read rects off a returned `Frame` snapshot, breaking the implicit coupling.

**C. Theme catalog split.** Replace the 61-field `RenderPalette` with a small set of role-specific sub-palettes (`SurfacePalette`, `MessagePalette`, `MarkdownPalette`, `OverlayPalette`). Each render path takes the sub-palette it needs. The five `Color::Rgb` literals in `render_palette()` move to `DEFAULT_THEME_TOML` so the configurable theme covers all visible colors. The cross-purpose duplication (`surfaces.title_fg` → `title_fg` + `heading_1_fg` + `user_badge_fg`) is broken: each role gets its own source field.

**D. Visual regression harness.** Add buffer-snapshot tests to `render_tests.rs` that drive `render()` through `TestBackend` and assert on `terminal.buffer()` cell content (string form) for each panel, in multiple states (focused/unfocused, sidebar collapsed/expanded, empty response/with response). These tests run on every `cargo test` invocation and are the contract that any later refactor (A/B/C) must keep green.

The four axes are not independent: A's `PaletteView` cannot be built until C defines the sub-palettes; B's `ViewModel` cannot be cleanly extracted until A's `Panel` builder owns the rect mutation; D's snapshot suite is the validation net for the whole sequence. Execution order is therefore:

1. **D first** (Task 39): snapshot baseline so the rest of the work has regression coverage.
2. **A, B, C, D-component-name** in parallel where safe (Task 39A–39D).
3. **`render()` split** (Task 39E) once A–D land.
4. **`App` split** (Task 39F) once `render()` is decomposed.
5. **`response.rs` split** (Task 39G) once `App` is decomposed.
6. **`RenderPalette` split** (Task 39H) once `response.rs` is decomposed.
7. **Theme TOML annotation** (Task 39I) last, after the palette split.

The user symptom — "改什么都没变化" — is addressed by axis D first: with snapshot tests in place, visual changes are observable. The structural symptoms — clutter, inconsistency, hard to refactor — are addressed by axes A–C in order.

## Alternatives considered

**A1. Do nothing, keep tweaking individual colors and strings.** Rejected. The 5/10 score indicates the cost of incremental tweaks is rising while the success rate is falling. Without a regression harness, every change is unverifiable from CI.

**A2. Skip D, refactor structure first.** Rejected. The architect skill warns against premature abstraction; refactoring `render()` into components without snapshot tests means each component extraction is a leap of faith. D is the safety net for the whole refactor.

**A3. Rewrite the renderer (e.g. switch from ratatui to tui-rs-tree or a custom layout engine).** Rejected. The user's complaint is visual and structural, not about the rendering engine. The 1177-line sidebar and 600-line god render function would still exist after a renderer swap. We address the engine only if the new component layer can't be expressed within ratatui's widget model — to be evaluated during Task 39A.

**A4. Big-bang rewrite of `App` into a pure Elm-style Model+View+Update.** Rejected. Too large a single change. The split-into-`ViewModel`+`Controller` direction (axis B) is the Elm-style move but done incrementally so each slice is verifiable.

## Consequences

Positive:

- Visual changes become observable from CI (axis D).
- Each panel's chrome lives in one place (axis A), so "make all panels feel quieter" is a single theme edit, not a god-function surgery.
- The 61-field palette becomes a role-indexed catalog (axis C), so designers can change `user_badge_fg` without accidentally also changing `heading_1_fg`.
- `App` shrinks to a state container (axis B), making event handler reasoning local.
- Subsequent additions (new `MsgKind`, new sidebar section, new theme) become additive rather than shotgun-surgery.

Negative:

- Snapshot tests in `render_tests.rs` will be brittle against intentional visual change — every deliberate visual edit will need to regenerate snapshots. We accept this as the cost of regression coverage; the alternative (silently break the visual) is worse.
- Axis C requires touching every render call site that takes `&RenderPalette`. We will update them in one PR to keep the diff coherent.
- The split into `ViewModel` + `Controller` + `Frame` adds one indirection at the event-handler boundary. We accept this as the cost of breaking the implicit `*_rect` coupling.

Out of scope (will not be addressed by this refactor):

- Switching TUI engines (ratatui stays).
- i18n of panel titles. Chrome centralization (axis A) makes i18n cheaper later but does not itself introduce it.
- Re-specifying the TUI visual philosophy from scratch. The existing `docs/specs/omega-tui-visual-refresh.md` and `docs/specs/omega-tui-ui-reference.md` remain the design intent; this refactor is structural, not visual-revision.

## Plan

See `docs/TODO.md` (Task 39 ~ 39I) for the ordered task list. Each task has a single concrete deliverable, an explicit dependency on the prior task, and a snapshot-test pass as the validation gate.
