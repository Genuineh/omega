---
status: draft
owner: omega-team
created: 2026-03-19
updated: 2026-03-20
version: 1.1
supersedes: []
related_prds:
  - docs/prds/observability-logging.md
---

# Omega TUI 非 UI 职责剥离规格

> Status note (2026-03-20): `Task 15D` 的首轮目标已完成，`omega-session` 与 `omega-observability` 已落地，应用入口也已迁到 `omega-app`。本文件保留为该次剥离的设计基线；文中涉及 `omega-repl` 的表述仅保留为阶段性历史。后续活跃规划以 `docs/specs/omega-runtime-ui-message-contract.md` 与 `docs/specs/omega-app-package.md` 为主。

## Overview

`Task 15C` 已经把 REPL 从 `omega-tui` 的入口职责中分离出去，但当前 `omega-tui` 仍然同时拥有四类责任：terminal/ratatui 渲染、交互状态管理、Agent turn 编排、tracing 初始化与日志路由。这个边界对当前里程碑足够可用，但已经开始违反 `001` 中的独立 crate 原则，也与 `005` 中“可观察性是跨前端基础设施”的定位不一致。

本规格定义 `Task 15C` 之后的第二阶段交互层整理方案：把 `omega-tui` 中不属于 UI 的部分继续抽成独立 crate，其中第一优先级是 `logging` 和 `agent_session`，第二优先级是从 `app` 与 `event` 中继续抽出前端无关的交互状态与命令分发。

## Goals

- 让 `omega-tui` 只保留终端 UI 直接相关的职责。
- 将 `agent_session` 抽为可复用的会话编排 crate，优先服务 TUI 主路径，并为未来其他前端保留复用可能。
- 将 tracing 初始化、ANSI 清洗、文件日志策略从 `omega-tui` 中抽离，建立前端共享的可观察性 crate。
- 为 `app` / `event` 的后续继续拆分给出明确边界，避免“先都留着，之后再说”。
- 保持 `omega-core` 前端无关，不把面板、鼠标、键位或颜色语义回灌到核心层。

## Non-Goals

- 不在本次规划中重写 `omega-core::Agent` 的运行模型。
- 不在本规格中直接实现新的 launcher 模式；`omega-app` 仅作为单一应用装配入口，而不是多模式前端框架。
- 不为了“每个模块都成 crate”而把 `render`、`terminal` 这类纯 TUI 模块强行拆出去。
- 不在本次文档中承诺 Web UI、GUI 或 MCP 前端的实现。

## Architecture Assessment

### 当前评分

按仓库现有架构原则评估，当前 `omega-tui` 的交互层边界约为 `6/10`：

| 维度 | 评分 | 说明 |
|------|------|------|
| 单一职责 | 1 | `omega-tui` 同时拥有 UI、运行时、session orchestration、logging bootstrap |
| 关注点分离 | 1 | `app.rs` 与 `event.rs` 混合 UI 状态和前端无关状态 |
| 可扩展性 | 1 | REPL 之外的新前端无法直接复用 `agent_session` / tracing bridge |
| 可测试性 | 1 | `AgentSession` 可单测，但被埋在 TUI crate 中，边界表达不清 |
| 可维护性 | 2 | 当前实现还能工作，但继续堆功能会放大 shotgun surgery 风险 |

### 当前红旗

- `agent_session.rs` 没有任何 ratatui/crossterm 依赖，却挂在 `omega-tui` 内部。
- `logging.rs` 实际上实现的是前端共享的 tracing 初始化策略，而不是 TUI widget。
- `app.rs` 同时保存 `ListState` / `Rect` 这类 TUI 状态和 turn 生命周期、输出缓冲、输入编辑这类交互状态。
- `event.rs` 同时承担 terminal 事件适配和用户命令语义分发，后者不应依赖 crossterm。

## Current Module Disposition

下表中的 `src/agent_session.rs` 与 `src/logging.rs` 指的是 Task 15D 开始前的历史位置，用于解释为什么要拆分；当前实际代码已经分别迁入 `omega-session` 与 `omega-observability`。

| File | 当前责任 | 判定 | 处理策略 |
|------|----------|------|----------|
| `src/render.rs` | ratatui 布局、样式、文本 wrap | 保留在 `omega-tui` | 纯 UI |
| `src/terminal.rs` | raw mode、alternate screen 生命周期 | 保留在 `omega-tui` | 纯 terminal UI 基础设施 |
| `src/runtime.rs` | TUI 主循环、帧刷新、通道消费 | 保留在 `omega-tui` | 仍然是 TUI 运行时壳层 |
| `src/agent_session.rs` | Agent turn 编排、中断恢复、后台线程衔接 | 必须剥离 | 新 crate `omega-session` |
| `src/logging.rs` | tracing subscriber、UI sink、文件日志、ANSI 清洗 | 必须剥离 | 新 crate `omega-observability` |
| `src/app.rs` | TUI 状态 + 交互状态混合 | 拆分后保留部分 | 引入 `omega-interaction` 后仅保留 TUI 专属字段 |
| `src/event.rs` | crossterm 事件适配 + 命令语义 | 拆分后保留部分 | UI 层保留事件解码，命令语义下沉到 `omega-interaction` |
| `src/main.rs` | 前端装配 | 迁出 `omega-tui` | 后续迁往 `omega-app`，避免 UI crate 持有应用入口 |

## Target Architecture

### Phase 1 目标分层

```text
omega-core
  ↑
omega-session
  ↑
omega-tui

omega-observability
  ↑
omega-tui
```

Phase 1 的目标不是一次性把所有交互状态都清空，而是先把“明显不属于 UI 的部分”迁走，优先打掉最强耦合点。

### Post-Phase 1 Packaging Target

```text
omega-core
  ↑
omega-session
  ↑
omega-app
  ↓
omega-tui

omega-observability
  ↑
omega-app
```

在 `omega-session` 与 `omega-observability` 已经落地之后，下一步不应再让 `omega-tui` 保留应用装配入口，而应由 `omega-app` 接手 bootstrap 与 wiring。

### Phase 2 目标分层

```text
omega-core
  ↑
omega-session
  ↑
omega-interaction
  ↑
omega-tui

omega-observability
  ↑          ↑
omega-tui  future-frontend?
```

Phase 2 只在以下条件成立时推进：

- `app.rs` / `event.rs` 在继续迭代高级 TUI 能力时再次出现职责膨胀。
- 若未来重新引入第二种前端，并且它需要共享 turn 状态、输入缓冲、更新协议，而不是只共享 `AgentSession`。

## New Crate Design

### 1. `omega-session`

#### Responsibility

`omega-session` 负责“前端如何驱动一次 agent turn”，而不是“如何把状态画到终端上”。它从 `omega-tui/src/agent_session.rs` 演进而来，当前首先服务于 `omega-tui` 这条 richer shell 路径；未来若 REPL 或其他前端需要 workflow-aware orchestration，再复用同一层。

#### Must Own

- `AgentSession` 生命周期
- 当前 turn id 与 checkpoint 管理
- 中断后的 agent 恢复
- 后台线程与 `tokio::runtime::Handle` 的桥接
- turn 期间向前端广播的更新协议

#### Must Not Own

- 面板焦点、滚动状态、颜色主题
- crossterm / ratatui 类型
- tracing subscriber 初始化

#### Public API

```rust
pub struct AgentSessionConfig {
    pub client: DynLlmClient,
    pub system: String,
    pub cwd: PathBuf,
    pub runtime_handle: Handle,
}

pub enum SessionUpdate {
    ToolCallPreview {
        turn_id: u64,
        command: Option<String>,
        preview: String,
    },
    AssistantText {
        turn_id: u64,
        text: String,
    },
    TurnFinished {
        turn_id: u64,
    },
}

pub struct AgentSession { /* private fields */ }

impl AgentSession {
    pub fn new(config: AgentSessionConfig) -> anyhow::Result<Self>;
    pub fn is_ready(&self) -> bool;
    pub fn checkpoint_current_messages(&self);
    pub fn interrupt(&self, replacement_turn_id: u64) -> anyhow::Result<()>;
    pub fn spawn_turn(
        &self,
        input: String,
        turn_id: u64,
        tx: mpsc::Sender<SessionUpdate>,
    ) -> anyhow::Result<()>;
}
```

#### Design Notes

- 当前 `LogUpdate` 命名过窄，因为其中同时承载工具命令预览和 assistant 文本。迁移时统一重命名为 `SessionUpdate`。
- `preview_text()` 一并迁入 `omega-session`，因为它服务于 turn 更新协议，而不是 UI。
- `ToolCallPreview` 使用 `command: Option<String>` 而不是硬编码只认 `bash`，这样后续前端可以决定如何展示不同工具。

#### Dependencies

- 直接依赖：`omega-core`, `tokio`, `tracing`, `anyhow`
- 不依赖：`ratatui`, `crossterm`, `tracing-subscriber`, `chrono`, `dirs`

### 2. `omega-observability`

#### Responsibility

`omega-observability` 承接 `005` 的实现载体，负责 subscriber 初始化、行式 UI sink、文件日志策略和 ANSI 清洗。它不是 TUI crate 的内部模块，而是所有前端共享的基础设施。

#### Must Own

- `init_tracing` 与环境变量读取
- UI sink writer 封装
- 文件日志目录与日期滚动策略
- `strip_ansi()` 这类输出清洗工具

#### Must Not Own

- `App`、面板、widget 或 terminal 生命周期
- `omega-core::Agent` 运行逻辑
- 特定前端的标题文案或颜色方案

#### Public API

```rust
pub struct TracingConfig {
    pub env_filter: Option<String>,
    pub log_dir: Option<PathBuf>,
    pub enable_file_log: bool,
    pub human_readable_sink: Option<mpsc::SyncSender<String>>,
}

pub fn init_tracing(config: TracingConfig) -> anyhow::Result<()>;
pub fn init_tracing_channel() -> anyhow::Result<mpsc::Receiver<String>>;
pub fn strip_ansi(text: &str) -> String;
```

#### Design Notes

- `init_tracing_channel()` 继续保留，是为了让 `omega-tui` 和未来其他 line-based frontend 低成本接入。
- `UiWriter` 迁入 `omega-observability` 后，`omega-tui` 只知道自己拿到了一个 `Receiver<String>`，不再负责 writer 细节。
- REPL 是否展示 tracing 行可以后置决定，但不能再被 `omega-tui` 独占。

#### Dependencies

- 直接依赖：`tracing`, `tracing-subscriber`, `chrono`, `dirs`, `anyhow`
- 不依赖：`ratatui`, `crossterm`, `omega-core`

### 3. `omega-interaction`

#### Responsibility

这是第二阶段候选 crate，用于承接当前 `app.rs` 和 `event.rs` 中前端无关的交互状态。它不是 Phase 1 的前置条件，但必须在规划中明确边界，避免后续继续把逻辑堆回 `omega-tui`。

#### Candidate Scope

- turn 生命周期状态：`active_turn_id`, `is_running`
- 输出缓冲：`output_msgs`, `log_lines`
- 输入编辑状态：`input_buffer`, `cursor_pos`
- 用户命令语义：`Submit`, `Interrupt`, `Quit`
- `SessionUpdate -> InteractionState` 的归并逻辑

#### Keep In `omega-tui`

- `ListState`, `Rect`, 面板焦点
- 鼠标命中测试
- 滚动 pinning 与渲染后行数
- crossterm `Event -> UserCommand` 的适配

#### Trigger Conditions

满足以下任一条件时，应启动该 crate 的实现：

- `app.rs` 再次同时新增 UI 字段和交互逻辑字段
- 新的第二前端开始需要共享输入编辑或 turn 状态
- TUI 测试必须频繁构造 `ListState` 才能验证纯交互逻辑

## `omega-tui` After Extraction

Phase 1 完成且入口迁移完成后，`omega-tui` 应只保留以下责任：

- ratatui 渲染
- crossterm 事件读取和 terminal 生命周期
- TUI 专属状态，例如面板焦点、滚动位置、视图布局缓存
- 将 `SessionUpdate` 和 tracing 行映射到 TUI 展示模型
- 消费由 `omega-app` 注入的日志/会话更新通道

对应地，`omega-tui` 不再包含：

- `AgentSession` 的定义
- tracing subscriber 配置细节
- ANSI 清洗工具
- 应用 `main` 入口

## Migration Plan

### Phase 1A: 先剥离 `omega-session`

1. 新建 `crates/omega-session`
2. 迁移 `agent_session.rs` 和现有单测
3. 将 `LogUpdate` 重命名为 `SessionUpdate`
4. `omega-tui` 改为依赖 `omega-session`
5. 验证 `cargo test -p omega-session` 与 `cargo test -p omega-tui`

### Phase 1B: 再剥离 `omega-observability`

1. 新建 `crates/omega-observability`
2. 迁移 `logging.rs` 实现
3. `omega-tui` 以及未来可能的第二前端均改用新 crate 初始化 tracing
4. 保持 `OMEGA_LOG`, `OMEGA_LOG_DIR`, `OMEGA_LOG_FILE` 行为不变
5. 验证日志面板与 JSONL 文件输出不回归

### Phase 2: 视复杂度引入 `omega-interaction`

1. 从 `app.rs` 提炼非 ratatui 状态对象
2. 从 `event.rs` 提炼命令语义与提交/中断流程
3. TUI 保留事件适配和视图状态
4. 如未来再次出现第二种前端，再评估是否接入同一交互边界

## Current Usage Baseline

为避免本规格继续被误读成“REPL 已经走 session 主路径”，当前事实基线明确如下：

- `omega-tui -> omega-session -> omega-core` 是当前唯一的用户交互主路径。
- `omega-session` 当前应被视为 TUI shell 与 agent runtime 之间的编排边界。
- 若未来重新引入第二种前端，应视为新的架构收敛任务，而不是当前已存在的前提。

## Dependency Rules

- `omega-session` 只能向下依赖 `omega-core`，不能反向依赖 `omega-tui`。
- `omega-observability` 不允许依赖任何前端 crate。
- `omega-tui` 可以依赖 `omega-session` 与 `omega-observability`，但不得重新声明其内部实现。
- `omega-core` 不得新增任何 TUI 面板、键位、spinner 或日志面板概念。

## Testing Strategy

- `omega-session`: 保留当前 checkpoint、中断恢复、UTF-8 preview 单测。
- `omega-observability`: 为 ANSI 清洗、文件路径解析、禁用文件日志分支补单测。
- `omega-tui`: 保留 stale update 过滤、正在运行时拒绝重复提交、渲染辅助测试。
- smoke checks: `cargo fmt --all`, `cargo build`, `cargo test`。

## Risks

| Risk | Level | Mitigation |
|------|-------|------------|
| `SessionUpdate` 改名导致前端适配遗漏 | Medium | 先迁移单测，再做 import 替换 |
| tracing 初始化迁出后出现重复 subscriber | Medium | `omega-observability` 统一保留 `try_init()` 语义 |
| 过早引入 `omega-interaction` 导致抽象过度 | Medium | 明确它是 Phase 2 条件性任务，而不是 Phase 1 前置 |
| 继续把新逻辑写回 `omega-tui` | High | 在 TODO 中把 Phase 1A/1B 设为高优先级阻断项 |

## Acceptance Criteria

- `agent_session` 不再位于 `omega-tui` crate 内。
- tracing 初始化与 ANSI 清洗不再位于 `omega-tui` crate 内。
- `omega-tui` 仅保留 UI 相关模块和薄装配逻辑。
- 至少完成 `omega-session` 和 `omega-observability` 两个新 crate 的边界设计与迁移顺序定义。
- `omega-interaction` 的触发条件和保留在 `omega-tui` 的字段范围有明确说明。

---

### Change Log

- 2026-03-19: 初版规格，定义 `omega-session`、`omega-observability` 和候选 `omega-interaction` 的职责边界与迁移顺序。
- 2026-03-20: v1.1 — 补充 Task 15D 完成后的状态说明，明确本文件保留为历史设计基线；后续交互入口已进一步收敛为 `omega-tui -> omega-session -> omega-core` 单一路径。
- 2026-03-20: 补充 `omega-app` 目标装配层，明确 `omega-tui` 的 `main` 应迁出 UI crate。