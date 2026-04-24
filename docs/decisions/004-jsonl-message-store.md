---
adr_number: 004
author: omega-team
content_revision: 120
date: 2026-03-18
generation_id: gen_000046_r000120
projection_version: 46
reviewed_by: []
source_doc_id: "adr:docs-decisions-004-jsonl-message-store"
status: accepted
---

# 004: 消息系统使用 JSONL 文件存储

## Status

Accepted

## Context

Omega Team 功能需要消息队列来协调多个 agent 之间的通信。需要选择合适的持久化方案。

## Decision

使用 JSONL (JSON Lines) 文件存储消息：

- 每个收件箱一个 .jsonl 文件
- 每行一个 JSON 消息
- 读取时一次性加载，写入时 append

## Consequences

### Positive
- 实现简单，无需额外依赖
- 天然 append-only，适合消息队列
- 易于调试（可直接查看文件）
- 支持多消费者（读取后清空）

### Negative
- 不适合大量消息（需优化）
- 并发写入需加锁
- 删除消息需要重写文件

## Alternatives Considered

### Alternative 1: SQLite
**Pros**: 功能强大，查询方便
**Cons**: 需要额外依赖
**Why Rejected**: 过度工程化

### Alternative 2: Redis
**Pros**: 高性能，支持队列
**Cons**: 需要额外服务
**Why Rejected**: 增加部署复杂度

### Alternative 3: 内存 + 定期落盘
**Pros**: 性能好
**Cons**: 丢失风险
**Why Rejected**: 不符合 learn-claude-code 的持久化设计

## Notes

- 实现位置: crates/omega-message
- 参考: learn-claude-code agents/s09_agent_teams.py
