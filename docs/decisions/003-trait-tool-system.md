---
adr_number: 003
author: omega-team
content_revision: 101
date: 2026-03-18
generation_id: gen_000017_r000101
projection_version: 17
reviewed_by: []
source_doc_id: "adr:docs-decisions-003-trait-tool-system"
status: accepted
---

# 003: 工具系统采用 Trait 接口

## Status

Accepted

## Context

Omega Agent 需要支持多种工具（bash, read, write, edit 等）。我们需要设计一个可扩展的工具系统，便于添加新工具。

## Decision

采用 Trait 接口设计工具系统：

```rust
pub trait ToolHandler: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn execute(&self, input: serde_json::Value) -> Result<String>;
}

pub struct ToolDispatcher {
    handlers: HashMap<String, Box<dyn ToolHandler>>,
}
```

- omega-tools: 定义 ToolHandler trait 和 ToolDispatcher
- omega-tools-builtin: 实现 BashHandler, ReadHandler, WriteHandler

## Consequences

### Positive
- 符合开闭原则，便于添加新工具
- 工具实现与调度解耦
- 易于单元测试（可 mock）

### Negative
- 需要定义 trait 接口
- 动态分发有一定性能开销（可忽略）

## Alternatives Considered

### Alternative 1: 枚举驱动
**Pros**: 简单，类型安全
**Cons**: 添加新工具需修改枚举和 match
**Why Rejected**: 不符合开闭原则

### Alternative 2: 函数注册表
**Pros**: 简单直接
**Cons**: 难以传递状态
**Why Rejected**: 工具可能需要状态（如工作目录）

### Alternative 3: 宏生成
**Pros**: 声明式
**Cons**: 复杂度高
**Why Rejected**: 过度工程化

## Notes

- 实现位置: crates/omega-tools, crates/omega-tools-builtin
