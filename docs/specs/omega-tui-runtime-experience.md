---
status: draft
owner: omega-team
created: 2026-03-19
updated: 2026-03-19
version: 1.0
supersedes: []
related_prds: []
---

# Omega TUI Runtime Experience Specification

## Overview

当前主线待办里，`omega-skills`、`omega-subagent`、`omega-compression`、`omega-tasks`、`omega-background`、`omega-message`、`omega-team`、`omega-worktree` 都会产生“应该被用户看到”的运行态信息。如果继续按功能逐个往 `omega-tui` 塞新面板，TUI 会很快退化为面板堆砌；如果完全不提供可见反馈，这些能力在日常使用里又会接近不可感知。

本规格定义一套统一的 TUI 体验策略：把未来运行态能力收敛为状态栏徽章、可切换的 Activity 面板，以及少量持续可见的固定面板，避免每个 crate 各自发明一套 UI。

## Goals

- 为后续主线任务提供统一的 TUI 落点，避免面板数量失控。
- 明确哪些信息应该进入状态栏、Activity 面板、Todo 面板、日志流和 Response 主面板。
- 保持 `omega-core` / `omega-session` 前端无关，不把 widget 语义或布局决策回灌到核心 crate。
- 为未来 `15B-11` 搜索、`15B-12` 会话统计与可调面板预留稳定的信息架构。

## Non-Goals

- 不在本规格中直接实现新的 TUI widget 或键位映射。
- 不在本次设计中引入图形化弹窗、鼠标拖拽编辑、复杂多窗口或树形导航。
- 不要求每个主线任务都必须先做 TUI 才能落地；核心能力仍可先以 REPL/内部协议打通。

## Affected Roadmap Tasks

| Task | Crate | Why TUI Needs a UX Plan |
|------|-------|-------------------------|
| Task 5 | `omega-skills` | 用户需要知道本轮实际加载了哪些 skills，避免“看不见的 prompt 变化” |
| Task 10 | `omega-subagent` | 委派是否发生、子任务是否仍在运行、结果是否回收，需要前端可见 |
| Task 11 | `omega-compression` | 历史是否被压缩、当前上下文压力如何，必须有轻量反馈 |
| Task 4 | `omega-tasks` | 持久化任务与 turn-local todo 不同，需要独立可见性 |
| Task 12 | `omega-background` | 后台任务天然需要列表、状态和完成回收提示 |
| Task 3 | `omega-message` | 消息总线/收件箱如果没有可见入口，团队协作会失去可操作性 |
| Task 13 | `omega-team` | 多 agent 团队状态、角色和最近活动不适合仅靠日志理解 |
| Task 6 | `omega-worktree` | 多 worktree 执行时需要让用户知道当前上下文在哪个工作树 |

## Information Architecture

### Fixed Surfaces

| Surface | Purpose | Data Class |
|---------|---------|-----------|
| `Response` | 当前主对话、工具输出和最终回答 | turn-primary content |
| `Todos` | 当前任务的局部执行计划 | short-lived task plan |
| `Activity` | 与运行时能力相关的可切换详情视图 | runtime secondary state |
| status bar | 一眼可见的紧凑摘要与告警 | compact badges |

## Sidebar Shell

右侧区域不应继续演进为多个彼此独立的小面板，而应收敛为单一可收起的 `Sidebar` shell。`Todos` 与 `Activity` 共享同一个右侧容器；容器顶部保留稳定的图标/状态 rail，用于展示折叠 section 与未来的 Activity view 入口。

对当前阶段，这意味着：

- 整个右侧辅助区可以被快捷键整体收起或展开。
- `Todos` 与 `Logs` 可以在侧边栏内部折叠为顶部图标入口。
- 展开状态下，多个 section 在侧边栏主体区域做垂直弹性排列。

具体交互与布局规则见 `docs/specs/omega-tui-collapsible-sidebar.md`。

### Core Rule

不要为 `skills`、`subagent`、`background`、`team`、`message`、`worktree` 分别新增固定常驻面板。未来右侧下半区应从当前 `Logs` 的单一角色演进为可切换的 `Activity` 面板，日志只是其中一个 view。

推荐结构：

- 左侧：`Response`
- 右侧：统一 `Sidebar`
- `Sidebar` 顶部：section/view rail
- `Sidebar` 主体：`Todos` + `Activity` 的一个或多个展开 section
- `Activity` 内部可切换 `Logs / Skills / Delegations / Tasks / Background / Inbox / Team / Worktree`

这样可以保持固定的空间结构，同时容纳后续运行态能力。

## Status Bar Badges

状态栏只承载“摘要”，不承载长文本细节。推荐后续统一追加如下徽章：

- `Skills: N`：本轮已加载 skill 数量
- `Ctx: 72%` 或 `Ctx: compacted`：上下文压力与压缩事件
- `Subagents: 2 running`：委派中的子智能体数量
- `Bg: 1 failed` / `Bg: idle`：后台任务总览
- `Inbox: 3 unread`：团队/消息未读数
- `WT: feature-x`：当前活跃 worktree

规则：

- 徽章必须可在窄终端下截断为短格式，而不是把状态栏挤爆。
- 警告类状态优先于信息类状态，例如压缩失败、后台任务 error、subagent 异常。
- 状态栏只展示最新摘要；完整细节进入 `Activity` 面板。

## Activity Views

### Logs View

- 保留当前日志语义，作为默认回退 view。
- 继续承担调试输出、事件流和错误详情。

### Skills View

- 展示本轮或当前会话已加载 skills 列表。
- 每条包含：skill 名称、来源、加载原因或匹配依据摘要。
- 若本轮未加载任何 skill，应明确显示 `No skills loaded for this turn.`。

### Delegations View

- 用于 `omega-subagent`。
- 每条包含：subagent 名称、当前状态、最近一步动作、结果摘要。
- 运行中条目需要可见区分，不应只靠日志刷新。

### Tasks View

- 用于 `omega-tasks`。
- 只显示持久化任务，不与 `Todos` 混合。
- 默认展示最近更新任务与状态变化。

### Background View

- 用于 `omega-background`。
- 列表字段至少包括：任务 ID、状态、命令摘要、最近结果。
- 完成或失败时应在状态栏留短暂摘要，同时保留详情在该 view。

### Inbox View

- 用于 `omega-message`。
- 展示未读消息数、最近消息来源和摘要。
- 重点是“可发现”，而不是一开始就实现完整邮件客户端式交互。

### Team View

- 用于 `omega-team`。
- 展示 team 成员、角色、状态和最近活动。
- 避免把团队状态埋进原始日志文本里。

### Worktree View

- 用于 `omega-worktree`。
- 展示当前活跃 worktree，以及最近涉及的 worktree 上下文。
- 只有存在多 worktree 或切换行为时才需要强化显示，避免日常单 worktree 噪音过大。

## Task-to-Surface Mapping

| Capability | Status Bar | Activity View | Response | Todo |
|-----------|------------|---------------|----------|------|
| skills | count / warning | `Skills` | 可在回答中简述，但不重复完整列表 | 否 |
| subagent | running count / error | `Delegations` | 保留最终回收结果 | 否 |
| compression | pressure / compacted | `Logs` 或未来 `Context` | 否 | 否 |
| tasks | updated count | `Tasks` | 可回显任务操作结果 | 否 |
| background | running / failed count | `Background` | 仅在用户明确查询或任务回收时回显 | 否 |
| message | unread count | `Inbox` | 需要时提示新消息 | 否 |
| team | team health summary | `Team` | 汇报最终团队产出 | 否 |
| worktree | active worktree | `Worktree` | 需要时说明执行上下文 | 否 |

## Interaction Model

推荐后续交互规则：

- 全局快捷键解析应先经过 `Task 15B-13` 的模态 keymap 层，而不是继续在 `omega-tui` 事件处理中硬编码分支。
- 默认交互模式至少区分 `Normal` 与 `Insert`；导航、搜索、面板切换、Activity view 切换等行为应主要留在 `Normal`。
- `Tab` 继续在固定面板间切换，不随着 Activity view 数量增加而增加常驻焦点数量。
- Activity 内部 view 切换应走 leader 映射和 mode-aware 快捷键，而不是把每个 view 变成独立焦点面板。
- `15B-11` 搜索应先面向当前聚焦面板工作；Activity view 只需复用同一搜索框架，不单独设计。
- `15B-12` 会话统计优先放在状态栏与 Activity 中，不额外创建第四块永久面板。

相关模态与配置规则见 `docs/specs/omega-tui-modal-keymap.md`。

## Session Boundary Implications

为了保持边界清晰，后续 runtime-visible 能力应优先通过 `omega-session` 暴露“前端可消费但不带 widget 语义”的更新，而不是让 `omega-tui` 直接读取各个 crate 的内部 manager。

建议遵守以下规则：

- `omega-core` 只产生领域数据，不理解 `Activity view`、badge 顺序、颜色和焦点。
- `omega-session` 负责把领域事件归一为稳定的前端更新协议。
- `omega-tui` 负责决定这些协议映射到哪个 badge、哪个 Activity view、以及窄终端如何退化。

## Responsive Degradation

窄终端下遵循以下策略：

- 先保留 `Response` 和状态栏摘要。
- `Todos` 已有规则继续保留为优先级最高的辅助信息。
- `Activity` 隐藏时，不丢失关键 runtime 状态，而是退化到状态栏 badges + 关键事件 toast/日志摘要。
- 不允许出现“面板被隐藏但仍可获得焦点”的状态。

## Task Planning Impact

为避免未来主线任务完成后还要返工 TUI，建议增加一个专门的前置设计/基础设施任务：

- `Task 15B-16`: 为 `Activity` 面板与状态栏徽章建立统一基础，作为后续 skills/subagent/background/team/worktree 等能力接入 TUI 的统一承载层。

该任务不必先于 `Task 5` / `Task 10` 的核心 crate 实现完成，但应在这些能力正式追求 TUI 可用体验前落地。

## Technical Decisions

| Decision | Choice | Rationale |
|---------|--------|-----------|
| future sidebar model | collapsible `Sidebar` with `Todos` + `Activity` | 避免每个运行态能力都要求独立常驻面板，并让专注模式下可让出主空间 |
| logs placement | fold into `Activity` | 日志仍保留，但不再垄断整个下半侧栏 |
| status visibility | badges first | 紧凑摘要比长文本更适合持续可见 |
| task visibility split | `Todo` vs `Tasks` 分离 | turn-local 计划与持久化任务语义不同 |
| session boundary | typed updates through `omega-session` | 保持 `omega-core` 前端无关 |

## Testing Strategy

- 规格层：后续所有带运行态可见性的 crate 在落地时，都应明确对应到 status bar 或 `Activity` view，而不是直接把文本塞进日志。
- TUI 层：新增 view 切换后，测试必须覆盖可见 view 状态、隐藏侧栏退化、badge 摘要正确性。
- 交互层：后续 `15B-11` / `15B-12` 的搜索和统计能力应基于此规格验证没有引入额外常驻面板膨胀。

---

### Change Log
- 2026-03-19: 新增跨任务的 TUI 运行态体验规格，统一规划 skills/subagent/compression/tasks/background/message/team/worktree 在 TUI 中的可见落点。