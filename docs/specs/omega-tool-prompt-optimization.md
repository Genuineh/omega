---
content_revision: 118
created: 2026-04-01
generation_id: gen_000037_r000118
last_verified_commit: N/A
owner: omega-team
projection_version: 37
related_prds: []
source_doc_id: "spec:docs-specs-omega-tool-prompt-optimization"
status: draft
supersedes: []
updated: 2026-04-01
---

# Omega Tool And Prompt Optimization Specification

## Overview

Omega 当前已经具备稳定的工具执行基础，包括 `ToolHandler`、`ToolResult`、`ToolErrorKind`、step 级 tool visibility，以及 runtime tool lifecycle。这些能力解决了“工具能不能安全执行”的问题，但还没有解决“工具系统是否已经成为完整产品能力”的问题。

这份规格的目标不是再补几个工具，而是把 Omega 的工具系统升级为统一框架：每个工具都必须具备一致的九类能力，并在此基础上支持更强的工具形态，包括用户提到的 `WebSearch`、`WebFetchTool`、`TodoWrite`、`FileEdit`、`AskUserQuestion`、`BashTool` 等。

九类统一能力如下：

1. 工具本体
2. 提示词与使用策略
3. 数据处理
4. UI 联动
5. 上下文联动
6. 统一数据契约 + tool 个性化数据契约
7. 权限联动
8. 存储系统联动
9. 监控

参考 `learn/claude-code-source-code` 的成熟点，Omega 下一阶段最值得吸收的不是 prompt-first 的 agent shell，而是“工具作为产品能力”的完整建模方式：工具说明、失败纠偏、UI 联动、审批、记忆写入、运行态监控都必须成为工具系统的正式部分。

## Goals

- 把 Omega 工具系统从“可调用的一组 handlers”升级为“带有 prompt、UI、context、permission、storage、monitoring 的正式子系统”。
- 提升模型在 read-only inspection、web research、editing、todo、用户确认、workflow orchestration 等场景中的工具选择质量。
- 让每个工具的使用方式、禁用场景、替代工具、失败后下一步建议成为一等数据，而不是散落在 system prompt 中。
- 让工具调用结果能够驱动后续主流程稳定继续，而不是只返回一段文本。
- 建立统一的 tool capability framework，使未来新增工具时不再重复设计 prompt、UI、权限和监控逻辑。
- 为后续权限系统、project memory 入口、command workflow、subagent 与 TUI 深度联动提供稳定底座。

## Non-Goals

- 不替换现有 `ToolResult` / `ToolErrorKind` / `ToolRun` 契约。
- 不把 Omega 退化成 prompt-first agent loop。
- 不在本轮直接引入 Claude Code 风格 classifier 或高不确定性自动审批模型。
- 不为了“工具更聪明”而放宽现有 workspace safety、tool visibility 或 output contract。
- 不把所有优化都塞进 `bash` fallback。
- 不要求首轮就落地完整浏览器自动化、远程多用户协作或 MCP 全量生态。

## Current Assessment

### Strengths

- `omega-tools` 已有稳定的 `ToolHandler` / `ToolDispatcher` / `ToolResult` / `ToolErrorKind` 基础。
- `omega-session` 已有 step-scoped visible tools 与 prompt builder，可承接 tool strategy 分层。
- `omega-session` 与 `omega-tui` 已有 `ToolRun` lifecycle，说明工具运行态已是一等可视对象。
- `omega-tools-builtin` 已具备 `list_dir`、`glob_search`、`grep_search`、`read_file`、`apply_patch`、`create_file`、`edit_file`、`batch`、`bash` 等关键雏形。
- `ToolErrorKind::Validation | Policy | Execution | Timeout | UnknownTool` 已经为失败纠偏提供了良好基础。

### Gaps

1. 工具定义还主要停留在“名称 + schema + 执行函数”层面，没有完整 capability profile。
2. prompt 层没有专门的 tool strategy 架构，工具使用知识仍过于松散。
3. runtime 可展示工具运行，但缺少 per-tool UI contract，无法稳定表达预览、CTA、审批、跳转和详情面板。
4. 工具对上下文的读取仍偏隐式，没有明确的 tool-scoped context view contract。
5. 工具失败后的回注还不够像“下一步行动建议”，难以稳定纠偏主流程。
6. permission、storage、observability 已有局部实现，但还不是工具定义的一部分。
7. 工具体系仍偏本地 workspace，`WebSearch`、`WebFetch`、`AskUserQuestion` 这类产品级工具尚无统一规划。

## Design Principles

### 1. Contract First

任何 prompt、UI 或工具策略优化都不能替代：

- `ToolResult`
- `ToolErrorKind`
- `ToolRun`
- step input/output contract

prompt 和 UI 负责提升选择质量与纠偏稳定性，不能承担 correctness 的最终责任。

### 2. Tools Are Runtime Capabilities, Not Just Functions

一个合格工具不只是“可调用函数”，而必须同时定义：

- 如何被模型理解
- 如何被 runtime 呈现
- 如何读取上下文
- 如何写入存储
- 如何受权限控制
- 如何被监控与调试

### 3. Decision-Oriented Guidance Over Raw Schema

工具说明必须优先回答：

- 什么时候优先使用
- 什么时候不要使用
- 与相邻工具如何区分
- 失败后改用什么

而不是只描述字段和参数。

### 4. Family-Level Guidance Before Tool-Level Detail

先教模型“当前问题属于 inspection / web research / editing / planning / interaction / escape hatch 哪一类”，再教它选具体工具。family-level guidance 比堆 20 个工具描述更稳。

### 5. Stable Failures Must Produce Stable Next Steps

当工具因为 validation、policy、timeout 或 execution 失败时，主流程必须拿到可操作 remediation，而不是自由文本报错。

### 6. Tool Metadata Must Be Explicit And Testable

工具的 guidance、UI、context、permission、storage、monitoring 都必须是显式数据结构或显式配置，而不是隐式埋在 handler 逻辑里。

## Tool Capability Framework

本节定义所有工具必须具备的九类能力。后续新增工具时，至少要完成这些能力的最小实现。

### Capability 1: Tool Core

定义工具本体，即它“做什么”。

必备内容：

- `id`
- `display_name`
- `family`
- `handler`
- `input_schema`
- `output_shape`
- `stability`

建议 family：

- `WorkspaceInspection`
- `WebResearch`
- `Editing`
- `Planning`
- `Interaction`
- `EscapeHatch`

### Capability 2: Prompt Strategy

定义模型如何理解和使用该工具。

必备内容：

- `summary`
- `when_to_use`
- `when_not_to_use`
- `prefer_over`
- `fallback_to`
- `examples`
- `anti_patterns`

这部分应支持：

- global tool strategy
- family strategy
- step-specific hints
- tool-specific guidance

### Capability 3: Data Processing

定义工具输入输出如何被校验、格式化和截断。

必备内容：

- 输入 schema 校验
- 输入规范化
- 输出格式化
- 输出截断策略
- 结构化 metadata
- typed error mapping

目标是让工具输出稳定驱动主流程，而不是把复杂解析工作留给模型。

### Capability 4: UI Integration

定义工具如何与 TUI 或未来 UI 交互。

必备内容：

- invocation preview
- running state
- result preview
- detail overlay data
- action affordances
- completion / failure notice

典型 UI 联动：

- `AskUserQuestion` 触发确认对话框或输入弹层
- `WebFetch` 在详情面板展示 URL、标题、摘要、状态码
- FileEdit 家族展示 diff preview
- `TodoWrite` 驱动 todo sidebar 更新

### Capability 5: Context Integration

定义工具能看到哪些上下文，以及看到的形式。

必备内容：

- tool-scoped context view
- current workflow/step metadata
- visible workspace root / working set
- selected files / current artifact refs
- optional memory summaries

原则：

- 工具不直接看“全部上下文”，而是拿到经过筛选的 `ToolExecutionContext`。
- context exposure 必须与 step policy 和 permission 对齐。

### Capability 6: Unified Contract + Tool-Specific Contract

每个工具都要遵守统一结果契约，同时可扩展自己的个性化 payload。

统一部分建议包括：

- `output`
- `preview`
- `metadata`
- `truncated`
- `error_kind`
- `remediation`
- `ui_effects`
- `storage_effects`
- `observability`

个性化部分示例：

- `WebSearch`: 查询词、命中列表、来源、置信度、分页信息
- `WebFetch`: URL、标题、抓取状态、正文摘要、内容类型
- `TodoWrite`: 变更的 todo 项、变更类型、影响范围
- `FileEdit`: diff、受影响文件、是否需要人工复核
- `AskUserQuestion`: 问题类型、选项、回答状态、用户响应

### Capability 7: Permission Integration

定义工具的审批和授权面。

必备内容：

- permission class
- default policy mode
- approval requirement
- escalation strategy
- denial remediation

示例：

- `WebSearch`: 通常是低风险，可默认允许
- `WebFetch`: 可能受外网策略影响，需要 network policy
- `FileEdit`: 受 step policy 和 write permission 双重控制
- `BashTool`: 受 allowlist、workdir、timeout、approval 多重控制
- `AskUserQuestion`: 不是危险工具，但会阻塞主流程，需要 UI approval semantics

### Capability 8: Storage Integration

定义工具与 session store、memory、artifacts、workspace state 的关系。

必备内容：

- 是否写 session journal
- 是否产生 artifact
- 是否写 memory / todo / cache
- 是否需要 rollback clue
- 是否产生 replayable state

示例：

- `TodoWrite` 应写入 todo store 和 runtime timeline
- `WebFetch` 可写 fetch cache / page snapshot metadata
- `FileEdit` 应记录 diff metadata 与 affected path list
- `AskUserQuestion` 应持久化用户回答，以便 resume/replay

### Capability 9: Monitoring

定义工具如何被 tracing、metrics、diagnostics 观测。

必备内容：

- invocation count
- success/failure counts
- latency
- truncation count
- policy denial count
- retry count
- tool-switch-after-failure

目标不是只看“工具有没有跑”，而是看“工具是否帮助主流程更稳定前进”。

## Architecture

### Components

#### 1. Tool Manifest Layer

在现有 tool definition 之上增加正式 manifest，承载除 handler 之外的全部 capability metadata。

**集成方案：Manifest wraps Handler。** `ToolDispatcher` 改为持有 `HashMap<String, ToolManifest>`，manifest 内含 `handler: Box<dyn ToolHandler>`。现有 `ToolHandler` trait 签名（`name()`、`description()`、`input_schema()`、`execute_v2(&self, input: Value) -> Result<ToolResult>`）保持不变。Manifest 作为注册时的 envelope，handler 本身不感知 manifest 的存在。

**最小实现规则：** 只有 `Tool Core`（id/family/handler）和 `Prompt Strategy` 两类 profile 是 **required**，其余七类 profile 全部为 `Option<T>`，迁移现有工具时不需要填写无关 profile。

建议结构：

```rust
struct ToolManifest {
    // --- required ---
    id: String,
    display_name: String,
    family: ToolFamily,
    stability: ToolStability,
    handler: Box<dyn ToolHandler>,
    prompt: ToolPromptProfile,
    // --- optional ---
    io: Option<ToolIoProfile>,
    ui: Option<ToolUiProfile>,
    context: Option<ToolContextProfile>,
    permissions: Option<ToolPermissionProfile>,
    storage: Option<ToolStorageProfile>,
    observability: Option<ToolObservabilityProfile>,
}
```

`ToolDispatcher` 的 `dispatch()` 方法从 manifest 中取 handler 执行，不改变调用方式：

```rust
impl ToolDispatcher {
    pub fn register_manifest(&mut self, manifest: ToolManifest) { ... }
    pub fn dispatch(&self, name: &str, input: Value) -> Result<ToolResult> {
        let manifest = self.manifests.get(name).ok_or(...)?;
        manifest.handler.execute_v2(input)
    }
    pub fn manifest(&self, name: &str) -> Option<&ToolManifest> { ... }
}
```

#### 2. Tool Prompt Builder

基于 visible tools 和 current step，构建分层 tool strategy：

- global strategy
- family strategy
- step hints
- tool-specific guidance

#### 3. Tool Execution Context Bridge

为需要上下文的工具提供明确的 `ToolExecutionContext`，而不是破坏现有 handler 签名。

**关键约束：不改变 `ToolHandler` trait 签名。** 现有 11 个 built-in handler 仍然只接收 `Value` 输入，不强制所有工具接受大型 context struct。

**集成方式：** 需要富上下文的新工具（如 `ask_user_question`、`todo_write`、`web_search`）通过构造注入获取所需服务（`Arc<dyn ContextProvider>` 等），不是通过 execute 签名传递。`ToolDispatcher` 在调度时可选地从当前 step state 组装 context，注入到持有 context slot 的 handler。

**与 `OmegaContextFacade` 的边界对齐：** `ToolContextProfile` 是声明式 request descriptor（"本工具需要什么层级的上下文"），实际上下文交付由 `OmegaContextFacade` 统一履行。工具不直接访问 `omega-memory` 或 `omega-document`——这些仍然是 `omega-context` 的内部实现细节。

建议包含：

- workspace root（已有，handler 构造时注入）
- current scene / workflow / step（新增，通过 dispatcher 注入）
- visible tool set（已有，`SessionToolCatalog` 提供）
- active artifact refs（由 `OmegaContextFacade.assembler` 提供）
- selected file refs（由 `OmegaContextFacade.assembler` 提供）
- memory summary refs（由 `OmegaContextFacade.memory` 提供）
- permission snapshot（由 permission bridge 提供）

#### 4. Tool Outcome Builder

在 handler 原始输出基础上统一封装：

- `ToolResult`
- remediation
- ui effects
- storage effects
- observability payload

#### 5. Tool UI Bridge

把 tool outcome 映射到 `ToolRun`、TUI overlay、notices、sidebars，以及未来的 browser or prompt surfaces。

#### 6. Tool Permission Bridge

将 workflow static policy、repo-local tool config 和 runtime approval 三层统一起来。

#### 7. Tool Storage Bridge

将工具副作用统一写入：

- todo store
- memory store
- artifact journal
- session replay log

#### 8. Tool Observability Bridge

把工具运行结果映射到 tracing span、metrics 和 diagnostics。

## Data Flow

```text
Tool manifests + handlers
  -> SessionToolCatalog resolves visible tools
  -> Tool Prompt Builder emits strategy blocks
  -> LLM selects tool
  -> ToolExecutionContext scoped for current step
  -> Handler validates and executes
  -> Tool Outcome Builder adds remediation/ui/storage/observability payload
  -> Runtime updates ToolRun + UI surfaces + stores + diagnostics
  -> Next loop round receives structured result and stable next-step guidance
```

## Data Models

### ToolManifest

| Field | Type | Required | Purpose |
| --- | --- | --- | --- |
| `id` | `String` | **yes** | 稳定工具标识 |
| `display_name` | `String` | **yes** | UI 显示名 |
| `family` | `ToolFamily` | **yes** | 工具家族 |
| `stability` | `ToolStability` | **yes** | 版本成熟度 |
| `handler` | `Box<dyn ToolHandler>` | **yes** | 现有 handler（签名不变） |
| `prompt` | `ToolPromptProfile` | **yes** | 提示词策略 |
| `io` | `Option<ToolIoProfile>` | no | 输入输出和格式化规则 |
| `ui` | `Option<ToolUiProfile>` | no | UI 联动配置 |
| `context` | `Option<ToolContextProfile>` | no | 上下文暴露规则（声明式 request descriptor） |
| `permissions` | `Option<ToolPermissionProfile>` | no | 权限与审批配置 |
| `storage` | `Option<ToolStorageProfile>` | no | 存储副作用配置（待存储 API 稳定后定义） |
| `observability` | `Option<ToolObservabilityProfile>` | no | tracing/metrics/diagnostics 配置 |

### ToolPromptProfile

| Field | Type | Purpose |
| --- | --- | --- |
| `summary` | `String` | 一句话用途 |
| `when_to_use` | `Vec<String>` | 应使用场景 |
| `when_not_to_use` | `Vec<String>` | 不应使用场景 |
| `prefer_over` | `Vec<String>` | 优先于哪些工具 |
| `fallback_to` | `Vec<String>` | 失败后替代工具 |
| `examples` | `Vec<String>` | 典型调用模式 |
| `anti_patterns` | `Vec<String>` | 反模式 |

### ToolIoProfile

| Field | Type | Purpose |
| --- | --- | --- |
| `max_output_bytes` | `usize` | 单次输出上限 |
| `truncation_strategy` | `TruncationStrategy` | 截断方式（`Tail` / `Head` / `Middle`） |
| `output_format` | `OutputFormat` | 结果格式（`PlainText` / `Json` / `Diff`） |
| `normalize_input` | `bool` | 是否对输入做规范化（路径解析、编码统一） |

### ToolContextProfile

注：此 profile 是**声明式 request descriptor**——工具声明自己需要什么层级的上下文，实际交付由 `OmegaContextFacade` 统一履行。工具不直接访问 `omega-memory` 或 `omega-document`。

| Field | Type | Purpose |
| --- | --- | --- |
| `needs_workspace_root` | `bool` | 是否需要仓库根 |
| `needs_step_metadata` | `bool` | 是否需要 scene/workflow/step |
| `needs_selection` | `bool` | 是否需要当前 selection/artifact refs |
| `memory_scope` | `MemoryScopeLevel` | 可见 memory 层级 |
| `network_context` | `bool` | 是否需要网络/远端策略上下文 |

### ToolRemediation

`remediation` 是本轮优化的关键字段，必须是结构化类型，而不是自由文本。结构化后主流程才能稳定消费（如自动切换到 fallback 工具、自动重试带修正参数）。

```rust
struct ToolRemediation {
    kind: ToolErrorKind,              // 复用已有 enum
    suggestion: String,               // 给模型的自然语言建议
    alternative_tools: Vec<String>,   // 推荐替代工具
    recoverable: bool,                // 是否可自动重试
}
```

### ToolOutcome Extension

| Field | Type | Purpose |
| --- | --- | --- |
| `remediation` | `Option<ToolRemediation>` | 给主流程的结构化下一步建议 |
| `ui_effects` | `Vec<RuntimeUiEffect>` | UI 更新意图（复用现有 RuntimeUiEffect） |
| `storage_effects` | `Vec<ToolStorageEffect>` | 持久化副作用 |
| `observability` | `ToolObservabilityPayload` | 指标和诊断数据 |

## Target Tool Families And Priority Tools

### Family A: Workspace Inspection

目标：减少本地检索类场景退回 `bash`。

核心工具：

- `list_dir`
- `glob_search`
- `grep_search`
- `read_file`
- `batch`

### Family B: Editing

目标：让写路径稳定、可预览、可诊断。

核心工具：

- `apply_patch`
- `create_file`
- `edit_file`
- 可选后续：`multi_edit`

### Family C: Web Research

这是本轮新增重点，吸收 Claude Code 中 `WebSearch` 与 `WebFetchTool` 的优势。

核心工具：

- `web_search`
- `web_fetch`

要求：

- 明确网络权限与策略
- 支持结果摘要和来源信息
- 支持 UI 中展示 URL、标题、摘要、状态
- 与 memory、artifact、prompt context 可控衔接

### Family D: Planning And Coordination

目标：把“让主流程稳定推进”相关动作正式工具化。

核心工具：

- `todo_write`
- `todo_read`
- `task`
- `ask_user_question`

要求：

- `todo_write` 不只是改状态，还要能驱动 runtime timeline 与 sidebar
- `ask_user_question` 必须有明确 UI semantics，而不是普通文本提问
- `task` 应与 subagent / fresh-context execution 对齐

### Family E: Escape Hatch

核心工具：

- `bash`

原则：

- 保留为 fallback
- 做强约束、强诊断、强审批
- 不再承担默认 read/search 主路径

## Tool Blueprints For High-Value Targets

### `web_search`

- **Purpose**: 对外部信息源做关键词检索，返回结构化候选结果。
- **Prompt profile**:
  - 适合：查找外部文档、新闻、API 参考、公开资料。
  - 不适合：读取本地文件、需要完整正文时。
  - 优先于：`bash`、`web_fetch`（当还不知道 URL 时）。
  - 失败后：缩小查询词或改用 `web_fetch` 读取已知 URL。
- **UI integration**: 展示查询词、命中结果数量、来源列表。
- **Context integration**: 可见当前任务主题、限定域名策略、网络权限快照。
- **Storage integration**: 可选写 search session cache。
- **Monitoring**: query latency、zero-result rate、follow-up fetch rate。

### `web_fetch`

- **Purpose**: 获取已知 URL 内容并输出结构化摘要。
- **Prompt profile**:
  - 适合：用户给出 URL、`web_search` 已选中某条结果后。
  - 不适合：还不知道访问哪个页面时。
- **Data processing**: content-type handling、正文提取、截断摘要。
- **UI integration**: 展示 URL、标题、状态码、内容摘要。
- **Storage integration**: 写 fetch metadata / optional cache。
- **Monitoring**: success rate、timeout rate、content extraction failures。

### Web Tools Infrastructure Open Questions

以下决策在实现 Task 8S 前必须解决，当前标记为开放问题：

| 决策项 | 候选方案 | 备注 |
| --- | --- | --- |
| 搜索 API 选型 | Brave Search / Tavily / SearXNG (self-host) | 需要评估 rate limit、价格、结果质量 |
| HTTP 客户端 | 复用 `reqwest`（已在 omega-client 中使用）| 避免引入额外依赖 |
| 内容提取策略 | readability (Rust port) / jina reader API / 自建 | 影响正文质量和延迟 |
| 响应体大小上限 | 建议 1MB raw + 截断到 `ToolIoProfile.max_output_bytes` | 防止 OOM 和 token 膨胀 |
| API key 管理 | `OMEGA_*` env var / `.omega/secrets.toml` | 需要与现有 env config 对齐 |
| Rate limiting | 工具级限流（令牌桶/滑动窗口）| 防止循环调用耗尽配额 |

### `todo_write`

- **Purpose**: 明确更新 todo state，而不是把 todo 变更埋在自然语言里。
- **Prompt profile**:
  - 适合：任务分解、任务状态推进、收尾同步。
  - 不适合：只是在思考，不确定是否真正开始时。
- **UI integration**: 驱动 todo sidebar、状态带、活动提示。
- **Storage integration**: 写 todo store、session journal。
- **Monitoring**: todo mutation count、task completion cadence、stale todo rate。

### `file_edit`

这里对应 Omega 现有 `apply_patch` / `edit_file` 家族，目标是形成统一的 FileEdit 能力面。

- **Purpose**: 以结构化方式修改本地文件。
- **Prompt profile**:
  - 适合：局部替换、补丁式编辑、创建新文件。
  - 不适合：只读检查或需要多仓库复杂批处理时。
- **UI integration**: diff preview、受影响路径、错误定位。
- **Context integration**: 当前文件选择、最近读取上下文、active task。
- **Permission integration**: write approval、step visibility、path policy。
- **Storage integration**: artifact journal、patch metadata、optional checkpoint。

### `ask_user_question`

- **Purpose**: 在必须用户参与时，用显式工具暂停主流程并收集结构化回答。
- **Prompt profile**:
  - 适合：权限确认、需求澄清、分支选择。
  - 不适合：模型自己可以从上下文推断时。
- **UI integration**: modal / inline prompt / options selector。
- **Storage integration**: 持久化用户回答，用于 resume/replay。
- **Monitoring**: question count、resolution latency、question-abandon rate。

### `bash`

- **Purpose**: 逃逸口，用于结构化工具不适配的命令执行。
- **Prompt profile**:
  - 适合：已有结构化工具无法表达的受控命令。
  - 不适合：目录浏览、文本搜索、文件读取、常规编辑。
- **UI integration**: command preview、workdir、runtime status、tail output。
- **Permission integration**: allowlist、workdir policy、approval。
- **Monitoring**: fallback count、policy denial count、high-output truncation count。

## Prompt Layering For Tool Use

建议将工具相关 prompt 明确拆成四层：

1. `Global Tool Strategy`
   - inspection first
   - patch-centric editing
   - ask user only when needed
   - bash is fallback

2. `Family Strategy`
   - 当前 step 可见哪些 family
   - 当前场景 family 优先级

3. `Step Tool Hints`
   - root routing / plan / execute / report 的局部约束

4. `Tool Local Guidance`
   - 单工具的 when_to_use / when_not_to_use / examples / anti_patterns

这四层必须可独立演进，不应继续散落在 base prompt、workflow prompt、repo note 和 skill 文件内部。

## Unified Result Contract And Remediation

在保留 `ToolResult` 的前提下，建议把工具结果的 runtime 消费面明确为：

| Field | Purpose |
| --- | --- |
| `output` | 主流程回看的正文 |
| `preview` | UI 与摘要优先使用的短文本 |
| `metadata` | tool-specific structured payload |
| `truncated` | 是否被截断 |
| `error_kind` | 统一失败分类 |
| `remediation` | 结构化 `ToolRemediation`：error kind + suggestion + alternative_tools + recoverable |
| `ui_effects` | `Vec<RuntimeUiEffect>`（复用现有 enum，不另建体系） |
| `storage_effects` | todo/memory/journal/artifact 变更 |
| `observability` | metrics/diagnostics payload |

其中 `remediation` 是这轮优化的关键：

- `Validation`: 告诉模型哪个字段缺失或格式错误，以及是否应改用其他工具。
- `Policy`: 告诉模型当前 step 不允许该动作，以及下一步是切换工具、切换阶段，还是请求用户确认。
- `Timeout`: 提示缩小范围、减少输出或改用更窄工具。
- `Execution`: 提示是输入问题、环境问题还是目标不存在，并给出可继续的下一步。

## Permission Integration Model

工具权限应统一分三层：

1. `Static Workflow Policy`（即现有 `StepToolRequest::Inherit | Extend | Block`）
   - 该 step 里工具是否可见
   - 由 `omega-workflow` 的 `StepDefinition` 決定，经 `SessionToolCatalog::resolve_for_step()` 解析
   - 不可见的工具直接返回 `ToolErrorKind::Policy`

2. `Repo / Session Tool Policy`（现有 `ToolPolicyConfig` 的扩展）
   - 工具是否启用
   - 网络是否允许
   - bash allowlist / timeout / workdir
   - 这一层是可见性之上的额外约束：工具可见但可能被 repo 策略禁用

3. `Runtime Approval`（新增）
   - ask once
   - allow for turn
   - allow for session
   - 这一层是可见且启用之上的审批要求：工具可用但需要用户确认

层级关系：

```text
StepToolRequest (visibility) → ToolPolicyConfig (enablement) → RuntimeApproval (approval)
不可见 → 直接拒绝           禁用 → PolicyError             未审批 → 触发审批流程
```

原则：

- runtime approval 只能在 static policy + repo policy 均允许的前提下生效。
- `AskUserQuestion` 本身应成为权限交互正式工具，而不是让模型直接输出“请确认”。

## Storage Integration Model

工具与存储系统的联动应收敛到统一 effect 模型：

- `TodoEffect`
- `MemoryEffect`
- `ArtifactEffect`
- `JournalEffect`
- `CacheEffect`

**延迟设计注意事项：** `omega-memory` 和 `omega-document` 目前仍在建设中（`omega-context-management.md` 也是 draft 状态），底层 API 尚不稳定。因此：

- 当前阶段只需在 manifest 中预留 `storage: Option<ToolStorageProfile>` 占位。
- Effect 模型的完整实现应等 `OmegaContextFacade` 的 memory/governance API 稳定后再落地。
- 工具通过 `OmegaContextFacade` 执行存储副作用，不直接访问 `omega-memory` 或 `omega-document`。

示例：

- `todo_write` -> `TodoEffect + JournalEffect`
- `web_fetch` -> `CacheEffect + ArtifactEffect(optional)`
- `file_edit` -> `ArtifactEffect + JournalEffect`
- `ask_user_question` -> `JournalEffect + MemoryEffect(optional)`

## UI Integration Model

工具 UI 联动不另建独立 effect 体系，而是复用现有 `RuntimeUiEffect` enum（已包含 `ShowOverlay`、`BeginToolRun`、`SetStatusSlot` 等变体）。工具的 UI effect 应表达为 `RuntimeUiEffect` 的新 variant 或现有 variant 的组合，而不是新建并行体系。

需要新增的 variant（在 `RuntimeUiEffect` 中扩展）：

- `RequestInput { ... }` — `AskUserQuestion` 触发的用户输入请求
- `OpenDiffPreview { ... }` — FileEdit 家族的 diff 预览
- `OpenWebResultView { ... }` — web 工具的结果展示

现有可复用的 variant：

- `ShowOverlay` — overlay 展示
- `SetStatusSlot` — 状态栏更新
- `BeginToolRun` / `UpdateToolRun` / `CompleteToolRun` — 工具运行生命周期

这保证 omega-tui 只需消费一套 effect 体系，不会出现两套来源重叠的问题。

## Monitoring And Diagnostics

除已有运行日志外，建议统一增加以下维度：

- `tool_attempt_count`
- `tool_success_count`
- `tool_failure_count_by_kind`
- `tool_policy_denial_count`
- `tool_timeout_count`
- `tool_truncation_count`
- `bash_fallback_count`
- `question_block_count`
- `tool_switch_after_failure`
- `same_intent_retry_count`

并增加 per-family 视角：

- inspection stability
- web research stability
- editing stability
- planning stability

## Technical Decisions

| Decision | Choice | Rationale |
| --- | --- | --- |
| Manifest-handler integration | Manifest wraps Handler | 现有 `ToolHandler` trait 签名不变，manifest 作为注册时 envelope；`ToolDispatcher` 持有 manifest map |
| Tool metadata ownership | Add explicit manifest layer | 避免 prompt、UI、permission 逻辑散落在 handler 中 |
| Profile requirement level | Core + Prompt required, others Optional | 避免 readonly 工具填写大量空 profile；渐进增强 |
| Prompt strategy granularity | Global + family + step + tool | 提升稳定性并便于缓存/测试 |
| Result contract direction | Keep `ToolResult`, add runtime outcome extensions | 不破坏已有 contract，同时补足主流程推进能力 |
| Remediation modeling | Structured `ToolRemediation` type | 让主流程能稳定消费 remediation（alternative tools、recoverable），而非自由文本 |
| UI coupling style | Extend `RuntimeUiEffect` enum | 复用已有 enum 而非新建并行体系，避免 TUI 消费两套来源 |
| ToolHandler trait stability | 不改变 execute_v2 签名 | 新工具通过构造注入 `Arc<dyn ContextProvider>` 获取富上下文，不在 trait 层传大 context struct |
| Context exposure | `ToolContextProfile` 为 request descriptor，交付由 `OmegaContextFacade` 履行 | 与 omega-context 的 facade boundary rule 对齐 |
| Permission layering | `StepToolRequest`(visibility) > `ToolPolicyConfig`(enablement) > `RuntimeApproval`(approval) | 三层递进，前两层直接映射现有实现，第三层为新增 |
| Storage side effects | Deferred — `Option<ToolStorageProfile>` placeholder | 等 omega-memory/omega-document API 稳定后再落地完整 effect model |
| Monitoring source | Runtime diagnostics + tracing + metrics | 复用现有 observability 基础 |

## Workstreams

### Workstream A: Tool Capability Manifest

#### Task A1: Add Tool Manifest Layer

- **Type**: Design + Implementation
- **Complexity**: L
- **Dependencies**: None
- **Description**: 在现有 tool definitions 之上增加 manifest layer，承载 prompt/io/ui/context/permissions/storage/observability metadata。
- **Deliverable**: manifest model、builder、compat adapter。

#### Task A2: Migrate Core Built-In Tools To Manifests

- **Type**: Implementation
- **Complexity**: M
- **Dependencies**: Task A1
- **Description**: 首轮迁移 `list_dir`、`glob_search`、`grep_search`、`read_file`、`apply_patch`、`create_file`、`edit_file`、`batch`、`bash`。
- **Deliverable**: 核心工具 capability profile 初版。

### Workstream B: Prompt Strategy And Remediation

#### Task B0: Lightweight Tool Strategy Prompt (Quick Win)

- **Type**: Implementation
- **Complexity**: S
- **Dependencies**: None
- **Description**: 基于现有 `ToolDefinition` 和 `SessionToolCatalog`，把当前 prompt 中的 `"Visible tools: x, y, z"` 升级为包含 family-level guidance 和 when_to_use/when_not_to_use 的简洁 prompt block。不依赖 manifest，直接硬编码现有 11 个工具的策略。
- **Deliverable**: 升级后的 `render_visible_tools()`、工具选择质量的即时改善。

#### Task B1: Add Global/Family/Step/Tool Prompt Sections

- **Type**: Design + Implementation
- **Complexity**: M
- **Dependencies**: Task A1
- **Description**: 基于 manifest 的 `ToolPromptProfile`，把工具相关 prompt 分层并纳入 prompt assembly。替代 B0 的硬编码版。
- **Deliverable**: tool prompt builder、prompt asset layout、tests。

#### Task B2: Add Remediation Builder

- **Type**: Implementation
- **Complexity**: M
- **Dependencies**: Task A1
- **Description**: 基于 `ToolErrorKind` 和 tool manifest 生成结构化 `ToolRemediation`（kind + suggestion + alternative_tools + recoverable）。初版可基于 `ToolErrorKind` match 硬编码，后续基于 manifest 的 `ToolPromptProfile.fallback_to` 泛化。
- **Deliverable**: `ToolRemediation` 类型、remediation builder、runtime glue、tests。

### Workstream C: New High-Value Tools

#### Task C1: Web Research Tools

- **Type**: Design + Implementation
- **Complexity**: L
- **Dependencies**: Task A1, Task B1
- **Description**: 新增 `web_search` 与 `web_fetch`，补齐 network policy、UI、cache、observability 设计。

#### Task C2: Planning/Interaction Tools

- **Type**: Design + Implementation
- **Complexity**: L
- **Dependencies**: Task A1, Task B1
- **Description**: 新增或正式化 `todo_write`、`todo_read`、`ask_user_question`、`task`。

#### Task C3: FileEdit Surface Consolidation

- **Type**: Implementation
- **Complexity**: M
- **Dependencies**: Task A2, Task B2
- **Description**: 收口 `apply_patch` / `edit_file` / `create_file` 的 guidance、diff UI、write permission 与 storage effects，使其对模型呈现为统一 FileEdit 家族。

### Workstream D: UI, Context, Permission, Storage Bridges

#### Task D1: Add Declarative Tool UI Effects

- **Type**: Design + Implementation
- **Complexity**: M
- **Dependencies**: Task A1
- **Description**: 为工具结果增加显式 UI effects，并接入 TUI。新增的 effect 应作为 `RuntimeUiEffect` enum 的新 variant，不另建独立体系。

#### Task D2: Add Scoped ToolExecutionContext

- **Type**: Design + Implementation
- **Complexity**: M
- **Dependencies**: Task A1
- **Description**: 将工具上下文从隐式环境提升为显式 bridge。不改变 `ToolHandler` trait 签名，通过构造注入和 dispatcher 层提供上下文。实际交付由 `OmegaContextFacade` 统一履行。

#### Task D3: Add Tool Permission Profiles

- **Type**: Design + Implementation
- **Complexity**: M
- **Dependencies**: Task A1
- **Description**: 为每个工具定义 permission class、approval mode 与 denial remediation。三层模型：`StepToolRequest`(visibility) > `ToolPolicyConfig`(enablement) > `RuntimeApproval`(approval)。

#### Task D4: Add Tool Storage Effects

- **Type**: Design + Implementation
- **Complexity**: M
- **Dependencies**: Task A1, omega-context facade API 稳定
- **Description**: 把工具副作用统一映射到 todo/memory/artifact/journal/cache。应等 `omega-memory` 和 `omega-document` 的 facade API 稳定后再落地，当前只预留 `Option<ToolStorageProfile>` 占位。

### Workstream E: Monitoring And Diagnostics

#### Task E1: Add Tool Capability Metrics

- **Type**: Implementation
- **Complexity**: M
- **Dependencies**: Task A2, Task B2
- **Description**: 增加 per-tool/per-family metrics 和 diagnostics。

#### Task E2: Add Tool Stability Regression Matrix

- **Type**: Testing
- **Complexity**: M
- **Dependencies**: Task C1, Task C2, Task D1, Task D2, Task D3, Task D4
- **Description**: 建立“工具是否帮助主流程稳定推进”的回归矩阵。

## Relationship To Existing Tool Tasks

与现有 `docs/specs/omega-tool-system-upgrade.md` 的关系如下：

- `8C Tool Contract V2`：作为本规格的结果契约基础，继续保留。
- `8D Structured Workspace Inspection Tools`：对应本规格的 Workspace Inspection family。
- `8E Patch-Centric Editing Toolset`：对应本规格的 FileEdit family 基础。
- `8F Batch Read-Only Tool`：继续作为 inspection orchestration 工具。
- `8G Bash V2`：继续作为 escape hatch 强约束工具。
- `8H Tool Policy Surface`：应被扩展为 permission + context + storage bridge 的底座，而不只是 enable/disable config。

本规格是在 `omega-tool-system-upgrade.md` 之上增加“完整工具系统能力面”的上层设计，不替代该文档的基础升级方向。

## Implementation Order

推荐顺序（含并行 early-win track）：

```text
Quick-win track (no manifest dependency):
  B0 (Lightweight Tool Strategy Prompt)
  B2-lite (基于 ToolErrorKind match 的初版 remediation)

Manifest track:
  A1 -> A2
  B1 (manifest-based prompt, 替代 B0)
  B2 (manifest-based remediation, 替代 B2-lite)
  D1 -> D2 -> D3
  C3
  C1 -> C2
  E1 -> E2

Deferred (等存储 API 稳定):
  D4
```

排序理由：

- B0 和 B2-lite 可与 A1 并行进行，无需等待 manifest 就能立即提升工具选择质量。
- 没有 manifest，就没有统一 capability framework。
- 没有 prompt/remediation，工具还是难以稳定选对。
- 没有 UI/context/permission bridges，新增工具仍会重复造轮子。
- D4（Storage Effects）降优先级，等 omega-context facade API 稳定后再落地。
- FileEdit 家族应先收口，因为本地编辑仍是 Omega 高频路径。
- Web/Ask/Todo 这类更产品化工具应建立在统一桥接层之上。

## Testing Strategy

### Unit Tests

- manifest 构造与序列化
- prompt builder
- remediation builder
- tool context scoping
- permission profile mapping
- storage effects mapping

### Integration Tests

- inspection 场景优先选结构化只读工具而不是 `bash`
- editing 场景优先选 FileEdit family
- web research 场景从 `web_search` 正确切到 `web_fetch`
- permission denied 后主流程能稳定切换到 `ask_user_question` 或替代工具
- `todo_write` 能正确驱动 UI 与 store

### Runtime/UI Tests

- tool UI effects 正确映射到 overlay/sidebar/notice
- `ask_user_question` 能稳定暂停并恢复主流程
- diff preview / web result view / todo updates 都有回归覆盖

### Diagnostics Regression

- `bash_fallback_count` 在典型 inspection flow 中下降
- `same_intent_retry_count` 在 tool guidance 引入后下降
- `tool_switch_after_failure` 在 remediation 引入后上升

## Security Considerations

- manifest metadata 不能绕开现有 step visibility 或 workspace safety。
- `ToolExecutionContext` 必须做最小暴露，而不是把全部 session state 直接下发给工具。
- `web_search` / `web_fetch` 必须纳入 network policy，不应默认放开外网访问。
- `ask_user_question` 的回答应作为结构化输入对待，避免未校验地回流主流程。
- storage effects 不能在未记录审计的情况下静默修改 todo、memory 或 artifacts。

## Performance Requirements

- tool prompt builder 应尽量复用 cacheable blocks，避免每轮大幅增加 token。
- per-tool monitoring 不应显著增加 runtime message volume。
- UI effects 应声明式、小 payload，避免把大量正文复制进 ToolRun。
- web tools 必须有明确超时和内容截断策略。

## Success Criteria

满足以下条件时，可认为本轮工具系统升级成功：

1. 新增工具不再需要重复单独设计 prompt、UI、permission、storage、monitoring。
2. 模型在 inspection、editing、todo、web research 场景中更稳定地选择正确工具。
3. tool failure 后主流程更容易拿到稳定的下一步行动建议。
4. UI 能明确展示工具的运行、结果和后续交互，而不是靠自由文本猜测。
5. 工具与 context、permission、storage、monitoring 的关系变成显式 contract。
6. 现有 step contract、`ToolResult`、`ToolRun` 与 runtime diagnostics 不退化。

## Related Documents

- `docs/specs/omega-tool-system-upgrade.md`
- `docs/specs/omega-context-management.md`
- `docs/specs/omega-workflow-package.md`
- `docs/specs/omega-runtime-message-pipeline.md`
- `learn/claude-code-source-code-vs-omega-analysis.md`
- `learn/omega-learning-roadmap-from-claude-code.md`

## Change Log

- 2026-04-01 (v0.3): 基于架构审查修正 11 项问题：明确 Manifest-wraps-Handler 集成方案、声明不改变 ToolHandler trait 签名、ToolContextProfile 改为声明式 request descriptor 并与 OmegaContextFacade 对齐、remediation 从 Option<String> 改为结构化 ToolRemediation 类型、UI effects 复用 RuntimeUiEffect enum 不另建体系、Permission 三层模型明确映射 StepToolRequest/ToolPolicyConfig/RuntimeApproval、九类 profile 只有 Core+Prompt 必填其余 Optional、补充 ToolIoProfile 字段定义、补充 web tools 基础设施开放问题、Storage Effects 降优先级等存储 API 稳定、新增 B0 轻量 prompt 任务允许与 manifest 并行推进。
- 2026-04-01 (v0.2): 扩展为完整 tool system blueprint，新增九类统一能力框架、tool manifest 层、WebSearch/WebFetch/TodoWrite/FileEdit/AskUserQuestion/BashTool 蓝图，以及 UI/context/permission/storage/monitoring 联动设计。
- 2026-04-01 (v0.1): 初版，聚焦 tool guidance、tool-family prompt layering、tool misuse remediation 与 selection diagnostics，目标是在不削弱现有契约的前提下提升 Omega 的 tool 使用质量与稳定性。
