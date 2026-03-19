---
status: active
owner: omega-team
created: 2026-03-18
updated: 2026-03-19
version: 1.0
related_prds: []
---

# Omega Agent 技术规格

## Overview

Omega 是一个用 Rust + ratatui 实现的 AI Agent，完整复刻 learn-claude-code 的 12 阶段教程。每个阶段对应一个独立 crate，通过依赖组合构建完整系统。核心原则：单一职责、可组合、接口清晰。`omega-client` 不绑定单一厂商实现，而是提供统一消息接口与 provider 适配层，首个 provider 为 Minimax。

## Goals

- 复刻 learn-claude-code 全部 12 个阶段
- 每个子功能独立 crate，便于测试和复用
- 提供 TUI 界面交互
- 保持与 Python 原版相同的心智模型

## Non-Goals

- 不实现 MCP 协议完整细节
- 不实现复杂的权限治理系统
- 不包含完整的 hooks 事件总线

## Architecture

### Crate 结构（当前实现）

```
omega/
├── Cargo.toml                    # 工作空间根配置
├── crates/
│   ├── omega-client/            # LLM 抽象接口与 provider 适配器
│   ├── omega-message/            # 消息系统 (s09)
│   ├── omega-tasks/             # 任务系统 (s07)
│   ├── omega-skills/            # Skill 加载 (s05)
│   ├── omega-worktree/          # Worktree 隔离 (s12)
│   ├── omega-tools/             # 工具抽象 (s02)
│   ├── omega-tools-builtin/     # 内置工具实现
│   ├── omega-todo/              # Todo 管理 (s03)
│   ├── omega-subagent/          # 子智能体 (s04)
│   ├── omega-compression/       # 上下文压缩 (s06)
│   ├── omega-background/        # 后台任务 (s08)
│   ├── omega-team/              # 团队协作 (s09-s11)
│   ├── omega-core/              # 核心 agent (组合所有)
│   ├── omega-repl/              # 最小 stdin/stdout REPL
│   └── omega-tui/               # Ratatui TUI 界面库 + 薄 wrapper
```

### 依赖关系图

```
omega-client       (无依赖)
                  ↑
omega-message     <- omega-client
omega-tasks       (无依赖)
omega-skills      (无依赖)
omega-worktree    (无依赖)
omega-tools       <- anyhow + serde + serde_json (测试依赖 omega-client)
omega-tools-builtin <- omega-tools
omega-todo        (无依赖)
omega-subagent    <- omega-client
omega-compression <- omega-client
omega-background  (无依赖)
omega-team        <- omega-message + omega-tasks
omega-core        <- omega-client + omega-tools + omega-todo + omega-subagent
                  + omega-compression + omega-tasks + omega-background + omega-skills
                  + omega-message + omega-team + omega-worktree
omega-repl        <- omega-core
omega-tui         <- omega-core
```

> 交互层说明：`Task 15C` 已完成。当前结构为 `omega-tui` 负责 TUI 库与薄 wrapper，`omega-repl` 独立承接行式 REPL，边界见 [docs/specs/omega-interaction-layer-refactor.md](docs/specs/omega-interaction-layer-refactor.md)。

### 计划中的交互基础设施

`Task 15B-13` 已将后续高级 TUI 输入系统规划为独立 keymap 基础设施：未来会新增 `omega-keymap` crate，用于加载 `.omega/keymap.toml`、定义 `Normal` / `Insert` 模式映射、处理 leader 序列与快捷键条件匹配。该 crate 尚未实现，因此不计入上面的“当前实现”列表，但应作为后续 `omega-tui` 高级交互能力的统一输入边界。

### 数据流

```text
REPL Input -> omega-repl -> omega-core -> omega-client -> LLM API
TUI Events -> omega-tui  -> omega-core -> omega-client -> LLM API
```

## API Specification

### omega-client

#### LlmClient Trait

```rust
#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ClientError>;
    fn provider_name(&self) -> &'static str;
}

pub type DynLlmClient = Arc<dyn LlmClient>;
```

#### 通用消息模型

```rust
pub struct ChatRequest {
    pub system: Option<String>,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
    pub max_tokens: u32,
}

pub struct Message {
    pub role: Role,
    pub content: MessageContent,
}

pub enum MessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

pub struct ChatResponse {
    pub id: String,
    pub model: Option<String>,
    pub content: Vec<ContentBlock>,
    pub stop_reason: Option<String>,
}
```

#### ContentBlock

```rust
pub enum ContentBlock {
    Text { text: String },
    ToolUse { id: String, name: String, input: serde_json::Value },
    ToolResult { tool_use_id: String, content: serde_json::Value, is_error: Option<bool> },
}
```

#### Minimax Provider

```rust
pub struct MinimaxConfig {
    pub api_key: String,
    pub model: String,
    pub base_url: String,
    pub anthropic_version: String,
}

pub struct MinimaxClient {
    http_client: Client,
    config: MinimaxConfig,
}
```

### omega-tools

#### ToolHandler Trait

```rust
pub trait ToolHandler: Send + Sync {
    fn name(&self) -> &str
    fn description(&self) -> &str
    fn input_schema(&self) -> serde_json::Value
    fn execute(&self, input: serde_json::Value) -> Result<String>
}
```

#### ToolDispatcher

```rust
pub struct ToolDispatcher {
    handlers: HashMap<String, Box<dyn ToolHandler>>,
}

impl ToolDispatcher {
    pub fn new() -> Self
    pub fn register(&mut self, handler: Box<dyn ToolHandler>)
    pub fn dispatch(&self, name: &str, input: serde_json::Value) -> Result<String>
    pub fn to_schemas(&self) -> Vec<serde_json::Value>
    pub fn len(&self) -> usize
    pub fn is_empty(&self) -> bool
    pub fn has_tool(&self, name: &str) -> bool
    pub fn tool_names(&self) -> Vec<&str>
}
```

### omega-core

#### Agent

```rust
pub struct Agent {
    client: DynLlmClient,
    messages: Vec<Message>,
    tools: Vec<serde_json::Value>,
    system: String,
}

impl Agent {
    pub fn new(client: DynLlmClient, system: String, tools: Vec<serde_json::Value>) -> Self
    pub fn add_message(&mut self, role: &str, content: &str)
    pub async fn run_loop<F>(&mut self, handler: F) -> Result<()>
    pub fn get_messages(&self) -> &[Message]
}

pub fn create_default_tools(root: &PathBuf) -> ToolDispatcher
```

## Data Models

### omega-tasks

```rust
pub struct Task {
    pub id: u32,
    pub subject: String,
    pub description: String,
    pub status: TaskStatus,
    pub owner: Option<String>,
    pub blocked_by: Vec<u32>,
}

pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
}
```

### omega-todo

```rust
pub struct TodoItem {
    pub content: String,
    pub status: TodoStatus,
    pub active_form: String,
}

pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}
```

### omega-message

```rust
pub struct Message {
    pub msg_type: String,
    pub from: String,
    pub content: String,
    pub timestamp: f64,
}
```

### omega-worktree

```rust
pub struct Worktree {
    pub name: String,
    pub path: String,
    pub branch: String,
    pub task_id: Option<u32>,
    pub status: String,
}
```

### omega-team

```rust
pub struct TeamConfig {
    pub team_name: String,
    pub members: Vec<TeamMember>,
}

pub struct TeamMember {
    pub name: String,
    pub role: String,
    pub status: String,
}
```

### omega-background

```rust
pub struct BackgroundTask {
    pub id: String,
    pub status: TaskStatus,
    pub command: String,
    pub result: Option<String>,
}

pub enum TaskStatus {
    Running,
    Completed,
    Error,
}
```

## Technical Decisions

| 决策 | 选择 | 理由 |
|------|------|------|
| 语言 | Rust | 性能好，类型安全，与原版 Python 对应 |
| TUI 框架 | ratatui | 纯 Rust 实现，无外部依赖 |
| 异步 | tokio | 成熟的异步运行时 |
| HTTP 客户端 | reqwest | 配合 tokio 的最佳选择 |
| Provider 策略 | 抽象 trait + 适配器 | 避免与单厂商耦合，后续可接入更多 Anthropic-compatible 提供商 |
| 工具系统 | trait 接口 | 便于扩展新工具 |
| 持久化 | 文件系统 | 简单可靠，无需额外依赖 |

## 阶段映射

| 阶段 | 原版文件 | 对应 Crate | 核心概念 |
|------|----------|------------|----------|
| s01 | s01_agent_loop.py | omega-client + omega-core | Agent 循环 + stop_reason |
| s02 | s02_tool_use.py | omega-tools + omega-tools-builtin | 工具注册 + dispatch map |
| s03 | s03_todo_write.py | omega-todo | TodoWrite 规划 |
| s04 | s04_subagent.py | omega-subagent | 子智能体隔离 |
| s05 | s05_skill_loading.py | omega-skills | 按需加载知识 |
| s06 | s06_context_compact.py | omega-compression | 上下文压缩 |
| s07 | s07_task_system.py | omega-tasks | 持久化任务系统 |
| s08 | s08_background_tasks.py | omega-background | 后台任务 |
| s09 | s09_agent_teams.py | omega-message | 消息总线 |
| s10 | s10_team_protocols.py | omega-team | 团队协议 |
| s11 | s11_autonomous_agents.py | omega-team | 自治智能体 |
| s12 | s12_worktree_*.py | omega-worktree | Worktree 隔离 |
| full | s_full.py | omega-core + omega-tui + omega-repl | 完整整合（交互层已拆分为 omega-tui + omega-repl） |

## Security Considerations

- bash 工具执行危险命令过滤
- 文件操作限制在工作目录内
- API Key 通过环境变量传入
- 不存储敏感凭证

## Performance Requirements

- Agent 循环延迟 < 1s (不含 LLM 调用)
- 工具调度开销 < 10ms
- 内存占用 < 100MB (不含 LLM 上下文)
- 支持 1000+ 条消息历史

## Testing Strategy

- 每个 crate 独立单元测试
- omega-core 集成测试
- 工具 handler mock 测试
- TUI 组件渲染测试

## 实现顺序

1. **Task 1**: 工作空间初始化 (Cargo.toml)
2. **Task 2**: omega-client - LLM 客户端
3. **Task 3**: omega-message - 消息系统
4. **Task 4**: omega-tasks - 任务系统
5. **Task 5**: omega-skills - Skill 加载
6. **Task 6**: omega-worktree - Worktree 隔离
7. **Task 7**: omega-tools - 工具抽象
8. **Task 8**: omega-tools-builtin - 内置工具
9. **Task 9**: omega-todo - Todo 管理
10. **Task 10**: omega-subagent - 子智能体
11. **Task 11**: omega-compression - 上下文压缩
12. **Task 12**: omega-background - 后台任务
13. **Task 13**: omega-team - 团队协作
14. **Task 14**: omega-core - 核心 Agent
15. **Task 15**: 交互层（当前为 omega-tui + omega-repl）
