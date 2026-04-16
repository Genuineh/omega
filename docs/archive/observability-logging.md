---
archived: 2026-04-02
content_revision: 101
created: 2026-03-18
generation_id: gen_000015_r000101
owner: omega-team
projection_version: 15
related_prds: []
source_doc_id: "archive:docs-archive-observability-logging"
status: archived
superseded_by:
  - docs/decisions/005-tracing-observability.md
  - docs/guide/omega-dev-guide.md
supersedes: []
updated: 2026-04-02
---

# 可观察性与日志系统

## Overview

> **Archived 2026-04-02**: This PRD was completed and no longer drives active work. Keep it for the original rollout rationale and task breakdown. Use `docs/decisions/005-tracing-observability.md` for the durable decision and `docs/guide/omega-dev-guide.md` for the current operator-facing logging workflow.

## Summary

为 Omega Agent 建立基于 `tracing` 的结构化日志与可观察性基础设施。覆盖 Agent Loop、LLM 调用、工具执行、会话管理四大维度，输出可读终端日志 + 机器可解析 JSONL 文件日志，为后续所有里程碑的调试、性能分析和行为追溯提供统一基础。

## Problem

当前所有运行时信息通过 `eprintln!` 输出，存在以下问题：

1. **不可过滤** — 无法按级别、模块、来源分别查看
2. **不可追溯** — 无 session ID、iteration ID，无法关联一次用户请求的完整执行链
3. **不可分析** — 纯文本无结构，无法做 token 消耗统计、工具耗时分析、错误频率统计
4. **不可持久化** — 所有信息随终端关闭丢失
5. **开发效率低** — 新增 crate 或 handler 时无统一日志范式，每人各写各的 println

## Users

- **Omega 开发者**：调试 Agent 行为、定位 LLM 交互问题、分析性能瓶颈
- **Omega Agent 用户**：查看执行日志、理解 Agent 做了什么、排查失败原因

## Requirements

### Must Have (P0)

- 所有 crate 使用 `tracing` 宏替代裸 `println!/eprintln!`
- 结构化 span 覆盖四大维度：
  - **session**: 会话级 span，带 `session_id`
  - **agent_loop**: 循环迭代级 span，带 `iteration`、`message_count`
  - **llm_call**: LLM 请求级 span，带 `model`、`duration_ms`、`input_tokens`、`output_tokens`、`stop_reason`
  - **tool_exec**: 工具执行级 span，带 `tool_name`、`duration_ms`、`success`
- 终端输出层（`tracing-subscriber` EnvFilter）— 默认 `INFO`，可通过 `OMEGA_LOG` 环境变量调整
- JSONL 文件日志层 — 输出到 `~/.omega/logs/` 目录，按日期轮转
- `omega-tui` 初始化 subscriber，其他 crate 仅使用 `tracing` 宏

### Should Have (P1)

- LLM 请求/响应原始 JSON 在 `TRACE` 级别记录（仅开发调试用）
- 工具输入/输出摘要在 `DEBUG` 级别记录
- Token 使用量在 `INFO` 级别每次 LLM 调用后记录
- 错误和超时统一使用 `error!` / `warn!` 宏

### Nice to Have (P2)

- 可选 OpenTelemetry 导出层（未来对接外部可观察性平台）
- 结构化日志查询 CLI 工具（从 JSONL 文件检索/统计）
- 日志文件大小限制与自动清理

## Design

### 层级架构

```
omega-tui (subscriber 初始化)
  ├── UI 层: fmt::Layer (compact) → TUI Logs 面板 (实时显示)
  └── 文件层: fmt::Layer<Json> → ~/.omega/logs/omega-YYYY-MM-DD.jsonl

omega-core (span 定义 + 事件发射)
  ├── session span
  ├── agent_loop span (per iteration)
  ├── llm_call span
  └── tool_exec span

omega-client (LLM 调用事件)
  ├── info: 请求发送 + 响应接收 + token 统计
  ├── debug: 请求/响应头部信息
  └── trace: 原始 JSON body

omega-tools / omega-tools-builtin (工具执行事件)
  ├── info: 工具调用 + 结果摘要
  ├── debug: 完整输入/输出
  └── warn/error: 执行失败 + 安全拦截
```

### Span 结构

```
session{session_id}
  └── agent_loop{iteration, message_count}
        ├── llm_call{model, max_tokens}
        │     → event: response received {input_tokens, output_tokens, stop_reason, duration_ms}
        ├── tool_exec{tool_name}
        │     → event: tool completed {success, duration_ms, output_preview}
        └── tool_exec{tool_name}
              → event: tool completed {success, duration_ms, output_preview}
```

### 环境变量

| 变量 | 说明 | 默认值 |
|------|------|--------|
| `OMEGA_LOG` | 日志级别过滤 (EnvFilter 语法) | `info` |
| `OMEGA_LOG_DIR` | JSONL 日志文件目录 | `~/.omega/logs` |
| `OMEGA_LOG_FILE` | 是否启用文件日志 | `true` |

### Crate 职责分配

| Crate | 变更 |
|-------|------|
| `omega-tui` | 初始化 `tracing-subscriber` registry，配置 UI 层（Logs 面板）+ 文件层（JSONL） |
| `omega-core` | 在 `run_loop_with` 中创建 `session` 和 `agent_loop` span |
| `omega-client` | `MinimaxClient::chat()` 中创建 `llm_call` span，记录 token 用量 |
| `omega-tools` | `ToolDispatcher::dispatch()` 中创建 `tool_exec` span |
| `omega-tools-builtin` | `BashHandler::execute()` 中记录命令执行细节和安全拦截 |

### 依赖变更

所有需要发射日志事件的 crate 添加：

```toml
[dependencies]
tracing.workspace = true
```

`omega-tui` 额外添加：

```toml
[dependencies]
tracing.workspace = true
tracing-subscriber = { workspace = true, features = ["env-filter", "json"] }
chrono = "0.4"
dirs = "6"
```

## Implementation Tasks

### Task O1: omega-tui — 初始化 tracing subscriber
- **Type**: Implementation
- **Complexity**: M
- **Dependencies**: None
- **Description**: 在 `main()` 中配置 `tracing-subscriber` registry：UI 层 (compact format → TUI Logs 面板，mpsc::sync_channel + MakeWriter) + 文件层 (JSON format, 按日期文件名)。移除现有 `eprintln!` 改用 `info!`/`debug!`。

### Task O2: omega-client — LLM 调用追踪
- **Type**: Implementation
- **Complexity**: M
- **Dependencies**: O1
- **Description**: 在 `MinimaxClient::chat()` 中添加 `llm_call` span 和事件。记录请求参数（model, max_tokens, tool_count, message_count）、响应指标（input_tokens, output_tokens, stop_reason, duration_ms）、原始 JSON（TRACE 级别）。

### Task O3: omega-tools — 工具执行追踪
- **Type**: Implementation
- **Complexity**: S
- **Dependencies**: O1
- **Description**: 在 `ToolDispatcher::dispatch()` 中添加 `tool_exec` span。记录工具名、执行耗时、成功/失败。安全拦截事件使用 `warn!`。

### Task O4: omega-tools-builtin — BashHandler 追踪
- **Type**: Implementation
- **Complexity**: S
- **Dependencies**: O3
- **Description**: 在 `BashHandler::execute()` 中添加结构化日志。命令执行 `info!`，安全拦截 `warn!`，超时 `error!`，输出截断 `debug!`。

### Task O5: omega-core — Agent Loop 追踪
- **Type**: Implementation
- **Complexity**: M
- **Dependencies**: O2, O3
- **Description**: 在 `Agent::run_loop_with()` 中创建 `session` span（带 uuid）和每次迭代的 `agent_loop` span。记录 iteration 数、message 数、stop_reason。

### Task O6: 验证与文档
- **Type**: Testing + Docs
- **Complexity**: S
- **Dependencies**: O1-O5
- **Description**: 确认 `OMEGA_LOG=debug cargo run -p omega-tui` 产出终端日志 + JSONL 文件。更新开发指南中的日志使用说明。

## Success Metrics

- 运行 `cargo run -p omega-tui` 后，`~/.omega/logs/` 出现当日 JSONL 日志文件，且 TUI Logs 面板实时显示日志行
- JSONL 中每个 LLM 调用有 `input_tokens`、`output_tokens`、`duration_ms` 字段
- `OMEGA_LOG=trace` 可在 Logs 面板看到完整 tracing 事件（包括原始 LLM 请求/响应 JSON）
- `OMEGA_LOG=error` 仅在 Logs 面板显示错误信息
- 所有 crate 中无 `println!/eprintln!` 残留（调试输出除外）

## Open Questions

- 是否需要 session 级别的日志摘要文件（每次会话结束后自动生成 summary）？
- JSONL 文件是否需要按大小轮转（如单文件 > 100MB 时切割）？
