---
adr_number: 001
author: omega-team
content_revision: 101
date: 2026-03-18
generation_id: gen_000015_r000101
projection_version: 15
reviewed_by: []
source_doc_id: "adr:docs-decisions-001-crate-architecture"
status: accepted
---

# 001: 采用独立 Crate 架构

## Status

Accepted

## Context

Omega 项目需要复刻 learn-claude-code 的 12 阶段教程，每个阶段有独立的功能模块。我们需要决定如何组织代码结构，使得：
- 每个功能模块可以独立测试
- 便于理解和复用
- 依赖关系清晰

## Decision

采用独立 Crate 架构，将每个功能模块拆分为独立的 crate：

- omega-client: LLM 客户端
- omega-message: 消息系统
- omega-tasks: 任务系统
- omega-skills: Skill 加载
- omega-worktree: Worktree 隔离
- omega-tools: 工具抽象
- omega-tools-builtin: 内置工具
- omega-todo: Todo 管理
- omega-subagent: 子智能体
- omega-compression: 上下文压缩
- omega-background: 后台任务
- omega-team: 团队协作
- omega-core: 核心 Agent
- omega-tui: TUI 界面

## Consequences

### Positive
- 每个 crate 独立编译，编译时间短
- 便于单元测试
- 依赖关系通过 Cargo.toml 显式声明
- 代码职责清晰，易于理解

### Negative
- 需要维护多个 Cargo.toml
- 循环依赖风险（已通过依赖关系设计规避）
- 早期需要更多样板代码

## Alternatives Considered

### Alternative 1: 单 crate 多模块
**Pros**: 简单，所有代码在一个 crate 内
**Cons**: 编译时间长，难以独立测试
**Why Rejected**: 不符合"每个子功能独立"的需求

### Alternative 2: 3 个大 crate (core, tools, ui)
**Pros**: 减少 Cargo.toml 数量
**Cons**: 粒度过粗，职责不清晰
**Why Rejected**: 不符合单一职责原则

## Notes

- 依赖关系：底层 crate 无依赖，上层依赖下层
- 2026-03-20: 交互层当前真实入口已迁移为 `omega-app`；早期 `omega-repl` 路径已退役，不再计入当前 crate 结构基线
- 2026-03-20: `omega-app` 已作为应用装配入口落地，并计入当前已实现 crate 基线
- 详见: docs/specs/omega-agent-impl-plan.md
