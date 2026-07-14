---
content_revision: 174
generation_id: gen_000087_r000174
language: bilingual
last_verified_commit: d8c30e3e9e310ce38cffa965be4688ed55a87787
projection_version: 87
source_doc_id: "spec:docs-specs-omega-project-plan-system"
source_path: docs/specs/omega-project-plan-system.md
updated: 2026-06-03
---

# Omega Project Plan System Specification

## Overview

status: active
last_verified_commit: N/A
owner: omega-team
created: 2026-04-13
updated: 2026-04-15
version: 0.9
supersedes: []
related_prds:
  - docs/prds/omega-project-plan-management.md
  - docs/specs/omega-project-system.md
  - docs/specs/omega-command-system.md
  - docs/specs/omega-tui-runtime-experience.md

## Overview

Omega 当前已经明确区分了两类 surface：

- `docs/TODO.md`：只保留当前 open work、优先级顺序和当前 baseline。
- `omega-todo`：只负责当前任务、当前 turn / session 的 runtime working set。

但系统仍缺少一个正式的 **project-scoped long-term planning surface**。结果是：项目级任务、历史任务、优先级、依赖链、任务日志，以及 design / implementation traceability 目前都没有稳定 owner，也没有和 command、session、TUI 主路径对齐。

本规格引入 `ProjectPlan` 作为 repo-scoped、durable、frontend-neutral 的长期计划合同。它不是 runtime `todo` 的持久化版本，也不是 `docs/TODO.md` 的替代品；它是 Omega 的项目级 source of truth，用于管理长期任务图，并把“选中任务 -> 发给 AI -> 交付回写”的闭环收口为正式系统能力。

Status note (2026-04-15): `Task 4H ~ 4K` 已完成并完成最终 cutover。当前实现基线已把 `/plan` 的 canonical persistence 收口到 `docs-data/tasks/`；`ProjectPlanStore` 读写 `project-plan.toml`、`project-tasks.jsonl` 与 `logs/*.jsonl`，剩余 `.omega/plans/` compatibility/import layer 也已移除。涉及 rollout 历史与 convergence rationale 仍见 `docs/specs/omega-project-plan-docs-data-convergence.md`。

## Goals

- 为每个 project 建立一个内置且稳定存在的 plan home。
- 提供 project-scoped 的 typed task graph，覆盖当前任务与历史任务。
- 让任务具备明确的 priority、ordering、dependency chain、task logs 与 artifact links。
- 明确 runtime `todo`、selected task context 和 project plan board 三者边界。
- 提供 `/plan` command family，支持管理任务、选择任务，并把任务作为 requirement 发起普通 AI turn。
- 在 TUI 中提供长期计划的 summary、detail 和当前选中任务可见性。
- 让 `docs/TODO.md` 继续保持精简，只作为 open work projection，而不是长期历史总账。
- 避免将时间字段当成一等管理维度；Phase 1 以 priority + order + dependency 为主。

## Non-Goals

- 不把 `omega-todo` 直接升级成 project plan store。
- 不要求引入 deadline、calendar、estimate、burn-down 或 completed-at dashboard。
- 不要求在 Phase 1 自动回填所有历史已完成任务；历史从该系统落地后开始稳定累积，旧里程碑按需导入。
- 不把 `omega-project` 变成新的 god object；project 只负责绑定，任务域逻辑仍由 `omega-plan` 拥有。
- 不把 `omega-tasks`（runtime agent task delegation 原语）与 project plan board 混入同一 crate；两者职责不同，分 crate 隔离（见 Architecture / Crate Split Rationale）。
- 不要求 background/team/subagent 在 Phase 1 全部接入该系统，只保留未来挂接 seam。

## System Model

### Three Adjacent But Distinct Surfaces

| Surface | Lifetime | Owner | Purpose |
|--------|----------|-------|---------|
| runtime `todo` | turn / session | `omega-todo` | 当前任务的短期执行清单，只解决“这一次执行现在要做什么” |
| selected task context | session | `omega-session` + `omega-plan` | 当前会话绑定的 project task，用于后续 turn prompt grounding |
| project plan board | project | `omega-plan` + `omega-project` | 长期任务图、历史任务、依赖、日志、artifact links |

核心规则：

- runtime `todo` 不进入 project plan store。
- project plan task 不自动等于 runtime `todo` item。
- current turn 的 planner 可以从 selected task 派生 runtime `todo`，但两者永远不是同一数据结构。

## Architecture

### Crate Split Rationale

`omega-tasks` 的原始设计意图（`omega-agent-spec.md`、`foundation-crates.md`）是为 workflow / subagent / team 提供"机器对机器"的 runtime task delegation 原语，`omega-team` 已声明对它的依赖。project plan board 是"人对AI"的 project knowledge surface，生命周期、存储位置、API 语义完全不同。两者混入同一 crate 会产生 god object，且 `omega-core` 对 `omega-tasks` 的依赖不应因 project plan 逻辑而膨胀。

因此，项目计划系统由新增的独立 crate **`omega-plan`** 实现。`omega-tasks` 继续作为未来 runtime agent task delegation 的预留 crate，不承担 plan board 职责。

### Ownership

| Layer | Responsibility |
|------|----------------|
| `omega-plan` | `ProjectPlanStore`、`ProjectPlanAccess` trait、task graph、priority/rank、dependency validation、task logs、artifact links、query/mutation API |
| `omega-project` | project-bound path/layout、plan handle 绑定、project summary 聚合；不拥有任务域规则 |
| `omega-command` | `/plan` descriptor、hint metadata、parse contract |
| `omega-app` | session/bootstrap 装配、runtime policy、TUI launch |
| `omega-session` | `/plan` handler mutation orchestration、plan store 生命周期、selected task state（`selected_task_id`）、prompt injection、task-bound turn dispatch、delivery log 回写 |
| `omega-tui` | plan summary、project-panel task visibility、task detail overlay、selected-task input hint |
| `omega-document` | artifact link resolution、`docs/TODO.md` projection/health integration |

### Dependency Direction

```text
omega-app
   │
   ├────> omega-session ────> omega-plan
   │              │
   │              └────> omega-project ────> omega-project-layout
   │
   ├────> omega-plan
   ├────> omega-command
   └────> omega-tui
```

约束：

- `omega-command` 只定义 `/plan` 的描述层，不持有 store 或 session。
- `omega-session` 通过 `ProjectPlanAccess` trait 消费 plan store，不直接依赖 `ProjectPlanStore` 具体实现（可降级 mock 注入，便于单元测试）。
- `omega-project` 只提供 path/layout 绑定，不拥有任务语义。
- `omega-tui` 只消费 typed runtime projections，不直接读 project plan persistence files。
- `omega-plan` 不依赖 `omega-session`，依赖方向只能是 session → plan。

## Implementation Contracts

本节补全规格 v0.1 遗漏的关键实现边界决策。每个 Task 实施前须先参照本节，做出具体编码决策再动手。

### Plan Store 初始化与生命周期

`ProjectPlanStore` 由 `omega-app` 负责初始化，不由 `omega-session` 或 `omega-project` 自行创建。

```
omega-app 启动 / project bind 路径：
  1. 解析 project root（已有 omega-project 机制）
  2. 调用 ProjectPlanStore::open_or_scaffold(project_root)
     - `docs-data/tasks/project-plan.toml` 不存在 → 自动创建 `docs-data/tasks/` 与默认 plan manifest（schema_version = 1）
     - 已存在 `project-plan.toml` → 检查 schema_version
       - version > supported → 返回 Err，abort with user message
       - manifest 损坏（parse 失败）→ 返回 Err，abort with user message
  3. 成功 → Arc<ProjectPlanStore>，存入 AppState.plan_store: Option<Arc<ProjectPlanStore>>
  4. 传入 AgentSessionConfig.plan_store: Option<Arc<dyn ProjectPlanAccess + Send + Sync>>
```

失败策略：scaffold/open 失败时 `plan_store = None`；`/plan` 命令全部返回 `OmegaCommandOutcome::Error("plan store unavailable")`；session 降级为无 selected task 模式；不中止整个应用启动。

### `/plan send` 与 `spawn_turn` 注入机制（选型 C）

当前 `AgentSession::spawn_turn(input: String, ...)` 接受纯字符串。task context 注入采用以下机制，**不改变** `spawn_turn` 签名：

1. `AgentSessionConfig` 新增 `plan_store: Option<Arc<dyn ProjectPlanAccess + Send + Sync>>`。
2. `AgentSession` 持有该引用（只读，不负责初始化）。
3. `SessionRuntimeState` 新增 `selected_task_id: Option<String>`。
4. `AgentSession` 公开：
   - `set_selected_task(&self, task_id: Option<String>)`
   - `current_selected_task(&self) -> Option<String>`
5. `prompt_builder` 在每次 turn 开始时，若 `selected_task_id` 为 Some 且 `plan_store` 存在，调用 `plan_store.resolve_task_context(id)` 把结果序列化为 JSON，插入为 system prompt 附加块（语义等同现有 `structured_input` 机制，不额外注入 user role message）。
6. `/plan send [<id>] <prompt>` 的 `omega-app` handler：
   - 提供显式 id：`session.set_selected_task(Some(id))` + `session.spawn_turn(prompt)`
   - 无显式 id：读 `current_selected_task()`；若 None → `OmegaCommandOutcome::Error("No task selected. Use /plan select <id> first.")`
7. 普通用户输入（非 `/plan send`）不变；`prompt_builder` 自动消费 `selected_task_id`，两条路径共享同一 prompt 组装逻辑。

`ProjectPlanAccess` trait（定义在 `omega-plan`）：

```rust
pub trait ProjectPlanAccess: Send + Sync {
    fn resolve_task_context(
        &self,
        task_id: &str,
    ) -> anyhow::Result<Option<SelectedProjectTaskContext>>;

    fn get_task(&self, task_id: &str) -> anyhow::Result<Option<PlannedTask>>;

    fn list_tasks(&self, filter: TaskListFilter) -> anyhow::Result<Vec<PlannedTask>>;
}
```

### Delivery Log 回写回调链

task-bound turn 完成后的日志回写当前由 **`omega-session`** 在 turn 完成路径直接触发：

```
omega-session 处理 turn 终止信号时：
  1. 读 session.current_selected_task() → Option<String>
  2. Some(task_id) 且 plan_store 有效：
     a. 收集 turn-scoped delivery summary（当前为 session-owned accumulator）
     b. success → plan_store.append_delivery_log(task_id, TaskLogEntry { kind: DeliveryAttached, ... })
     c. interrupted / failed → plan_store.append_delivery_log(task_id, TaskLogEntry { kind: PartialDelivery, ... })
  3. 中断 / 取消 turn：同一路径，log kind 改为 PartialDelivery，不静默丢弃
```

当前实现已经把成功路径提升为 `TaskLogKind::DeliveryAttached`，并附带 session-owned 的 delivery summary（model / token / tool / changed-files 摘要）。Task 15F-36 ~ 15F-38 仍负责把这份摘要升级为 frontend-neutral contract，并补齐更可信的 knowledge search / workspace mutation evidence。

### 并发写安全范围

Phase 1 声明为**单写入者**：只有前台 session 通过 `ProjectPlanStore` API 进行 mutation，不存在多 session 并发写。

- `tasks/<id>.toml` 和 `plan.toml` 无需文件锁。
- `logs/<id>.jsonl` 追加写，天然安全。
- 当 Task 13（multi-session / team）引入多写入者时，mutation 路径升级为 advisory `flock` 或 CAS `order_key` 乐观锁，届时在规格补充变更；`omega-plan` API 设计时预留该升级空间（mutation 方法应 `&self` 而非 `&mut self`，内部用 `Mutex` 管理写锁）。

### Plan Schema 升级策略

- `plan.toml` 中 `schema_version: u32` 是 plan store open 的第一校验项。
- Phase 1 支持 version = 1。读到更高版本时 abort with user-visible message，不静默部分解析。
- 迁移脚本不在 Phase 1 交付范围；未来版本升级由独立 `PlanMigrator` 模块处理。

### Task ID 与 `order_key` 分配

- **Task ID**：格式 `TASK-{seq:04}` 零填充递增，`plan.toml` 中 `next_task_seq: u64` 字段自增分配。删除任务不回收 ID；单调递增，不保证连续。
- **`order_key`**：新建任务默认 `order_key = last_in_band + 1000`（留出插入空间）。`/plan prioritize --before <ref>` 和 `--after <ref>` 通过重新计算受影响任务的 `order_key` 调整顺序，不全局重排。当相邻两个 `order_key` 差值 ≤ 1 时，对该 band 执行一次重规格化（间距恢复为 1000）。

## Persistence Layout

### Docs-Data Canonical Source Of Truth

长期计划的 repo-local canonical persistence 已收口到 `docs-data/tasks/`：

```text
<project-root>/docs-data/tasks/
  project-plan.toml
  project-tasks.jsonl
  logs/
    TASK-0001.jsonl
    TASK-0002.jsonl
```

规则：

- `project-plan.toml` 持有 board-level metadata，例如 schema version、priority bands、default view、managed `docs/TODO.md` export policy。
- `project-tasks.jsonl` 是 canonical task graph store，供 `omega-plan` 统一排序、查询和 mutation。
- `logs/<id>.jsonl` 采用 append-only ledger，避免每次小日志都重写整个 task graph。
- project 打开时如果 `docs-data/tasks/project-plan.toml` 不存在，Omega 应自动 scaffold 最小 manifest，确保该能力“总是存在”。

### Runtime-Owned State

runtime 不应再造第二份 source of truth。Phase 1 只允许以下派生状态：

- session snapshot 中记录 `selected_task_id` 和最近一次 task-bound dispatch metadata
- 可选的 `.omega-state/plans/` graph cache / search cache

规则：

- 缓存缺失时必须可由 `docs-data/tasks/*` 重建。
- UI/command 不得把 `.omega-state/plans/` 当成 canonical mutation target。

## Data Models

### Plan Manifest

```rust
pub struct ProjectPlanManifest {
    pub schema_version: u32,
    pub priority_bands: Vec<TaskPriority>,
    pub default_view: PlanViewMode,
    pub managed_todo_export: TodoExportMode,
}
```

### Planned Task

```rust
pub struct PlannedTask {
    pub id: String,
    pub title: String,
    pub kind: PlannedTaskKind,
    pub status: PlannedTaskStatus,
    pub priority: TaskPriority,
    pub order_key: i64,
    pub summary: String,
    pub requirement: String,
    pub acceptance: Vec<String>,
    pub parent_id: Option<String>,
    pub depends_on: Vec<String>,
    pub tags: Vec<String>,
    pub design_links: Vec<TaskArtifactLink>,
    pub implementation_links: Vec<TaskArtifactLink>,
}
```

字段规则：

- `requirement` 是发送给 AI 的 canonical task requirement，不要求与 `summary` 相同。
- `acceptance` 是显式完成标准，供 UI、command 和 task-bound turn 共用。
- `priority` 只表达 band，`order_key` 负责同 band 内排序。
- `parent_id` 用于 epic/task 层级；依赖关系仍由 `depends_on` 显式表达。
- Phase 1 不要求 due date、ETA、story points。

### Status And Priority

```rust
pub enum PlannedTaskStatus {
    Backlog,
    Ready,
    InProgress,
    Blocked,
    Done,
    Archived,
}

pub enum TaskPriority {
    P0,
    P1,
    P2,
    P3,
}
```

状态分组规则：

- current tasks = `Backlog | Ready | InProgress | Blocked`
- history tasks = `Done | Archived`

### Task Log

```rust
pub struct TaskLogEntry {
    pub seq: u64,
    pub kind: TaskLogKind,
    pub actor: TaskActor,
    pub summary: String,
    pub related_session_id: Option<String>,
    pub related_turn_id: Option<u64>,
    pub related_delivery_id: Option<String>,
}
```

规则：

- task log 采用单 task append-only sequence，而不是依赖 wall-clock time 排序。
- `seq` 是 canonical ordering；machine timestamp 仅可作为调试字段，不进入主 UI 排序语义。
- 典型日志类型包括：`created`、`status_changed`、`priority_changed`、`dependency_added`、`note_added`、`task_sent_to_ai`、`delivery_attached`、`artifact_linked`。

### Artifact Links

```rust
pub enum TaskArtifactKind {
    Prd,
    Spec,
    Guide,
    Adr,
    Code,
    Test,
    Delivery,
}

pub struct TaskArtifactLink {
    pub kind: TaskArtifactKind,
    pub path: String,
    pub label: Option<String>,
}
```

规则：

- design links 默认面向 `prd/spec/guide/adr`。
- implementation links 默认面向 code/test/delivery evidence。
- path 允许指向 repo-relative doc/code path；外部 URL 不是 Phase 1 必需项。

## Command Specification

### `/plan` Command Family

`/plan` 是 project-level long-term planning 的显式 operator surface。它与 workflow step 里的“plan 阶段”不同：前者管理长期任务图，后者只负责当前 turn 的执行计划。

| Command | Effect |
|--------|--------|
| `/plan list [--status <...>] [--priority <...>]` | 枚举 current 或 history tasks |
| `/plan show <task-id>` | 查看 task detail、dependency chain、logs、artifact links |
| `/plan create ...` | 新建 task record |
| `/plan update <task-id> ...` | 更新 summary / requirement / acceptance / status / tags |
| `/plan prioritize <task-id> <p0|p1|p2|p3> [--before <task-id>] [--after <task-id>]` | 调整优先级与同级顺序 |
| `/plan depends <add|remove> <task-id> <other-id>` | 维护依赖边 |
| `/plan log <task-id> <note>` | 追加 task log |
| `/plan link <task-id> <design|implementation> <path>` | 添加 artifact link |
| `/plan select <task-id>` | 将 task 绑定到当前 session |
| `/plan select none` | 清除当前 session 的 selected task |
| `/plan send [<task-id>] <prompt>` | 以 task 为 requirement 发起一次普通 AI turn |
| `/plan sync-todo` | 将当前 open work 摘要投影到 `docs/TODO.md` |
| `/plan migrate-todo` | 将 `docs/TODO.md` 当前 open tasks 首轮导入 plan board |
| `/plan load <source> [--kind <auto|todo|task-headings>] [--apply]` | 从指定文件或目录预览/导入长期规划 task blocks |

### `load`

`/plan load` 是面向现有文档资产的显式导入入口，用来把 repo 内已经存在的长期规划文档变成 plan board task，而不是要求用户手工重新录入。

Phase 1 语义：

1. `source` 必须是 project-root 相对路径，例如 `docs`、`docs/prds/feature-a.md`、`docs/specs`。
2. 若 `source` 是目录，只扫描其下的 `*.md` 文件；跳过 `docs/archive/`、`target/`、`.omega-state/` 等非活跃内容。
3. 默认行为是 **preview only**：返回候选 task、来源文件、跳过原因与歧义 warning，不修改 plan store。
4. 只有带 `--apply` 时才真正写入/更新 task。

当前 preview 交互：

- `/plan load <source>` 会先弹出 operator picker overlay，列出本次将导入的 candidate tasks。
- `Enter` 打开所选 candidate 的 detail preview。
- `Ctrl-L` 打开 confirm overlay；只有用户确认后，才会提交真正的 `/plan load <source> ... --apply`。
- `Esc` 或在 confirm overlay 里取消，会停止导入，不写 plan store。

Phase 1 导入器类型：

- `auto`：依次尝试已知的确定性适配器
- `todo`：复用现有 `migrate-todo` 逻辑，只识别 `docs/TODO.md` 风格 task block
- `task-headings`：识别 markdown 中显式的 `### Task ...` / `#### Task ...` heading block，以及紧随其后的 `Status`、`Priority`、`Description`、`Blocked by`、`Related` metadata bullets

Phase 1 非目标：

- 不从自由 prose、路线图段落、checklist 或 rollout 表格里猜测 task
- 不在 apply 时自动删除已存在但源文档中消失的 task
- 不跨 repo 读取外部文件，只处理当前 project root 下的路径

写入规则：

- 每个导入 task 都写入稳定 source identity，例如 `doc-load:<relpath>#<task-key>` tag，供 rerun 时去重/upsert。
- apply 时优先按 source identity upsert，而不是重复创建新 task。
- `Blocked by` 映射到 dependency edge；`Related` 中可解析的 doc path 映射到 design links。
- 任务详情里追加一条 command log，记录导入来源与导入模式。

建议输出：

- preview：`candidates / matched / skipped / warnings`
- apply：`created / updated / skipped`，并列出受影响 task ids

### `select` And `send`

`/plan select` 只变更 session state，不触发模型。

`/plan send` 的语义必须与“用户正常输入一条需求”保持一致：

1. 解析 task（显式传入 id，或回退到当前 selected task）
2. 组装 `SelectedProjectTaskContext`
3. 把 task requirement、acceptance、dependency chain、recent logs、artifact links 注入 structured turn input
4. 调用现有 `session.spawn_turn()` 主路径

换言之，`/plan send` 不是单独的 tool loop，也不是隐藏 prompt macro；它只是把 project task 变成一条更有结构的普通用户请求。

## Session Integration

### Selected Task Context

`omega-plan` crate 定义以下只读快照类型，由 `ProjectPlanAccess::resolve_task_context()` 组装：

```rust
pub struct SelectedProjectTaskContext {
    pub task_id: String,
    pub requirement: String,
    pub acceptance: Vec<String>,
    pub dependency_chain: Vec<String>,   // 直接前置任务 title 列表
    pub recent_logs: Vec<TaskLogEntry>,  // 最近 N 条日志
    pub design_links: Vec<TaskArtifactLink>,
    pub implementation_links: Vec<TaskArtifactLink>,
}
```

`omega-session` 中的变更（Task 4C 范围），详见 Implementation Contracts / 注入机制：

- `AgentSessionConfig` 新增 `plan_store: Option<Arc<dyn ProjectPlanAccess + Send + Sync>>`
- `SessionRuntimeState` 新增 `selected_task_id: Option<String>`
- `SessionRestoreSnapshot` 新增可选字段 `selected_task_id: Option<String>`；restore 时若 task 已不存在，清空该字段并在 TUI Activity 栏显示 warning，不中止 restore
- 公开方法：`set_selected_task(id: Option<String>)`、`current_selected_task() -> Option<String>`

规则：

- selected task 是 session-owned state，不是 global singleton。
- 普通输入和 `/plan send` 都通过 `prompt_builder` 自动消费 selected task context；`/plan send` 额外做显式 `set_selected_task` 保证绑定到位。
- planner step 允许基于 selected task 生成当前轮 runtime `todo`，但不得反向改写 project task。

### Delivery Backfill

当 turn 是 task-bound（来自 `/plan send` 或当前 session 已显式选中 task）时，回写由 **`omega-session`** 触发，详见 Implementation Contracts / Delivery Log 回写回调链：

- turn 完成后由 `omega-session` 为该 task 追加 `delivery_attached` log
- delivery summary 中的 changed files / structured evidence 当前先写入 log summary；更系统的 implementation-link / evidence attachment 继续依赖 Task 15F-36
- 失败或中断 turn 也应产生 partial log（kind = `partial_delivery`），不静默丢失上下文

## TUI Integration

长期计划不能与右侧 `Todos` 面板混用。Phase 1 推荐以下呈现路径：

- `Project` panel：展示 plan summary，例如 open/history counts、selected task、blocked count、top priority task
- `Project` panel task lines：对 selected / next / blocked task 提供直接激活入口
- task detail overlay：展示 requirement、acceptance、dependency chain、recent logs、artifact links
- input context bar：当 session 有 selected task 时，展示 `Task: <id> <title>`，让用户知道后续输入的 grounding 来源

当前实现基线：TUI 已将长期计划可见性收口到 sidebar `Project` section 和 detail overlay，而不是单独的 `Activity::Tasks` enum variant。

规则：

- runtime `todo` 仍只展示当前 turn 的短期执行清单。
- project plan 详情由 `Project` / `Tasks` 相关 surface 承载，不塞进 `Todos`。
- 未选中 task 时，TUI 也应能查看整个长期计划，而不是只能看当前绑定项。

## `docs/TODO.md` Relationship And Migration

`docs/TODO.md` 与 project plan 的关系必须是 **projection，不是双向 source of truth**：

- project plan board：长期 source of truth
- `docs/TODO.md`：当前 open work 的精简摘要

同步规则：

- `sync-todo` 必须是显式操作，不在每次 task mutation 后自动重写文档
- projection 只导出当前 open tracks、优先级顺序、必要的依赖关系和 spec links
- `Current Baseline` 这类文档性说明仍保留在 `docs/TODO.md`，不要求完全由 plan system 生成

漂移：`docs/TODO.md` 手动编辑后与 plan board 的漂移无自动告警（Phase 1）；约定人工在手动编辑 `docs/TODO.md` 后运行一次 `/plan sync-todo` 覆写 open-work 区段。

首轮迁移（Task 4E 范围）：`docs/TODO.md` 中的 open task 结构（`### Task N:`、`- **Status**:`、`- **Blocked by**:`、`- **Related**:`）可机械映射到 `PlannedTask` 字段，具体字段映射规则在 Task 4E 实现时以代码注释记录，不要求在规格层提前枚举所有字段细节。

后续文档导入（`/plan load` 范围）：

- `migrate-todo` 继续保留为 `docs/TODO.md` 的专用快捷入口
- `/plan load` 负责更一般的 file/dir import，但仍只支持显式 task-shaped markdown block，不扩展到任意 prose 提取
- 当 `source = docs` 时，loader 允许组合多个适配器，但必须在 preview 中按文件列出匹配来源，避免“导入了什么”不可追踪

迁移规则：

- 首轮只导入当前仍 open 的 TODO tracks
- 已完成的历史 milestone 不强制一次性补录；如需补录，可按主题分批导入
- 旧文档中的“完成历史”继续留在 spec/changelog/git history，不强制复制到 plan board

## Testing Strategy

- **`omega-plan` 单测**：task TOML parse/save roundtrip、plan manifest `open_or_scaffold`（目录不存在/已存在/schema_version 不匹配三种路径）、priority ordering、dependency DAG cycle detection（直接环 + 间接环）、log append 后 seq 单调递增、artifact link mutation、`order_key` 分配（new 默认值、before/after 重排、重规格化触发）、task ID 自增（不回收间隙）
- **`omega-plan` 单测**：`ProjectPlanAccess` mock impl，验证 `omega-session` 在无文件系统依赖条件下可注入 mock 完成 prompt injection 测试
- **`omega-command` / `omega-app` 集成测试**：`/plan` 各 subcommand parse、disabled/invalid feedback、handler mutation path、`/plan send` 无 task 时返回 Error outcome、`/plan send` 有 task 时 dispatch 到普通 `spawn_turn` 路径（验证 prompt 包含 task context block）、`/plan load` preview/apply 与 upsert 语义
- **`omega-session` 单测**：`set_selected_task`/`current_selected_task`、prompt injection（task context 出现在 assembled prompt 中）、悬空 `selected_task_id` restore 后清空、`plan_store = None` 时无 task context 降级行为
- **`omega-tui` 单测**：project plan summary counts、task activity list render、task detail overlay render、selected-task context bar render
- **`omega-app` 集成测试**：turn 完成后 plan store 收到正确 delivery log entry；中断 turn 产生 partial log 而非静默
- **docs/integration 测试**：`/plan sync-todo` 生成的 open-work projection 不破坏 `docs/TODO.md` 当前基线结构；`/plan load docs --apply` 只导入被识别的 task-shaped blocks，并对 rerun 保持幂等

## Planned Rollout

| Task | Scope | Current status |
|------|-------|----------------|
| `Task 4A ~ 4G` | foundational plan system rollout | Completed baseline |
| `Task 4H` | docs-data-backed plan store contract | Completed; canonical plan manifest, task graph, and task logs now live under `docs-data/tasks/` |
| `Task 4I` | `/plan` runtime migration to docs-data | Completed; `ProjectPlanStore`, `/plan` handlers, selected task resolve, and task log write-back no longer treat `.omega/plans/` as canonical |
| `Task 4J` | unified TODO projection and doc task convergence | Completed; `docs/TODO.md` open-work projection now derives from the docs-data-backed plan graph, with structured TODO bootstrap when the canonical TODO record is missing |
| `Task 4K` | legacy `.omega/plans` cutover | Completed; remaining `.omega/plans/` compatibility/import layer has been removed, and canonical persistence now targets `docs-data/` only |

## Technical Decisions

| Decision | Choice | Rationale |
|---------|--------|-----------|
| Plan store crate | 新增 `omega-plan`，不复用 `omega-tasks` | `omega-tasks` 原始意图是 runtime agent delegation；混用产生 god object，`omega-core`/`omega-team` 依赖膨胀 |
| Long-term source of truth | `docs-data/tasks/*` canonical；legacy `.omega/plans/` layer removed | project plan、doc task 和 `docs/TODO.md` projection 现已共享 docs-data-backed base directory，避免 dual storage 漂移 |
| Runtime todo 关系 | 独立 surface，生命周期不同 | 当前 turn checklist 与长期任务图语义完全不同 |
| Primary ordering | priority + order_key + dependency | 用户明确不需要时间驱动的管理模型 |
| AI dispatch 注入机制 | 选型 C：session 内置 trait 注入 + `selected_task_id`，`prompt_builder` 自动消费 | 不改变 `spawn_turn` 签名；mock trait 可独立测试；普通输入 / `/plan send` 共享同一条 turn 路径 |
| Plan store init owner | `omega-session` startup path（`AgentSession::new` / project rebind） | 当前实现把 plan store 作为 session-owned runtime dependency，跟随 active project 切换重建 |
| Delivery log write-back | `omega-session` 在 turn 终止信号处触发 | task-bound turn、selected task state 和当前 session-owned delivery summary 都在同一层，最小实现边界更清晰 |
| 并发写范围 Phase 1 | 单写入者，无文件锁 | Phase 1 只有单个前台 session；多写入者时升级为 flock |
| `docs/TODO.md` 关系 | 单向 projection，`sync-todo` 显式操作 | 避免双写 source of truth 和历史回流 |
| `/plan load` 导入模式 | 显式 preview/apply + 确定性适配器 | 避免对整个 `docs/` 做不可解释的启发式导入，保证 rerun 可追踪且可幂等 |

---

### Change Log

- 2026-04-15: v0.9 — removed the remaining `.omega/plans/` compatibility/import layer from `omega-plan` and `omega-project-layout`; `docs-data/tasks/*` is now the only persisted project plan path described by the spec.
- 2026-04-15: v0.8 — `Task 4H ~ 4K` completed: `ProjectPlanStore` and `/plan` now use `docs-data/tasks/*` as canonical persistence, `docs/TODO.md` projection derives from the docs-data-backed graph, and `.omega/plans/` is reduced to a legacy import/compat surface.
- 2026-04-14: v0.7 — follow-up planning update: `.omega/plans/` is now treated as the shipped baseline, while `Task 4H ~ 4K` and `docs/specs/omega-project-plan-docs-data-convergence.md` define the migration of `/plan` canonical persistence into `docs-data/`.
- 2026-04-14: v0.6 — 交互补充：`/plan load` preview 改为 overlay-first flow，先展示 candidate task picker，再经 confirm overlay 明确确认后才提交 `--apply` 导入；preview 遇到坏 task block 时降级为 warning，不再整条命令失败。
- 2026-04-14: v0.5 — 实现补充：`/plan load` 已落地，支持 file/dir source、preview/apply、`todo`/`task-headings` 两类确定性 adapter、`docs/TODO.md` import identity 兼容，以及 doc-source upsert 的幂等重跑。
- 2026-04-14: v0.4 — 设计补充：新增 `/plan load <source> [--kind] [--apply]` 方案，明确 file/dir import、preview/apply、source identity 去重、`todo` 与 `task-headings` 两类 Phase 1 适配器，以及与现有 `migrate-todo` 的边界关系。
- 2026-04-14: v0.3 — 实现收口修订：`/plan` command family 补齐 `migrate-todo`；selected-task restore 对 dangling id 显式 warning 并清空绑定；`ProjectDetailSnapshot.plan` 成为 Project panel / task overlay 的稳定 projection；task-bound turn 成功路径升级为 `delivery_attached`，失败/取消保持 `partial_delivery`；spec 中 ownership / delivery callback / TUI rendering 路径同步到当前实现。
- 2026-04-13: v0.2 — 架构审查修订：新增独立 `omega-plan` crate 替代 `omega-tasks` 作为 plan store owner；新增 Implementation Contracts 章节，明确 plan store 初始化/生命周期、`spawn_turn` 注入（选型 C）、delivery log 回写回调链、单写入者并发范围；补充 `order_key` 分配规则与 task ID 格式；补充 schema_version 失败策略；补充 `Activity::Tasks` 前置扩展说明；明确首轮迁移映射方向；补充悬空 `selected_task_id` restore 处理；明确 `/plan send` 无 task 时 Error outcome；补充 `omega-project-layout` 常量作为 Task 4A 前置步骤；测试策略全面扩充；技术决策表新增 5 项。
- 2026-04-13: v0.1 — 初版规格，定义 project-scoped 长期计划系统、`/plan` command family、selected task context、TUI 集成与 `docs/TODO.md` projection 边界。

## Implementation Note

The `omega-project-layout`, `omega-memory`, `omega-document`, and `omega-doc-cli` crates referenced in this spec moved to the `omega-hpc/` sub-workspace on 2026-06-02 and are now `omega-hpc-paths`, `omega-hpc-memory`, `omega-hpc-document`, and `omega-hpc-doc-cli` respectively. Public type and binary names are unchanged. See [`docs/specs/omega-hpc-extraction.md`](omega-hpc-extraction.md) for the full mapping and [`docs/decisions/007-omega-hpc-extraction.md`](../decisions/007-omega-hpc-extraction.md) for the architecture decision.
