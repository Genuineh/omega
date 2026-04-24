---
content_revision: 120
created: 2026-03-25
generation_id: gen_000046_r000120
last_verified_commit: N/A
owner: omega-team
projection_version: 46
related_prds: []
source_doc_id: "spec:docs-specs-omega-agent-impl-plan-execution-runtime-crates"
status: active
supersedes: []
updated: 2026-03-25
---

# Omega Agent Plan: Execution And Runtime Crates

## Overview

本文收敛原实现计划中的 Tasks 8-14，覆盖从 builtin tools 到核心 agent loop 的执行运行时能力。

## Covered Tasks

| Task | Status | Scope |
|------|--------|-------|
| Task 8 | Implemented baseline | `omega-tools-builtin` |
| Task 9 | Implemented baseline | `omega-todo` |
| Task 10 | Implemented baseline | `omega-subagent` |
| Task 11 | Implemented baseline | `omega-compression` |
| Task 12 | Implemented baseline | `omega-background` |
| Task 13 | Implemented baseline | `omega-team` |
| Task 14 | Implemented baseline | `omega-core` |

## Task 8: omega-tools-builtin - 内置工具

- Scope: 将常用工具实现为可注册 builtin handlers，并与 trait-based tool system 对齐。
- Runtime role: 为 `omega-core`、`omega-session` 和 workflow steps 提供统一工具注册基础。

## Task 9: omega-todo - Todo 管理

- Scope: typed todo item、状态流转、排序与会话内快照。
- Runtime role: 既是用户可见的执行面板，也是 feature workflow 后续结构化计划与执行闭环的桥点。

## Task 10: omega-subagent - 子智能体

- Scope: 子执行体定义、工作目录隔离、父子通信与结果回传。
- Runtime role: 为后续 scene/workflow 中的 delegation 能力提供执行容器。

## Task 11: omega-compression - 上下文压缩

- Scope: 会话摘要、预算控制、压缩产物边界。
- Runtime role: 与 `omega-session` 的 step summaries、context diagnostics 长期演进直接相关。

## Task 12: omega-background - 后台任务

- Scope: 长时任务、状态追踪与非阻塞运行支持。
- Runtime role: 保持 UI shell 不承担执行调度；后台执行状态应通过统一 runtime contract 暴露。

## Task 13: omega-team - 团队协作

- Scope: 多执行体协作、分工、消息与任务联动。
- Runtime role: 复用 `omega-message`、`omega-tasks`、`omega-subagent` 和 session-owned assets，而不是复制一套并行执行模型。

## Task 14: omega-core - 核心 Agent

- Scope: `Agent` loop、tool dispatch glue、single-response 与 bounded tool loop 基础能力。
- Runtime role: 只拥有底层 agent 对话与工具调用能力，不理解 workflow 编排，也不持有前端语义。
- Follow-up pattern: 若 `omega-core` 内部继续膨胀，应维持“薄入口 + 内部模块”结构，而不是重新把 tool factory、loop 和 helper 聚回单文件。

## Verification Baseline

- 该任务组的验证核心是 crate 级 build/test/clippy，而不是 UI 端到端。
- 后续如果做内部重构，应优先使用对应 crate 的窄验证集，避免把运行时任务都推到全 workspace 测试上。

---

### Change Log

- 2026-03-25: 从 `omega-agent-impl-plan.md` 中拆出 Tasks 8-14，按执行运行时主题收敛说明。
