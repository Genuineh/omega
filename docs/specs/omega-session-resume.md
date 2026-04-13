---
status: draft
last_verified_commit: N/A
owner: omega-team
created: 2026-04-10
updated: 2026-04-11
version: 0.2
supersedes: []
related_prds: []
---

# Omega Session Lifecycle, Resume, And Context Ledger Specification

## Overview

当前仓库已经具备 Phase 1 的 `/session` control plane、overlay-first picker、resume hydration 和 repo-local sidecar persistence，但这套基线仍建立在两个已经不满足目标的假设上：

1. **启动阶段即决定 active session**：应用启动时会复用或创建一个 session，这让“打开应用但尚未开始任务”与“已经进入某个 session”混在一起，也让 resume 语义变得含糊。
2. **`snapshot.json + log.jsonl` 双文件恢复模型**：恢复上下文、恢复 UI 日志和搜索旧记忆依赖不同 sidecar，导致 source of truth 分裂，后续压缩与 recall 无法围绕同一份可解析历史演进。

本版规格将 session 系统重定向到新的基线：

1. **lazy session binding**：应用启动后处于 `Unbound` 状态，不自动创建或恢复 session；首条真实用户消息或显式 `/session new` 才创建 session，显式 `/session resume` 才绑定旧 session。
2. **canonical context ledger**：每个 session 使用统一、可解析、可追加的 `session.context.jsonl` 作为唯一历史真相，替代 `snapshot.json` 与 `log.jsonl` 的分裂模型。
3. **compression-driven context assembly**：`omega-compression` 负责在默认 400k token 预算下，对 `session.context.jsonl` 做近期优先装载、历史压缩、查询检索和恢复补全。
4. **single-source resume and hydration**：恢复上下文与 UI 日志都从同一份 ledger 投影，而不是分别依赖 snapshot 和 replay sidecar。

overlay-first 的 `/session` operator UX 仍保留，交互层契约继续由 [docs/specs/omega-operator-picker-overlay.md](docs/specs/omega-operator-picker-overlay.md) 补充定义；本规格只重写 session lifecycle、storage 和 context assembly 的底层合同。

## Goals

- 启动应用时不自动创建 session；只有首条真实用户输入、显式 `/session new` 或显式 `/session resume` 才会绑定 session。
- 用统一的 `session.context.jsonl` 记录 session 历史、working-state 投影和压缩 checkpoint，作为 resume、context load、UI hydration 和历史 recall 的唯一 source of truth。
- 默认在 400k token 上下文预算内优先装载近期记录，并让更老的历史通过压缩 checkpoint 和搜索 recall 继续可用。
- 保持 `/session` 的 overlay-first operator 路径，但把“resume 什么、加载多少上下文、如何显示旧日志”统一建立在 ledger projection 上。
- 保持 `omega-project` 只负责 repo-local session storage 与目录布局，避免把压缩策略和 runtime binding 逻辑下沉成新的 god object。
- 保持 `omega-session` 拥有 active binding / restore orchestration，`omega-context` 只拥有 context assembly，`omega-compression` 只拥有预算、压缩和检索策略。

## Non-Goals

- 不恢复正在进行中的 tool process、streaming token flow 或未完成 workflow step 的执行现场；恢复后只进入新的 turn。
- 不让启动流程隐式挑选一个旧 session 并直接加载；用户仍需通过首条输入或显式 resume 决定当前会话。
- 不把 TUI scroll/focus/widget 细节写进 ledger；持久化内容仍限制在 frontend-neutral 的 session 历史与 working-state 投影。
- 不让 `/session delete` 直接删除 repo-scoped memory/document observations；session ledger 只是 session 视角的上下文账本，不是整个 project knowledge store。
- 不在本阶段引入 session merge、branching conversation tree 或自动续跑旧 turn 等更重的 workflow 语义。

## Architecture

### Runtime Lifecycle

新的生命周期合同如下：

1. `omega-app` 启动时只完成 project/bootstrap、session catalog 读取和 UI 初始化；`AgentSession` 初始绑定态为 `Unbound`。
2. 用户输入第一条真实消息时，`omega-session` 才调用 `ensure_bound_session_for_turn()` 创建一个新 session，并在写入 turn 记录前创建 ledger。
3. 用户执行 `/session new` 时，显式创建并绑定一个空白新 session。
4. 用户执行 `/session resume` 时，先经 picker 或 direct command 选定目标 session，再从目标 ledger 构建 context window 与 UI hydration snapshot，最后完成绑定。
5. 应用关闭或重启后不自动恢复任何 active binding；持久化层最多只保留 `last_selected_session_id` 之类的 hint，用于 picker 排序，而不是启动自动恢复。

这意味着“当前 active session”是 runtime-only 概念，而不是启动时必须兑现的持久化承诺。

### Component Boundaries

| Component | Responsibility |
|----------|----------------|
| `omega-project` | session catalog、session directory、ledger 文件和迁移入口的 repo-local 持久化 |
| `omega-session` | runtime binding state、first-turn lazy create、resume orchestration、hydration request 生成 |
| `omega-context` | 根据压缩结果与 recall query 组装 prompt-facing session context |
| `omega-compression` | token 估算、ledger segment compaction、recency-first load、historical search / recall ranking |
| `omega-command` | `/session` subcommand surface、argument parsing、slash hint metadata |
| `omega-app` | startup/bootstrap、runtime message policy、config budget 注入 |
| `omega-tui` | picker/detail/confirm overlay、restore hydration projection、unbound startup surface |

### Storage Layout

每个 session 使用独立目录，统一承载 metadata 与 ledger：

```text
<project-root>/.omega/
    project.toml

<project-root>/.omega-state/
    project.json
    sessions/
    <session-id>/
      session.json               # lightweight catalog metadata
      session.context.jsonl      # canonical append-only context ledger
  memory/
  store/
```

设计约束：

- `session.json` 只保留 list/info/picker 所需的轻量 metadata，不塞入大体积 transcript。
- `session.context.jsonl` 是 session 历史的唯一 source of truth；resume、UI hydration、working-state restore 和 recall 都必须从它投影。
- 允许未来增加可重建的 cache/index sidecar，但这些 sidecar 不能替代 `session.context.jsonl` 成为 canonical source。
- 旧 `<session-id>.snapshot.json` / `<session-id>.log.jsonl` 只在迁移期以兼容入口存在，目标态不再写入。

## Data Models

### Catalog Metadata

`session.json` 继续承担 catalog 角色，但不再把“当前应用启动后应直接恢复它”编码成强绑定语义：

```rust
pub struct ProjectSessionRef {
    pub session_id: String,
    pub title: String,
    pub started_at: u64,
    pub last_active_at: u64,
    pub status: ProjectSessionStatus,
    pub turn_count: u64,
    pub last_user_turn_preview: Option<String>,
    pub resume_ready: bool,
    pub archived_turn_count: u64,
    pub context_schema_version: u32,
    pub last_compacted_at: Option<u64>,
}
```

- `resume_ready`：至少有可解析 ledger，且最近一轮 working-state 投影可恢复。
- `status` 在迁移期仍可保留 `Active/Idle/Archived`，但 `Active` 不再驱动启动自动绑定；实现阶段应逐步收敛成更弱的 catalog 状态语义。
- `context_schema_version`：标识 ledger schema，供迁移与向后兼容使用。
- `last_compacted_at`：辅助 list/info 展示和压缩健康检查，但不是恢复前提。

### Canonical Context Ledger

`session.context.jsonl` 采用 append-only JSONL，每条记录都带稳定 envelope：

```rust
pub struct SessionContextRecord {
    pub schema_version: u32,
    pub session_id: String,
    pub sequence: u64,
    pub recorded_at: u64,
    pub token_estimate: Option<u32>,
    pub record: SessionContextRecordKind,
}
```

记录类型至少覆盖以下几类：

```rust
pub enum SessionContextRecordKind {
    SessionOpened { title: String, cwd: Option<PathBuf> },
    UserTurn { turn_id: u64, body: String },
    AssistantTurn { turn_id: u64, sections: Vec<SessionSectionRecord> },
    CommandSection { command: String, status: String, body: String },
    WorkingSetSnapshot {
        selected_workflow_id: Option<String>,
        selected_skill_ids: Vec<String>,
        loaded_skill_ids: Vec<String>,
        todo_snapshot: Vec<TodoItemSnapshot>,
        latest_user_turn: Option<String>,
        last_known_cwd: Option<PathBuf>,
    },
    CompressionCheckpoint {
        checkpoint_id: String,
        source_range: std::ops::RangeInclusive<u64>,
        summary: String,
        keywords: Vec<String>,
        retained_facts: Vec<String>,
        token_count: u32,
    },
    SearchAnchor { text: String, tags: Vec<String> },
    SessionArchived { reason: Option<String> },
}
```

关键约束：

- 用于恢复 session working state 的内容不再单独存成 `snapshot.json`，而是写成 `WorkingSetSnapshot` 记录并附着在同一条 ledger 上。
- 用于 UI hydration 的可读日志不再单独存成 `log.jsonl`，而是由 `UserTurn` / `AssistantTurn` / `CommandSection` 等记录直接投影。
- `CompressionCheckpoint` 也写回同一份 ledger，这样被压缩的历史与压缩结论仍位于同一份可解析记录流中。

### Resume Projection

resume 不直接把整份 ledger 全量塞进上下文，而是让 `omega-compression` 产出三类投影：

```rust
pub struct SessionContextLoadRequest {
    pub session_id: String,
    pub max_tokens: usize,
    pub goal: SessionContextLoadGoal,
    pub query: Option<String>,
}

pub enum SessionContextLoadGoal {
    ResumeContext,
    PromptAssembly,
    HistoricalSearch,
    UiHydration,
}
```

```rust
pub struct SessionContextLoadResult {
    pub recent_records: Vec<SessionContextRecord>,
    pub checkpoint_records: Vec<SessionContextRecord>,
    pub matched_records: Vec<SessionContextRecord>,
    pub reconstructed_working_set: Option<WorkingSetSnapshot>,
    pub estimated_tokens: u32,
}
```

语义要求：

- `ResumeContext`：默认按 400k token 预算从尾部优先加载近期原始记录，再回填最新 checkpoint，最后按需要补搜索命中。
- `PromptAssembly`：只返回真正需要喂给模型的上下文片段，不必等于 UI 所见的全部日志。
- `HistoricalSearch`：基于 query 在近期原始记录和旧 checkpoint 中做匹配，返回可注入的历史证据。
- `UiHydration`：返回足够重建当前 UI 的可读 turn/section 投影；如果历史过长，可先返回 tail window，并显式标注存在被折叠的更早历史。

## API Specification

### Project Session Persistence

`omega-project` 需要从“管理 sidecar 文件”升级成“管理 session 目录与 canonical ledger”：

```rust
pub trait ProjectSessionStore: Send + Sync {
    fn upsert_session(&self, update: ProjectSessionUpdate) -> anyhow::Result<ProjectSessionRef>;
    fn load_session(&self, session_id: &str) -> anyhow::Result<ProjectSessionRef>;
    fn list_sessions(&self) -> anyhow::Result<Vec<ProjectSessionRef>>;
    fn append_context_records(&self, session_id: &str, records: &[SessionContextRecord]) -> anyhow::Result<()>;
    fn load_context_records(&self, session_id: &str) -> anyhow::Result<Vec<SessionContextRecord>>;
    fn migrate_legacy_session_artifacts(&self, session_id: &str) -> anyhow::Result<LegacyMigrationResult>;
    fn delete_session_artifacts(&self, session_id: &str) -> anyhow::Result<()>;
}
```

### Compression Surface

`omega-compression` 不应只做“压缩一段文本”，而应成为 ledger-aware 的窗口组装与 recall 引擎：

```rust
pub trait SessionContextCompressor: Send + Sync {
    fn load(&self, request: SessionContextLoadRequest) -> anyhow::Result<SessionContextLoadResult>;
    fn compact(&self, request: SessionCompactionRequest) -> anyhow::Result<SessionCompactionResult>;
    fn search(&self, request: SessionSearchRequest) -> anyhow::Result<Vec<SessionContextRecord>>;
}
```

默认 budget 为 400k tokens。配置命名可以在实现阶段与现有 model budget config 对齐，但语义上必须满足：

- 没有显式配置时默认 400k。
- budget 同时影响 resume context、prompt assembly 与历史 recall 的回填阈值。
- token 估算与压缩 checkpoint 的生成必须由 `omega-compression` 统一负责，而不是散落到 `omega-session` 或 `omega-context`。

### Runtime Hydration Contract

恢复仍不能把旧消息重新走一遍普通 producer pipeline，但 hydration 来源改成 canonical ledger 的 projection：

```rust
pub struct SessionRestoreSnapshot {
    pub session_id: String,
    pub title: String,
    pub visible_history: Vec<SessionContextRecord>,
    pub turn_count: u64,
    pub archived_turn_count: u64,
    pub latest_user_turn_preview: Option<String>,
    pub recent_context_record_count: usize,
    pub checkpoint_summary_count: usize,
    pub search_hit_count: usize,
    pub truncated_history: bool,
}

pub enum StateMessage {
    // existing variants...
    SessionRestored(SessionRestoreSnapshot),
}
```

语义要求：

- `SessionRestored` 使用新的 current turn envelope 发送，避免旧 `turn_id` 混入当前过滤语义。
- policy/TUI 收到 `SessionRestored` 后，先清空当前 response/activity view state，再基于 `visible_history` 做一次性 hydration。
- `SessionRestoreSnapshot` 还必须显式携带 `recent_context_record_count`、`checkpoint_summary_count` 与 `search_hit_count`，供 restore notice、direct command body 与 detail overlay 说明本次恢复采用的装配策略，而不是只给一个模糊的“history restored”。
- `truncated_history = true` 时，UI 必须明确提示“更早历史已折叠，可通过搜索/详情继续查看”，避免用户误以为日志丢失。
- 恢复完成后追加一条新的 `SystemNotice`，明确显示已恢复的 session id、标题、turn 数与上下文装配策略，例如“recent records=12, compression summaries=1, search hits=2”；若 `truncated_history = true`，还需同时提示更早历史应通过 search/detail 查看。

## Command Specification

### `/session` Family

Phase 1 命令面：

```text
/session
/session list [active|idle|archived]
/session info <session-id>
/session new [title]
/session resume <session-id>
/session switch <session-id>
/session archive <session-id>
/session delete <session-id>
```

说明：

- `resume` 与 `switch` 在当前阶段仍可指向同一 handler；保留两个词是为了贴合用户心智。
- 默认只操作当前 active project 的 session。若用户需要跨 project 恢复，命令输出必须先提示 `/project switch`。
- `new` 明确创建一个新的空白 session；它是 lazy binding 的显式入口之一。
- `/session` 与 `/session list` 默认不再把 session 列表写到 `Agent Response`；它们应打开 operator picker overlay。
- `/session resume` 在无参数时，也应打开同一个 picker，并把最近使用、`resume_ready` 且未归档的项置前。

### Subcommand Behavior

| Subcommand | Input | Output | Errors |
|-----------|-------|--------|--------|
| `list` | optional status filter | 打开 session picker overlay，展示当前 project 内 session 列表 | filter 无效 |
| `info` | `session_id` | 打开 detail overlay，展示 metadata、canonical ledger / compaction availability、record/replay/snapshot/checkpoint 计数、latest checkpoint 摘要与归档计数 | session 不存在 |
| `new` | optional title | 创建并绑定新 session；若当前 runtime 已绑定旧 session，则旧 binding 退回未选中状态 | 持久化失败 |
| `resume` / `switch` | optional `session_id` | 有参数时执行恢复；无参数时打开 session picker 并把 resume-ready 目标置前 | session 不存在、ledger 缺失、跨 project |
| `archive` | `session_id` | 将目标 session 标记 Archived，并保留 session metadata 与 canonical ledger；默认从 picker 触发并原地刷新列表 | 目标是当前 active session 且未确认切换 |
| `delete` | `session_id` | 删除 session metadata + canonical ledger；默认先经 confirm overlay | 删除当前绑定 session、session 不存在 |

### Required UX

- slash hint 需展示 `resume/switch/archive/delete` 这些 subcommand 的 argument hint。
- `/session list` 不应再把列表渲染成普通 command response section；默认入口应是 picker overlay。
- picker 中 `Enter` 默认打开 detail overlay，而不是直接执行恢复。
- `resume/archive/delete/new` 等动作应优先通过 picker 内 hotkey 闭环完成，并通过 picker 刷新、detail overlay、status notice 与 restore hydration 给出反馈。
- 成功的 session operator action 不应再向 `Agent Response` 追加普通“列表/详情/成功提示”文本；只有错误路径才退回 command error surface。
- `/session resume` 完成后，仍需通过 restore hydration + status notice 明确告知已切换到哪个 session、加载了多少近期上下文、是否折叠了更早历史，而不是静默切换。

## Interactive Operator Picker

`/session` 的默认用户路径应建立在通用 operator picker 之上，而不是继续扩张 command section 文本。该 picker contract 详见 [docs/specs/omega-operator-picker-overlay.md](docs/specs/omega-operator-picker-overlay.md)；对 session 的额外约束是：

- row 至少展示 `title`、`status`、`resume_ready`、`archived_turn_count` 与 `last_user_turn_preview`。
- `Enter` 打开 detail overlay。
- `Ctrl-R` 恢复选中 session。
- `Ctrl-A` 归档选中 session。
- `Ctrl-D` 删除选中 session，并经过 confirm overlay。
- `Ctrl-N` 创建新 session。

应用刚启动且 runtime 尚未绑定 session 时，picker 仍应可用；这时它是“选择一个旧 session 或创建一个新 session”的显式入口，而不是某个已绑定 session 的附属工具。

## Resume Flow

### Happy Path

1. 若当前 runtime 已绑定其他 session，先把当前 session 的 pending working-state 刷入该 session 的 `session.context.jsonl`。
2. 读取目标 `ProjectSessionRef`，校验它属于当前 active project，且 `resume_ready = true`。
3. 若目标仍是 legacy sidecar，会先执行一次兼容迁移，生成 `session.context.jsonl`。
4. 调用 `omega-compression` 以 `ResumeContext` 目标装载 ledger：默认 400k token，近期记录优先，必要时回填最新 checkpoint 和搜索命中。
5. 在 `omega-session` 内用 `reconstructed_working_set` 重建 `SessionContext`、todo snapshot、workflow/skill routing state 与当前 cwd。
6. 另外调用 `UiHydration` 投影构建 `SessionRestoreSnapshot`，发送新的 `StateMessage::SessionRestored(snapshot)`。
7. runtime 将目标 session 设为当前绑定 session，并把该选择写成 project/session picker 的排序 hint。

### Failure Handling

- 若 ledger 缺失但 catalog 存在：返回 `session exists but is not resume-ready`，不切换当前绑定 session。
- 若迁移 legacy sidecar 失败：停止恢复，保留旧 binding，不允许进入半迁移状态。
- 若 context load 或 checkpoint 解析失败：回滚绑定切换，不允许进入半恢复状态。
- 若 UI hydration projection 失败：恢复仍应终止，因为 ledger 已是唯一 source，不允许 context 成功但 UI 用另一条旧路径兜底。

## First-Turn Session Creation Flow

1. 启动后 `AgentSession` 处于 `Unbound`。
2. 用户输入普通消息且 runtime 尚未绑定 session 时，`omega-session` 先创建 session directory 与 `session.json`。
3. 在第一条 `UserTurn` 写入前创建 `session.context.jsonl`，并先写入 `SessionOpened` 记录。
4. 同一轮 turn 完成后，把 `AssistantTurn` 与最新 `WorkingSetSnapshot` 追加到 ledger。
5. 之后每轮 turn 都只增量追加 ledger，并按需要触发压缩 checkpoint。

这条流程确保“只有真实会话开始时才存在 session”，而不是应用刚启动就制造一个占位 session。

## Technical Decisions

| Decision | Choice | Rationale |
|---------|--------|-----------|
| Startup binding | `Unbound` by default | 把“打开应用”和“进入某个 session”拆开，避免隐式恢复 |
| Canonical history | `session.context.jsonl` | 恢复、压缩、搜索、UI hydration 都基于同一份账本演进 |
| Working-state persistence | `WorkingSetSnapshot` records inside ledger | 保留统一 source of truth，不再维护独立 snapshot file |
| Compression owner | `omega-compression` | 统一 token 预算、checkpoint 生成和历史 recall 逻辑 |
| Resume scope | current project only | 避免在本阶段把 project switch 与 session restore 强耦合 |
| Delete semantics | 删除 session metadata + ledger，不清理 project memory | 避免误删 repo-scoped knowledge 和治理证据 |

## Security And Integrity Considerations

- `session_id` 必须继续使用稳定且不可猜测的随机 id，避免手写路径拼接注入。
- session 目录与 ledger 的读取必须经过 project root 规范化，不允许路径逃逸到 `.omega/` 外。
- ledger 只允许写入 frontend-neutral 的文本与 typed state，不允许持久化敏感 tool 原始输入的未过滤 payload。
- `CompressionCheckpoint` 必须保留来源范围与生成时间，避免压缩后事实归属不明。
- `/session delete` 必须拒绝当前绑定 session，避免删掉当前运行态的恢复锚点。

## Performance Requirements

- `/session list` 在正常项目规模下应只读 `session.json`，不扫描整份 ledger。
- 默认 context budget 为 400k tokens；较小模型或显式配置可降低该值，但不得提升到“每次 resume 都全量扫完整 ledger”。
- UI hydration 必须支持 tail window 和折叠提示，避免长 session 恢复时把整份账本全部重绘。
- `WorkingSetSnapshot` 与 `CompressionCheckpoint` 应在 turn 完成或显式 session operator action 后写入，不在每个 streaming delta 上写盘。

## Testing Strategy

- `omega-project` 单测：session directory 创建、ledger append/load、legacy sidecar 迁移、delete/archive 行为。
- `omega-session` 集成测试：startup `Unbound`、首条用户消息创建 session、`/session new`、`/session resume` 和绑定切换的 happy path 与错误路径。
- `omega-compression` 单测：token 预算、checkpoint 生成、近期优先装载、query recall、被压缩历史仍可搜索。
- `omega-context` / memory 测试：`SessionContextLoadResult` 能正确重建 working set，不把别的 session 混入恢复路径。
- `omega-app` / `omega-tui` 测试：`StateMessage::SessionRestored` 会清空当前视图并按 tail window 正确加载 ledger 投影；startup 在 `Unbound` 状态下也能正常显示 picker / 状态 surface。
- command hint / parser 测试：`/session` subcommand 可被 hint resolution 正确识别并给出参数提示。

## Task Breakdown

建议按以下任务落到 `docs/TODO.md`：

1. `Task 17I`: startup-unbound lifecycle 与 first-turn lazy session binding。
2. `Task 17J`: canonical `session.context.jsonl` schema、session directory 布局与 legacy sidecar 迁移。
3. `Task 17K`: `omega-compression` 的 token budget、checkpoint compaction、recency-first load 与 historical search。
4. `Task 17L`: `omega-session` / `omega-context` 基于 ledger projection 的 resume/context assembly。
5. `Task 17M`: `/session` picker、detail、resume/new UX 在 unbound startup 模型下的调整。
6. `Task 17N`: regression、migration、docs/guide 同步与 phase-1 兼容清理。

---

### Change Log

- 2026-04-10: 初版规格，定义 `/session` control plane、resumable snapshot/replay log、context reload 与 TODO 拆分。
- 2026-04-10: 补充 overlay-first 的 session operator UX；`/session list` / `info` / picker 内动作默认改走 operator picker + detail/confirm overlay，而不是 Response 文本输出。
- 2026-04-10: 重写为 phase-2 目标态：启动不再自动绑定 session，引入 canonical `session.context.jsonl`、400k token 默认预算，以及 `omega-compression` 驱动的 resume / recall / hydration 合同。