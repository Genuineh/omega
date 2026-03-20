---
status: superseded
owner: omega-team
created: 2026-03-19
updated: 2026-03-20
version: 1.1
supersedes:
  - docs/specs/omega-agent-impl-plan.md#task-15
related_prds: []
archived: true
archived_date: 2026-03-20
replaced_by:
  - docs/specs/omega-runtime-ui-message-contract.md
  - docs/specs/omega-tui-non-ui-extraction.md
reason: omega-repl path retired; interaction model converged to omega-tui single entry
---

# Omega 交互层重构规格

> Archived on 2026-03-20. This document was archived because its central goal was to split and preserve `omega-repl` as an active user path. The repository has since converged to `omega-tui` as the only user-facing entry. Use `docs/specs/omega-runtime-ui-message-contract.md` for the active runtime path, and `docs/specs/omega-tui-non-ui-extraction.md` for the retained non-UI boundary rationale.

## Overview

当前 `omega-tui` 同时承担了 ratatui 界面状态、终端生命周期、tracing 路由、Agent worker 装配、历史最小 REPL 逻辑和可执行入口，导致包边界与职责边界混在同一个 `main.rs` 中。该状态已经足够支撑当前里程碑，但会显著放大后续 Markdown 渲染、输入历史、搜索、会话统计等功能的变更成本。

本规格定义新的交互层目标结构：`omega-tui` 收敛为可复用的 TUI 库，新增 `omega-repl` 承接行式 REPL，前端入口仅负责薄组合，不再把前端运行时和 UI 逻辑绑定在单一入口文件中。

## Goals

- 将 `omega-tui` 改造成可测试、可复用的 library-first crate。
- 为最小 REPL 建立独立包 `omega-repl`，消除与 ratatui TUI 的职责混用。
- 保持 `omega-core` 前端无关，不把 REPL/TUI 专属逻辑回灌到核心运行时。
- 在迁移期间尽量保持现有用户可见行为和命令习惯稳定。
- 在继续 `Task 15B-*` 高级 TUI 功能前，先稳定交互层边界。

## Non-Goals

- 不重写 `omega-core` Agent Loop、provider 抽象、工具系统或 Todo 系统。
- 不在本次重构中引入新的前端类型，例如 Web UI 或 GUI。
- 不为了“共享一切”额外引入新的通用前端框架 crate。
- 不改变现有日志语义、工具调用协议或消息模型。

## Architecture

### Target Responsibilities

| Component | Responsibility | Must Not Own |
|-----------|----------------|--------------|
| `omega-core` | Agent 生命周期、LLM 调用、工具调度、消息历史 | 终端渲染、键盘事件、REPL 输出格式 |
| `omega-tui` | App 状态机、ratatui 渲染、事件路由、面板滚动、TUI worker 更新协议、终端生命周期辅助 | REPL 行式交互、环境变量启动约定、面向用户的入口编排 |
| `omega-repl` | stdin/stdout REPL、文本格式化输出、tool callback 预览、退出语义 | ratatui 状态、面板渲染、TUI 布局 |
| thin binary entry | 装配 `MinimaxConfig`、client、tool dispatcher、system prompt，启动具体前端 | 业务状态、复杂 UI 逻辑 |

### Recommended Package Shape

#### `omega-tui`

- 类型：library-first crate
- 目标形态：`src/lib.rs` + 内部模块；二进制入口只保留为极薄 wrapper，或在后续彻底移出
- 依赖方向：`omega-tui -> omega-core`

推荐内部模块划分：

| Module | Responsibility |
|--------|----------------|
| `app` | `App` 状态、消息缓冲、面板焦点、输入缓冲、滚动策略 |
| `render` | ratatui widget 构建、布局、主题、文本 wrap |
| `event` | 键盘/鼠标事件到 `App` 操作的映射 |
| `runtime` | UI 主循环、帧刷新、worker 通道消费 |
| `agent_session` | 与 `omega-core::Agent` 的前端适配、turn/update 协议 |
| `logging` | UI log sink、tracing writer 适配 |
| `terminal` | raw mode、alternate screen、panic-safe cleanup |

#### `omega-repl`

- 类型：binary crate
- 职责：最小 REPL 前端，只负责 stdin/stdout 交互，不承担 TUI 逻辑
- 依赖方向：`omega-repl -> omega-core`
- 目标命令：`cargo run -p omega-repl`

### Binary Placement Decision

推荐方案：短期保留 `omega-tui` 中的极薄 TUI wrapper binary，长期将 `omega-tui` 视为 library-first crate；`omega-repl` 独立成单独 package。

理由：

- 这样能先把“逻辑与入口”分开，而不必立即引入第三个 launcher crate。
- 当前仓库只有两种交互形态：TUI 和 REPL。此时再加一个统一 launcher package 属于过早抽象。
- `omega-repl` 的独立价值已经明确，因为它对应的是不同的交互模型，而不是单纯的另一种入口文件。
- 对外命令稳定性可通过 thin wrapper 保持，例如继续让 `omega` 指向 TUI 二进制。

如果后续出现第三种以上前端，或必须提供统一的 `omega <mode>` 启动语义，再单独引入 launcher crate；该动作不属于本次重构的前置条件。

### Data Flow

#### REPL

```text
stdin -> omega-repl input loop -> omega-core::Agent
      -> tool callbacks / assistant text formatting
      -> stdout
```

#### TUI

```text
terminal events -> omega-tui runtime -> omega-tui::app state
                -> omega-core::Agent worker
                -> LogUpdate / OutputUpdate channel
                -> omega-tui render pipeline
                -> terminal frame
```

## Technical Decisions

| Decision | Choice | Rationale |
|---------|--------|-----------|
| `omega-tui` 形态 | library-first | 后续 TUI 功能迭代主要发生在状态机与渲染层，不应绑死在入口文件 |
| REPL 所属 | 新增 `omega-repl` package | 最小 REPL 是独立交互模式，不应继续作为 `omega-tui` 的历史残留 |
| launcher crate | 暂不引入 | 当前只有两种前端，立即抽出统一 launcher 会增加额外抽象层 |
| shared bootstrap | 保持薄装配，不提前抽象 | 避免为了少量重复代码引入新的“万能 runtime” |
| TUI feature sequencing | 先重构包边界，再继续 `15B-8+` | 否则高级功能会直接叠加到当前巨型 `main.rs` |

## Migration Plan

### Phase 1: 文档与目标边界对齐

- 新增本规格文档。
- 在 `docs/TODO.md` 中新增交互层重构任务。
- 在 `omega-agent-impl-plan.md` 中把 `Task 15A/15B` 的目标边界改写为 `omega-repl` + `omega-tui`。

### Phase 2: `omega-tui` 库化，但不改变行为

- 将当前 `crates/omega-tui/src/main.rs` 中的纯 UI 状态与渲染逻辑迁入 `lib.rs` 下模块。
- 保留一个薄入口，仅负责启动 runtime 和清理终端。
- 验证当前 `cargo run -p omega-tui` 行为不变。

### Phase 3: 引入 `omega-repl`

- 新建 `crates/omega-repl/`。
- 将历史最小 REPL 行为迁移到 `omega-repl`。
- 迁移内容仅限：输入循环、tool callback 文本格式、退出语义、环境变量启动装配。
- 不复用 ratatui 模块，不让 `omega-repl` 依赖 `omega-tui`。

### Phase 4: 清理职责交叉

- 删除 `omega-tui` 中残留的 REPL 专属格式化逻辑。
- 明确 `omega-tui` 的公开 API 与私有模块边界。
- 更新开发指南中的运行命令与 crate 职责说明。

### Phase 5: 恢复高级 TUI 功能迭代

- 在包边界稳定后，再继续 `Task 15B-8` 及后续任务。
- 此时新的 Markdown 渲染、输入历史、搜索、会话统计都应建立在库化后的模块结构上。

## Risks

### High-Risk Anti-Patterns

- 把 `omega-repl` 变成 `omega-tui` 的“简化模式”，导致两个交互模型重新耦合。
- 把环境变量读取、client 构造、工作目录解析塞进 `omega-tui` 库公开 API。
- 为了共享极少量装配逻辑，过早引入新的“前端公共 runtime” crate。
- 让 `omega-core` 开始理解 UI 面板、键位、spinner、颜色等前端概念。
- 迁移时同时改行为和改结构，导致无法区分回归来源。

### Migration Risks

- 二进制命名变化导致现有 `cargo run -p omega-tui` / `cargo run -p omega` 心智模型混乱。
- tracing 初始化与 UI log sink 被拆散后，出现日志重复输出或丢失。
- 现有 `main.rs` 的状态与线程更新协议被搬迁时破坏中断语义。

## Performance Requirements

- 结构重构后，TUI 帧刷新、滚动和输入响应不得明显退化。
- `omega-repl` 不得因为新包拆分引入额外运行时层级或明显启动开销。
- 迁移后仍应支持当前日志面板与 agent worker 更新模型。

## Testing Strategy

- `omega-tui`：保留并扩展 `App` 状态测试、stale update 过滤测试、渲染辅助测试。
- `omega-repl`：新增输入循环、退出条件、tool callback 输出格式测试。
- smoke tests：至少覆盖 `cargo build -p omega-tui`、`cargo build -p omega-repl`、`cargo test`。
- 行为回归：迁移前后分别验证 TUI 中断、日志面板、tool output 展示、REPL 退出语义。

## Acceptance Criteria

- [x] `omega-tui` 的主要逻辑从单文件 `main.rs` 拆出为库模块。
- [x] `omega-repl` 成为独立 package，承接最小 REPL。
- [x] `omega-core` 未新增任何 TUI/REPL 专属概念。
- [x] 高级 TUI 任务在新结构上继续推进，而不是继续堆叠在历史入口文件上。
- [x] 开发指南、实现计划和 TODO 与新的交互层边界保持一致。