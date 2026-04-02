---
adr_number: 005
date: 2026-03-18
status: accepted
author: omega-team
reviewed_by: []
related_prd: docs/archive/observability-logging.md
---

# 005: 使用 tracing 生态构建可观察性基础设施

## Status
Accepted

## Context

M1 里程碑完成后，Omega Agent 已可端到端运行（REPL → LLM 调用 → 工具执行 → 响应输出）。但调试和排查完全依赖 `eprintln!` 裸输出，存在以下痛点：

1. LLM 调用失败时只有错误字符串，无法定位是网络、解析还是 API 限制问题
2. 工具执行耗时无法量化，性能瓶颈不可见
3. Token 消耗无记录，无法做成本分析
4. 多轮对话中无法关联某次工具调用属于哪个迭代
5. 后续里程碑（子智能体、后台任务、团队协作）的调试复杂度会指数增长

需要在继续功能开发前，建立统一的结构化日志和追踪基础设施。

## Decision

采用 Rust `tracing` 生态系统作为唯一的可观察性基础设施：

- **`tracing`** — 所有 crate 使用 `tracing` 宏 (`info!`, `debug!`, `warn!`, `error!`, `trace!`) 替代裸 `println!/eprintln!`
- **`tracing-subscriber`** — 在 `omega-tui` 入口初始化 subscriber，配置两个输出层：
  - UI 层：compact 人类可读格式，通过 `mpsc::sync_channel` + 自定义 `MakeWriter` 发送到 TUI 的 Logs 面板实时显示
  - 文件层：JSON 格式输出到 `~/.omega/logs/omega-YYYY-MM-DD.jsonl`
- **Span 结构** — 使用 `#[instrument]` 和手动 span 构建四级追踪树：session → agent_loop → llm_call / tool_exec
- **零运行时成本** — 未启用的日志级别在编译时被优化掉

### 不采用的变更

- 不引入 OpenTelemetry（P2 留待未来）
- 不引入独立的 metrics 系统（tracing 事件已满足当前需求）
- 不修改 `LlmClient` trait 签名（追踪在实现层透明添加）

## Consequences

### Positive
- 统一日志范式，新 crate / handler 直接使用 `tracing` 宏
- 结构化 span 在多轮、多工具、子智能体场景下可关联完整执行链
- JSONL 文件日志可持久化，支持事后分析
- `OMEGA_LOG` 动态调级，debug 时不需改代码
- `tracing` 是 Rust 生态事实标准，生态完善（tower, axum, tokio 均内置支持）

### Negative
- 每个 crate 增加 `tracing` 依赖（编译增量极小）
- `omega-tui` 需额外依赖 `tracing-subscriber`, `chrono`, `dirs`
- 需要规范 span 字段命名，避免跨 crate 字段不一致
- Logs 面板和 JSONL 文件两条独立路由需要保持一致（已解决：共享同一 subscriber，零额外成本）

## Alternatives Considered

### Alternative 1: log + env_logger
**Pros**: 更简单，`log` crate 是最轻量的日志门面
**Cons**: 无 span 概念，无法构建追踪树；无结构化字段；无法原生输出 JSON
**Why Rejected**: Agent 的调用链天然是嵌套结构（session > iteration > llm/tool），`log` 无法表达这种层级关系

### Alternative 2: 自建日志模块
**Pros**: 完全定制，无第三方依赖
**Cons**: 重复造轮子，缺乏 subscriber 生态，维护成本高
**Why Rejected**: `tracing` 已是 Rust 异步生态标准，无理由重复实现

### Alternative 3: 延迟到后续里程碑再加
**Pros**: 当前不阻塞功能开发
**Cons**: 后续修改面更大，已完成的 crate 都需要回头改；越晚引入，调试成本越高
**Why Rejected**: 现在只有 5 个 crate 需改动，后续 M2-M10 会持续新增 handler 和模块，越早建立范式越省力

## Notes

- 历史设计与 rollout 任务见 [docs/archive/observability-logging.md](../archive/observability-logging.md)
- workspace 根 `Cargo.toml` 已声明 `tracing = "0.1"` 和 `tracing-subscriber = "0.3"`，各 crate 仅需在自身 Cargo.toml 引用
