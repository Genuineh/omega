---
status: draft
last_verified_commit: N/A
owner: omega-team
created: 2026-04-09
updated: 2026-04-09
version: 0.1
supersedes: []
related_prds:
  - docs/specs/omega-context-management.md
  - docs/specs/omega-command-system.md
  - docs/specs/omega-tui-document-memory-supervision.md
  - docs/specs/omega-app-package.md
---

# Omega Project System Specification

## Overview

当前 `omega-context` 同时承担了两类边界：

1. **session-local 上下文装配**：当前轮对话、step summaries、tool contracts、结构化输入和上下文预算。
2. **repository-owned 知识与治理**：`OmegaContextFacade::local(root)` 直接创建 `LocalMemoryService`、`OmegaDocument`、document governance 与 diagnostics 聚合，并默认把 `cwd/root` 当作唯一仓库边界。

这条边界在单仓库、单 session 模式下可工作，但已经开始阻碍后续演进：

- `omega-session` 只有 `cwd` 和 `OmegaContextFacade`，没有显式 `project_id`，因此无法稳定表达“多个 session 属于同一个项目”。
- `/document` 计划与当前实现都是 repo-scoped，但系统里没有一个正式的 `project` 根对象来持有 document/memory/session 关系。
- 当前 TUI 底部状态栏只有 `Workflow / Agent / Session` 三个 runtime slot，无法把“当前项目”和它的 knowledge/session 详情作为稳定可点击状态暴露出来。

本规格引入新的 `omega-project` 仓库级模块，把“项目识别、项目绑定的 document/memory、项目下多个 session、`/project` 命令和项目详情 UI”收敛成一个正式边界；同时把 `omega-context` 收敛为 **session-local context assembly layer**，不再直接拥有仓库级 document/memory 生命周期。

## Goals

- 能基于当前文件、当前 cwd 或显式选择，确定当前活跃 `project`。
- 让 `omega-document` 与项目根目录绑定，而不是以 session-local `cwd` 临时隐式持有。
- 让一个 project 下可同时关联多个 session，并把 `session_id -> project_id` 作为显式契约记录。
- 让 `omega-context` 只负责当前 session 的对话、step 资产和上下文装配；所有 project-scoped knowledge 通过 `omega-project` 提供。
- 为 `omega-command` 增加 `/project` 命令族，覆盖列出、切换、删除、查看详情、查看关联 session 和知识库内容等操作。
- 在底部状态栏增加当前 project 段，并支持点击打开 project detail overlay。

## Non-Goals

- 不在第一阶段实现跨机器同步的全局项目目录服务。
- 不要求第一阶段把 project detail 做成新的常驻 sidebar panel；overlay 即可作为主详情载体。
- 不要求第一阶段重写 `omega-document` 的内部索引结构；重点是 ownership 和 routing 边界迁移。
- 不把 `omega-project` 设计成新的 god object；command、session、TUI 仍通过明确接口消费 project 能力。
- 不把 `omega-todo` 纳入 `omega-project`；todo 继续保持 runtime / session 级 working state，不作为 project-bound persistence 或 knowledge surface 的一部分。

## Current Findings

### 当前实现锚点

- `omega-context` 的 `OmegaContextFacade::local(root)` 直接创建 `LocalMemoryService`、`OmegaDocument`、query/governance/diagnostics，说明 repo-scoped knowledge 目前挂在 context facade 内部。
- `omega-session::AgentSession` 当前仅持有 `cwd` 与 `Arc<OmegaContextFacade>`，没有 `project_id`、`project_handle` 或 project-scoped session catalog。
- `omega-command` 规格已经把 `/document` 定义成 repo-scoped operator surface，但前提仍是“`omega-context` 是 document/memory 的唯一上层入口”。
- `omega-tui` 当前 runtime UI 只有 `StatusSlot::Workflow | Agent | Session`；底部状态栏支持点击，但现在整条 bar 点击只会打开 delivery detail，并没有 segment-level 项目命中模型。
- `omega-todo` 当前以 `SharedTodoManager = Arc<Mutex<TodoManager>>` 的形式由 runtime/tool 装配注入，是当前会话内的 working set，不是 project root 下的 repo-local source of truth。

### 结论

现状的主要问题不是“缺一个 `/project` 命令”，而是**缺一个 project-owned 根对象**。如果继续把 repo-owned document/memory 挂在 `omega-context` 内，再叠加 `/project`、多 session 和项目详情 UI，会把 context 进一步推向 god object。

## Architecture

### Ownership Shift

| Layer | Responsibility |
|-------|----------------|
| `omega-project` (new crate) | 项目识别、项目注册表、当前项目选择、project-bound `omega-document` / project memory、project session catalog、project summary / diagnostics、`/project` adapter 所需的 service surface |
| `omega-context` | **只保留 session-local context assembly**：当前对话、step summaries、structured input、budget/cache、prompt assembly；通过 provider traits 消费 project knowledge，不再直接创建 `OmegaDocument` 或 memory store |
| `omega-session` | 持有 `session_id` + `Arc<OmegaProjectHandle>` + session-local `OmegaContextSession`，负责 session 生命周期与 project 关联写入 |
| `omega-command` | 新增 `/project` descriptor / parser / hint metadata；不直接持有存储或 UI 依赖 |
| `omega-app` | 装配 `ProjectRegistry`、active project selection、command handlers 和 TUI runtime wiring |
| `omega-tui` | 渲染当前 project 状态段，支持项目详情 overlay 和后续 drill-down |

### Runtime Todo Boundary

`omega-project` 的 ownership 仅覆盖 **repo-scoped、需要跨 session 稳定复用** 的资产：document、project memory、session catalog 与 project metadata。

`omega-todo` 明确保留为 runtime / session-scoped working state：

- 当前 `TodoManager` 生命周期跟随 runtime 注入与当前交互过程，不写入 project registry。
- `/project switch` 不迁移、不持久化当前 todo 列表；如需 project-bound task board，必须另立规格与数据模型。
- project detail overlay、project summary 和 project knowledge snapshot 默认不展示 runtime todo items，避免把 session working set 误报成 project knowledge。

### Dependency Direction

为避免循环依赖，采用以下方向：

```text
omega-session ─────┬─────> omega-project ─────> omega-document
                   │                │
                   │                └─────> omega-memory
                   │
                   └─────> omega-context

omega-context 只定义 project-facing provider traits，
不再直接依赖 omega-document / omega-memory 的 concrete ownership。
omega-project 实现这些 traits，并把 project-scoped knowledge 暴露给 session/context。
```

关键约束：

- `omega-context` 不再提供 `OmegaContextFacade::local(root)` 这种“直接用 root 构造 repo knowledge”的入口。
- `omega-project` 不拥有 session-local transcript 或 step outputs；它只拥有 project-scoped knowledge 和 session catalog。
- `omega-session` 既不直接依赖 `omega-document`，也不绕过 `omega-project` 写 project memory。

## Project Identification

### Resolution Inputs

项目解析按优先级使用以下输入：

1. `current_file_path`：编辑器当前激活文件。
2. `session cwd`：当前交互根目录。
3. `explicit project selection`：用户通过 `/project switch` 或 UI 选择的目标。

### Resolution Algorithm

1. 如果存在显式选择且当前文件仍位于该 root 下，保持当前 project。
2. 否则从 `current_file_path` 或 `cwd` 向上查找项目根标记：
   - `.omega/project.toml` 或 `.omega/project.json`
   - `.git/`
   - 仓库入口文件，如 `Cargo.toml`、`package.json`
3. 找到根目录后做 canonicalize，并生成稳定 `project_id`。
4. 若无显式仓库标记，则退化为最近可用目录的 `loose project`，readiness 标为 `unmanaged`，允许后续手动绑定。

### Project Identity

`project_id` 必须稳定且与路径重命名外的 session 生命周期解耦。推荐生成规则：

```text
project_id = short_hash(canonical_project_root_path)
```

显示名默认使用以下优先级：

1. `.omega/project.toml` 中声明的 `name`
2. 根目录名
3. fallback: canonical root path

## Persistence Layout

项目级状态与项目内知识应优先保持 repo-local，可通过当前仓库直接追踪：

```text
<project-root>/.omega/
  project.json              # project metadata snapshot
  sessions/
    <session-id>.json       # session metadata and last-active summary
  memory/                   # project-bound archived turns, observations, facts
  store/                    # omega-document manifest / tantivy / vector store
```

如需跨仓库 `list/switch`，再补一层 app-owned recent-project registry，仅保存 `project_id -> root path` 的最近使用缓存；repo-local 数据仍是 source of truth。

## Data Models

### Core Project Types

```rust
pub struct ProjectRecord {
    pub project_id: String,
    pub display_name: String,
    pub root: PathBuf,
    pub detection_kind: ProjectDetectionKind,
    pub created_at: u64,
    pub last_opened_at: u64,
    pub active_session_id: Option<String>,
}

pub enum ProjectDetectionKind {
    Explicit,
    CurrentFile,
    Cwd,
    LooseDirectory,
}

pub struct ProjectSessionRef {
    pub session_id: String,
    pub title: String,
    pub started_at: u64,
    pub last_active_at: u64,
    pub status: ProjectSessionStatus,
    pub turn_count: u64,
    pub last_user_turn_preview: Option<String>,
}

pub enum ProjectSessionStatus {
    Active,
    Idle,
    Archived,
}

pub struct ProjectKnowledgeSummary {
    pub document: DocumentSupervisionSnapshot,
    pub memory: MemorySupervisionSnapshot,
    pub session_count: usize,
    pub active_session_id: Option<String>,
}
```

### Runtime UI Projection

```rust
pub enum StatusSlot {
    Workflow,
    Agent,
    Session,
    Project,
}

pub enum StatusValue {
    // existing variants...
    ProjectSelection {
        project_id: String,
        display_name: String,
        root_name: String,
        session_count: usize,
        document_readiness: SupervisionReadiness,
        memory_readiness: SupervisionReadiness,
    },
}
```

项目详情 overlay 建议基于以下 snapshot 渲染：

```rust
pub struct ProjectDetailSnapshot {
    pub record: ProjectRecord,
    pub sessions: Vec<ProjectSessionRef>,
    pub knowledge: ProjectKnowledgeSummary,
    pub recent_documents: Vec<FileRecord>,
}
```

## Context Integration

### New Boundary

`omega-context` 改为 session-local assembly surface，不再直接拥有 project memory/document 的 concrete service：

```rust
pub trait ProjectKnowledgeProvider: Send + Sync {
    fn project_record(&self) -> ProjectRecord;
    fn search_documents(&self, query: SearchQuery) -> anyhow::Result<Vec<SearchResult>>;
    fn query_memory(&self, query: MemoryQuery) -> anyhow::Result<Vec<MemoryQueryHit>>;
    fn query_observations(&self, query: ObservationQuery) -> anyhow::Result<Vec<ProjectObservation>>;
    fn archive_turn(&self, turn: ProjectTurnData) -> anyhow::Result<()>;
    fn supervision_snapshot(&self) -> ContextSupervisionSnapshot;
}
```

`omega-context` 的职责变为：

- 接收当前 session 的对话与 step 资产
- 调用 `ProjectKnowledgeProvider` 做 document/memory recall
- 生成 session-local `AssembledContext`
- 聚合 budget/cache/session diagnostics

`omega-project` 的职责变为：

- 绑定 project root 下的 `omega-document`
- 绑定 project root 下的 project memory
- 维护 session catalog 和 project summary
- 把 project recall 与 turn archival 暴露给 `omega-context`

### Session Data Contract Changes

以下字段需要显式进入 session/context 契约：

- `session_id`
- `project_id`
- `project_root`
- `project_display_name`

`TurnData` 或其替代类型必须扩展为 project-aware：

```rust
pub struct ProjectTurnData {
    pub project_id: String,
    pub session_id: String,
    pub turn_id: u64,
    pub workflow_id: String,
    pub user_intent: String,
    pub summaries: Vec<ContextStepSummary>,
    pub signals: TurnRetentionSignals,
}
```

## Session Association

### Rules

- 一个 project 可以关联多个 session。
- 一个 session 在任意时刻只能绑定一个 project。
- session 创建时必须先解析或选择 project，再创建 `AgentSession`。
- session 结束时，project catalog 必须更新 `last_active_at`、turn count 和最后摘要预览。

### Lifecycle

1. `omega-app` 启动或编辑器当前文件变更时解析 active project。
2. 用户新开 session 时，`omega-project` 生成/记录 `session_id` 并返回 `OmegaProjectHandle`。
3. `omega-session` 使用该 handle 构造 session-local context service。
4. 每个 turn archival 通过 `omega-project` 记录，保证 memory 与 session catalog 同时更新。
5. `/project switch` 切换项目时，当前 session 要么关闭，要么以新的 `session_id` 重新绑定目标 project，禁止隐式跨项目复用同一 session transcript。

## Command System Integration

### `/project` Command Family

新增 builtin command family：

| Command | Purpose |
|---------|---------|
| `/project list` | 列出本地已知 project、当前 active project、最近活跃时间与 session/document readiness 摘要 |
| `/project switch <selector>` | 切换当前 active project；`selector` 可为 `project_id`、root path 或 display name |
| `/project info [project_id]` | 查看 project 详情，包含 root、detection、session count、document/memory readiness |
| `/project sessions [project_id]` | 查看该 project 关联 sessions，支持 active/idle/archived 状态摘要 |
| `/project knowledge [project_id]` | 查看该 project 的 document/memory 摘要、最近文档与 knowledge totals |
| `/project delete <project_id>` | 删除 project registry entry；默认只忘记 registry，`--purge` 才删除 `.omega/project.json` / session catalog / local recent cache |

### `/document` Rebinding Rule

`/document` 不再以 session `cwd` 作为默认根，而是必须使用 `CurrentProjectHandle`：

- 当前 project 有效时：直接在 project root 上执行 document ops
- 当前 project 缺失但可从当前文件解析时：先解析 project，再执行 document ops
- 当前 project 不可解析时：命令返回明确错误，并提示 `/project switch` 或显式绑定

## TUI Integration

### Bottom Status Bar

底部状态栏新增独立 `Project` segment，而不是复用 `Session` 或 `Aux`：

- 展示：`project <display_name> · <session_count> sessions · docs <readiness>`
- 位置：建议位于 `Session` / `Workflow` 之后、`Delivery` 之前
- 状态颜色：沿用 readiness 语义色，不自行发明新体系

### Click Behavior

当前状态栏点击行为是“整条 bar 打开 delivery detail”。改为 **segment-aware hit testing**：

- 点击 `Project` segment：打开 `Project Detail` overlay
- 点击 `Delivery` segment：保持现有 delivery detail 行为
- 点击其他 segment：仅聚焦或无操作，不隐式复用 delivery 行为

### Project Detail Overlay

overlay 至少展示：

- project display name / project id / root path
- detection source
- active session id
- session 列表摘要（最近活跃时间、turn count、preview）
- document readiness、active store version、indexed files/chunks
- memory readiness、archived turns、observations、last compaction

后续可扩展为“从 overlay 直接打开 `/project sessions` 或 `/project knowledge` 的 drill-down”，但第一阶段不强制要求常驻 sidebar panel。

## Technical Decisions

| Decision | Choice | Rationale |
|---------|--------|-----------|
| 项目标识 | canonical root path 的稳定 hash | 不依赖 session 生命周期，且无需额外全局 id 服务 |
| project state 存储 | 以 repo-local `.omega/` 为主，recent-project registry 为辅 | document/memory 与仓库强绑定，且便于直接追踪与迁移 |
| context 边界 | `omega-context` 只保留 session-local assembly | 避免 context 继续膨胀成 repo-owned knowledge god object |
| todo 边界 | `omega-todo` 保持 runtime / session scoped | 当前实现是 runtime 注入的 in-memory manager，不是 repo-local project persistence |
| command 入口 | `/project` 作为 builtin family | 用户显式操作和运维面需要稳定入口，不能依赖自然语言或 tool 拼装 |
| TUI 呈现 | 新增 `StatusSlot::Project` + detail overlay | 项目态是长期状态，不应挤进日志或临时 notice |

## Migration Plan

### Phase 1: Project Identity And Registry

- 新建 `omega-project` crate
- 增加 project detection / registry / record persistence
- 让 app 启动时解析 active project 并注入 runtime

### Phase 2: Session Binding

- `omega-session` 创建时必须拿到 `project_id` 和 `session_id`
- turn archival 通过 `omega-project` 记录
- 增加 project session catalog

### Phase 3: Context Ownership Shift

- 去掉 `omega-context` 对 `omega-document` / project memory 的直接 concrete ownership
- 引入 provider traits，由 `omega-project` 实现
- 更新 diagnostics / supervision snapshot 的来源边界

### Phase 4: `/project` Commands And `/document` Rebinding

- 新增 `/project` family handlers
- `/document` command 改用 current project handle
- 让 command hint / enablement 反映 active project readiness

### Phase 5: TUI Project Visibility

- 增加 `StatusSlot::Project`
- 实现 status segment hit test 与 project detail overlay
- 验证 project switch 后状态栏与 overlay 一致刷新

## Testing Strategy

- `omega-project` 单测覆盖：current-file detection、path canonicalization、project-id 稳定性、registry persistence。
- `omega-session` 集成测试覆盖：session 创建会绑定 `project_id`，turn archival 会写 project session catalog。
- `omega-context` 集成测试覆盖：project provider 缺失/切换时 recall 与 diagnostics 语义正确。
- `omega-command` / `omega-app` 集成测试覆盖：`/project list|switch|info|sessions|knowledge|delete` 的 parse/dispatch/output contract。
- `omega-tui` 测试覆盖：底部 project segment 渲染、鼠标点击打开 project detail overlay、project switch 后 UI 同步。

## Open Questions

- `loose project` 是否允许后续升级为显式 repo project，及其 `project_id` 迁移规则。
- `project delete --purge` 是否删除 `.omega/store/` 与 `.omega/memory/` 全量数据，还是只删 registry + session catalog，保留知识库存量。
- 当当前文件跨项目切换时，是否强制关闭当前 session，还是允许用户显式选择“保留旧 session 仅查看、新建 session 用于新 project”。

## Change Log

- 2026-04-09: 首次新增 `omega-project` 规格，定义 project-owned document/memory/session boundary、`/project` command family 和底部状态栏 project segment。