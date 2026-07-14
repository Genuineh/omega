---
content_revision: 174
created: 2026-03-20
generation_id: gen_000087_r000174
language: bilingual
last_verified_commit: d8c30e3e9e310ce38cffa965be4688ed55a87787
owner: omega-team
projection_version: 87
related_prds: "[]"
source_doc_id: "spec:docs-specs-omega-step-session-asset-model"
source_path: docs/specs/omega-step-session-asset-model.md
status: active
supersedes: "[]"
updated: 2026-03-25
---

# Omega Step And Session Asset Model Specification

## Overview

本文件现在作为 step model 与 session asset 规格的稳定入口页。原单体文档中关于 step definition、session-owned assets、session context/data contracts，以及 routing/repair/diagnostics 的内容已拆到主题子文档，以便分别维护“当前稳定边界”和“下一阶段演进方向”。

## Goals

- 保持 `step` 作为 workflow 的正式最小执行单元术语。
- 保持 `omega-session` 作为 tools、skills、routing state 与 step context 的统一运行态拥有者。
- 为通用 step 编排、结构化 context、routing handoff 与 diagnostics 提供稳定入口。

## Non-Goals

- 不在入口页重复展开所有 runtime policy、repair 细节和迁移阶段表。
- 不把 `omega-tui`、`omega-app` 或具体 provider 实现细节重新拉回本规格主文。

## Document Map

| Document | Scope | Contents |
|----------|-------|----------|
| `docs/specs/omega-step-session-asset-model/step-assets-and-execution.md` | Foundation | step definition、session asset ownership、dynamic tool visibility、shared execution model |
| `docs/specs/omega-step-session-asset-model/session-context-and-data-contracts.md` | Context | `SessionContext`、summary budget、step data contract、todo-driven execute contract |
| `docs/specs/omega-step-session-asset-model/routing-repair-and-diagnostics.md` | Runtime evolution | routing convergence、structured output repair、diagnostics、migration、risks、testing |

## Current Baseline

- 当前所有 root / chat / feature steps 已统一进入 bounded minimal agent loop。
- `omega-session` 已持有 session-owned tool/skill catalogs 与 typed `SessionContext`。
- root routing 已从自由文本主路径收敛到更强的 typed handoff。
- data contract、todo integration 与 diagnostics 已有首轮实现基础。

## Routing State Convergence

运行时应统一使用 `SessionContext.routing: RoutingContext` 作为唯一路由状态容器，而不是并存多个独立 routing state。详细约束、恢复策略与相关 runtime ownership 见 `docs/specs/omega-step-session-asset-model/routing-repair-and-diagnostics.md`。

## Usage Notes

- 需要看 step definition、session assets 或 execution contract 时，进入 `step-assets-and-execution.md`。
- 需要看 `SessionContext`、step summaries、structured input/output 或 todo-driven execute 时，进入 `session-context-and-data-contracts.md`。
- 需要看 routing convergence、repair strategy、context observability、migration、risks/testing 时，进入 `routing-repair-and-diagnostics.md`。

---

### Change Log

- 2026-03-25: 将原单体规格拆为“入口页 + 主题子文档”，保留 `Routing State Convergence` 入口节作为稳定链接落点。
- 2026-03-20: 初版规格创建。
