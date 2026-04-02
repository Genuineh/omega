---
status: active
owner: omega-team
created: 2026-03-18
updated: 2026-04-02
audience: developers
level: intermediate
---

# Omega 开发指南

## Overview

本指南用于快速进入当前可运行的 Omega 工作区。它聚焦当前真实主路径、常用命令、配置入口和文档导航，不重复展开已经迁入规格文档的设计细节。

阅读完本指南后，你应能完成四件事：
- 找到当前应阅读的文档入口
- 构建、测试并运行当前应用
- 理解主路径上的 crate 分层
- 知道配置、日志和排障入口在哪里

## Prerequisites

- Rust toolchain 已安装并可运行 `cargo`
- 了解 Rust 基础语法
- 熟悉命令行操作
- 若需连真实 provider，准备好对应 API key

## Getting Started

### Step 1: 构建工作区

```bash
cargo build
```

### Step 2: 运行测试

```bash
cargo test
```

### Step 3: 启动当前应用入口

```bash
cargo run -p omega-app
```

### Step 4: 进入文档入口

先读以下文档，再进入具体模块：
- `docs/TODO.md`
- `docs/README.md`
- `docs/specs/omega-agent-spec.md`
- `docs/specs/omega-agent-impl-plan.md`

## 当前工作区结构

当前 workspace 有 25 个 crate。日常开发不需要记住所有 crate 的细枝末节，先按主路径和支撑层理解即可。

### 主运行路径

| Crate | 当前职责 |
|-------|----------|
| `omega-app` | 唯一应用入口，负责 bootstrap、config、provider wiring 和 runtime policy |
| `omega-session` | 会话运行态编排、workflow turn 驱动、runtime-visible 更新归一 |
| `omega-core` | agent loop、工具分发和底层执行模型 |
| `omega-workflow` | workflow 定义、step 配置与运行策略 |
| `omega-tui` | 纯 UI shell、输入/渲染/事件循环 |
| `omega-observability` | tracing bootstrap、日志 sink 和文件日志策略 |
| `omega-client` | provider transport、Anthropic-compatible 抽象与 streaming |

### 上下文与内容层

| Crate | 当前职责 |
|-------|----------|
| `omega-context` | 对外统一上下文 facade |
| `omega-memory` | 会话记忆、summary ranking 与 compaction |
| `omega-document` | 文件治理、索引、检索与文档工具后端 |
| `omega-todo` | todo 工具与快照模型 |
| `omega-tasks` | 持久化任务层预留 |

### 工具与扩展层

| Crate | 当前职责 |
|-------|----------|
| `omega-tools` | 工具 contract 与 dispatch 边界 |
| `omega-tools-builtin` | 内置 repo inspection / edit / batch / bash 工具 |
| `omega-skills` | skill 目录加载与 prompt 注入 |
| `omega-subagent` | subagent 基础设施 |
| `omega-background` | 后台任务能力预留 |
| `omega-message` | runtime-visible message 能力预留 |
| `omega-team` | team / delegation 能力预留 |
| `omega-worktree` | 隔离执行与 worktree 能力预留 |

### 支撑与 UI 配套层

| Crate | 当前职责 |
|-------|----------|
| `omega-theme` | TUI 主题与视觉令牌 |
| `omega-keymap` | keymap 与 `.omega` 键位配置 |
| `omega-hooks` | step lifecycle hook 相关能力 |
| `omega-compression` | 历史压缩相关能力预留 |
| `omega-test-support` | 脚本化测试支撑 |

## 常见任务

### 运行当前入口

```bash
cargo run -p omega-app
```

### 运行定向测试

```bash
cargo test -p omega-client
cargo test -p omega-session
cargo test -p omega-tui
```

### 格式化与静态检查

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
```

### 查看当前优先级与文档入口

```bash
sed -n '1,120p' docs/TODO.md
sed -n '1,160p' docs/README.md
```

## 配置入口

| 文件 | 用途 |
|------|------|
| `.omega/model.toml` | provider、节流与模型相关配置 |
| `.omega/env.toml` | 仓库级环境变量注入 |
| `.omega/workflows/root.toml` | root workflow 路由配置 |
| `.omega/workflows/*.toml` | child workflow 定义 |
| `.omega/theme.toml` | TUI 主题覆盖 |

## 当前架构约束

- `omega-app` 拥有应用 bootstrap 与 runtime message policy。
- `omega-session` 是 TUI shell 与 agent runtime 之间的编排边界。
- `omega-tui` 只负责 UI，不承载会话编排或 observability bootstrap。
- 上下文主路径通过 `omega-context` 暴露；上层不应直接绕过它去依赖 `omega-memory` 或 `omega-document`。
- 当前 runtime 主链路以 `docs/specs/omega-runtime-message-pipeline.md` 为准；`RuntimeUiEnvelope` 只保留 compat baseline。

## 日志与排障

Omega 使用 `tracing` 输出两类日志：
- TUI 内的 `Activity & Logs` 面板
- `~/.omega/logs/omega-YYYY-MM-DD.jsonl` 下的 JSONL 文件

常用环境变量：

| 变量 | 说明 | 默认值 |
|------|------|--------|
| `OMEGA_LOG` | 日志级别 (`trace/debug/info/warn/error`) | `info` |
| `OMEGA_LOG_DIR` | JSONL 日志目录 | `~/.omega/logs` |
| `OMEGA_LOG_FILE` | 是否启用文件日志 | `true` |

常见排障动作：
- provider 交互异常时先用 `OMEGA_LOG=debug cargo run -p omega-app`
- 如果怀疑节流或 429，检查 `.omega/model.toml` 的 `[provider]` 覆盖项
- 如果 TUI 视图异常，先确认当前文档基线是否与 `docs/TODO.md` 和相关 spec 一致

## Troubleshooting

### 编译错误

**Problem**: 编译失败
**Solution**:
```bash
cargo build
```

### 运行时错误

**Problem**: API Key 未设置
**Solution**:
```bash
export OMEGA_API_KEY="your-api-key"
# 或者
export OMEGA_MINIMAX_API_KEY="your-api-key"
```

### 文档入口看起来互相冲突

**Problem**: 同一主题出现多份文档，不知道该信哪一份
**Solution**: 先看 `docs/README.md` 的分组导航；如果文档位于 `docs/archive/`，默认视为历史材料，不作为 active source of truth

## Best Practices

- 保持 crate 边界清晰，优先修正文档入口和 contract 再扩功能
- 修改实现时同步更新 `docs/TODO.md` 与相关 spec
- 默认把 `docs/specs/` 当作 source of truth，把 `docs/archive/` 当作历史背景
- 新行为要补测试，至少覆盖当前改动所经过的主路径

## Related Topics

- 技术规格文档: docs/specs/omega-agent-spec.md
- ADR 文档: docs/decisions/
- TODO 跟踪: docs/TODO.md
