---
adr_number: 006
author: omega-team
content_revision: 101
date: 2026-03-19
generation_id: gen_000016_r000101
projection_version: 16
related_spec: docs/specs/omega-app-package.md
reviewed_by: []
source_doc_id: "adr:docs-decisions-006-omega-tui-ui-boundary"
status: accepted
---

# 006: `omega-tui` 只拥有 UI 职责

## Status

Accepted

## Context

在交互层开始收敛时，`omega-tui` 仍然混合了 terminal UI、Agent turn orchestration、tracing bootstrap 和一部分前端无关交互状态。继续把功能堆进这个 crate，会削弱 `001` 中“独立 crate、边界清晰”的决策，也会让 `005` 中定义的 observability 基础设施被误实现成 TUI 私有模块。

## Decision

做出以下边界决策：

- `omega-tui` 只保留 terminal UI 直接相关的职责：ratatui 渲染、crossterm 事件适配、terminal 生命周期、TUI 专属状态。
- 顶层应用入口与装配职责不应长期停留在 `omega-tui`；后续应迁移到独立的 `omega-app`。
- `agent_session` 从 `omega-tui` 剥离为新 crate `omega-session`。
- `logging` 从 `omega-tui` 剥离为新 crate `omega-observability`。
- `app.rs` / `event.rs` 中剩余的前端无关交互状态，不要求在第一步全部迁走，但必须以后续候选 crate `omega-interaction` 的边界来约束新增实现。
- `omega-core` 继续保持前端无关，不接受面板、键位、spinner、日志面板等 UI 语义。

## Consequences

### Positive

- `omega-session` 保持前端无关边界，必要时可被未来前端复用。
- `omega-observability` 成为跨前端基础设施，符合 `005` 的设计意图。
- `omega-tui` 后续迭代时，变更面会明显缩小，测试边界更清晰。
- `omega-app` 可承接 provider/config/bootstrap/wiring，避免这些代码重新回流到 UI crate。
- 交互层继续演进时，有明确的“哪些逻辑不能再写进 TUI crate”的判断标准。

### Negative

- workspace 中会新增 crate，需要维护更多 `Cargo.toml`。
- 短期内会有一次纯结构性迁移，代码移动成本不可避免。
- `omega-interaction` 是否真的需要落地，需要在后续实现中继续审视，避免过早抽象。

## Alternatives Considered

### Alternative 1: 保持 `omega-tui` 现状，继续在内部模块化

**Pros**: 不新增 crate，短期动作最少。
**Cons**: 边界仍然由目录结构隐式表达，REPL 和未来前端无法清晰复用。
**Why Rejected**: 已经出现明显的 layer violation，继续拖延只会提高后续迁移成本。

### Alternative 2: 一次性引入统一 frontend runtime crate

**Pros**: 可能减少未来多个前端之间的少量装配重复。
**Cons**: 当前前端种类太少，容易形成过早抽象。
**Why Rejected**: 当前最紧迫的问题是把明显不属于 UI 的职责迁走，而不是再增加一个中间层。

## Notes

- 当前活跃边界见 [docs/specs/omega-app-package.md](../specs/omega-app-package.md) 与 [docs/specs/omega-runtime-ui-message-contract.md](../specs/omega-runtime-ui-message-contract.md)
- Task 15D 的历史设计基线见 [docs/archive/omega-tui-non-ui-extraction.md](../archive/omega-tui-non-ui-extraction.md)
- 本决策的更早历史来源见 [docs/archive/omega-interaction-layer-refactor.md](../archive/omega-interaction-layer-refactor.md)
