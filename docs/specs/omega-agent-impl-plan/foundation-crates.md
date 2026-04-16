---
content_revision: 101
created: 2026-03-25
generation_id: gen_000016_r000101
last_verified_commit: N/A
owner: omega-team
projection_version: 16
related_prds: []
source_doc_id: "spec:docs-specs-omega-agent-impl-plan-foundation-crates"
status: active
supersedes: []
updated: 2026-03-25
---

# Omega Agent Plan: Foundation Crates

## Overview

本文收敛原实现计划中的 Tasks 1-7，聚焦工作空间底座与首批基础 crate。需要总览时返回 `../omega-agent-impl-plan.md`。

## Covered Tasks

| Task | Status | Scope |
|------|--------|-------|
| Task 1 | Completed | Cargo workspace 初始化 |
| Task 2 | Completed baseline | `omega-client` 抽象与 provider 接入起点 |
| Task 3 | Planned baseline | `omega-message` 消息系统 |
| Task 4 | Planned baseline | `omega-tasks` 任务系统 |
| Task 5 | Planned baseline | `omega-skills` skill 加载 |
| Task 6 | Planned baseline | `omega-worktree` 隔离 |
| Task 7 | Planned baseline | `omega-tools` 工具抽象 |

## Task 1: 工作空间初始化

- Status: Completed
- Scope: 建立 workspace members、共享 package metadata 与通用依赖。
- Deliverable: 根 `Cargo.toml` 成为所有 crate 的统一依赖与版本入口。
- Verification: workspace 层 Cargo 配置已可支撑后续 crate 增长。

## Task 2: omega-client - LLM 抽象与 Minimax 适配器

- Status: Completed baseline
- Scope: 建立 `LlmClient` 抽象、请求/响应模型、provider config 与 Minimax 兼容适配器。
- Current role: 作为后续 Anthropic-compatible provider 抽象与 streaming contract 的基础层。
- Related follow-up: provider 内部拆分与 Anthropic-compatible transport 细节见其他 spec / TODO 子任务，不再在本计划入口内展开代码级草稿。

## Task 3: omega-message - 消息系统

- Status: Planned baseline
- Scope: 提供 JSONL-backed inbox/outbox message bus，支撑 agent/team 间低依赖消息交换。
- Design intent: 文件系统优先、追加写入、最小读取协议。
- Expected verification: `cargo build -p omega-message`

## Task 4: omega-tasks - 任务系统

- Status: Planned baseline
- Scope: typed task model、状态流转与基础持久化接口。
- Design intent: 为 workflow、subagent、team 分工提供最小任务语义，而不是直接把执行状态塞进 UI 层。

## Task 5: omega-skills - Skill 加载

- Status: Planned baseline
- Scope: 技能元数据、匹配、加载协议。
- Design intent: 让 skill 作为可组合能力被 session/subagent 重用，而不是 prompt 文本散落在各执行器中。

## Task 6: omega-worktree - Worktree 隔离

- Status: Planned baseline
- Scope: 独立工作目录、生命周期管理、并发执行隔离。
- Design intent: 为未来子执行体与受控仓库变更提供稳定边界。

## Task 7: omega-tools - 工具抽象

- Status: Planned baseline
- Scope: tool trait、schema 暴露、dispatch contract。
- Design intent: 工具系统应保持与 UI、workflow 配置分离，成为 `omega-core` 与 session runtime 的共同底层。

## Notes

- 这些任务定义了整个 workspace 的下层能力边界，后续 crate 重构应优先保持 public contract 稳定。
- 已完成的 crate 若发生大文件拆分或内部重构，应以对应 crate 当前实现和 `docs/TODO.md` 中的 follow-up 为准。

---

### Change Log

- 2026-03-25: 从 `omega-agent-impl-plan.md` 中拆出 Tasks 1-7，保留任务语义和边界说明，移除不再适合作为长期 source of truth 的大段代码草稿。
