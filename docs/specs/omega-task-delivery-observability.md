---
content_revision: 174
created: 2026-04-08
generation_id: gen_000087_r000174
language: bilingual
last_verified_commit: d8c30e3e9e310ce38cffa965be4688ed55a87787
owner: omega-team
projection_version: 87
related_prds: "[]"
source_doc_id: "spec:docs-specs-omega-task-delivery-observability"
source_path: docs/specs/omega-task-delivery-observability.md
status: draft
supersedes: "[]"
updated: 2026-04-08
---

# Omega Task Delivery Observability Specification

## Overview

当前 Omega 已经分别具备 step-level tool run、root skill loading、document/memory supervision、workflow routing、底部状态带与 Response timeline 等局部可见性，但这些信号仍然分散在不同 surface 中。用户可以看到“刚刚调用了哪个工具”或“当前加载了哪些 skills”，却仍然很难在一次任务完成后直接回答以下问题：

- 这次交付一共消耗了多少 token。
- 一共调用了多少次 LLM，用了哪个模型。
- 一共调用了多少工具、加载了多少 skills。
- document / memory system 各自 search 了多少次。
- 本次任务最终改了哪些文件。
- 如果想看细节，应该点哪里。

本规格定义一套统一的 task-level delivery observability 方案。首轮 scope 以“一次用户触发的单 turn 交付窗口”为准，把该窗口内的成本、能力使用与产出变更收敛为单一 `Task Delivery Summary`。该 summary 既要能在任务完成后以统一消息展示，也要能在 Sidebar / Bottom status / Overlay 中持续钻取细节。

## Goals

- 为单次任务交付建立统一的 frontend-neutral delivery summary contract，而不是继续依赖分散的日志、diagnostics 和零散 sidebar section。
- 在任务完成后输出一条统一的 `Task Delivery Summary` message，明确展示 token、LLM、tool、skill、document/memory search、changed files 与 model 信息。
- 为 TUI 提供一个统一的 `Delivery` 监控面板，支持从聚合摘要点击进入各类详情。
- 让底部状态带能显示紧凑的任务成本摘要，同时保持窄终端可退化。
- 保持现有 `ToolRun`、`SkillLoadSummary`、document/memory supervision 与 diagnostics contract 的 ownership 不变，只在 session boundary 上做统一聚合。

## Non-Goals

- 首轮不追求 provider billing-grade 精度；token 统计以当前 runtime 能拿到的 request/response/cached token 为准。
- 首轮不把“整个多轮会话”都折叠成一个 summary；默认只统计单次用户触发的 delivery window。
- 首轮不要求生成完整 git diff 或取代 `ToolRunDetail` / diagnostics / document health overlay 这些现有细粒度 surface。
- 首轮不把 subagent/team/background 的专门监控一并做成全量 dashboard；它们只在被当前任务使用时进入 summary。

## User Questions This Must Answer

任务完成后，用户应能直接回答：

1. 本次任务是否完成，还是 failed / interrupted。
2. 主模型是谁，是否还使用了其他模型。
3. 本次任务总共消耗了多少 token，调用了多少次 LLM。
4. 本次任务调用了多少工具、哪些工具最常用、是否有失败。
5. 本次任务加载了多少 skills，分别是 recognized / loaded / ignored 哪些。
6. document 与 memory system 各自 search 了多少次。
7. 本次任务最终修改了哪些文件，以及修改类型是什么。
8. 点击某一类摘要后，能否看到可读的 detail drill-down。

## Delivery Window Model

### Scope Definition

首轮把“任务交付窗口”定义为：

- 起点：一个用户输入被 `omega-session` 接受并启动 turn。
- 终点：该 turn 发出最终完成、失败或中断状态。

这意味着当前 summary 是 turn-scoped，而不是 conversation-scoped。该限制是刻意的：当前 runtime、tool run、routing、knowledge summary 与 skill loading 都以 turn 为主要 identity。后续如果需要跨多个 turn 汇总“同一个用户任务”，应另行引入更高层的 task/thread identity，而不是在首轮让现有 contract 失真。

### End States

`Task Delivery Summary` 至少支持以下状态：

- `complete`
- `failed`
- `interrupted`

即使失败或中断，也必须产出 partial summary，避免“代价已经发生，但面板里什么都没有”。

## Architecture

### Existing Inputs To Reuse

- `ToolRun` / `ToolRunDetail`: 工具调用次数、名称、结果、失败信息。
- `SkillLoadSummary`: recognized / loaded / ignored skills。
- workflow / routing state: active workflow、scene、最终 completion state。
- model config / client response metadata: 当前 turn 实际使用的模型。
- document / memory supervision / knowledge summary / recall diagnostics: search 次数与命中摘要。
- structured tool metadata / document ops: 文件修改 evidence。

### New Aggregation Layer

应在 `omega-session` 内增加一个 task-level accumulator，由它收敛本轮 delivery 期间的运行态事实。该 accumulator 只持有结构化 summary，不直接拥有 TUI surface 概念。

推荐边界：

- producers: `omega-client`、`omega-core`、`omega-session`、`omega-context`、structured tool results
- aggregator owner: `omega-session`
- rendering policy owner: `omega-app`
- panel / overlay / badges owner: `omega-tui`

### Frontend-Neutral Contract

推荐新增两层结构：

```rust
pub struct TaskDeliverySummary {
    pub turn_id: u64,
    pub status: TaskDeliveryStatus,
    pub primary_model: Option<String>,
    pub models_used: Vec<String>,
    pub llm_request_count: u32,
    pub token_usage: TaskTokenUsage,
    pub tool_usage: TaskToolUsage,
    pub skill_usage: TaskSkillUsage,
    pub knowledge_usage: TaskKnowledgeUsage,
    pub file_changes: TaskFileChangeSummary,
}

pub enum TaskDeliveryStatus {
    Complete,
    Failed,
    Interrupted,
}
```

该 summary 应能通过 `StateMessage` 或等价 frontend-neutral state contract 持续 upsert，并在 turn 完成时冻结为最终快照。

## Data Model

### Token Usage

首轮统计以下字段：

- `input_tokens`
- `output_tokens`
- `cache_read_tokens`
- `cache_write_tokens`
- `estimated_tokens_used`（只有缺少精确 token 数据时才回退）

展示规则：

- UI 默认展示总量与最重要的 1-2 个子项。
- 详情里再展开 request-level token split。
- 若某 provider 只能提供部分数据，summary 必须明确标记 `partial`，而不是假装精确。

### LLM Usage

- `llm_request_count`
- `models_used`
- `primary_model`
- `request_count_by_model`

`primary_model` 用于统一回答“这次任务用了哪个模型”；`models_used` 用于保留 regenerate、fallback、subagent 或 mixed-provider 场景的真实历史。

### Tool Usage

- `tool_call_count`
- `unique_tool_count`
- `count_by_tool`
- `failed_tool_count`
- `tool_names_in_order`（detail 用）

UI 上默认显示总次数与失败数；detail 再展开到每个 tool 名与父 section。

### Skill Usage

- `recognized_skill_count`
- `loaded_skill_count`
- `ignored_skill_count`
- `recognized_skill_ids`
- `loaded_skill_ids`
- `ignored_skill_ids`

该层直接复用现有 root skill loading contract，不再重新发明第二套 skill 统计来源。

### Knowledge Usage

至少区分：

- `document_search_count`
- `memory_search_count`
- `observation_search_count`（如果当前 runtime 已能分开）
- `document_queries`
- `memory_queries`

首轮的核心目标是统计“用了几次 document / memory search”，而不是一开始就做复杂 recall dashboard。若已存在 query / hit summary，可作为 detail drill-down 内容复用。

### File Change Summary

至少包含：

- `changed_file_count`
- `changed_paths`
- `change_count_by_kind`，推荐 `create / update / delete`
- `workspace_change_evidence`，优先来自 structured tool result / session-owned mutation evidence，而不是每次 turn 结束后做一次全局 git diff 猜测

若本轮没有真实文件变更，应明确显示 `No workspace files changed.`，不要让用户误以为统计丢失。

## UI Surfaces

### 1. Completion Message In Response

每次任务结束后，`Response` 主阅读区应追加一条统一的 `Task Delivery Summary` message / section，位置在最终 answer 之后或失败总结之后。

最小正文建议包含：

- status
- primary model
- total tokens
- llm request count
- tool call count
- loaded skill count
- document/memory search count
- changed file count

这条消息不应替代最终 answer，而是作为交付元摘要独立存在。

### 2. Delivery Panel In Sidebar

`omega-tui` 应提供统一的 `Delivery` 监控面板，持续展示当前 turn 的 delivery summary。

近端实现建议：

- 若当前 Sidebar 仍以显式 section 演进，可先新增 `Delivery` section。
- 但 contract 语义上应保持它可收敛到未来的 `Activity(Delivery)` view，而不是成为新的长期例外。

面板至少显示以下可点击条目：

- `Tokens`
- `LLM`
- `Tools`
- `Skills`
- `Document Searches`
- `Memory Searches`
- `Files Changed`
- `Model`

### 3. Bottom Status Badge

底部状态带只显示紧凑摘要，例如：

- `Delivery: 18k tok · 3 llm · 5 tools · 2 files`

规则：

- 窄终端优先保留 `tokens / llm / files`。
- 如果任务失败，可退化为 `Delivery: failed · 18k tok · 5 tools`。
- badge 只显示最终或当前累计摘要，不显示长文本明细。

### 4. Overlay Drill-Down

从 `Response` summary、Sidebar `Delivery` 行和状态带 badge 进入的详情应落到统一 detail overlay，而不是分裂成多套解释。

detail overlay 至少支持：

- token 细分
- per-model request counts
- per-tool counts / failures
- skill ids 列表
- document / memory queries
- changed file paths 与操作类型

## Interaction Rules

- 聚合行必须可激活，不能只有只读文字。
- 同一类指标无论从 `Response` 还是 `Sidebar` 进入，都应打开同一份 detail surface。
- 若某类指标没有数据，点击后应打开明确的 empty-state detail，而不是无反馈。
- `Task Delivery Summary` 在 turn 完成后默认可见，但不应打断用户阅读最终 answer；它属于紧随其后的 delivery metadata，而不是 toast。

## Technical Decisions

| Decision | Choice | Rationale |
|---------|--------|-----------|
| summary scope | single user turn | 与现有 runtime identity 对齐，避免首轮跨多 turn 聚合失真 |
| completion surface | dedicated Response summary section | 满足“任务完成后统一一个消息展示” |
| detail surface | shared overlay drill-down | 避免 Response / Sidebar 各自维护一套细节文本 |
| file change source | structured mutation evidence first | 比 post-hoc git diff 更稳，更能区分 create/update/delete |
| panel placement | Delivery panel in Sidebar, contract-compatible with future Activity view | 既兼容当前 TUI 结构，也不阻塞后续 Activity 收敛 |
| model display | primary model + all models used | 同时满足“用了哪个模型”与 mixed-model transparency |

## Implementation Phases

### Phase 1: Contract And Accumulator

- 为 task-level delivery summary 定义 frontend-neutral typed contract。
- 在 `omega-session` 中引入 accumulator，按 turn 生命周期收集 token / llm / tool / skill / knowledge / file-change 指标。

### Phase 2: Source Instrumentation

- 从 `omega-client` / `omega-core` 接入 token 与 model 使用信息。
- 从 `ToolRun`、`SkillLoadSummary`、context recall path 接入 tool / skill / document / memory 指标。
- 为 workspace mutation 增加统一 evidence 路径。

### Phase 3: Presentation

- `omega-app` 增加 summary mapping policy。
- `omega-tui` 增加 `Delivery` panel、bottom badge 与 detail overlay。
- `Response` 在 turn 完成后渲染统一的 `Task Delivery Summary` section。

### Phase 4: Validation

- session 单测：聚合与 end-state freeze
- app/tui 单测：message mapping、panel rendering、overlay activation
- 回归测试：失败 / interrupted 任务仍产出 partial summary

## Testing Strategy

- `omega-session` 单测：验证 token / llm / tool / skill / knowledge / file-change 指标在同一 turn 内累计正确。
- `omega-session` 单测：验证 failed / interrupted turn 也会发出 partial summary。
- `omega-app` 单测：验证 summary state 与 completion message 能被正确映射到 surface。
- `omega-tui` 单测：验证 Delivery panel、badge、completion summary 与 detail overlay 的可见性和激活路径。
- 集成测试：验证一个包含 LLM、tool、skill、document search、memory search 与文件修改的完整 task，最终只出现一条统一 `Task Delivery Summary` completion message。

---

### Change Log
- 2026-04-08: 初版规格，定义 task-level delivery observability 的目标、contract、surface 与分阶段实现路径。
