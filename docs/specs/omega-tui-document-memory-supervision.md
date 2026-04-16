---
content_revision: 101
created: 2026-04-02
generation_id: gen_000015_r000101
last_verified_commit: N/A
owner: omega-team
projection_version: 15
related_prds: []
source_doc_id: "spec:docs-specs-omega-tui-document-memory-supervision"
status: draft
supersedes: []
updated: 2026-04-07
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
- supervision 主要停留在 Sidebar / Overlay；Response 主阅读区仍缺少“本 step / 本 command 实际用了哪些 document/memory”的结构化可见性。
- document / memory 当前展示更像 diagnostics dump，而不是可扫描的知识摘要与可点击 drill-down 入口。

## UX Model

### Placement Rule

`Document Supervision` 与 `Memory Supervision` 应作为现有 `Sidebar` 中 `Activity` 下的专门视图，而不是再新增顶层固定列。

推荐布局：

- `Response`: 用户主阅读区
- `Todos`: 当前任务计划
- `Activity`: `Logs / Document / Memory / Skills / Delegations / ...`
- `Overlay`: detail drill-down 与 narrow-mode fallback

这样既满足“单独面板”的可发现性，又不破坏 `omega-tui-runtime-experience.md` 已确定的“不要无上限增加固定常驻面板”规则。

### Response Integration Rule

尽管 Sidebar 仍是长期监管主入口，但当当前 step 或 command 实际使用了 document recall、memory recall 或 observation recall 时，`Response` 主阅读区也必须出现轻量 `Knowledge Summary Lane`。

规则：

- 只显示当前 step / command 实际命中的知识摘要，不显示完整 diagnostics dump。
- 用户应能在阅读主回答时直接看出“用了 document 吗”“用了 memory 吗”“命中了什么”“没命中时为什么”。
- 卡片必须支持 `Enter` 或同等级交互打开 detail overlay。
- 完整 totals、health、recent activity 继续留在 Sidebar；Response 不退化为第二个 diagnostics 面板。

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

### Response Knowledge Lane

当 step/command 使用了知识系统时，在 Response 中增加紧邻正文的知识摘要 lane：

1. `Document recall` 卡片
    - readiness badge：`ready | degraded | uninitialized`
    - query preview
    - result count
    - top hit path + 1 行 preview
    - 无命中原因摘要，例如 `no promoted store version` / `no matches returned`
2. `Memory recall` 卡片
    - memory query / observation query preview
    - selected summary count
    - memory hit count / observation hit count
    - top summary / observation preview
3. `Inspect` affordance
    - 卡片可点击或可选中
    - `Enter` 打开 detail overlay
    - 后续可扩展为“在 Sidebar 中定位到对应 Document/Memory view”

该 lane 应复用 `omega-tui-step-tool-thinking-refinement.md` 的原则：Response 只承载“摘要 + drill-down”，完整详情继续留在 Overlay / Sidebar。

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

### Response-Facing Projection

在 `ContextSupervisionSnapshot` 之外，新增 step/command 级别的 response-facing projection：

```rust
pub struct StepKnowledgeSummary {
    pub document: Option<ResponseDocumentKnowledge>,
    pub memory: Option<ResponseMemoryKnowledge>,
}

pub struct ResponseDocumentKnowledge {
    pub readiness: SupervisionReadiness,
    pub query: String,
    pub reason: Option<String>,
    pub result_count: u32,
    pub top_hits: Vec<DocumentHitItem>,
}

pub struct ResponseMemoryKnowledge {
    pub memory_query: Option<String>,
    pub observation_query: Option<String>,
    pub selected_summary_count: u32,
    pub memory_hit_count: u32,
    pub observation_hit_count: u32,
    pub top_items: Vec<MemoryHitItem>,
}
```

要求：

- projection 必须绑定到具体 step section 或 command section，而不是只做全局 snapshot。
- 若 planner/search 已尝试 document recall 但无 active store version，应写出 `reason = Some("no promoted store version")`，不能只投影成空列表。
- 若当前 step 没有使用对应系统，则 Response 中不显示该 lane，避免噪音。

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

StateMessage::StepKnowledgeSummary {
    workflow_id: String,
    step_id: String,
    summary: StepKnowledgeSummary,
}
```

理由：

- 当前 document health popup 与 search overlay 只覆盖工具触发路径，不覆盖“当前 step 实际命中了什么 memory summary”。
- 使用 typed state message，避免 `omega-tui` 再从 diagnostics text lines 或 overlay body 文本做反向解析。
- `ContextSupervision` 继续服务 Sidebar；`StepKnowledgeSummary` 专门服务 Response 内知识摘要 lane，两者不可互相替代。

### Reducer Behavior

- 新 snapshot 到达时，更新 `Document` 与 `Memory` 面板的 header badge、totals 与 current-hit list。
- `Current Hits` 默认只保留当前 active turn / active step 的最近一次快照。
- 历史查询与历史 compaction/archive 进入 detail overlay，不在主面板无限累积。
- `StepKnowledgeSummary` 到达时，只更新对应 step/command section 绑定的知识摘要 lane；不得回写成全局 Sidebar 状态。

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

### Phase 4: Response And Drill-Down

- 新增 `StepKnowledgeSummary` projection 与 runtime wiring。
- 在 Response step/command section 中渲染 knowledge summary lane。
- 统一 Sidebar / Response / Overlay 三处知识详情的跳转与文案语义。

### Phase 5: Content Curation

- 收敛 document/memory 文案顺序，优先展示状态、原因、命中与下一步，而不是裸 totals dump。
- 把“未初始化 / 未 health check / 尝试过但无命中 / 有磁盘残留但无 active version”等状态做成稳定文案模板。

### Follow-Up Tasks

#### Task 11F-5: omega-context / omega-session / omega-app / omega-tui — Response-Facing Knowledge Summary Lane

- **Priority**: High
- **Complexity**: L
- **Dependencies**: Task 11F-4, Task 11G-4, Task 15B-22
- **Description**: 为当前 step / command 增加 `StepKnowledgeSummary` typed projection，并在 Response panel 渲染轻量 document/memory knowledge lane，让用户在主阅读区直接看见“用了哪些 knowledge source、命中了什么、没命中时为什么”。

#### Task 11F-6: omega-tui / omega-context — Knowledge Detail Overlay And Browse Interaction

- **Priority**: Medium
- **Complexity**: M
- **Dependencies**: Task 11F-5, Task 15B-29
- **Description**: 把现有 document/memory detail 从长文本 dump 收敛成更可浏览的 overlay：按 query、状态、原因、top hits、selected summaries 分块展示，并支持从 Response 与 Sidebar 双向进入同一详情视图。

#### Task 11F-7: omega-tui / omega-session — Knowledge View Content Curation

- **Priority**: Medium
- **Complexity**: M
- **Dependencies**: Task 11F-5
- **Description**: 重写 document/memory supervision 的内容顺序和文案模板，把当前 diagnostics 风格输出调整成更直观的阅读布局，并确保 Response / Sidebar / Overlay 的语义一致。

## Testing Strategy

- `omega-context` 单测：验证 disabled / idle / degraded / ready 状态投影正确。
- `omega-context` 单测：验证 `turn_archive_size_bytes` 与 Lance/Tantivy size 统计并存。
- `omega-session` 单测：验证当前 step 的 document hit summary 与 memory hit summary 会进入 snapshot。
- `omega-app` 单测：验证 `ContextSupervision` state message 会被路由到 TUI surface。
- `omega-tui` 单测：验证 `Document` / `Memory` 视图渲染 enabled badge、totals 与 current hits。
- `omega-tui` replay 测试：验证侧边栏切换、detail overlay 打开与窄终端退化路径。
- `omega-session` 单测：验证 `StepKnowledgeSummary` 会绑定到正确的 step/command section。
- `omega-tui` 单测：验证 Response 内知识摘要 lane 的展开、选择与 overlay 打开路径。
- 手动验证：运行 `cargo run -p omega-app --features document-backend`，确认 `/document query`、planner-driven document recall 与 memory recall 都会在 Response 中留下可点击的知识摘要。

## Open Questions

- memory `enabled` 是否总是为 true，还是要区分 archive backend 可用与“当前仅有 in-memory summaries”两种状态。
- `Current Hits` 是否只展示 active step，还是允许在 report step 回看上一 step 的命中快照。
- document/memory 面板是否需要固定排序，还是允许用户在 `Activity` rail 中自定义顺序。

---

### Change Log
- 2026-04-02: 初版规格，定义 `Document Supervision` / `Memory Supervision` 专门监管面板、typed hit-summary contract 与 TODO 拆分方向。
- 2026-04-07: v0.2 — 新增 Response integration rule、`StepKnowledgeSummary` projection、knowledge summary lane、detail overlay browse 规则，以及 `Task 11F-5 ~ 11F-7` 后续拆分。
