---
content_revision: 101
created: 2026-04-02
generation_id: gen_000017_r000101
last_verified_commit: N/A
owner: omega-team
projection_version: 17
related_prds:
  - docs/specs/omega-context-management.md
  - docs/specs/omega-tool-prompt-optimization.md
  - docs/specs/omega-tui-document-memory-supervision.md
source_doc_id: "spec:docs-specs-omega-command-system"
status: draft
supersedes: []
updated: 2026-04-03
---

# Omega Command System Specification

## Overview

Omega 需要一层独立的 command system，承担“显式用户入口”和“受控运维入口”的职责。它既不是直接暴露给模型的一组 tools，也不是仅存在于 `omega-tui` 输入框里的临时 slash parser；它应该是建立在 `tool + context + app runtime + tui response contract` 之上的正式子系统。

当前阶段，Omega command 只允许来自两类来源：

- builtin commands
- 扩展 tools 暴露出来的 command descriptors

repo-local config、workflow-generated、skill/plugin commands 暂不纳入本规格，也不作为 Phase 1/2 的保留实现面。

本规格基于 `learn/claude-code-source-code` 中 command 实现进行抽象，重点参考两个锚点：

- `learn/claude-code-source-code/src/commands.ts`：中心注册表，统一聚合 builtin / skill / plugin / workflow commands。
- `learn/claude-code-source-code/src/types/command.ts`：统一 command shape，区分 `prompt`、`local`、`local-jsx` 三类，并为 enablement、visibility、aliases、argument hints、sensitive args、source 等元数据提供稳定承载。

Omega 的第一条命令族应是 `/document`。它不是“再造一个新的文档工具层”，而是为当前已经存在的 `omega-context` / `omega-document` 能力提供显式操作面：

- 初始化仓库级向量数据库与索引目录
- 提供基于向量数据库的 RAG 查询入口
- 维护项目级 RAG 知识库与文档治理状态

## Goals

- 为 Omega 建立独立于 TUI widget 的 command registry、parser、executor 和 result contract。
- 明确 command 与 tool 的边界：command 是显式入口，tool 是模型可调用 primitive。
- 让 command 具备类似 Claude Code 的稳定元数据：`name`、`aliases`、`description`、`argument_hint`、`source`、`availability`、`is_enabled`（运行时求值）、`user_invocable`、`sensitive`。
- 让 command 统一驱动 app-owned builtin action 与 tool-backed extension action 两类执行路径，并与 TUI 的 slash 提示和 Response panel 渲染共用稳定契约。
- 以 `/document` 命令族验证首轮架构，直接复用已有 `scan_workspace`、`search`、`manage_document`、diagnostics/supervision snapshot 能力。
- 保持 `omega-context` 作为 memory/document 的唯一上层入口，不让上层 crate 直接耦合 `omega-document`。

## Non-Goals

- 不尝试 1:1 复刻 Claude Code 的全部 commands。
- 不把 command system 设计成又一套模型工具注册表。
- 不让 `omega-app` 或 `omega-tui` 直接调用 `omega-document`。
- 不在第一阶段支持任意脚本化 repo commands 或复杂 plugin marketplace。
- 不引入新的外部向量存储服务；仓库级知识库继续以 `.omega-state/store/` 为本地持久化基线。

## Reference Findings From Claude Code

| 参考实现观察 | Claude 证据 | 对 Omega 的直接启发 |
|-------------|-------------|----------------------|
| 命令是中心注册表统一装配 | `src/commands.ts` 统一聚合 builtin / skill / plugin / workflow commands | Omega 也需要统一 registry，而不是在 `omega-app`、`omega-session`、`omega-context` 各自藏一套入口 |
| 命令拥有独立类型系统和元数据 | `src/types/command.ts` 中 `CommandBase`、`PromptCommand`、`LocalCommand`、`LocalJSXCommand` | Omega 需要稳定的 descriptor layer，才能支持 autocomplete、visibility、policy、runtime rendering |
| 命令和 tools 是两套系统 | command 负责用户入口与 orchestration；tool 仍是模型可调用原语 | Omega 不应把 `/document` 直接实现成“帮用户拼一个 tool 调用字符串” |
| 命令来源可分层 | builtin、skill、plugin、workflow-generated、dynamic skill | Claude 的来源面更宽；Omega 当前阶段显式收窄为 builtin + tool-extension，先把 registry、slash UX 和 Response 呈现路径打透 |
| 命令支持显式 enablement 和 availability | `isEnabled()`、`availability`、`isHidden` | Omega 需要根据 feature flag、backend 是否启用、runtime policy 决定是否展示命令 |
| 命令可返回 UI 流程而不是纯文本 | `local-jsx` 支持交互式 flow | Omega 应允许命令触发 overlay/picker/form，而不把交互逻辑硬塞给大模型 |

## Why Omega Needs A Separate Command Layer

如果没有单独 command layer，Omega 会出现三个持续问题：

1. 用户显式操作入口缺失。当前很多能力只能通过自然语言描述或 tool 侧间接触发，不利于运维、初始化、诊断和知识库治理。
2. UI 和行为耦合。若直接在 `omega-tui` 里拼接 slash parser，未来 CLI、headless、remote surface 都会重复实现。
3. document system 没有明确的 operator surface。当前已有 `omega-document` 的索引、搜索和治理能力，但缺少“初始化仓库向量库”“显式发起 RAG 查询”“维护知识库”的稳定命令入口。

## Architecture

### Ownership

| Layer | Responsibility |
|-------|----------------|
| `omega-command` (new crate) | command descriptor types、registry、parser。**纯描述与解析层，无 I/O，无 `omega-context` 依赖。** |
| `omega-app` | 装配层：创建 `command_dispatch_tx` 通道、注入到 TUI context、注册 `CommandHandler` 实现、接收 outcome 并转发给 `RuntimeMessageBridge`。slash input 检测通过两分支 `handle_submit` 实现。 |
| `CommandHandler` implementations | 与具体服务交互的执行适配器。若依赖少，放在 `omega-app`；若依赖重，可单独建 `omega-command-adapters` crate。构造期注入依赖，不在 `omega-command` 内部存放。 |
| `omega-session` | command runtime event、activity/log projection、command outcome 到 `RuntimeMessageEnvelope` 的桥接 |
| `omega-context` | document/memory 相关 command adapter 的唯一上层依赖面 |
| `omega-document` | workspace scan、keyword/vector search、document governance、persistent TODO 的真实实现 |

### Core Types

```rust
// ── omega-command crate ────────────────────────────────────────────────────

pub enum OmegaCommandKind {
    Local,
    Tool,
}

pub enum OmegaCommandSource {
    Builtin,
    ToolExtension,
}

pub struct OmegaCommandDescriptor {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: String,
    pub argument_hint: Option<String>,
    pub kind: OmegaCommandKind,
    pub source: OmegaCommandSource,
    pub user_invocable: bool,
    /// Runtime-evaluated enablement.  Called on every autocomplete tick and
    /// every dispatch attempt.  Must capture runtime state (e.g. feature
    /// flags, backend readiness) — a bare `fn()` pointer cannot do this.
    /// `omega-command` depends on `alloc` but not on tokio or any async runtime.
    pub is_enabled: Arc<dyn Fn() -> bool + Send + Sync>,
    pub is_sensitive: bool,
    pub immediate: bool,
}

pub struct OmegaCommandInvocation {
    pub raw_input: String,
    pub name: String,
    /// Two-level hierarchy: `name` is the top-level command (`document`),
    /// `subcommand` is the second level (`init`, `query`, `health`, etc.).
    pub subcommand: Option<String>,
    pub args: Vec<String>,
}

// ── omega-command outcome types ────────────────────────────────────────────

/// Where the overlay should be rendered.
pub enum OverlayTarget {
    /// Full-width panel — health reports, long query results.
    Document,
    /// Small contextual hint anchored to the input area.
    Inline,
}

pub enum CommandActivityStatus {
    Started,
    Success,
    Failed { reason: String },
}

#[derive(Debug, Clone)]
pub enum OmegaCommandOutcome {
    Noop,
    Message { text: String },
    Overlay { target: OverlayTarget, title: String, body: String },
    /// Signals an activity event to the session activity lane.
    CommandActivity {
        command: String,
        status: CommandActivityStatus,
        detail: Option<String>,
    },
}

// ── CommandHandler — lives in omega-app or omega-command-adapters ──────────
// NOT part of omega-command.  Services are injected via constructor.
//
// `async fn` in trait requires Rust 1.75+ (RPITIT).  omega-command itself
// depends on core/alloc only — no tokio.  The async bound is satisfied by
// the executor in omega-app, which does run on tokio.

pub trait CommandHandler: Send + Sync {
    fn name(&self) -> &str;
    async fn execute(
        &self,
        invocation: OmegaCommandInvocation,
    ) -> OmegaCommandOutcome;
}
```

`omega-command` crate 不依赖 `omega-context`、`omega-document` 或 tokio。它依赖 `alloc`（用于 `Arc<dyn Fn() -> bool>`）但不依赖 async runtime。`CommandHandler` trait 使用 Rust 1.75+ native `async fn in trait`（RPITIT），不需要 `#[async_trait]` 宏。`CommandHandler` 实现放在 `omega-app` 或独立 `omega-command-adapters` crate 中，通过构造注入 `Arc<dyn KnowledgeQueryService>` 等依赖。

### Execution Kinds

| Kind | 用途 | 典型例子 |
|------|------|----------|
| `Local` | 确定性本地动作，不需要模型 | `/document init`、`/document health` |
| `Tool` | 命令解析后路由到扩展 tool 的 typed invocation | 未来 `/git status`、`/kb search` 一类由扩展 tool 提供的命令 |

### Registry And Source Model

Phase 1/2 只支持两类 descriptor source：

1. builtin descriptors
2. tool-extension descriptors

其中 tool-extension descriptor 的来源不是独立 plugin command registry，而是由扩展 tool 在注册时显式声明可暴露给用户的 command metadata。command registry 负责聚合这些 metadata，但不负责动态加载 repo-local command config，也不负责把 workflow/skill/plugin 另起一层 command source。

原则：

- registry 负责统一列出“当前可见命令”；
- source loader 负责把不同来源转换成 `OmegaCommandDescriptor`；
- tool-extension command 只是扩展 tool 的用户入口面，不是新的工具系统；
- enablement 必须是 runtime 再判定，而不是只在启动时静态展开一次。

### Relation To Tools

- command 不是 tool catalog 的别名。
- command 可以包装或触发 tool，但不能反过来要求模型把 command 当成 tool 调用。
- document family 的第一阶段应直接通过 `omega-context` facade 调用真实服务，而不是先走一层 LLM prompt 再让模型自己决定调用哪些工具。
- tool-extension command 的元数据由扩展 tool 注册时提供，但 slash 提示、enablement、Response 展示仍由 command system 统一拥有。
- 当前不支持 workflow-backed commands，也不支持 prompt-macro commands。

## Slash Input Flow

当前 `omega-tui::event::key::handle_submit()` 直接调用 `session.spawn_turn()`，没有 slash 检测分支。将输入框的 slash 检测静态植入 `omega-tui` 内部等同于把 `omega-command` 依赖拉进 TUI 组件。

**选定方案：Option B — 两分支 `handle_submit` + 注入 dispatch 通道**

```
omega-app::run()
  │
  ├── 创建 command_dispatch_tx: mpsc::Sender<OmegaCommandInvocation>
  ├── 将 command_dispatch_tx 注入 TuiContext（传入 run_tui()）
  │
  └── 启动 command executor task
        接收 OmegaCommandInvocation
        路由到匹配的 Box<dyn CommandHandler>
        把 OmegaCommandOutcome 转发给 RuntimeMessageBridge

omega-tui::event::key::handle_submit()
  │
  ├── if input.starts_with('/'):
  │     command_dispatch_tx.send(input).await?;   // raw String, no parse
  │
  └── else:
        session.spawn_turn(input, turn_id, tx.clone()).await?;

omega-app command executor task:
  │
  ├── let invocation = omega_command::parse(&raw_input)?;
  ├── lookup descriptor, check is_enabled()
  └── route to matching CommandHandler
```

关键约束：

- TUI **不调用** `omega_command::parse()`，也不依赖 `omega-command` crate 内部类型。TUI 仅持有 `mpsc::Sender<String>`（发送 raw 输入）和 `Arc<dyn CommandHintProvider>`（查询 hint）。
- `omega_command::parse()` 是纯解析函数，无 I/O，无异步依赖；它在 `omega-app` executor task 中调用，解析出 `name`、`subcommand`、`args`。
- 实际 `CommandHandler` 执行发生在 `omega-app` 的 executor task 中，完全不在 TUI 线程。
- dispatch 前必须再次调用 `descriptor.is_enabled()` 验证，过期的 enablement 状态不得通过。

### Slash Hint UX

slash 命令不是“用户自己记忆所有 `/xxx` 字符串然后盲打”的模式。TUI 必须在输入期提供可见提示。

最小交互要求：

- 当输入框内容以 `/` 开头时，TUI 进入 command hint mode。
- command hint mode 至少在输入框上方的 context bar 或同等位置展示一条结构化提示，不允许只把提示写进日志或 activity。
- 当输入仅为 `/` 时，提示展示当前可见 top-level commands，每项至少包含 `name`、`source`、`description`。
- 当 top-level command 已唯一匹配且存在 subcommand 时，提示切换为 subcommand 列表，并展示当前 command 的 `argument_hint`。
- 当输入可唯一解析到具体命令时，提示展示最终将执行的 `command/subcommand`、来源（builtin 或 tool-extension）、参数提示以及 disabled/unavailable 状态。
- 当输入无法解析或命中 disabled command 时，提示必须在 submit 前给出明确反馈，而不是等执行期失败后才让用户看到错误。

Phase 1 的最低基线是 inline hint strip；overlay picker 仍可推迟，但 slash 提示本身不能推迟。

#### Slash Hint Registry Query Handle

TUI 查询 hint 不通过直接访问 `CommandRegistry`，而是通过一个 trait object，由 `omega-app` 在启动时注入：

```rust
/// TUI-visible hint for a single command.
pub struct CommandHint {
    pub name: String,
    pub source: OmegaCommandSource,
    pub description: String,
    pub argument_hint: Option<String>,
    pub enabled: bool,
}

/// TUI 解析候选结果。
pub enum CommandHintResolution {
    /// 唯一匹配到一个命令，可能仍有 subcommand。
    Resolved {
        command: CommandHint,
        subcommands: Vec<CommandHint>,
    },
    /// 输入前缀匹配到多个候选。
    Ambiguous(Vec<CommandHint>),
    /// 输入无法匹配任何注册命令。
    NoMatch,
}

/// omega-app 实现，omega-tui 通过 Arc<dyn CommandHintProvider> 持有。
pub trait CommandHintProvider: Send + Sync {
    /// 返回当前所有可见 top-level commands。
    fn visible_commands(&self) -> Vec<CommandHint>;
    /// 给定已选 command name，返回其 subcommand 列表。
    fn subcommand_hints(&self, command: &str) -> Vec<CommandHint>;
    /// 给定完整输入（不含前导 `/`），返回解析候选结果。
    fn resolve_hint(&self, input: &str) -> CommandHintResolution;
}
```

此 trait 定义在 `omega-command` crate 中，由 `omega-app` 的 `CommandRegistry` 实现。TUI 通过 `Arc<dyn CommandHintProvider>` 持有，无需依赖注册表内部结构。

**为何不选 Option A / Option C：**

- Option A（在 `handle_submit` 内部解析并执行）：会把 service 依赖拖进 TUI widget，违反 UI 边界。
- Option C（slash 检测在 session 内部）：session 只应处理模型轮次，不应同时承担命令 dispatch 职责。

## Phase 1 Command Family: `/document`

### Why `/document` First

`omega-document` 和 `omega-context` 现有实现已经具备 command 所需的核心后端：

- `KnowledgeQueryService::scan_workspace()`
- `KnowledgeQueryService::search()`
- `DocumentGovernanceService::manage_document()`
- `DocumentGovernanceService::check_document_health()`

因此 `/document` 可以直接验证 command system 的三个关键维度：

- 本地确定性执行
- RAG 查询结果投影
- 持久化知识库治理

### User-Facing Shape

首期命令族建议如下：

```text
/document init
/document query <query>
/document health
/document sync
/document create --type <spec|prd|guide|adr|todo|readme> --path <path> --title <title>
/document archive --path <path> --reason <superseded|completed|outdated|history_only>
/document list [--type <...>] [--status <...>]
```

其中第一阶段必须先交付四个子命令：

- `init`
- `query`
- `health`
- `sync`

`create` / `archive` / `list` 可以紧随其后，用于把项目级 RAG 知识库维护面补齐。

### Command Mapping To Existing Services

| Subcommand | Service Boundary | Existing API | 说明 |
|------------|------------------|--------------|------|
| `/document init` | `KnowledgeQueryService` | `scan_workspace()` | 初始化 `.omega-state/store/`，构建或刷新 manifest、Tantivy、LanceDB |
| `/document sync` | `KnowledgeQueryService` | `scan_workspace()` | 与 `init` 走同一增量扫描管线，但面向日常刷新语义 |
| `/document query` | `KnowledgeQueryService` | `search(SearchQuery)` | 默认 hybrid retrieval，作为项目级 RAG 查询入口 |
| `/document health` | `DocumentGovernanceService` | `check_document_health()` | 输出治理与索引健康状态 |
| `/document create` | `DocumentGovernanceService` | `manage_document(DocumentOp::Create)` | 新建并纳入项目知识库 |
| `/document archive` | `DocumentGovernanceService` | `manage_document(DocumentOp::Archive)` | 归档过期/被替代文档 |
| `/document list` | `DocumentGovernanceService` | `manage_document(DocumentOp::List)` | 枚举已索引文档与治理状态 |

### `/document init`

职责：初始化或刷新仓库知识库索引。

要求：

- 直接调用 `scan_workspace()`。
- 在空仓库或首次启用时创建 `.omega-state/store/files.jsonl`、`.omega-state/store/tantivy/`、`.omega-state/store/lance/` 等派生存储。
- 输出至少包括 `files_indexed`、`chunks_indexed`、`deleted_marked`、keyword/vector index readiness。
- 如果当前编译未启用 `document-backend`，命令必须被隐藏或明确标记 unavailable，而不是运行时报 panic。
- 成功后刷新 diagnostics/supervision snapshot，让 TUI 能马上看到最新 index 状态。

### `/document query`

职责：提供仓库级 RAG 查询入口。

默认策略：

- 默认 `mode = hybrid`
- 默认 `max_results = 8`
- 支持按 `path_glob`、`doc_type`、`language`、`status` 做过滤
- 输出展示命中路径、chunk 摘要、匹配模式、相关度排序说明

退化策略：

- 若 LanceDB revision 未 ready，自动降级到 keyword search
- 结果中必须明确写出 `hybrid -> keyword fallback`，避免用户误以为向量检索仍在生效

### `/document health`

职责：显式暴露文档治理和索引健康状态。

至少应回答：

- 文档目录结构是否符合 repo 规则
- 归档/替换链接是否缺失
- keyword/vector index 是否 ready
- 当前 store 规模、最近一次 index revision

### `/document sync`

职责：面向日常维护的增量刷新命令。

与 `init` 的语义区别：

| 维度 | `/document init` | `/document sync` |
|------|-----------------|-----------------|
| 主要场景 | 首次建立知识库；`.omega-state/store/` 尚不存在或需完全重建 | 日常刷新；`store/` 已存在，只需消化增量变更 |
| store 创建 | 显式创建目录与持久化文件 | 若 store 不存在则报错，提示用户先运行 `init` |
| 扫描范围 | 全量扫描整个 workspace | 仅处理 changed files（长远目标；首轮实现可与 `init` 共用全量路径，但必须在输出中标注 `full-scan fallback`）|
| 输出侧重 | 构建摘要：`files_indexed`、`chunks_indexed`、store creation paths | 刷新摘要：`files_changed`、`deleted_marked`、index freshness timestamp |

首轮实现：两者都调用 `scan_workspace()`，但 `init` 负责 store 目录创建，`sync` 在无 store 时给出明确错误提示而非 panic。后续版本引入 changed-files 追踪时 `sync` 自然演进为增量路径。

### `/document create` / `archive` / `list`

这组三个命令用于把“项目级 RAG 知识库维护”做成显式操作面：

- `create`：创建新文档并进入治理/索引体系
- `archive`：把被替代或完成的文档迁入 archive，同时补 archive note
- `list`：列出当前知识库中的文档范围与状态

这些命令必须直接复用 `manage_document` 的 staged governance 语义，而不是新增一套平行写路径。

## Runtime And TUI Integration

### Required Integration Points

- 输入框 `/` 前缀触发命令解析分支（见 Slash Input Flow）。
- 输入框进入 command hint mode 后，TUI 必须展示 registry 驱动的提示内容，至少覆盖 top-level command、subcommand、`argument_hint`、source 和 enablement 状态。
- `Activity` / runtime lane 需要能看到"哪个命令正在运行、是否成功、产出了什么结果"。
- `/document query` 的命中摘要应进入已有 document supervision / search overlay 数据通路，同时在 Response panel 留下可识别的 command section，而不是只作为普通聊天消息一闪而过。
- `/document init` / `sync` 的索引结果应写回 diagnostics snapshot，统一回答"是否启用、大小多少、命中了什么"。
- 所有用户可见 command 结果都必须能被 Response panel 识别为 command-originated content，并进行特殊样式区分。

### OmegaCommandOutcome → RuntimeMessageEnvelope Mapping

command executor task 接收 `OmegaCommandOutcome` 后，将其转换为 `RuntimeMessageEnvelope` 并通过 `RuntimeMessageBridge` 投递给 TUI surface.

| `OmegaCommandOutcome` variant | 对应 `RuntimeMessage` / `StateMessage` variant | 说明 |
|-------------------------------|------------------------------------------------|------|
| `Noop` | _(不发送消息)_ | 命令静默完成 |
| `Message { text }` | `RuntimeMessageEnvelope { turn_id: command_turn_id, .. }` → `RuntimeMessage::Effect(Begin/Append/CompleteResponseSection { kind = Command, origin = SectionOrigin::Command { .. } })` + `RuntimeMessage::State(StateMessage::Activity { text })` | 在 Response panel 写入 command section（通过 `command_turn_id`），并在 activity lane 镜像一行回执 |
| `Overlay { target, title, body }` | `RuntimeMessageEnvelope { turn_id: command_turn_id, .. }` → `RuntimeMessage::Effect(Begin/Append/CompleteResponseSection { kind = Command, origin = SectionOrigin::Command { .. } })` + `RuntimeMessage::State(StateMessage::ShowOverlay { target, title, body })` | 既触发 TUI overlay，也在 Response panel 留下对应 command section |
| `CommandActivity { command, status, detail }` | `RuntimeMessage::State(StateMessage::CommandActivity { ... })` | 新增 variant，用于 activity lane 展示命令执行进度 |

**`StateMessage::CommandActivity` 新增要求：** 需向 `omega-session` 的 `StateMessage` enum 中添加此 variant（与 `Activity`、`ShowOverlay` 并列）。`omega-app` 的 `apply_state()` 需路由该 variant 到 activity/progress 面板。此变更在 Phase 1 实现时落地，不在规格层引入，但规格必须明确声明该依赖。

### Response Panel Special Rendering

当前 `omega-tui` 的 Response timeline 已经基于 typed response sections 渲染 `Routing`、`Step`、`FinalAnswer`、`Thinking`。command system 必须复用同一条 typed 通路，而不是回退成普通日志文本。

#### SectionOrigin：区分 workflow 与 command

现有 `ResponseSectionMetadata` 的 `workflow_id` 和 `workflow_role` 字段对 command 无意义。为避免给 command 编造假 workflow 数据或将字段改为 `Option`，引入 `SectionOrigin` 枚举：

```rust
/// 每个 ResponseSection 的来源标识。
pub enum SectionOrigin {
    /// 普通 workflow turn 产生的 section（保持现有 workflow_id + role）。
    Workflow {
        workflow_id: String,
        workflow_role: WorkflowRunRole,
    },
    /// command system 产生的 section。
    Command {
        command_name: String,
        source: OmegaCommandSource,
    },
}
```

`ResponseSectionMetadata` 的 `workflow_id: String` + `workflow_role: WorkflowRunRole` 字段被替换为 `origin: SectionOrigin`。ResponseSection 创建时根据 section 来源填入对应 variant，TUI 渲染时按 variant 分支处理样式。

#### command_turn_id 策略

`RuntimeMessageEnvelope` 要求 `turn_id: u64`。为避免 command 与 session turn 的 id 空间冲突，command executor task 使用独立的 `AtomicU64`，起始值为 `u64::MAX / 2`。两个 id 空间方向相反（session 从 0 递增，command 从 `u64::MAX / 2` 递增），在可预见的生命周期内不会碰撞。

新增要求：

- runtime message / response section contract 需要新增 `ResponseSectionKind::Command`。
- command section 至少携带 `command_name`、`source`、`status` 三类渲染所需元数据（通过 `SectionOrigin::Command` 传递）。
- `omega-tui` 在渲染 `Command` section 时，应与普通 assistant step 区分样式，例如使用 command badge、source badge、独立标题行或更紧凑的结果摘要布局。
- command section 允许附带 secondary overlay，但 overlay 不是唯一用户可见面；关闭 overlay 后，Response panel 仍应保留 command 结果痕迹。
- command error 不能伪装成 assistant error reply；它应作为 failed command section 呈现，让用户一眼分辨“这是 slash command 失败”，而不是“模型回答失败”。

### Result Contract Guidance

Phase 1 不需要复杂 UI，但至少需要两种用户可见结果：

- `Message`：简要文本回执，但必须同步投影到 `ResponseSectionKind::Command`
- `Overlay`：较长的 health report 或 query result drill-down，需携带正确的 `OverlayTarget`，同时也必须在 Response panel 生成 command section

不要把 command output 退化成 bash 原始 stdout；command system 的价值之一，就是让这些结果保持结构化与可观测。

## Rollout Plan

### Phase 0: Spec And Task Framing

- 新增本规格文档
- 更新 `docs/README.md`
- 更新 `docs/TODO.md`

### Phase 1: Command Foundation

- 新建 `omega-command` crate（descriptor + parser，无 I/O 依赖）
- 定义 `CommandHandler` trait 和 `OmegaCommandOutcome` contract
- `omega-app` 安装 command executor task 和 `command_dispatch_tx` 注入点
- `omega-tui::handle_submit()` 添加两分支（slash → dispatch，otherwise → session turn）
- `omega-session` 添加 `StateMessage::CommandActivity` variant 和 `apply_state()` 路由
- `omega-tui` 增加 slash command hint mode，至少提供 inline hint strip 与 submit 前 invalid/disabled feedback
- Response timeline / runtime message contract 增加 `ResponseSectionKind::Command`，并完成特殊样式渲染
- **Phase 1 picker overlay 仍可推迟，但 slash 提示与 Response special rendering 不可推迟**

### Phase 2: `/document` MVP

- 交付 `/document init`
- 交付 `/document query`
- 交付 `/document health`
- 交付 `/document sync`
- **如需要更强交互，再实现 slash autocomplete picker：按注册表驱动候选列表，支持两级（命令 + subcommand）**

### Phase 3: Knowledge Base Maintenance

- 交付 `/document create`
- 交付 `/document archive`
- 交付 `/document list`
- command 结果与 supervision panel 做 typed 联动

### Phase 4: Generalize The System

- richer tool-extension command families
- permission / approval / sensitive-args policy 对齐
- 如果 builtin + tool-extension 模型稳定，再单独评估是否需要 workflow-backed 或其他 command source

## Testing Strategy

### Unit Tests

- `omega-command` 单测：parser（含多级子命令解析）、alias resolution、`is_enabled` 评估、visible filtering
- `omega-command` 单测：`OmegaCommandInvocation` builder、所有 outcome variant 的序列化
- `omega-tui` 单测：slash command hint mode 在 `/`、唯一匹配、disabled、invalid 四种状态下的提示渲染

### Integration Tests

- `omega-app` 集成测试：两分支 `handle_submit` — slash 走 dispatch channel，普通文本走 `spawn_turn`
- `omega-app` 集成测试：dispatch 前 `is_enabled()` 失败时返回 `Message` 而非 panic
- `omega-session` 集成测试：`CommandActivity` → `StateMessage` 投递 → `apply_state()` 路由
- `omega-session` / `omega-tui` 集成测试：command result 进入 `ResponseSectionKind::Command` 后能在 Response panel 被特殊区分渲染
- `omega-context` / `omega-document` 集成测试：`/document init/query/health/sync` 到真实 service boundary 的连通性

### Test Seam: MockCommandHandler

`CommandHandler` trait 即为主要测试接缝。测试中注入 `MockCommandHandler` 替代真实 service：

```rust
pub struct MockCommandHandler {
    pub name: String,
    pub outcome: OmegaCommandOutcome,
}

impl CommandHandler for MockCommandHandler {
    fn name(&self) -> &str { &self.name }
    async fn execute(&self, _inv: OmegaCommandInvocation) -> OmegaCommandOutcome {
        self.outcome.clone()
    }
}
```

命令 executor task 接受 `Vec<Box<dyn CommandHandler>>`，测试时传入 `MockCommandHandler` 列表，无需启动真实 LanceDB / Tantivy。这隔离了解析器正确性测试和 service 集成测试，让两者可以独立并行推进。

### Acceptance Tests

- 在 sample workspace 上验证首次 `init`、后续 `sync`、hybrid `query`、治理 `health` 的完整回路。
- 验证 `document-backend` feature 关闭时命令不可见（`is_enabled()` 返回 false），不报 panic。
- 验证 slash 输入触发 dispatch channel，非 slash 输入走 `spawn_turn`（通过 mock channels 断言调用路径）。
- 验证 slash 输入在 submit 前就能看到 command hint / invalid feedback，而不是执行后才报错。
- 验证 command result 在 Response panel 中显示为独立 command section，而不是伪装成普通 assistant reply。

## Acceptance Criteria

- 用户可以通过 `/document init` 显式初始化仓库知识库，不必依赖自然语言碰运气触发。
- 用户可以通过 `/document query <text>` 发起稳定的项目级 RAG 查询，并知道当前是否命中向量检索或 keyword fallback。
- 用户可以通过 `/document health` 和 `/document sync` 管理知识库状态。
- 用户在输入 `/` 命令时可以看到即时提示，能够在提交前分辨 top-level command、subcommand 和不可用状态。
- Response panel 能把 command 相关结果识别为 command section，并与普通 assistant reply 明确区分。
- 项目级知识库维护路径不新增平行后端，仍以 `omega-context -> omega-document` 为唯一业务边界。
- command system 本身不依赖某个具体 UI 组件，可以被 `omega-app` 以外的 future surfaces 复用。

---

## Architecture Decisions

以下是 v0.1 到 v0.2 审查过程中确定的关键架构决策，以后不得无声回退。

### AD-1: omega-command 不依赖 omega-context

`omega-command` crate **仅**包含如下内容：descriptor types、registry、parser、`CommandHandler` trait定义、`OmegaCommandOutcome` types。

它不持有也不构造任何 `Arc<dyn KnowledgeQueryService>` 或其他 service 实例。执行适配器实现 `CommandHandler` trait，但存放在 `omega-app` 或 `omega-command-adapters` crate。此边界确保 `omega-command` 可被任何 surface crate 轻量复用，不施加 runtime 重依赖。

### AD-2: slash 拦截通过 command_dispatch_tx 通道实现

`omega-tui::handle_submit()` 添加两分支 — slash 路走 `command_dispatch_tx.send()`，非 slash 路走原有 `session.spawn_turn()`。通道 sender 在 `omega-app::run()` 组装期注入到 TUI context。`omega-tui` 不持有任何执行逻辑，也不直接感知 `CommandHandler` 实现。

### AD-3: OmegaCommandDescriptor.is_enabled 是闭包

`is_enabled: Arc<dyn Fn() -> bool + Send + Sync>` 替代 `is_hidden: bool`。使用 `Arc` 包装的闭包而非裸函数指针，允许捕获运行时状态（如 feature flag 引用、backend 可用性检查）。每次 hint 刷新和每次 dispatch 前都必须重新调用。这与 Claude Code `isEnabled()` 模式对齐，确保 feature flag 切换、backend 启用/停用在运行时立即生效。

### AD-4: Phase 1 不实现 picker autocomplete

Phase 1 必须实现 slash inline hint strip，并能在 `/`、唯一匹配、subcommand、invalid、disabled 几种状态下给出即时提示。picker overlay 驱动的候选列表可以推迟到 Phase 2，但 slash 提示本身不能推迟。

### AD-5: 当前 command source 仅允许 builtin + tool-extension

当前 command registry 不支持 repo-local、workflow-generated、skill、plugin 等其他来源。所有非 builtin command 都必须作为 tool-extension descriptor 暴露，并继续受统一 enablement、visibility、Response 渲染契约约束。

### AD-6: 所有用户可见 command 结果都必须写入 Response section

command result 可以同时镜像到 Activity 或 Overlay，但只要用户可见，就必须先被投影到 typed `Command` response section。不得把 slash command 的成功/失败结果只写进 activity lane，更不得伪装成普通 assistant step/final answer。

---

### Change Log

- 2026-04-03: v0.4 — 二次架构审查修复（10 项 findings）：`is_enabled` 改为 `Arc<dyn Fn() -> bool + Send + Sync>` 闭包（F1）；`ResponseSectionMetadata` 引入 `SectionOrigin` enum 替代 non-optional `workflow_id`（F2）；command_turn_id 使用独立 `AtomicU64` 空间（F3）；TUI 不调用 `parse()`，仅持有 `Sender<String>` + `Arc<dyn CommandHintProvider>`（F4）；删除 `WorkflowInput` variant（F5）；`async fn` in trait 标注 RPITIT Rust 1.75+（F6）；新增 `CommandHintProvider` trait 定义和注入说明（F7）；changelog 日期修正（F8）；frontmatter 升级 v0.4（F9）；`OmegaCommandOutcome` 加 `Clone` derive（F10）。
- 2026-04-02: v0.3 — 收窄 command source 到 builtin + tool-extension；删去 workflow/prompt-macro 当前范围；把 slash hint mode 与 `ResponseSectionKind::Command` 特殊展示提升为 Phase 1 基线；明确 command 结果必须进入 Response panel 的 typed section 通路。
- 2026-04-02: v0.2 — 根据架构审查（8 项 findings）全面修订：CommandHandler trait 与 executor 边界分离（HIGH-1）；slash 检测改为两分支 + dispatch 通道方案（HIGH-2）；OmegaCommandOutcome::Overlay 增加 OverlayTarget，新增 CommandActivity variant（HIGH-3）；autocomplete 推迟到 Phase 2（HIGH-4）；新增 subcommand 字段（MEDIUM-5）；init/sync 语义明确区分（MEDIUM-6）；is_hidden 改为 is_enabled fn 指针（MEDIUM-7）；新增 MockCommandHandler 测试接缝（LOW-8）。
- 2026-04-02: v0.1 初版规格创建，完成 Claude command system 参考分析，并将 `/document` 定义为 Omega 首个 command family。
