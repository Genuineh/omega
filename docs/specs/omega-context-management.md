---
content_revision: 101
created: 2026-03-27
generation_id: gen_000017_r000101
last_verified_commit: N/A
owner: omega-team
projection_version: 17
related_prds: []
source_doc_id: "spec:docs-specs-omega-context-management"
status: active
supersedes: []
updated: 2026-04-03
---

# Omega Context Management Specification

## Overview

Status note (2026-04-02): `Task 11A ~ 11F-3` 已完成并进入当前主路径。本规格现作为上下文管理基线与后续迭代入口使用，而不再只是前置草案。

当前 omega 在持续多轮对话中面临三个系统性问题：

1. **上下文膨胀导致 execute 失败**：step 的系统提示、tool 定义、session summaries 与 todo 快照在多轮后累积超出上下文窗口或严重稀释模型注意力，导致 structured output 质量退化（如 `missing required key completed_tasks`）。
2. **跨会话无记忆**：每次 turn 结束后 session context 在内存中清空，同一项目的后续对话无法复用先前的决策、发现与工件状态。
3. **项目文档管理无规则约束**：文档的创建、归档、交叉引用和健康检查完全依赖模型的即时能力，缺乏稳定的规则引擎来保证文档结构、命名和生命周期一致性。

本规格定义一套三层上下文管理体系：

- **omega-memory**（内部）：会话内多轮记忆与压缩。
- **omega-document**（内部）：仓库级持久化知识、文件管理、文档治理与向量检索。
- **omega-context**（唯一对外接口）：统一上下文编排库，封装 memory + document，对上层（omega-session / omega-app）暴露完整 API 和工具集。

**核心原则：omega-session、omega-app 和任何上层业务模块只依赖 omega-context，不直接导入 omega-memory 或 omega-document。**

## Goals

- 先修复当前 execute 场景中的上下文退化与 structured output 失败，再扩展长期知识能力。
- 建立稳定的公开边界：上层只依赖 omega-context，而不是直接耦合 memory/document/tool 细节。
- 为仓库知识建立可恢复、可校验、可查询的持久化存储与文档治理规则。
- 将上下文预算、缓存命中、索引状态和文档健康纳入统一诊断面。

## Non-Goals

- 第一阶段不追求“全量知识平台”一次到位；向量检索、混合检索和 TUI 仪表盘都晚于根因修复。
- 不把 tool 注册、TUI 呈现、存储实现细节直接塞进同一个公共 service trait。
- 不要求文档治理工具在首次落地时直接执行多文件写入；默认先做校验和变更计划。

## Problem Statement

### 上下文膨胀与结构化输出退化

当前 `runner.rs` 的 `select_step_summaries()` 使用线性 token 估算（`chars / 4`）和贪心倒序裁剪——准确度低、无优先级权重、不区分 thinking/tool output/元数据的信息密度差异。随着 execute 进入 itemized loop，每个 item run 增加新的 step context、structured input 与 tool results，上下文窗口消耗加速。

当模型的有效注意力被低价值上下文稀释后，structured output（如 `completed_tasks` / `open_tasks`）的准确率显著下降，导致 schema 校验失败 → repair/regenerate → 更多上下文消耗的恶性循环。

### 多轮对话无持久记忆

`SessionContext.begin_turn()` 清空 `step_outputs`。`Agent.messages()` 仅保留当前 turn 的完整 transcript。同一个仓库的下一次对话无法知道上次做了什么、哪些文件改了、哪些决策已确认。

### Provider 缓存能力未利用

`AnthropicCacheControl` 类型已实现（`omega-client/src/anthropic/types.rs`）但未被 session/prompt builder 注入。每个 step 重建完整 system prompt、tool 定义和 session context，缺少主动缓存锚点。

### 文件与文档管理缺乏规则引擎

当前对文件的理解局限于 tool 直接读写——没有索引、没有分块、没有版本追踪、没有文档生命周期管理。LLM "知道"项目里有什么文件完全依赖随机的搜索能力，而文档创建、归档、交叉引用的规则全靠 system prompt 里的自然语言约束。这种模式在复杂项目中不可靠——需要一套规则引擎在工具层面保证文档治理的一致性。

### 知识检索缺乏多维查询能力

当前没有任何结构化的知识库检索能力。未来的 `search_codebase` 不应只限于全文搜索——它需要支持向量相似度、文件属性过滤、时间范围、语言类型等多维度的组合查询。这要求底层接入轻量级向量数据库，而非仅靠 grep + BM25。

## Architecture

### Three-Layer Model with Facade

```
┌────────────────────────────────────────────────────────────────┐
│                  omega-session / omega-app                      │
│           仅依赖 omega-context，不直接导入下层 crate            │
└───────────────────────────┬────────────────────────────────────┘
                            │  OmegaContextFacade API + tool 注册
┌───────────────────────────▼────────────────────────────────────┐
│                     omega-context  (唯一对外接口)               │
│  OmegaContextFacade: 统一 facade                               │
│  - context assembly / budget / cache_control                   │
│  - memory ops (通过内部 omega-memory)                           │
│  - document ops (通过内部 omega-document)                       │
│  - tool 注册: search_codebase, manage_document, manage_todo    │
│  - diagnostics / metrics 聚合                                  │
├──────────────────────┬─────────────────────────────────────────┤
│   omega-memory        │          omega-document                │
│   (内部 crate)        │          (内部 crate)                  │
│   - turn archive     │    - 文件永久索引 + 向量数据库           │
│   - micro-compact    │    - chunk manager                      │
│   - cache strategy   │    - 文档治理引擎                       │
│   - thinking 蒸馏    │    - persistent TODO                    │
│                      │    - 多维复合查询                        │
└──────────────────────┴─────────────────────────────────────────┘
```

### Facade Boundary Rule

**上层只依赖 `omega-context`。** `omega-memory` 和 `omega-document` 是 omega-context 的内部实现细节：

```
✅ omega-session  → omega-context
✅ omega-app      → omega-context
✅ omega-workflow  → omega-context (hooks)
❌ omega-session  → omega-memory     (禁止)
❌ omega-session  → omega-document   (禁止)
❌ omega-app      → omega-document   (禁止)
```

omega-context 对外暴露一组聚焦接口，并由 `OmegaContextFacade` 统一组合。上层仍只依赖 omega-context，但不直接依赖一个巨大的 god trait：

```rust
pub trait ContextAssembler: Send + Sync {
    fn assemble_context(&self, request: ContextRequest) -> anyhow::Result<AssembledContext>;
    fn estimate_tokens(&self, text: &str) -> u32;
    fn count_tokens_precise(&self, messages: &[Message]) -> anyhow::Result<u32>;
}

pub trait MemoryService: Send + Sync {
    fn archive_turn(&self, turn: &TurnData) -> anyhow::Result<()>;
    fn compact_context(&self, policy: CompactionPolicy) -> anyhow::Result<CompactionResult>;
    fn get_turn_history(&self, limit: usize) -> anyhow::Result<Vec<TurnSummary>>;
}

pub trait KnowledgeQueryService: Send + Sync {
    fn scan_workspace(&self) -> anyhow::Result<ScanResult>;
    fn search(&self, query: SearchQuery) -> anyhow::Result<Vec<SearchResult>>;
}

pub trait DocumentGovernanceService: Send + Sync {
    fn manage_document(&self, op: DocumentOp) -> anyhow::Result<DocumentOpResult>;
    fn manage_todo(&self, op: TodoOp) -> anyhow::Result<TodoOpResult>;
    fn check_document_health(&self) -> anyhow::Result<DocumentHealthReport>;
}

pub trait ContextDiagnosticsProvider: Send + Sync {
    fn context_diagnostics(&self) -> ContextDiagnostics;
    fn cache_diagnostics(&self) -> Option<CacheDiagnostics>;
}

/// omega-context 的唯一公共入口类型。
/// facade 只负责组合各子接口，不把所有能力塞进同一个 trait。
pub struct OmegaContextFacade {
    pub assembler: Arc<dyn ContextAssembler>,
    pub memory: Arc<dyn MemoryService>,
    pub query: Arc<dyn KnowledgeQueryService>,
    pub governance: Arc<dyn DocumentGovernanceService>,
    pub diagnostics: Arc<dyn ContextDiagnosticsProvider>,
}
```

tool 注册不是 `OmegaContextFacade` 的核心职责，而是 omega-context 的 integration adapter：它把 facade 暴露为 omega-core 能消费的 tool definitions，但不反向污染业务接口。

### Crate Responsibilities

#### omega-memory（内部 crate，不对上层暴露）

**会话内多轮记忆**。绑定到单个 `AgentSession` 生命周期。omega-context 内部调用。

职责：
- **Turn Archive**：每轮结束后提取 turn-level summary 并持久化到 `.omega-state/memory/turns/{turn_id}.jsonl`。
- **Micro-Compact**：对 thinking blocks、tool output、冗余元数据进行选择性压缩，保留语义骨架。
- **Cache Strategy**：管理 Anthropic `cache_control` 锚点，确保稳定的 system prompt 前缀命中缓存。
- **Summary Trigger**：当累计 token 达到上下文窗口阈值或显式 workflow 触发时，自动执行摘要。
- **Priority Selection**：替代现有贪心倒序裁剪，按 recency × relevance × step-type 加权选择上下文。

#### omega-document（内部 crate，不对上层暴露）

**仓库级持久化知识、文件管理与文档治理**。生命周期跨越多次对话。omega-context 内部调用。

职责：
- **Permanent File Store**：文件元数据永久索引，增量更新，不丢弃历史。
- **Vector Store**：接入轻量级嵌入式向量数据库（LanceDB），存储文件与 chunk embeddings。
- **Chunk Manager**：对大文件按语义边界（AST、heading）划分与增量更新。
- **Multi-Dimensional Query**：支持向量相似度 + 文件属性（语言、路径、时间、大小）+ 全文关键词的复合查询。
- **Document Governance Engine**：基于规则的文档治理——命名约束、结构校验、归档策略、交叉引用检查、健康评估，遵循 docs-skill 设计模式。
- **Persistent TODO**：持久化任务跟踪，替代纯内存 `TodoManager`。
- **Full-Text Index**：基于 tantivy 的全文检索索引。

#### omega-context（唯一对外 crate）

**统一上下文编排库 + facade**。是上层模块与 memory/document 之间的唯一桥梁。

职责：
- **Facade Composition**：组合 `ContextAssembler`、`MemoryService`、`KnowledgeQueryService`、`DocumentGovernanceService`、`ContextDiagnosticsProvider`。
- **Context Assembly**：slot-based 声明式上下文组装，替代 `prompt_builder.rs` 硬编码拼接。
- **Budget Manager**：精确 token 计算（优先 provider `count_tokens` API），slot 间优先级分配。
- **Cache Control Injection**：自动注入 `cache_control: { type: "ephemeral" }` 标记。
- **Compaction Orchestrator**：按策略触发 memory 压缩与 document chunk 更新。
- **Integration Adapter**：向 omega-core 暴露 `search_codebase`、`manage_document`、`manage_todo` 等工具，但 tool 注册实现不进入核心 facade trait。
- **Diagnostics Aggregator**：聚合 memory/document/cache 指标，驱动 TUI 和 observability。

## Detailed Design

### 1. Anthropic Cache Control Integration

#### 缓存层次

Anthropic 缓存遵循 `tools → system → messages` 前缀顺序。每个 `cache_control` 断点前最多回溯 20 个块，一次请求最多 4 个 `cache_control` 标记。缓存 TTL 5 分钟，命中后自动续期。

#### 缓存锚点策略

omega-context 应在 message assembly 时注入最多 4 个 `cache_control` 断点：

| 锚点 | 位置 | 稳定性 | 说明 |
|-------|------|--------|------|
| **Anchor 1** | tools 数组末尾 | 高（step 内稳定） | tool 定义在同一 step 内不变 |
| **Anchor 2** | system prompt 中 skills + routing context 末尾 | 高（turn 内稳定） | skills 与 routing 在一轮内不变 |
| **Anchor 3** | system prompt 中 session summaries 末尾 | 中（step 间可能变化） | summaries 在 step 切换时更新，但 step 内多次 retry 稳定 |
| **Anchor 4** | messages 数组中最近 assistant turn 末尾 | 中（retry 间稳定） | 在 repair/regenerate 循环中命中 |

#### 实现要求

- `omega-client` 已有 `AnthropicCacheControl` 类型，只需 prompt builder / context assembler 注入标记。
- `AnthropicMessageCreateRequest::contains_cache_control()` 已存在，无需修改 client 层。
- 非 Anthropic provider 忽略 `cache_control`，不影响兼容性。

#### Token 计费追踪

以 `usage.cache_creation_input_tokens`、`cache_read_input_tokens`、`input_tokens` 三字段为基础，在 diagnostics 中新增缓存命中率指标：

```rust
pub struct CacheDiagnostics {
    pub cache_creation_tokens: u32,
    pub cache_read_tokens: u32,
    pub uncached_input_tokens: u32,
    pub cache_hit_ratio: f32,
}
```

### 2. Memory Layer (omega-memory) — 内部 crate

> omega-memory 不对上层暴露，所有操作通过 omega-context facade 进行。

#### Turn Archive

每轮结束时 memory 执行 turn archival pipeline：

```
raw messages → extract entities/decisions → generate turn summary → persist to .omega-state/memory/turns/{turn_id}.jsonl
```

Turn summary 结构：

```rust
pub struct TurnSummary {
    pub turn_id: u64,
    pub timestamp: u64,
    pub user_intent: String,
    pub workflow_id: String,
    pub steps_executed: Vec<StepSummaryCompact>,
    pub decisions: Vec<String>,
    pub changed_paths: Vec<String>,
    pub estimated_tokens: u32,
}

pub struct StepSummaryCompact {
    pub step_id: String,
    pub outcome: StepOutcome, // Completed | Failed | Skipped
    pub key_findings: Vec<String>,
    pub token_cost: u32,
}
```

#### Micro-Compact

对留存在多轮对话 context 中的内容执行分层压缩：

| 内容类型 | 压缩策略 | 压缩比目标 |
|----------|----------|-----------|
| Thinking blocks | 提取结论性语句，丢弃中间推理链 | 10:1 |
| Tool output | 保留结论/错误，丢弃完整 stdout | 5:1 |
| Structured output | 保留 schema 字段值，丢弃重复 key | 2:1 |
| Step 正文 | 保留首尾段落 + 实体/数字/路径 | 3:1 |
| 冗余元数据 | `workflow_id`, `step_label` 等去重 | ∞ (只保留一份) |

#### 压缩触发策略

Memory compaction 在以下条件触发：

1. **预算阈值**：累计 context tokens ≥ `context_window × 0.7` 时强制压缩。
2. **Turn 结束**：每轮结束时执行 turn archive + 轻量压缩。
3. **Step 切换**：workflow step 切换时压缩上一步的 thinking/tool output。
4. **Workflow 声明**：step 可在 `loop_contract` 中声明 `compact_before = true`。

#### Cache-Aware Memory

Memory 压缩必须尊重 cache 边界：

- 被 `cache_control` 标记的前缀内容不可被修改（否则缓存失效）。
- 只对最后一个 `cache_control` 断点之后的内容执行压缩。
- 压缩后自动重新计算断点位置并更新标记。

### 3. Document Layer (omega-document) — 内部 crate

> omega-document 不对上层暴露，所有操作通过 omega-context facade 和相应的 tool 进行。

#### 3.1 Permanent File Store

文件元数据永久索引。不同于临时缓存，文件在索引中是"只增不删"——删除的文件标记为 `deleted` 而非移除，保留历史轨迹。

```rust
pub struct FileStore {
    pub root: PathBuf,
    pub manifest_path: PathBuf,
}

pub struct FileRecord {
    pub path: String,
    pub size_bytes: u64,
    pub modified_at: u64,
    pub created_at: u64,
    pub language: Option<String>,
    pub file_type: FileType,          // Source | Doc | Config | Asset | Test | Other
    pub doc_type: Option<DocType>,    // Spec | PRD | Guide | ADR | Todo | Archive | None
    pub status: FileStatus,           // Active | Deleted | Archived | Moved { to }
    pub content_hash: String,         // blake3 hash
    pub chunk_count: u32,
    pub total_tokens: u32,
    pub tags: Vec<String>,            // 用户/规则引擎添加的标签
    pub last_indexed_at: u64,
}

pub enum FileType {
    Source,   // .rs, .ts, .py, etc.
    Doc,      // .md, .txt, .adoc
    Config,   // .toml, .json, .yaml
    Asset,    // images, fonts, etc.
    Test,     // *_test.rs, *.test.ts
    Other,
}

pub enum DocType {
    Spec,     // docs/specs/
    PRD,      // docs/prds/
    Guide,    // docs/guide/
    ADR,      // docs/decisions/
    Todo,     // TODO.md
    Archive,  // docs/archive/
    Readme,   // README.md
    Changelog,// CHANGELOG.md
}
```

`FileStore` 是系统的元数据真源（source of truth），默认持久化到 `.omega-state/store/files.jsonl`。tantivy 与 LanceDB 都是派生索引，而不是主存储。

#### 3.2 Embedded Vector Database

接入 **LanceDB** 作为轻量级嵌入式向量数据库。选择理由：

| 特性 | LanceDB | 备选方案对比 |
|------|---------|-------------|
| 部署模式 | 嵌入式，无需独立服务 | Qdrant 需要 server 进程 |
| 存储格式 | 列式 Lance 格式，基于文件 | SQLite-vss 受限于 SQLite 生态 |
| 向量搜索 | 原生 ANN (IVF-PQ, HNSW) | tantivy 无原生向量搜索 |
| 过滤查询 | SQL-like filter + vector search 组合 | Qdrant 功能强但重 |
| Rust SDK | `lancedb` crate，原生支持 | 无需 FFI |
| 存储位置 | `.omega-state/store/lance/` | 随项目走，无外部依赖 |

LanceDB 表结构：

```
files 表:
  path: String (primary)
  size_bytes: UInt64
  modified_at: UInt64
  language: String
  file_type: String
  doc_type: String
  status: String
  content_hash: String
  tags: List<String>

chunks 表:
  chunk_id: String (primary)
  file_path: String (foreign → files.path)
  byte_range_start: UInt64
  byte_range_end: UInt64
  content_hash: String
  estimated_tokens: UInt32
  embedding: FixedSizeList<Float32, 384>  // fastembed dimension
  content_preview: String                  // 前 200 chars，用于快速预览

turns 表:
  turn_id: UInt64 (primary)
  timestamp: UInt64
  user_intent: String
  workflow_id: String
  decisions: List<String>
  changed_paths: List<String>
  summary_embedding: FixedSizeList<Float32, 384>
```

#### 3.2.1 Index Consistency Contract

双索引模型必须定义一致性边界，否则 keyword 与 semantic 结果会长期漂移。约束如下：

```rust
pub struct IndexRevision {
        pub revision_id: u64,
        pub manifest_hash: String,
        pub committed_at: u64,
}

pub struct IndexCommitLog {
        pub current_manifest_revision: u64,
        pub tantivy_revision: u64,
        pub lance_revision: Option<u64>,
}
```

规则：
- `FileStore` manifest 是真源，chunk 切分结果先写 manifest revision，再派生更新 tantivy/LanceDB。
- search 只暴露最后一个“已提交 revision”；未完成的索引更新不可见。
- 如果 LanceDB 落后于 manifest revision，semantic/hybrid 自动降级为 keyword。
- 如果 tantivy 损坏或落后，系统进入 `index_degraded` 状态并触发 rebuild，不阻塞会话主流程。
- `manage_document` 与 `scan_workspace` 的变更都以 revision 为单位提交，保证恢复与回放。

#### 3.2.2 Store Exclusion Rules (`.omega/.storeignore`)

为避免生成产物、vendored 目录、快照文件或低价值大文本持续进入 embedding/LanceDB 管线，仓库可选提供一个 repo-local 规则文件：

```
.omega/.storeignore
```

目标边界：
- 声明“哪些路径完全不进入 `.omega-state/store` 知识库产物”。
- 不替代 walk-level 硬排除（`.git/`、`target/`、`.omega-state/store/`）。
- 匹配文件不会写入 `FileStore` manifest、tantivy 或 LanceDB。
- `/document init|sync` 需要暴露 ignored/indexed/embedded 样本，便于解释本次处理范围。

首期语法采用 gitignore-like 的受限子集：
- 每行一个相对仓库根目录的 glob 规则。
- 空行忽略。
- `#` 开头视为注释。
- 支持 `*`、`**`、`?` 与目录前缀匹配。
- 首期不支持 `!` 反选，避免规则优先级与 revision replay 复杂化。

建议数据模型：

```rust
pub struct StoreIgnoreRules {
    pub patterns: Vec<String>,
}

pub struct FileRecord {
    // existing fields ...
    pub vector_index_eligible: bool,
}

pub struct ScanResult {
    pub files_indexed: usize,
    pub chunks_indexed: usize,
    pub deleted_marked: usize,
    pub vector_ignored_files: usize,
    pub vector_ignored_paths: Vec<String>,
    pub indexed_paths: Vec<String>,
    pub embedded_paths: Vec<String>,
    pub manifest_path: String,
    pub keyword_index_path: String,
}
```

扫描与索引数据流：

```
1. Walk workspace with existing hard exclusions
2. Load `.omega/.storeignore` if present
3. Drop matching paths before record/chunk creation
4. Build/refresh FileStore records only for non-ignored files
5. Rebuild tantivy and LanceDB only from manifest-backed active records/chunks
6. Expose ignored/indexed/embedded samples in scan diagnostics and command output
```

查询语义要求：
- `Keyword`：不会命中 `.storeignore` 排除文件。
- `Semantic`：不会返回 `.storeignore` 排除文件。
- `Hybrid`：不会返回 `.storeignore` 排除文件。
- diagnostics / command output 需要暴露 `vector_ignored_files`、`vector_ignored_paths`、`indexed_paths` 与 `embedded_paths`，让 `/document init|sync` 能解释“这次具体处理了哪些文件”。

验收标准：
- 缺失 `.omega/.storeignore` 时行为与当前实现完全一致。
- 新增规则后，匹配文件不会写入 `.omega-state/store/files.jsonl`。
- keyword / semantic / hybrid 都不会返回被排除文件。
- `/document init|sync` Response 至少展示 ignored/indexed/embedded 的样本列表。
- 测试覆盖 parser、scan 统计、semantic/hybrid 边界与 revision 回放。

#### 3.3 Multi-Dimensional Composite Query

查询系统支持向量相似度 + 结构化属性 + 全文关键词的联合查询：

```rust
/// 复合查询请求。所有维度可选组合。
pub struct SearchQuery {
    /// 自然语言查询（用于向量搜索 + 关键词搜索）
    pub text: Option<String>,
    /// 向量搜索模式
    pub mode: SearchMode,         // Keyword | Semantic | Hybrid
    /// 结构化过滤条件（AND 组合）
    pub filters: Vec<SearchFilter>,
    /// 排序维度
    pub sort: Option<SortField>,
    /// 结果上限
    pub max_results: usize,
}

pub enum SearchFilter {
    Language(Vec<String>),          // language IN ["rust", "typescript"]
    FileType(Vec<FileType>),        // file_type IN [Source, Doc]
    DocType(Vec<DocType>),          // doc_type IN [Spec, PRD]
    PathGlob(String),               // path GLOB "crates/omega-session/**"
    ModifiedAfter(u64),             // modified_at > timestamp
    ModifiedBefore(u64),            // modified_at < timestamp
    Status(Vec<FileStatus>),        // status IN [Active]
    Tag(Vec<String>),               // tags CONTAINS ANY ["architecture", "api"]
    MinTokens(u32),                 // estimated_tokens >= N
    MaxTokens(u32),                 // estimated_tokens <= N
}

pub enum SearchMode {
    /// 仅关键词（tantivy full-text），无需 embedding
    Keyword,
    /// 仅向量相似度（LanceDB ANN），需要 embedding
    Semantic,
    /// 关键词 + 向量的融合排序（RRF），需要 embedding
    Hybrid,
}

pub enum SortField {
    Relevance,     // 默认：按查询相关度排序
    ModifiedDesc,  // 最近修改优先
    TokensAsc,     // 最小文件优先（适合 context budget 感知）
}
```

查询执行流程：

```
1. Parse SearchQuery
2. If text present + mode=Semantic|Hybrid:
   a. Embed query text → vector
   b. LanceDB vector_search(vector).filter(structured_filters).limit(N)
3. If text present + mode=Keyword|Hybrid:
   a. tantivy full-text search with structured pre-filter
4. If mode=Hybrid:
   a. RRF (Reciprocal Rank Fusion) merge vector + keyword results
5. Apply SortField re-ranking
6. Return top-N results with content_preview + metadata
```

#### 3.4 Chunk Manager

大文件按语义边界划分为 chunks，chunk 与向量 embedding 一一对应：

```rust
pub struct Chunk {
    pub id: ChunkId,
    pub file_path: PathBuf,
    pub byte_range: (u64, u64),
    pub content_hash: String,      // blake3，增量更新用
    pub estimated_tokens: u32,
    pub content_preview: String,   // 前 200 chars
}
```

分块策略：
- Rust 文件按 `fn` / `impl` / `mod` 边界（tree-sitter 解析优先，fallback 到正则）。
- Markdown 按 `##` heading 边界。
- 其他文件按固定大小（约 500 tokens）+ 行边界兜底。
- Chunk 大小上限 1000 tokens，超限则继续分割。

增量更新：对比 `content_hash`，仅重新索引变化的 chunk，避免全量重建。

#### 3.5 Document Governance Engine

**基于规则的文档治理**，对齐仓库 AGENTS.md 中的 docs skill 设计，通过 omega-context 工具暴露给 LLM 和用户。

##### 治理规则模型

```rust
/// 文档治理规则集，从项目根目录的 .omega/doc-rules.toml 加载。
pub struct DocGovernanceRules {
    pub structure: StructureRules,
    pub naming: NamingRules,
    pub lifecycle: LifecycleRules,
    pub cross_ref: CrossRefRules,
}

pub struct StructureRules {
    /// 期望的目录结构（匹配 AGENTS.md 中的 docs layout）
    pub expected_dirs: Vec<ExpectedDir>,
    /// 必须存在的文件
    pub required_files: Vec<String>,  // ["README.md", "docs/TODO.md", "LICENSE"]
}

pub struct ExpectedDir {
    pub path: String,         // "docs/specs"
    pub purpose: String,      // "Formal specs, contracts, and repository rules"
    pub file_pattern: String, // "*.md"
    pub doc_type: DocType,    // Spec
}

pub struct NamingRules {
    /// 子目录全部小写
    pub lowercase_dirs: bool,
    /// 文件名允许的 pattern（如 omega-*.md）
    pub file_patterns: Vec<NamingPattern>,
}

pub struct LifecycleRules {
    /// 归档触发条件
    pub archive_when: Vec<ArchiveTrigger>,
    /// 归档时必须的操作
    pub archive_checklist: Vec<String>,
    /// 文档头 frontmatter 必须包含的字段
    pub required_frontmatter: Vec<String>,
}

pub enum ArchiveTrigger {
    Superseded,              // 被新文档替代
    CompletedAndInactive,    // 完成且不再活跃
    StructurallyOutdated,    // 与当前仓库结构不符
    HistoryOnly,             // 仅保留历史
}

pub struct CrossRefRules {
    /// README.md 必须索引所有 active specs
    pub readme_must_index: Vec<String>,  // ["docs/specs/*.md", "docs/prds/*.md"]
    /// 归档时必须更新的文件
    pub archive_update_targets: Vec<String>, // ["README.md", "docs/TODO.md"]
    /// 替代文档必须链回被替代版本
    pub replacement_must_backlink: bool,
}
```

##### 治理操作

通过 omega-context 暴露的 `manage_document` 工具支持 staged 变更控制，而不是直接执行黑盒多文件修改：

```rust
pub enum DocumentMutationMode {
    Check,
    Plan,
    Apply,
}

pub enum DocumentOp {
    /// 创建新文档，自动校验命名和结构规则
    Create {
        mode: DocumentMutationMode,
        path: String,
        doc_type: DocType,
        title: String,
        content: String,
    },
    /// 归档文档，自动执行归档 checklist
    Archive {
        mode: DocumentMutationMode,
        path: String,
        reason: ArchiveTrigger,
        replaced_by: Option<String>,
    },
    /// 更新文档 frontmatter / 标签
    UpdateMetadata {
        mode: DocumentMutationMode,
        path: String,
        updates: Vec<MetadataUpdate>,
    },
    /// 检查文档健康状态
    HealthCheck,
    /// 列出某类型所有文档
    List {
        doc_type: Option<DocType>,
        status: Option<FileStatus>,
    },
}

pub struct DocumentChangePlan {
    pub primary_path: String,
    pub affected_paths: Vec<String>,
    pub validation_issues: Vec<String>,
    pub proposed_mutations: Vec<DocumentMutation>,
}

pub struct DocumentHealthReport {
    pub total_docs: usize,
    pub structure_violations: Vec<StructureViolation>,
    pub naming_violations: Vec<NamingViolation>,
    pub orphaned_docs: Vec<String>,      // 未被 README 索引的文档
    pub broken_crossrefs: Vec<CrossRefIssue>,
    pub stale_docs: Vec<StaleDoc>,       // 长时间未更新的活跃文档
    pub missing_frontmatter: Vec<String>,
    pub overall_health: HealthScore,     // Good | NeedsAttention | Critical
}
```

执行规则：
- `Check` 只返回规则校验结果，不产生写入。
- `Plan` 返回 `DocumentChangePlan`，列出 README/TODO/archive note 等所有将被修改的文件。
- `Apply` 只能执行已经通过校验的计划；默认要求上层明确调用。
- 对多文件变更，omega-context 必须先生成 plan，再统一 apply，避免隐式跨文件副作用。

##### 规则加载与默认值

- 从 `.omega/doc-rules.toml` 加载自定义规则。
- 若文件不存在，使用默认规则（对齐 AGENTS.md `Documentation Structure` 章节）。
- 规则热加载：文件修改后下次操作即生效。

#### 3.6 Persistent TODO

取代纯内存 `CoreSharedTodoManager`，提供跨 session 持久化：

```
.omega-state/store/todos.jsonl   # 持久化 todo items
```

兼容现有 `TodoManager` 接口（`update()` / `render()` / `has_open_items()` / `should_nag()`），增加：
- `load()` / `save()`：启动时从持久存储恢复，每次更新同步写入。
- `history()`：已完成项保留历史，不删除。
- Context assembly 时按 recency 筛选活跃项。

通过 omega-context governance 接口操作，不直接暴露 TodoManager。

#### 3.7 Full-Text Index (tantivy)

为不依赖 embedding 的快速关键词搜索提供 [tantivy](https://github.com/quickwit-oss/tantivy) full-text index：

```
.omega-state/store/tantivy/   # tantivy index 目录
```

索引字段：`path`、`content`、`language`、`file_type`、`doc_type`。

索引更新策略：
- 首次 `scan_workspace()` 时全量建索引。
- 后续增量更新（对比 `content_hash`），仅重新索引变化文件。
- 索引与 LanceDB chunk embeddings 并行维护。

### 4. Context Assembly Layer (omega-context)

#### 4.1 Slot-Based Context Model

取代 `prompt_builder.rs` 中硬编码的字符串拼接，引入声明式 context slots：

```rust
pub struct ContextSlot {
    pub id: String,
    pub priority: SlotPriority,
    pub content: String,
    pub estimated_tokens: u32,
    pub cache_eligible: bool,
    pub compressible: bool,
}

pub enum SlotPriority {
    Critical,  // output contract, current step prompt — 不可裁剪
    High,      // structured input, current todo item — 仅在 budget 极端不足时裁剪
    Medium,    // session summaries, routing context — 按 recency × relevance 裁剪
    Low,       // historical summaries, completed todo — 优先裁剪
    Optional,  // extended context, RAG results — 空间允许时才包含
}
```

#### 4.2 Assembly Pipeline

```
1. Collect slots (skills, routing, summaries, structured_input, todo, output_contract, step_prompt, execute_item)
2. Estimate tokens per slot (prefer provider count_tokens API, fallback to chars/4)
3. Sort by priority descending
4. Greedy pack: include Critical/High unconditionally, then Medium/Low/Optional by budget
5. Inject cache_control markers at stable boundaries
6. Build final system prompt + user message
7. Emit ContextBudgetDiagnostics for observability
```

#### 4.3 Workflow-Driven Compaction

Step 可声明上下文管理指令：

```toml
[steps.execute]
context_policy = { compact_before = true, max_history_turns = 3, prefer_recent_summaries = true }
```

Runner 在进入该 step 前调用 omega-context memory/assembler 接口执行指定压缩策略。

#### 4.4 Tool Registry

omega-context 的 integration adapter 统一注册知识管理相关工具。上层不需要单独注册 document/memory 工具，但 tool registry 保持在适配层而非 facade 核心接口中：

```rust
pub struct ContextToolRegistry {
    facade: Arc<OmegaContextFacade>,
}

impl ContextToolRegistry {
    pub fn register_tools(&self) -> Vec<ToolDefinition> {
        vec![
            self.search_codebase_tool(),
            self.manage_document_tool(),
            self.manage_todo_tool(),
            self.check_doc_health_tool(),
        ]
    }
}
```

**search_codebase tool**

```json
{
  "name": "search_codebase",
  "description": "Search the project codebase for relevant code, documentation, or context. Supports keyword, semantic, and hybrid search with structured filters.",
  "input_schema": {
    "type": "object",
    "properties": {
      "query": { "type": "string", "description": "Natural language or keyword search query" },
      "mode": { "type": "string", "enum": ["keyword", "semantic", "hybrid"], "default": "hybrid" },
      "filters": {
        "type": "object",
        "properties": {
          "language": { "type": "array", "items": { "type": "string" } },
          "file_type": { "type": "array", "items": { "type": "string" } },
          "doc_type": { "type": "array", "items": { "type": "string" } },
          "path_glob": { "type": "string" },
          "modified_after": { "type": "string", "format": "date" },
          "status": { "type": "array", "items": { "type": "string" } },
          "tags": { "type": "array", "items": { "type": "string" } }
        }
      },
      "max_results": { "type": "integer", "default": 10 }
    },
    "required": ["query"]
  }
}
```

**manage_document tool**

```json
{
  "name": "manage_document",
  "description": "Create, archive, update, or inspect documents following project governance rules. Validates naming, structure, cross-references, and frontmatter automatically.",
  "input_schema": {
    "type": "object",
    "properties": {
    "action": { "type": "string", "enum": ["create", "archive", "update_metadata", "health_check", "list"] },
    "mode": { "type": "string", "enum": ["check", "plan", "apply"], "default": "check" },
      "path": { "type": "string", "description": "Document path (relative to project root)" },
      "doc_type": { "type": "string", "enum": ["spec", "prd", "guide", "adr", "todo", "readme"] },
      "title": { "type": "string" },
      "content": { "type": "string" },
      "archive_reason": { "type": "string", "enum": ["superseded", "completed", "outdated", "history_only"] },
      "replaced_by": { "type": "string" }
    },
    "required": ["action"]
  }
}
```

### 5. Token Estimation Upgrade

#### Provider-Backed Estimation

当 provider 支持 `count_tokens` API 时（Anthropic 已支持），优先使用精确计数：

```rust
pub trait TokenEstimator {
    fn estimate_tokens(&self, text: &str) -> u32;
    fn count_tokens_precise(&self, messages: &[Message]) -> anyhow::Result<u32>;
    fn is_precise(&self) -> bool;
}
```

Fallback 仍使用 `chars / TOKEN_ESTIMATE_DIVISOR`。

#### Budget Diagnostics Enhancement

在 `StepDiagnostics` 中报告完整的 budget 分解：

```rust
pub struct ContextBudgetDiagnostics {
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub safety_margin: u32,
    pub available_input_budget: u32,
    pub slots: Vec<ContextSlotUsage>,
    pub cache: Option<CacheDiagnostics>,
    pub total_used: u32,
    pub headroom: i32,
}

pub struct ContextSlotUsage {
    pub slot_id: String,
    pub priority: SlotPriority,
    pub tokens: u32,
    pub included: bool,
    pub truncated: bool,
}
```

## Dependency Flow

```
omega-app ──────────┐
                    │
omega-session ──────┤
                    ▼
              omega-context (唯一对外 facade)
                    │
            ┌───────┴───────┐
            ▼               ▼
       omega-memory    omega-document
                            │
                    ┌───────┴───────┐
                    ▼               ▼
               LanceDB          tantivy
              (向量存储)      (全文检索)
```

关键约束：
- **omega-context 是唯一对外 crate**。omega-session / omega-app / omega-workflow 只依赖 omega-context。
- `omega-memory` 与 `omega-document` 互不依赖，由 `omega-context` 内部编排。
- `omega-memory` 与 `omega-document` 不出现在上层 crate 的 `Cargo.toml` 依赖中。
- `omega-client` 保持 provider-agnostic，`cache_control` 只是可选标记。
- `omega-compression` 现有 crate 合并入 `omega-memory`。

### 存储布局

```
.omega/
├── store/
│   ├── lance/              # LanceDB 文件（files 表 + chunks 表 + turns 表）
│   ├── tantivy/            # tantivy full-text index
│   └── todos.jsonl         # 持久化 todo items
├── memory/
│   └── turns/              # turn archive JSONL files
├── doc-rules.toml          # 文档治理规则（可选，有默认值）
└── context.toml            # 上下文管理配置
```

## Migration Path

### Phase 1: Prompt Path Stabilization (根因修复优先)

- 在 `prompt_builder.rs` / message assembly 层注入 `cache_control` 标记。
- 升级 token estimation 支持 provider `count_tokens` API。
- 在 `omega-session` 内先落地 slot budget MVP、priority-weighted summary selection、compaction trigger，直接替换当前贪心倒序裁剪。
- 暂不引入文档治理、向量数据库和 TUI 仪表盘。
- **收益**：先修复 execute 上下文退化与 structured output 失败。

### Phase 2: omega-memory Extraction + omega-context Facade

- 新建 `omega-memory` 与 `omega-context` crate。
- 把 Phase 1 中已验证的上下文组装与压缩能力从 `omega-session` 抽离到 `omega-context` + `omega-memory`。
- 公开聚焦接口和 `OmegaContextFacade`，而不是一次性暴露巨型 trait。
- **收益**：先稳定行为，再建立长期公共边界。

### Phase 3: omega-document — File Manifest + Governance + Keyword Search

- 新建 `omega-document` crate，接入 omega-context facade。
- 实现 `FileStore` manifest、chunk manager、persistent TODO。
- 实现 tantivy keyword 全文检索。
- 实现 document governance engine + health check + staged mutation planning。
- 通过 omega-context 注册 `search_codebase`（keyword 模式）和 `manage_document` 工具。
- **收益**：跨 session 连续性；规则化文档管理；不引入向量存储前也可稳定运行。

### Phase 4: LanceDB + Hybrid Retrieval

- 接入 LanceDB 作为派生向量索引，而不是替代 `FileStore` 真源。
- chunk embeddings 入库（fastembed 本地 embedding）。
- 实现 revision-aware multi-dimensional composite query（vector + filter + text）。
- `search_codebase` 支持 semantic 与 hybrid 模式。
- **收益**：精确的代码/文档检索，复合查询能力。

### Phase 5: Observability + TUI + Background Indexing

- 增加后台索引 worker、索引 readiness 状态和 TUI 展示。
- 实现 context budget/dashboard、document health popup、search overlay。
- 将索引与诊断事件完整接入 omega-observability。
- **收益**：可运维、可观测、不会阻塞启动路径。

## Workflow Integration Points

### Step Lifecycle Hooks

```
before_step   → OmegaContextFacade.assembler: assemble_context(), inject cache markers
                OmegaContextFacade.memory: compact_context() if threshold reached
during_step   → OmegaContextFacade.query / governance: search_codebase / manage_document tools available
after_step    → OmegaContextFacade.memory: archive_turn(), extract entities
end_turn      → OmegaContextFacade.memory / governance: archive turn summary, persist todo changes
```

### Mandatory Compaction Points

在 workflow 配置中强制摘要时机：

```toml
[workflow.context_policy]
compact_on_step_switch = true
compact_on_budget_threshold = 0.7
max_uncompacted_turns = 5
```

## Observability & Monitoring

### Context Metrics

omega-context 通过 `ContextDiagnostics` 聚合所有子系统指标，由 omega-observability 消费：

```rust
pub struct ContextDiagnostics {
    pub budget: ContextBudgetDiagnostics,
    pub cache: Option<CacheDiagnostics>,
    pub memory: MemoryDiagnostics,
    pub document: DocumentDiagnostics,
    pub store: StoreDiagnostics,
}

pub struct MemoryDiagnostics {
    pub total_turns_archived: u64,
    pub compactions_triggered: u64,
    pub last_compaction_at: Option<u64>,
    pub current_summary_tokens: u32,
    pub compression_ratio_avg: f32,
}

pub struct DocumentDiagnostics {
    pub total_files_indexed: u64,
    pub total_chunks: u64,
    pub total_embeddings: u64,
    pub index_staleness_seconds: u64,         // 距上次扫描的秒数
    pub governance_health: HealthScore,
    pub last_health_check: Option<u64>,
}

pub struct StoreDiagnostics {
    pub lance_db_size_bytes: u64,
    pub tantivy_index_size_bytes: u64,
    pub todo_items_count: u32,
    pub turn_archive_count: u32,
}
```

### Tracing Integration

关键操作记入 tracing spans：

| Span | 触发条件 | 关键字段 |
|------|---------|---------|
| `context.assemble` | 每次 context assembly | slots_included, tokens_used, headroom, cache_eligible |
| `context.compact` | 每次压缩触发 | trigger_reason, tokens_before, tokens_after, compression_ratio |
| `context.search` | search_codebase tool 调用 | mode, filters, results_count, latency_ms |
| `context.doc_op` | manage_document tool 调用 | action, path, violations |
| `context.index` | 文件索引更新 | files_added, files_updated, chunks_changed |
| `memory.archive` | turn archive | turn_id, steps_count, tokens_saved |
| `document.health` | health check | violations_count, overall_health |

### Runtime Message Integration

omega-context 通过 `RuntimeMessage` 向 TUI 报告状态变化：

```rust
pub enum ContextRuntimeMessage {
    /// Context budget snapshot — 每次 assembly 后发送
    BudgetSnapshot(ContextBudgetDiagnostics),
    /// Cache hit/miss event — 每次 API 响应后发送
    CacheEvent { hit: bool, tokens_saved: u32 },
    /// Compaction event — 压缩触发时发送
    CompactionEvent { trigger: String, tokens_freed: u32 },
    /// Index update event — 文件索引变更时发送
    IndexUpdate { files_changed: u32, chunks_changed: u32 },
    /// Document governance alert — 违规检测时发送
    GovernanceAlert { violations: Vec<String>, severity: HealthScore },
}
```

## TUI Integration

### Context Budget Indicator

在 TUI status bar 或 sidebar 中显示实时 context budget 占用：

```
┌─ Context Budget ─────────────────────┐
│ ████████████░░░░░░░  62% (124k/200k) │
│ Cache: 78% hit  │  Slots: 8/12 incl  │
│ Memory: 3 turns │  Index: 1.2k files  │
└──────────────────────────────────────┘
```

**指标来源**：`ContextDiagnostics` → TUI reducer → render。

### Context Slot Breakdown View

可折叠的 slot 详情面板，显示每个 slot 的 priority/tokens/included 状态：

```
┌─ Context Slots ──────────────────────┐
│ [C] output_contract     1.2k  ✓ incl │
│ [C] step_prompt         0.8k  ✓ incl │
│ [H] structured_input    2.4k  ✓ incl │
│ [H] current_todo        0.3k  ✓ incl │
│ [M] session_summaries   8.2k  ✓ incl │
│ [M] routing_context     0.5k  ✓ incl │
│ [L] history_summaries   4.1k  ✗ drop │
│ [O] rag_results         1.8k  ✗ drop │
└──────────────────────────────────────┘
```

### Document Health Dashboard

通过 overlay popup 或 sidebar 子页面展示文档治理状态：

```
┌─ Document Health ────────────────────┐
│ Overall: ✓ Good                      │
│                                      │
│ Structure: 12/12 dirs OK             │
│ Naming:    45/46 files OK            │
│   ⚠ docs/specs/Old-Name.md          │
│ Cross-Ref: 31/32 refs OK            │
│   ⚠ README.md missing omega-ctx.md  │
│ Stale:     2 docs > 30 days         │
└──────────────────────────────────────┘
```

### Search Results Overlay

`search_codebase` tool 结果在 TUI 中以 overlay 展示（当 diagnostics 面板激活时）：

```
┌─ Search: "cache control injection" ──┐
│ Mode: hybrid  │  Results: 5          │
│──────────────────────────────────────│
│ 1. crates/omega-client/src/anthropic │
│    /types.rs:42 [0.94]  Source/Rust  │
│    pub struct AnthropicCacheControl  │
│                                      │
│ 2. docs/specs/omega-context-mgmt.md  │
│    :204 [0.87]  Doc/Spec             │
│    Cache Control Injection...        │
└──────────────────────────────────────┘
```

### TUI 集成实现路径

| 阶段 | TUI 变更 | 依赖 |
|------|---------|------|
| Phase 1 | 无新增 TUI 负担，优先修复执行路径 | — |
| Phase 2 | 最小化 budget/caching 调试视图 | CacheDiagnostics, ContextBudgetDiagnostics |
| Phase 3 | Document health popup + keyword search overlay | DocumentDiagnostics, SearchResult |
| Phase 4 | Search mode toggle (keyword/semantic/hybrid) | Multi-dimensional query |
| Phase 5 | Full context dashboard + compaction/index event feed | Complete ContextDiagnostics |

## Testing Strategy

### Unit Tests

#### omega-memory
| 测试分类 | 测试场景 | 验证标准 |
|---------|---------|---------|
| Turn Archive | 序列化/反序列化 round-trip | JSONL parse 与 struct 一致 |
| Turn Archive | 多轮归档后加载历史 | 返回正确数量和排序 |
| Micro-Compact | Thinking block 压缩 | 压缩比 ≥ 10:1，结论保留 |
| Micro-Compact | Tool output 压缩 | 错误信息保留，stdout 丢弃 |
| Micro-Compact | tool_use_id 一致性 | 压缩后 id 不变 |
| Cache-Aware | 不修改 cache boundary 前内容 | cache 前缀不变 |
| Priority Selection | recency × relevance 排序 | 最近且相关的 step 优先 |

#### omega-document
| 测试分类 | 测试场景 | 验证标准 |
|---------|---------|---------|
| File Store | 增量扫描 | 新增/修改/删除文件正确反映 |
| File Store | 删除文件标记 deleted 不丢弃 | 历史记录可查 |
| Chunk Manager | Rust 文件按 fn/impl 分块 | 块边界在函数起止 |
| Chunk Manager | Markdown 按 heading 分块 | 块边界在 ## 行 |
| Chunk Manager | 增量更新 | 只重建 hash 变化的 chunk |
| Persistent TODO | load/save round-trip | items 与 status 一致 |
| Persistent TODO | 崩溃恢复 | JSONL 最后一行截断仍可恢复 |
| Doc Governance | 命名违规检测 | 大写目录、非法字符报错 |
| Doc Governance | 归档 checklist 执行 | 归档后 README/TODO 引用更新 |
| Doc Governance | health check | 检出 orphan、broken ref、stale |
| Full-Text Index | tantivy 索引 + 查询 | keyword 查询返回正确文件 |
| LanceDB | vector search + filter | 过滤条件正确应用 |
| Composite Query | hybrid mode RRF fusion | 结果合理排序 |

#### omega-context
| 测试分类 | 测试场景 | 验证标准 |
|---------|---------|---------|
| Slot Assembly | Critical slot 不可裁剪 | 超 budget 也保留 |
| Slot Assembly | Optional slot 按 budget 裁剪 | budget 不足时丢弃 |
| Budget Manager | provider count_tokens 优先 | 有 API 时不用 chars/4 |
| Cache Injection | 稳定 boundary 注入 anchor | anchor 位置在正确 slot 边界 |
| Tool Registry | register_tools 返回完整工具集 | 所有工具可序列化为 JSON schema |
| Facade | OmegaContextFacade 组合子接口 | 组装/query/governance/diagnostics 边界清晰 |
| Diagnostics | 指标聚合完整 | budget + cache + memory + document 均有值 |

### Integration Tests

| 测试场景 | 范围 | 验证标准 |
|---------|------|---------|
| 多轮 execute 稳定性 | omega-context + omega-session | 10+ 轮 execute 后 structured output 仍然 valid |
| Compaction 正确性 | omega-context + omega-memory | 压缩后 context 仍包含 critical slots |
| 跨 session 恢复 | omega-context + omega-document | 新 session 能读到上次的 todo + file index |
| search_codebase E2E | omega-context + omega-document | keyword/semantic/hybrid 返回预期结果 |
| manage_document E2E | omega-context + omega-document | 创建/归档/health_check 遵循规则 |
| Cache hit ratio | omega-context + omega-client | 连续 step 中 cache_hit_ratio > 0.5 |
| TUI diagnostics | omega-context + omega-tui | ContextRuntimeMessage 正确渲染 |

### Test Infrastructure

```rust
/// 测试用 omega-context facade 工厂，使用 temp_dir 隔离存储
pub fn test_context_facade(root: &Path) -> OmegaContextFacade {
    let config = ContextConfig::test_defaults(root);
    DefaultOmegaContextFacade::new(config).unwrap()
}

/// Mock TokenEstimator，用于确定性测试
pub struct MockTokenEstimator {
    pub tokens_per_char: f32,
}
```

## Configuration

### .omega/context.toml

```toml
[memory]
turn_archive_dir = ".omega-state/memory/turns"
max_archived_turns = 100
compact_threshold = 0.7          # fraction of context_window
thinking_compression_ratio = 10
tool_output_compression_ratio = 5

[document]
chunk_max_tokens = 500
chunk_overlap_tokens = 50        # chunk 间重叠，保证边界连续性
embedding_model = "disabled"    # Phase 4 之前默认禁用，避免启动路径被 embedding 阻塞
embedding_dimension = 384        # fastembed default
scan_on_startup = false          # 默认不阻塞启动；由后台索引 worker 触发首次扫描
scan_interval_minutes = 10       # 增量扫描间隔
background_indexing = true       # 启动后异步索引

[store]
lance_path = ".omega-state/store/lance"
tantivy_path = ".omega-state/store/tantivy"
todo_path = ".omega-state/store/todos.jsonl"

[governance]
rules_path = ".omega/doc-rules.toml"  # 自定义规则（可选）
health_check_on_startup = false
stale_threshold_days = 30

[cache]
enabled = true
max_anchors = 4
anchor_tools = true
anchor_system_prefix = true
anchor_summaries = true
anchor_last_assistant = true

[budget]
safety_margin_tokens = 2000
prefer_provider_count = true
fallback_divisor = 4
```

### .omega/doc-rules.toml（默认值对齐 AGENTS.md）

```toml
[structure]
required_files = ["README.md", "docs/TODO.md", "LICENSE"]

[[structure.expected_dirs]]
path = "docs/specs"
purpose = "Formal specs, contracts, and repository rules"
file_pattern = "*.md"
doc_type = "spec"

[[structure.expected_dirs]]
path = "docs/prds"
purpose = "Plans, architecture, and design details"
file_pattern = "*.md"
doc_type = "prd"

[[structure.expected_dirs]]
path = "docs/guide"
purpose = "Usage guides and contributor workflows"
file_pattern = "*.md"
doc_type = "guide"

[[structure.expected_dirs]]
path = "docs/decisions"
purpose = "Durable architecture decisions"
file_pattern = "*.md"
doc_type = "adr"

[[structure.expected_dirs]]
path = "docs/archive"
purpose = "Retired, superseded, or historical documents"
file_pattern = "*.md"
doc_type = "archive"

[naming]
lowercase_dirs = true

[lifecycle]
archive_checklist = [
    "Add archive note at top of file",
    "Update README.md links",
    "Update docs/TODO.md",
    "Record in CHANGELOG.md if milestone"
]
required_frontmatter = ["status"]

[cross_ref]
readme_must_index = ["docs/specs/*.md", "docs/prds/*.md", "docs/guide/*.md"]
archive_update_targets = ["README.md", "docs/TODO.md"]
replacement_must_backlink = true
```

## Risks

| Risk | Level | Mitigation |
|------|-------|------------|
| Cache anchor instability causes cache miss storms | High | Monitor `cache_hit_ratio`; fallback to no-cache if ratio < 0.3 |
| Over-aggressive compression loses critical context | High | Critical/High priority slots never compressed; only Low/Optional |
| LanceDB storage growth on large repos | Medium | 定期 compact + 可选限制索引范围（.gitignore 排除） |
| `.storeignore` 规则误配导致知识库缺项 | Medium | 在 scan/command diagnostics 中暴露 ignored/indexed/embedded 样本，并允许用户通过编辑 `.omega/.storeignore` 后重建 |
| Persistent TODO corruption on crash | Medium | JSONL append-only with checksum; replay from scratch on error |
| Embedding quality variance across models | Medium | fastembed 本地默认；semantic 模式可选 |
| Doc governance false positives | Medium | 规则可覆盖（.omega/doc-rules.toml）；warning 不 block |
| Provider count_tokens latency adds overhead | Low | Batch estimation; cache results within step |
| tantivy index corruption | Low | 启动时 validate + 自动 rebuild on error |

## Related Specs

- `docs/specs/omega-step-session-asset-model/session-context-and-data-contracts.md`
- `docs/specs/omega-client-anthropic-api-abstraction.md`
- `docs/specs/omega-runtime-message-pipeline.md`
- `docs/specs/omega-step-lifecycle-hooks.md`
- `docs/specs/omega-tool-system-upgrade.md`

---

### Change Log

- 2026-03-27 v0.1: 初版规格，定义三层上下文管理体系（Memory / Document / Context），接入 Anthropic cache_control 主动缓存，规划五阶段实施路径。
- 2026-03-27 v0.2: 重大修订：(1) omega-context 作为唯一对外 facade，omega-memory / omega-document 不直接暴露；(2) 接入 LanceDB 嵌入式向量数据库 + tantivy 全文检索，支持多维复合查询；(3) 新增 Document Governance Engine，基于规则的文档生命周期管理（对齐 AGENTS.md docs skill）；(4) 新增 Observability/Monitoring 章节、TUI Integration 章节、完整 Testing Strategy。
- 2026-03-27 v0.3: 根据架构评审优化：(1) 将根因修复前移到 Phase 1，避免向量/治理基础设施先于 execute 修复；(2) 用聚焦接口 + `OmegaContextFacade` 替代单一 god trait；(3) 明确 `FileStore` 为真源、tantivy/LanceDB 为派生索引，并加入 revision 一致性协议；(4) `manage_document` 改为 check/plan/apply staged 模式；(5) 默认禁用启动期 embedding，并改为后台索引。
- 2026-04-03 v0.4: `.omega/.storeignore` 已调整为 store-level 排除规则：匹配路径在扫描阶段直接跳过，不进入 `FileStore` manifest、tantivy 或 LanceDB；`/document init|sync` 还会暴露 ignored/indexed/embedded 样本，帮助解释本次处理范围。
