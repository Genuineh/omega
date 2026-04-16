---
content_revision: 96
created: 2026-04-07
generation_id: gen_000013_r000096
last_verified_commit: N/A
owner: omega-team
projection_version: 13
related_prds: []
source_doc_id: "spec:docs-specs-omega-knowledge-evolution"
status: active
supersedes: []
updated: 2026-04-07
version: v0.1
---

# Omega Knowledge Evolution Specification

## Overview

本规格把 `learn/` 下基于 Hindsight 的调研结论收敛成 Omega 可执行的实现方案。

Status note (2026-04-07): `Phase 1 ~ Phase 6` 的 retention / memory query / observation / unified recall planner / recall quality 基线已经落地。当前实现已补齐 deterministic `RecallQueryBundle`、CJK-friendly memory lexical retrieval、multi-query document merge/rerank、bounded report-step rewrite fallback，以及 Response / Sidebar / Overlay 的 recall semantics split；后续工作应聚焦 trigger policy 微调、命中质量继续评估与更重的 retrieval strategy，而不是回退到单 query recall。

目标不是把 Omega 改造成通用 memory engine，而是在保留当前 `repo-local source of truth + omega-context facade + workflow-first assembly + supervision` 基线的前提下，补齐长期项目知识的保留、整理与召回能力。

本规格同时修正调研阶段的几个过度设计点：

- **不在 Phase 1 引入统一 `RecallRequest`**。当前 `MemoryService` 还没有 query surface，先统一 contract 只会制造假抽象。
- **不把 strategy-specific chunking 和 relation graph 提前到低风险阶段**。这两项都依赖更重的解析和增量维护能力，应放到后续阶段。
- **不让 observation 取代 source-of-truth**。观察层只能是带证据引用的派生知识，不能覆盖 `files.jsonl`、store ledger、真实文件状态或治理结果。

## Goals

- 在不破坏现有 `omega-context` / `omega-memory` / `omega-document` 分层的前提下，提高长期项目知识密度。
- 为 turn archival 增加 mission-like retention profiles 与 noise gate，减少调试噪音污染长期记忆。
- 在原始 turn summaries 和 document/governance facts 之上，增加 evidence-backed 的项目观察层。
- 在 observation 之前，先为 memory 建立最小可用的 query surface，再考虑跨 memory/document 的统一 recall planner。
- 让新增能力继续进入 supervision、diagnostics 与 command surface，而不是形成内部黑盒。

## Non-Goals

- 不引入 Hindsight 风格的 bank、disposition、personality 或 reflect agent 作为主入口。
- 不引入 PostgreSQL、外部 control plane 或 graph database 作为本地默认路径。
- 不在第一阶段引入 cross-encoder rerank、通用 relation graph 或 AST-aware chunking。
- 不让 observation 或 curated cards 直接驱动 apply 类治理动作。

## Current Baseline

当前实现已经提供了后续演进所需的几个关键基础：

1. `omega-context` 通过 `OmegaContextFacade` 对上层暴露分离的 `ContextAssembler`、`MemoryService`、`KnowledgeQueryService`、`DocumentGovernanceService` 与 `ContextDiagnosticsProvider`。
2. `omega-memory` 当前负责 `archive_turn()`、`compact_context()` 与 `get_turn_history()`；它还没有搜索接口。
3. `DefaultContextAssembler` 已通过 `rank_summary_candidates()` 和 token budget 进行 budget-aware summary selection，因此“当前 step 的 memory 命中”已经存在，但属于 context assembly 而不是通用 recall。
4. `omega-document` 已支持 `keyword / semantic / hybrid` 检索、store version ledger、history snapshot、operator usage 与 supervision hits。
5. `omega-session` / `omega-tui` 已能展示 `DocumentHitSummary` 与 `MemoryHitSummary`，说明监管与可观测面并不缺基础容器，缺的是新的知识层和更明确的阶段边界。

## Problem Statement

尽管当前基线已经解决了上下文膨胀、文档索引与 supervision 的主问题，长期项目知识仍存在四个空缺：

1. **长期保留策略缺失**
   `archive_turn()` 目前没有区分 `project facts`、`developer preferences`、`open threads` 和 `ephemeral debug`。长期记忆容易被临时日志和短期试错稀释。

2. **缺少证据驱动的综合知识层**
   当前有 raw turn summaries、document hits、health/version diagnostics，但没有一层可以表达“仓库当前有哪些稳定认知、这些认知由哪些证据支持、是否已过期”。

3. **memory 还没有独立 query surface**
   当前 memory 只能通过 context assembly 被动参与当前 step，无法作为独立检索源被系统规划使用。

4. **relation-aware / temporal-aware retrieval 仍是后续能力**
   document search 已经具备 hybrid retrieval，但 relation graph、工程时间窗口和 strategy-specific chunking 仍缺基础维护路径。

## Design Principles

### 1. Local Source Of Truth Remains Primary

真实文件树、`.omega-state/store/files.jsonl`、store version ledger、document governance 结果与 turn archive 仍是主真相。

任何 observation、insight card 或 recall cache 都只能是派生层，且必须指回主真相。

### 2. Introduce Capabilities In Service Order

实现顺序必须遵循真实依赖关系：

1. retention profiles + noise gate
2. memory query surface
3. evidence-backed observations
4. unified recall planner
5. relation-aware / temporal-aware retrieval

不允许跳过中间层，直接在 facade 顶部发明一个“统一 recall”抽象。

### 3. Preserve Existing Budget-Aware Selection

当前 `rank_summary_candidates()` + token budget 的上下文装配逻辑是已经成立的能力。新设计应复用它，而不是把它推倒重写成另一套 planner。

### 4. Derived Knowledge Must Be Auditable

任何 observation 都必须包含：

- evidence refs
- created_at / updated_at
- freshness state
- correction or supersession history

没有这些字段的综合结论不应进入长期项目知识层。

### 5. Observability Is Part Of The Contract

保留、整理、修正、召回都要进入 supervision 和 diagnostics。新增能力如果只能被模型内部使用，而 operator 无法看到，就不算完成。

## Phase 1: Retention Profiles And Noise Gate

### Goal

先解决“什么值得长期保留”，而不是先扩展检索形态。

### Integration Points

- `omega-memory::archive_turn()`：增加长期保留判定与候选分类。
- `omega-session`：在 turn 完成时补充更明确的 retention signals，例如 changed paths、明确决策、未关闭线程、重复出现的用户偏好。
- `omega-context` diagnostics：记录 accepted / dropped retention candidates。

### Data Model

```rust
pub enum RetentionProfile {
    ProjectFacts,
    DeveloperPreferences,
    OpenThreads,
    EphemeralDebug,
}

pub struct RetentionCandidate {
    pub profile: RetentionProfile,
    pub text: String,
    pub evidence_refs: Vec<RetentionEvidenceRef>,
    pub accepted: bool,
    pub reason: String,
}

pub enum RetentionEvidenceRef {
    StepSummary { workflow_id: String, step_id: String },
    ChangedPath { path: String },
    GovernanceEvent { label: String, at: u64 },
}
```

### Design Notes

- Phase 1 不要求引入新的 public trait。
- `RetentionCandidate` 可以先作为 archive-time sidecar metadata 落盘，或先作为 turn archive schema 的 additive 字段。
- `EphemeralDebug` 默认只做计数，不进入长期 recall 语义。

### Acceptance Criteria

- turn archive 能显式区分高价值知识与调试噪音。
- diagnostics 能回答 accepted / dropped 的数量与 profile 分布。
- 当前 `MemoryHitSummary` 与 context assembly 行为不发生回归。

## Phase 2: Memory Query Surface

### Goal

在统一 recall 之前，先让 memory 成为真正可查询的数据源。

### Integration Points

- `MemoryService`：增加最小 query API。
- `omega-context`：新增 memory query adapter，但仍不暴露跨层 unified recall。
- `omega-session` / `omega-tui`：当 memory query 被实际使用时，补充 current hits 来源。

### Proposed API

```rust
pub struct MemoryQuery {
    pub text: Option<String>,
    pub profiles: Vec<RetentionProfile>,
    pub max_results: usize,
}

pub struct MemoryQueryHit {
    pub profile: RetentionProfile,
    pub title: String,
    pub preview: String,
    pub evidence_refs: Vec<RetentionEvidenceRef>,
    pub last_updated_at: u64,
}

pub trait MemoryService: Send + Sync {
    fn archive_turn(&self, turn: &TurnData) -> Result<()>;
    fn compact_context(&self, policy: CompactionPolicy) -> Result<CompactionResult>;
    fn get_turn_history(&self, limit: usize) -> Result<Vec<TurnSummary>>;
    fn query(&self, query: MemoryQuery) -> Result<Vec<MemoryQueryHit>>;
}
```

### Design Notes

- 这一步是后续 unified recall planner 的前置条件。
- 第一版 query 只需要覆盖 archived summaries 和 retention candidates，不需要 observation layer 一起到位。
- 不要求新的 slash command 在这一阶段就完全产品化；优先保证 runtime 和 supervision 能消费这条路径。

## Phase 3: Evidence-Backed Project Observations

### Goal

在 raw summaries 与 document/governance facts 之上建立稳定、可修正的项目观察层。

### Ownership

观察层由 `omega-context` 编排生成，但存储保持 repo-local。推荐初版落在 `.omega-state/memory/observations.jsonl`，原因是：

- 仍属于长期项目记忆而非 document source-of-truth
- 可以同时引用 turn archive 与 document/store evidence
- 不要求 `omega-context` 立即拥有新的独立持久化目录树

### Data Model

```rust
pub enum ObservationFreshness {
    Fresh,
    Stale,
    Superseded,
    Corrected,
}

pub struct ObservationEvidenceRef {
    pub kind: String,
    pub locator: String,
    pub observed_at: u64,
}

pub struct ProjectObservation {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub freshness: ObservationFreshness,
    pub created_at: u64,
    pub updated_at: u64,
    pub effective_at: Option<u64>,
    pub evidence_refs: Vec<ObservationEvidenceRef>,
    pub supersedes: Vec<String>,
}
```

### Evidence Sources

- accepted retention candidates from `omega-memory`
- `DocumentStoreVersion` changes
- document health / governance results
- high-signal `DocumentOperatorUsage` and recent activity

### Design Notes

- observation synthesis 必须是 additive 的，不能覆盖原始 evidence。
- 每条 observation 至少需要两个信号中的一个：重复出现的 memory/project fact，或 document/governance 的显式状态变化。
- 观察层先服务于 supervision 和 context quality，不作为 apply 类动作的 authority。

## Phase 4: Unified Recall Planner

### Goal

在 memory query 和 observation layer 都成立之后，再把跨层召回统一到一个 planner。

### Scope

- unified recall 的输入应由 `omega-context` 持有
- planner 调度 document query、memory query、observation recall 和已有 context summary selection
- 输出仍受 token budget 约束

### Guardrail

在此阶段之前，不引入新的顶层 `RecallRequest` 公开契约。

更合理的顺序是：

1. 先让 `MemoryService::query()` 成立
2. 先让 `ProjectObservation` 成立
3. 再在 `omega-context` 内部建立 planner
4. 最后再决定是否需要对外暴露 unified request type

## Phase 5: Advanced Retrieval Follow-Ups

### Deferred Items

以下能力有明确价值，但不应提前到 Phase 1：

1. **strategy-specific chunking**
   - code: AST / symbol / impl block
   - docs: heading / section
   - todo/logs: item / event block

2. **relation-aware retrieval**
   - doc crossrefs
   - archive replacement links
   - crate/module ownership
   - command/operator to affected path links

3. **temporal-aware retrieval**
   - recent task window
   - version promotion windows
   - before/after refactor comparisons

这些能力应建立在 observation lifecycle 和 memory query 已稳定后再推进。

## Phase 6: Recall Query Quality And Empty-Hit Recovery

### Goal

让已落地的 unified recall planner 真正具备稳定命中质量，特别是面向长自然语言、中文请求、弱文件锚点和首次空命中的场景。

### Problems Confirmed In Current Baseline

1. `plan_recall()` 当前直接复用 `latest_user_turn` 作为 memory query、observation query 与 document query，缺少 step-aware query planning。
2. `omega-memory` 当前以 whitespace split + `contains` 计分为主，对中文、长句和同义表达非常脆弱。
3. document recall 仍主要是单 query hybrid search，缺少 multi-query merge / dedupe / rerank。
4. 当前 `MemoryHitSummary` 和 `ResponseMemoryKnowledge` 仍会把 selected step summaries 与真实 archived memory hits / observation hits 并列展示，容易把“summary selection”误读成“memory hit”。
5. LLM 目前没有 bounded recall rewrite/fallback path；首次 recall 全空时，系统只能继续依赖原 query 或等待模型自己调用工具补查。

### Scope

- 在 `omega-context` 内部引入 deterministic `RecallQueryBundle`，不新增 facade 顶层 public request type。
- 升级 `omega-memory` 的 lexical retrieval，使其至少对中文与长自然语言查询更稳健。
- 把 document recall 升级为 multi-query recall + merge/rerank。
- 只在必要时引入 LLM-assisted rewrite fallback，而不是让每次 recall 都先走模型改写。
- 收敛 diagnostics / TUI 语义，明确区分 selected summaries、memory hits 与 observation hits。

### Non-Goals

- 不把 recall query planning 变成新的对外 workflow 或 slash command。
- 不在本阶段把 recall rewrite 变成 always-on LLM pass。
- 不引入 cross-encoder rerank、重量级 relation graph 或 AST-grade chunking。
- 不允许 `omega-session` / `omega-tui` 越过 `omega-context` 直接操作 memory/document internals。

### Proposed Internal Contract

```rust
pub struct RecallQueryBundle {
        pub raw_query: String,
        pub keyword_queries: Vec<String>,
        pub concept_queries: Vec<String>,
        pub path_hints: Vec<String>,
        pub doc_type_hints: Vec<DocType>,
        pub rewrite_reason: Option<String>,
}
```

说明：

- `raw_query` 保留原始用户意图，不被 rewrite 覆盖。
- `keyword_queries` 来自 deterministic 提炼，例如 step label、execute item label、structured input anchors、todo labels。
- `concept_queries` 用于长自然语言压缩后的短语检索。
- `path_hints` / `doc_type_hints` 只作为 rerank/filter hint，不要求首版全部强绑定到 query API。
- LLM fallback 若触发，只能补充 bundle，不得替换掉 deterministic queries。

### Deterministic Query Planning Rules

优先从以下信号构建 recall bundle：

1. `latest_user_turn`
2. `step.label`
3. `current_execute_item.item_label`
4. `structured_input` 中的标题、路径、任务名、schema key
5. `todo_snapshot` 中当前 open item 的文本
6. 最近 selected summaries 中重复出现的高频实体

首版规则：

- 生成 2-5 条 candidate queries，而不是单 query。
- 对长 query 做 deterministic 压缩，优先保留名词短语、路径、crate 名、step 名和 task label。
- 若请求明显是中文或混合语言，memory lexical path 需要产出 CJK-friendly query terms，而不是仅按空格切分。

### Retrieval Quality Strategy

#### Memory / Observation

- lexical scoring 改为 field-weighted：`title > summary > user_intent`
- 支持 CJK-friendly tokenization（如 bigram/trigram 或等价轻量策略）
- 继续保留 profile filter / freshness gating
- 对完全空 query 或弱 query，允许 bundle 中多 query 并行评分再合并

#### Document

- 对 `RecallQueryBundle` 中的 queries 执行 multi-query search
- 合并结果时按 path 去重
- rerank 继续结合 changed-path evidence、observation path hints、recent window bias
- 首版仍保留 lightweight heuristic，不强制引入重 reranker

### LLM-Assisted Rewrite Fallback

只在以下条件满足时触发：

- deterministic bundle 首轮 recall 全空或极弱
- 原 query 过长、过口语化、缺少可检索锚点
- 当前 step 明显依赖 recall（如 research/report/document-heavy steps）

fallback 要求：

- 输出必须是 bounded query bundle，而不是自由文本分析
- 保留 deterministic bundle 作为基线，不允许覆盖
- 记录 `rewrite_reason` 和 fallback 是否触发到 diagnostics/supervision

### Diagnostics And UI Semantics

- `selected summaries` 必须从 `archived memory hits` 与 `observation hits` 中分离展示
- empty-hit case 必须区分：
    - `no query planned`
    - `query planned but memory returned 0`
    - `query planned but document store unavailable`
    - `query rewritten after initial empty hit`
- TUI Response / Sidebar / Overlay 使用同一套 recall semantics，避免把 selected summary 误读成 memory hit

### Acceptance Criteria

- recall planner 不再只依赖 `latest_user_turn` 单 query。
- memory query 对中文和长自然语言请求的命中率有可验证提升。
- document recall 能消费 multi-query bundle，并对空命中/弱命中给出更明确 diagnostics。
- LLM rewrite fallback 只在受控条件下触发，并能被 supervision 观察到。
- TUI 中 `selected summaries`、`memory hits` 与 `observation hits` 的语义清晰分离。

## Commands And Supervision

### Phase 1-2

- 不强制新增用户命令。
- 先把 retention 与 memory query 结果进入 diagnostics / supervision snapshot。

建议新增的 supervision 统计：

- retention candidates accepted
- retention candidates dropped
- dropped by profile
- memory query count
- memory query hit mix

### Phase 3+

当 observation layer 成立后，再考虑：

- `/memory context`
- `/memory observations`
- `/memory refresh`

这些命令必须复用同一份 typed state，而不是自己重新拼文本。

## Technical Decisions

| Topic | Decision | Rationale |
|------|----------|-----------|
| Unified recall timing | Defer until Phase 4 | `MemoryService` still lacks query API |
| Observation storage | Repo-local JSONL under `.omega-state/memory/observations.jsonl` | Keeps derived knowledge local without changing document source-of-truth |
| Retention profiles | Add at archive-time | Noise should be filtered before long-term storage |
| Strategy-specific chunking | Later phase | Requires parser and maintenance cost not suitable for low-risk rollout |
| Relation graph | Lightweight and deferred | Current codebase has no graph maintenance path |
| Recall query planning | Deterministic bundle first, LLM fallback second | Keeps latency/cost bounded and preserves non-LLM baseline |
| Rewrite triggering | Empty-hit / weak-hit / long-query only | Avoids always-on LLM overhead for every recall |

## Testing Strategy

- `omega-memory` unit tests for retention profile classification, noise gate decisions, and query filtering.
- `omega-context` integration tests for archive -> diagnostics -> supervision propagation.
- `omega-session` integration tests confirming current `MemoryHitSummary` and token-budget assembly do not regress.
- `omega-tui` tests for new supervision fields and future observation/current-hit rendering.
- Regression tests ensuring document search, store versioning, and `/document` flows remain unchanged while memory capabilities are introduced.

## Implementation Order

1. Phase 1: retention profiles + noise gate + diagnostics counters
2. Phase 2: `MemoryService::query()` + runtime/supervision wiring
3. Phase 3: project observations + freshness/correction lifecycle
4. Phase 4: internal unified recall planner
5. Phase 5: strategy-specific chunking, relation-aware retrieval, temporal retrieval
6. Phase 6: recall query quality, empty-hit recovery, and UI semantics split

## Open Questions

1. retention candidates 是否作为 turn archive schema 的 additive 字段存储，还是拆到单独 sidecar JSONL。
2. observation synthesis 是否允许轻量模型参与，还是先用 heuristic + explicit signals 起步。
3. Phase 4 planner 是否只作为 `omega-context` 内部 helper，还是最终升级为 facade public API。
4. Recall rewrite fallback 是否需要按 step type 设单独 budget / trigger policy，还是统一由 `omega-context` 内部阈值控制。
