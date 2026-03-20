---
status: active
owner: omega-team
created: 2026-03-18
updated: 2026-03-18
audience: developers
level: intermediate
---

# Omega 开发指南

## Overview

本指南帮助开发者快速上手 Omega 项目的开发。Omega 是一个用 Rust + ratatui 实现的 AI Agent，完整复刻了 learn-claude-code 的 12 阶段教程。

阅读完本指南后，你将能够：
- 理解 Omega 项目的架构设计
- 搭建本地开发环境
- 编译和运行项目
- 理解各个 crate 的职责

## Prerequisites

- Rust 1.70+ 已安装
- 了解 Rust 基础语法
- 熟悉命令行操作
- 了解 AI Agent 基本概念

## Getting Started

### Step 1: 克隆项目

```bash
git clone https://github.com/your-repo/omega.git
cd omega
```

### Step 2: 验证 Rust 环境

```bash
rustc --version
cargo --version
```

### Step 3: 编译项目

```bash
cargo build
```

### Step 4: 运行示例

```bash
# Ratatui TUI
cargo run -p omega-tui
```

## 项目结构

Omega 采用独立 crate workspace。当前工作区已有 18 个 crate，其中 `omega-client`、`omega-tools`、`omega-tools-builtin`、`omega-todo`、`omega-core`、`omega-session`、`omega-workflow` 与 `omega-tui` 已具备可运行基础，其余 crate 按实现计划继续推进。

| Crate | 功能 |
|-------|------|
| omega-client | LLM 抽象接口与厂商适配器 |
| omega-message | 消息系统 |
| omega-tasks | 任务系统 |
| omega-skills | Skill 加载 |
| omega-worktree | Worktree 隔离 |
| omega-tools | 工具抽象 |
| omega-tools-builtin | 内置工具 |
| omega-todo | Todo 管理 |
| omega-subagent | 子智能体 |
| omega-compression | 上下文压缩 |
| omega-background | 后台任务 |
| omega-team | 团队协作 |
| omega-core | 核心 Agent |
| omega-tui | TUI 界面 |

## 常见任务

### 当前可执行任务

1. 运行 `cargo run -p omega-tui` 验证当前唯一用户交互入口
2. 按 `docs/TODO.md` 优先推进 `omega-skills` 与 `omega-subagent`
3. 为新增行为补充测试并保持 `cargo test` 全工作区通过

### 添加新工具

1. 在 `omega-tools-builtin` 中实现 `ToolHandler` trait
2. 在 `omega-core` 中注册工具

### 添加新 Crate

1. 在 `crates/` 下创建新目录
2. 创建 `Cargo.toml`
3. 在根 `Cargo.toml` 中添加成员
4. 定义公共接口

### 运行单个测试

```bash
cargo test -p omega-client
```

### 检查代码格式

```bash
cargo fmt
cargo clippy
```

## Troubleshooting

### 编译错误

**Problem**: 编译失败
**Solution**:
```bash
# 清理并重新编译
cargo clean
cargo build
```

### 依赖冲突

**Problem**: 依赖版本冲突
**Solution**: 检查根 `Cargo.toml` 中的版本定义

### 运行时错误

**Problem**: API Key 未设置
**Solution**:
```bash
export OMEGA_API_KEY="your-api-key"
# 或者
export OMEGA_MINIMAX_API_KEY="your-api-key"
```

## Best Practices

- 保持 crate 职责单一
- 遵循 Rust 命名规范
- 添加必要的注释
- 编写单元测试
- 使用 `cargo clippy` 检查代码

## 日志系统

Omega 使用 `tracing` 框架实现结构化日志。日志路由到两个输出：

- **TUI Logs 面板**：compact 人类可读格式，实时显示在界面右侧面板
- **JSONL 文件**：完整结构化数据，持久化到 `~/.omega/logs/omega-YYYY-MM-DD.jsonl`

日志不再输出到终端控制台（避免干扰 TUI 渲染）。

### 环境变量

| 变量 | 说明 | 默认值 |
|------|------|--------|
| `OMEGA_LOG` | 日志级别过滤 (trace, debug, info, warn, error) | `info` |
| `OMEGA_LOG_DIR` | JSONL 日志文件目录 | `~/.omega/logs` |
| `OMEGA_LOG_FILE` | 是否启用文件日志 (`true`/`false`) | `true` |

### 日志级别

```bash
# 查看所有日志（最详细）
OMEGA_LOG=trace cargo run -p omega-tui

# 查看调试信息（包括工具输入输出）
OMEGA_LOG=debug cargo run -p omega-tui

# 仅查看关键信息
OMEGA_LOG=info cargo run -p omega-tui

# 仅查看错误
OMEGA_LOG=error cargo run -p omega-tui
```

### 日志位置

JSONL 文件存储在 `~/.omega/logs/`，按日期轮转：
```
~/.omega/logs/omega-2026-03-18.jsonl
```

### Span 结构

Omega 定义了四种 span 类型：

| Span | 位置 | 字段 |
|------|------|------|
| `session` | agent_loop 外层 | session_id |
| `agent_loop` | 每次迭代 | iteration, message_count |
| `llm_call` | LLM 调用 | model, max_tokens, duration_ms, input_tokens, output_tokens, stop_reason |
| `tool_exec` | 工具执行 | tool_name, duration_ms, success |

### TUI 多面板布局

```
┌──────────────────────────────────────────────────────────┐
│  Omega Agent │ model-name │ ● Idle │ Focus: Response │
├───────────────────────────────┬──────────────────────────┤
│  Agent Response               │  Activity & Logs        │
│                               │                          │
│  > hello                      │  llm_call.model=...     │
│  I will help you...           │  tool_exec.tool_name=...│
│                               │  [flow 3/4] Execute ... │
│                               │  [tool] $ cat AGENTS.md │
│                               │  [tool] # Repository... │
│                               │                          │
├───────────────────────────────┴──────────────────────────┤
│  Input: > _                                            │
└──────────────────────────────────────────────────────────┘
```

- **Agent Response 面板**：用户输入、各 workflow step 的文本结果与最终 assistant 回复
- **Activity & Logs 面板**：workflow phase、tool preview、todo 刷新与 tracing 日志行，不承载 step 的正文结果
- **Input 区域**：用户输入，回车发送

### TUI 快捷键

| 按键 | 行为 |
|------|------|
| `Enter` | 发送输入 |
| `↑` / `↓` | 滚动当前焦点面板（3 行/次） |
| `Tab` | 切换 Response ↔ Logs 面板焦点 |
| 鼠标滚轮 | 滚动对应面板（根据光标列位置判断） |
| `Ctrl+C` | 中断当前正在运行的 agent turn |
| `Ctrl+Q` | 退出程序 |
| `q` / `exit` | 回车后退出 |

### 在代码中使用日志

```rust
use tracing::{info, instrument};

// 在函数上添加 span
#[instrument(skip(self, request), fields(llm_call.model = %model))]
async fn chat(&self, request: Request) -> Result<Response> {
    // 记录事件
    info!(llm_call.started = true);

    // 记录动态字段
    tracing::Span::current().record("llm_call.duration_ms", duration_ms);
}
```

### 禁止事项

- 禁止使用 `println!` / `eprintln!`（调试输出除外）
- 禁止使用 `log` crate，应使用 `tracing`

## Related Topics

- 技术规格文档: docs/specs/omega-agent-spec.md
- ADR 文档: docs/decisions/
- TODO 跟踪: docs/TODO.md
