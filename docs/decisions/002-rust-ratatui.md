---
adr_number: 002
author: omega-team
content_revision: 96
date: 2026-03-18
generation_id: gen_000013_r000096
projection_version: 13
reviewed_by: []
source_doc_id: "adr:docs-decisions-002-rust-ratatui"
status: accepted
---

# 002: 使用 Rust + Ratatui 实现

## Status

Accepted

## Context

Omega 项目需要提供用户交互界面。learn-claude-code 是 Python 实现，我们需要选择合适的技术栈来实现 TUI 界面。

## Decision

使用 Rust 作为开发语言，ratatui 作为 TUI 框架：

- **语言**: Rust (stable)
- **TUI 框架**: ratatui
- **异步运行时**: tokio
- **HTTP 客户端**: reqwest

## Consequences

### Positive
- Rust 性能优秀，适合长期运行的服务
- ratatui 纯 Rust 实现，无外部依赖
- tokio 成熟的异步生态
- 与原版 Python 对应，便于学习和对比

### Negative
- Rust 学习曲线较陡
- 开发速度可能慢于 Python
- 某些库不如 Python 丰富

## Alternatives Considered

### Alternative 1: Python + Textual
**Pros**: 与原版一致，开发快
**Cons**: 非目标语言
**Why Rejected**: 用户明确要求 Rust

### Alternative 2: Rust + cursive
**Pros**: 成熟稳定
**Cons**: 维护不活跃
**Why Rejected**: ratatui 更活跃，且与 ratatui (JS) 命名一致

### Alternative 3: Go + tview
**Pros**: 简单易学
**Cons**: 非目标语言
**Why Rejected**: 用户明确要求 Rust

## Notes

- 相关技术选型: ADR 003 (工具系统), ADR 004 (消息存储)
