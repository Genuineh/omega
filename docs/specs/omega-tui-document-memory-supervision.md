---
status: draft
owner: omega-team
created: 2026-04-02
updated: 2026-04-02
version: 0.1
supersedes: []
related_prds: []
---

# Omega TUI Document And Memory Supervision Specification

## Overview

当前 `omega-context` / `omega-document` / `omega-memory` 已经具备统一 `ContextDiagnostics` 快照、document health popup、search results overlay 与基础 store 统计，但 TUI 仍然只有“点状入口”：用户能看到某次搜索结果、某次 health check、某次 diagnostics 行，却没有稳定的单独面板去持续监管文档系统与 memory 系统。

本规格定义两类专用监管面板：`Document Supervision` 与 `Memory Supervision`。它们应作为现有 `Sidebar / Activity` 信息架构中的专门视图存在，而不是新增独立常驻列；在需要更大阅读空间时，再通过 overlay 打开 detail drill-down。目标是让用户能够持续回答三类问题：

1. 相关功能是否启用，以及当前是 ready / degraded / disabled 的哪种状态。
2. 当前总量、大小、staleness、archive/index 健康度如何。
3. 当前 step 实际命中了哪些文档结果与 memory 摘要，并能看到可读的摘要预览。

## Goals

- 为 document system 提供持续可见的启用状态、索引健康、容量统计与当前命中摘要面板。
- 为 memory system 提供持续可见的启用状态、archive/compaction 统计与当前命中摘要面板。
- 将当前散落在 diagnostics line、search overlay、document popup、tool metadata 中的信息收敛为稳定的监管视图。
- 保持 `omega-context` / `omega-session` / `omega-app` / `omega-tui` 的职责边界清晰：领域状态由 context 层拥有，UI 语义由 TUI 拥有。
- 为后续 `Task 15B-12` 的会话统计与 `Task 11F` 的 dashboard 演进提供统一容器，而不是继续叠加临时 overlay。

## Non-Goals

- 不要求本任务把所有 context/store 指标都做成实时图表。
- 不要求首次实现就支持多窗口或固定双列 dashboard。
- 不把 document / memory 监管逻辑直接放进 `omega-core` 或 `omega-document` 的 UI 专用类型里。
- 不要求 search results overlay、document health popup、diagnostics sidebar 在本轮完全删除；它们可以保留为 dedicated panel 的 drill-down surface。

## Current Baseline

当前已存在以下能力，可作为新面板的可信输入源：

- `ContextDiagnostics` 已是统一快照，覆盖 `budget / cache / memory / document / store`。
- `search_codebase` 会把 indexed files/chunks、staleness、tantivy/lance size 写入 runtime search overlay。
- `manage_document health_check` 会触发 document health detail overlay。
- `omega-tui` diagnostics sidebar/detail 已能展示 `context memory / context docs / context store` 聚合文本。

当前缺口：

- 没有稳定的“document enabled / memory enabled”前端语义，只能从 feature wiring 或空指标间接推断。
- 没有单独的 document/memory 常驻监管 view；当前信息散落在 diagnostics 与临时 overlay。
- 没有 typed “current hit summary” contract。document 命中仍主要依赖 search tool 输出文本；memory 命中只能看到 summary source title/count，缺少命中内容摘要。
- `ContextStoreDiagnostics` 当前只有 `turn_archive_count`，没有 `turn_archive_size_bytes`，导致 memory 存储规模无法与 Lance/Tantivy 尺寸并列展示。

## UX Model

### Placement Rule

`Document Supervision` 与 `Memory Supervision` 应作为现有 `Sidebar` 中 `Activity` 下的专门视图，而不是再新增顶层固定列。

推荐布局：

- `Response`: 用户主阅读区
- `Todos`: 当前任务计划
- `Activity`: `Logs / Document / Memory / Skills / Delegations / ...`
- `Overlay`: detail drill-down 与 narrow-mode fallback

这样既满足“单独面板”的可发现性，又不破坏 `omega-tui-runtime-experience.md` 已确定的“不要无上限增加固定常驻面板”规则。

### Document Panel

`Document Supervision` 面板至少包含四个区块：

1. `Status`
   - backend enabled / disabled
   - indexing readiness: `disabled | idle | indexing | ready | degraded`
   - governance health: `good | needs_attention | critical | unknown`
2. `Totals`
   - indexed files
   - indexed chunks
   - indexed embeddings
   - index staleness seconds
   - Tantivy size bytes
   - LanceDB size bytes
3. `Current Hits`
   - last query
   - mode: `keyword | semantic | hybrid`
   - result count
   - degraded-from mode（如果有）
   - top hit summaries（path + short preview）
4. `Recent Actions`
   - last health check summary
   - last index update summary
   - last governance warning summary

### Memory Panel

`Memory Supervision` 面板至少包含四个区块：

1. `Status`
   - memory enabled / disabled
   - archive readiness: `disabled | idle | archiving | ready | degraded`
   - compaction status: `idle | triggered | stale | failed`
2. `Totals`
   - total turns archived
   - current summary tokens
   - current summary count
   - compactions triggered
   - compression ratio avg percent
   - turn archive count
   - turn archive size bytes
3. `Current Hits`
   - selected summary count / available summary count
   - currently included memory sources
   - per-hit short preview（workflow, step, title, excerpt）
   - whether todo state / structured input was injected from memory-adjacent state
4. `Recent Actions`
   - last compaction event
   - last archive event
   - most recent compression trigger reason

## Data Contract

### New Snapshot Layer

新增 frontend-neutral 的监管快照：

```rust
pub struct ContextSupervisionSnapshot {
    pub document: DocumentSupervisionSnapshot,
    pub memory: MemorySupervisionSnapshot,
}

pub struct DocumentSupervisionSnapshot {
    pub enabled: bool,
    pub readiness: SupervisionReadiness,
    pub totals: DocumentTotals,
    pub current_hits: Option<DocumentHitSummary>,
    pub recent_activity: Vec<SupervisionActivitySummary>,
}

pub struct MemorySupervisionSnapshot {
    pub enabled: bool,
    pub readiness: SupervisionReadiness,
    pub totals: MemoryTotals,
    pub current_hits: Option<MemoryHitSummary>,
    pub recent_activity: Vec<SupervisionActivitySummary>,
}
```

说明：

- 这不是替代 `ContextDiagnostics`，而是对其做 UI-facing projection。
- `ContextDiagnostics` 继续负责长期稳定的聚合计数；`ContextSupervisionSnapshot` 负责监管面板真正需要的“状态 + totals + current hits + recent activity”。

### Enablement Semantics

为了满足“是否启用相关功能”的明确表达，需要新增显式 enablement，而不是让前端靠 `0` 值猜测：

```rust
pub enum SupervisionReadiness {
    Disabled,
    Idle,
    Indexing,
    Archiving,
    Ready,
    Degraded,
}
```

建议语义：

- `Document.enabled = false`：编译 feature 未启用，或当前 runtime 明确关闭 document backend。
- `Document.readiness = Idle`：backend enabled，但还未执行首次 scan。
- `Document.readiness = Degraded`：Lance revision 落后、语义查询失败回退、或 health check 失败。
- `Memory.enabled = false`：archive/compaction path 当前不可用或未接通。
- `Memory.readiness = Idle`：memory backend 已接通，但本轮尚未产生 archive/selection。

### Totals Contract

`DocumentTotals` 应由现有 `ContextDiagnostics.document + ContextDiagnostics.store` 派生。

`MemoryTotals` 除现有 `ContextDiagnostics.memory + ContextDiagnostics.store.turn_archive_count` 外，还应新增：

```rust
pub struct StoreDiagnostics {
    pub lance_db_size_bytes: u64,
    pub tantivy_index_size_bytes: u64,
    pub todo_items_count: u32,
    pub turn_archive_count: u32,
    pub turn_archive_size_bytes: u64,
}
```

这是满足“能够展示总的统计和大小”等用户目标所必需的 additive change。

### Current Hit Summary Contract

#### Document

```rust
pub struct DocumentHitSummary {
    pub query: String,
    pub mode: String,
    pub degraded_from: Option<String>,
    pub result_count: u32,
    pub top_hits: Vec<DocumentHitItem>,
}

pub struct DocumentHitItem {
    pub path: String,
    pub score: Option<f32>,
    pub preview: String,
}
```

来源建议：

- 首选 `search_codebase` 的 structured result，而不是再次从 overlay 文本反解析。
- 若当前 step 未发生搜索，则 `current_hits = None`，面板显示 `No document hits for current step.`。

#### Memory

```rust
pub struct MemoryHitSummary {
    pub selected_summary_count: u32,
    pub available_summary_count: u32,
    pub hits: Vec<MemoryHitItem>,
}

pub struct MemoryHitItem {
    pub workflow_id: String,
    pub step_id: String,
    pub title: String,
    pub preview: String,
}
```

来源建议：

- 以当前 `StepDiagnostics.input.summary_sources` 为主键来源。
- 为满足“命中内容摘要”，需要在 diagnostics projection 中为每个 selected summary 增加短 preview；不能只停留在 `title`。
- preview 只需展示规范化摘要的前 1-2 行，不要求把完整 archive 正文搬进 TUI。

## Runtime Integration

### Ownership

- `omega-context`: 拥有 document/memory 原始状态与 projection helper。
- `omega-session`: 在 step diagnostics、tool result 与 context assembly 事件后生成 supervision snapshot。
- `omega-app`: 继续通过 runtime message policy 把 snapshot 送入 UI surface。
- `omega-tui`: 拥有 panel state、focus、sorting、detail overlay 与 narrow-mode fallback。

### Message Path

建议新增 additive-only runtime state message：

```rust
StateMessage::ContextSupervision {
    snapshot: ContextSupervisionSnapshot,
}
```

理由：

- 当前 document health popup 与 search overlay 只覆盖工具触发路径，不覆盖“当前 step 实际命中了什么 memory summary”。
- 使用 typed state message，避免 `omega-tui` 再从 diagnostics text lines 或 overlay body 文本做反向解析。

### Reducer Behavior

- 新 snapshot 到达时，更新 `Document` 与 `Memory` 面板的 header badge、totals 与 current-hit list。
- `Current Hits` 默认只保留当前 active turn / active step 的最近一次快照。
- 历史查询与历史 compaction/archive 进入 detail overlay，不在主面板无限累积。

## UI States

### Document Panel States

- `Disabled`: 明确显示 `Document backend disabled`，并附带原因摘要。
- `Idle`: backend enabled，但尚未建索引。
- `Ready`: 显示 totals + current hits。
- `Degraded`: 显示 fallback 原因，例如 `semantic retrieval degraded to keyword`。

### Memory Panel States

- `Disabled`: 明确显示 `Memory supervision unavailable`。
- `Idle`: 尚未发生 archive/compaction 或当前 step 未命中历史摘要。
- `Ready`: 显示 totals + current hits。
- `Degraded`: archive read failure、compaction failure 或 preview projection 失败。

### Narrow Terminal Fallback

- 侧边栏隐藏时，不丢失 enabled/readiness 信号；底部状态带至少显示 `Doc:` 与 `Mem:` 两个紧凑徽章。
- 当前命中详情通过 overlay 打开，而不是在窄屏继续挤压 `Response`。

## Technical Decisions

| Decision | Choice | Rationale |
|---------|--------|-----------|
| supervision surface | dedicated `Activity` views, not new permanent columns | 与现有 `Sidebar` 壳层一致，避免面板数量失控 |
| source of truth | `ContextDiagnostics` + additive supervision snapshot | 保留现有快照稳定性，同时补 current-hit 语义 |
| document hit source | structured search result projection | 避免从 overlay 文本反解析 |
| memory hit source | selected summary sources + preview projection | 当前唯一可信的“实际命中”来源在 context assembly path |
| memory storage size | add `turn_archive_size_bytes` | 否则 memory 无法与 Tantivy/LanceDB 做同维度监管 |

## Task Breakdown

### Phase 1: Data Contract

- 扩展 `ContextStoreDiagnostics`，新增 `turn_archive_size_bytes`。
- 新增 `ContextSupervisionSnapshot` 与 `DocumentHitSummary` / `MemoryHitSummary` typed model。
- 为 selected memory summaries 增加 preview projection。

### Phase 2: Runtime Wiring

- `omega-session` 在 context assembly、search result、document health、compaction/archive 后更新 supervision snapshot。
- `omega-app` 把该 snapshot 纳入 runtime message policy。

### Phase 3: TUI Panels

- 在 `Activity` 中新增 `Document` 与 `Memory` 视图。
- 实现 header badge、totals block、current hits list、detail overlay。
- 在窄终端下退化为底部状态徽章 + overlay。

## Testing Strategy

- `omega-context` 单测：验证 disabled / idle / degraded / ready 状态投影正确。
- `omega-context` 单测：验证 `turn_archive_size_bytes` 与 Lance/Tantivy size 统计并存。
- `omega-session` 单测：验证当前 step 的 document hit summary 与 memory hit summary 会进入 snapshot。
- `omega-app` 单测：验证 `ContextSupervision` state message 会被路由到 TUI surface。
- `omega-tui` 单测：验证 `Document` / `Memory` 视图渲染 enabled badge、totals 与 current hits。
- `omega-tui` replay 测试：验证侧边栏切换、detail overlay 打开与窄终端退化路径。

## Open Questions

- memory `enabled` 是否总是为 true，还是要区分 archive backend 可用与“当前仅有 in-memory summaries”两种状态。
- `Current Hits` 是否只展示 active step，还是允许在 report step 回看上一 step 的命中快照。
- document/memory 面板是否需要固定排序，还是允许用户在 `Activity` rail 中自定义顺序。

---

### Change Log
- 2026-04-02: 初版规格，定义 `Document Supervision` / `Memory Supervision` 专门监管面板、typed hit-summary contract 与 TODO 拆分方向。