---
content_revision: 101
created: 2026-03-19
generation_id: gen_000015_r000101
last_verified_commit: N/A
owner: omega-team
projection_version: 15
related_prds: []
source_doc_id: "spec:docs-specs-omega-tui-runtime-experience"
status: draft
supersedes: []
updated: 2026-04-10
---

# Omega TUI Runtime Experience Specification

## Overview

当前主线待办里，`omega-skills`、`omega-subagent`、`omega-compression`、`omega-tasks`、`omega-background`、`omega-message`、`omega-team`、`omega-worktree` 都会产生“应该被用户看到”的运行态信息。如果继续按功能逐个往 `omega-tui` 塞新面板，TUI 会很快退化为面板堆砌；如果完全不提供可见反馈，这些能力在日常使用里又会接近不可感知。

本规格定义一套统一的 TUI 体验策略：把未来运行态能力收敛为底部状态徽章、可切换的 Activity 面板，以及少量持续可见的固定面板，避免每个 crate 各自发明一套 UI。

当前事实补充：本规格讨论的是当前由 `omega-tui` 消费的体验边界。当前真实入口已经迁移到 `omega-app`，由其完成装配并驱动 `omega-tui`；与现行主路径和装配路径有关的依赖图，以 `docs/specs/omega-runtime-ui-message-contract.md` 与 `docs/specs/omega-app-package.md` 为最新基线。当前 `omega-tui` 也已落地统一的 `TuiUpdateReducer`，作为 `RuntimeUiEnvelope` 的稳定 consumer/sink。

## Goals

- 为后续主线任务提供统一的 TUI 落点，避免面板数量失控。
- 明确哪些信息应该进入状态栏、Activity 面板、Todo 面板、日志流和 Response 主面板。
- 保持 `omega-core` / `omega-session` 前端无关，不把 widget 语义或布局决策回灌到核心 crate。
- 为未来 `15B-11` 搜索、`15B-12` 可调面板与 task-level delivery observability 预留稳定的信息架构。
- 为未来统一 runtime UI message/effect contract 预留稳定 target 与 surface 语义。

## Non-Goals

- 不在本规格中直接实现新的 TUI widget 或键位映射。
- 不在本次设计中引入操作系统级多窗口、任意层级 modal stack、鼠标拖拽编辑或树形导航。
- 不要求每个主线任务都必须先做完整 TUI 体验才能落地；核心能力仍可先以内层协议、测试或非交互集成方式打通。

## Affected Roadmap Tasks

| Task | Crate | Why TUI Needs a UX Plan |
|------|-------|-------------------------|
| Task 15F-1 | `omega-workflow` | 用户需要看到当前 turn 处于分析、计划、执行还是报告阶段 |
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
| `Response` | 当前主对话、按 scene/workflow/step 分组的结构化 turn timeline、最终用户可见回答与未来 thinking block | turn-primary content |
| `Todos` | 当前任务的局部执行计划 | short-lived task plan |
| `Activity` | 与运行时能力相关的可切换详情视图 | runtime secondary state |
| `Overlay` | 搜索、确认、详情查看等短时浮动交互 | transient focused interaction |
| `Input context bar` | 左列输入框上方的当前交互提示与短消息 | input-adjacent context |
| `Input info bar` | 共享输入壳层底部的一行短状态栏，带内边距，承载 model / token / 右侧 state icon | input-adjacent runtime strip |
| `Bottom status bar` | 全宽底部的全局状态摘要与扩展槽，不再重复展示 model | compact badges |

`Response` 的详细规划见 `docs/specs/omega-tui-response-thinking-experience.md`。当前该面板已经按 `route / step / final / thinking` 结构化渲染 turn timeline；provider-exposed thinking 现已作为独立、低噪音且可折叠的 block 接入，并支持通过 `.omega/tui.toml` 关闭可见性。

## Bottom Layout Rule

底部区域应采用稳定的双层结构，而不是继续把提示和系统状态拆散到顶部与底部：

- `Input context bar`：位于左列 `Response` 下方、输入框上方，承载 leader 提示、当前模式提示和局部短消息；当前固定为两行并允许换行，不再横跨 full-height `Sidebar` 下方。Insert 模式提示现显式区分 `Enter=Send`、`Shift+Enter=Newline` 与 `↑/↓=Line`。
- `Input info bar`：位于共享输入壳层底部，带上下左右留白，承载当前模型、token 摘要与右侧运行态图标；token 仅保留 `k` 值，段间只用固定空格分隔。其右侧 running 动画现为一行高的压缩单点 orbit：glyph 会在 `● / ◉ / ◎ / ○ / ·` 间切换，但任一时刻只显示一个，而不是单 glyph 旋转圈或多点 trail。其上方输入区现为固定高度的多行 viewport，长输入会软换行并在超出可见高度时自动滚动；鼠标滚轮停留在输入区内时只滚动该 viewport。
- `Bottom status bar`：位于整页底部，承载 mode、workflow/route/project 等全局摘要。

原顶部 header 不再作为主要状态承载区。`Idle / Running` 图标与 delivery token 摘要收敛到共享输入壳层底部的 `Input info bar`；`Mode` 回到底部状态带最左侧；`model` 不再在底部状态带重复展示；`KM / Focus / Omega Agent` 这类高噪音信息不应继续作为固定 header 保留。具体布局规则见 `docs/specs/omega-tui-input-status-layout.md`。

## Sidebar Shell

右侧区域不应继续演进为多个彼此独立的小面板，而应收敛为单一可收起的 `Sidebar` shell。`Todos` 与 `Activity` 共享同一个右侧容器；容器顶部保留稳定的图标/状态 rail，用于展示折叠 section 与未来的 Activity view 入口。

对当前阶段，这意味着：

- 整个右侧辅助区可以被快捷键整体收起或展开。
- `Delivery` 与 `Skills` 默认收起，减少首屏噪音并为下游 section 留出稳定预算。
- 当前 section 现收敛为 `Diagnostics / Delivery / Skills / Knowledge / Todos / Logs`；其中 `Knowledge` 统一承接 document + memory supervision 摘要。
- 当展开 section 过多、窗口高度不足时，sidebar body 会按 rail 或当前 focused section 锚定到一个稳定 viewport，只显示当前窗口能装下的一段 sections，而不是让下方 panel 直接被挤出可见区。

具体交互与布局规则见 `docs/specs/omega-tui-collapsible-sidebar.md`。

### Core Rule

不要为 `skills`、`subagent`、`background`、`team`、`message`、`worktree` 分别新增固定常驻面板。未来右侧下半区应从当前 `Logs` 的单一角色演进为可切换的 `Activity` 面板，日志只是其中一个 view。

推荐结构：

- 左侧：`Response`
- 右侧：统一 `Sidebar`
- 左列底部：`Context bar + Input shell`
- `Sidebar` 顶部：section/view rail
- `Sidebar` 主体：`Todos` + `Activity` 的一个或多个展开 section
- `Activity` 内部可切换 `Logs / Skills / Delegations / Tasks / Background / Inbox / Team / Worktree`

这样可以保持固定的空间结构，同时容纳后续运行态能力。

## Bottom Status Bar Badges

底部状态带只承载“摘要”，不承载长文本细节。推荐后续统一追加如下徽章：

- `Skills: N`：本轮已加载 skill 数量
- `Scene: chat`：当前识别出的工作场景
- `Flow: Explore`：当前工作流阶段
- `WF: feature/report`：当前 active workflow 或其关键阶段
- `Ctx: 72%` 或 `Ctx: compacted`：上下文压力与压缩事件
- `Subagents: 2 running`：委派中的子智能体数量
- `Bg: 1 failed` / `Bg: idle`：后台任务总览
- `Inbox: 3 unread`：团队/消息未读数
- `WT: feature-x`：当前活跃 worktree
- `Project: omega` / `Route: chat -> feature` / `Item: execute-2 2/5`：当前全局上下文摘要

规则：

- 徽章必须可在窄终端下截断为短格式，而不是把底部状态带挤爆。
- 警告类状态优先于信息类状态，例如压缩失败、后台任务 error、subagent 异常。
- 底部状态带只展示全局摘要；完整细节进入 `Activity` 面板，而 token 短摘要进入 `Input info bar`。

当前对 scene-aware routing 的落地规则：

- `route` 徽章展示当前 recognized scene 与 selected workflow；在 scene 已识别但 child workflow 尚未选定时显示 `scene -> selecting`。
- `flow` 徽章展示当前 active workflow step，并显式带出 `root` / `child` 角色与 workflow id，例如 `root:root Scene Recognition 1/2`、`child:feature Explore 1/4`。
- 新 turn 开始时应清空上一轮 route 摘要，避免上一轮 scene/workflow 残留到当前 turn。

## Activity Views

### Logs View

- 保留当前日志语义，作为默认回退 view。
- 继续承担调试输出、事件流和错误详情。
- workflow phase 切换、tool preview、todo 刷新这类 runtime activity 应优先进入该 view，而不是混入 `Response` 主对话流。
- `Response` 面板应承载用户真正需要阅读的文本产物，包括 step 正文结果与最终 assistant 回复；纯运行态事件仍留在 `Activity & Logs`。
- scene/workflow routing 相关事件应统一加上稳定前缀：routing 决策与 fallback 使用 `[route] ...`，root workflow step summary 使用 `[root:root ...] ...`，child workflow step summary 使用 `[child:<workflow> ...] ...`。

### Skills View

- 展示本轮或当前会话已加载 skills 列表。
- 每条包含：skill 名称、来源、加载原因或匹配依据摘要。
- 若本轮未加载任何 skill，应明确显示 `No skills loaded for this turn.`。

### Delivery View

- 用于 task-level delivery observability。
- 展示当前 turn 的 token 消耗、LLM 调用次数、使用的模型、tool/skill 数量、document/memory search 次数与 changed files 摘要。
- 每个聚合条目都应支持进入 detail drill-down，而不是停留在只读统计。
- 任务完成后，该 view 应与 `Response` 中的统一 completion summary 对齐，避免两套数字口径不一致。

### Knowledge View

- 当前 sidebar 已把原 `Document` / `Memory` supervision 合并为统一 `Knowledge` 面板。
- 主视图优先展示一次 turn 的 `doc / mem / obs` 命中 mini-bar、store/summary 总量与 query/correction 摘要，避免继续以大段 prose 堆满侧栏。
- drill-down 仍保留 document lane 与 memory lane 两条细分详情，但入口统一为同一个 knowledge detail overlay。

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

## Overlay Usage

对短时但需要继续交互的场景，优先使用浮动 overlay，而不是继续扩张常驻面板：

- 搜索输入与搜索结果摘要
- task delivery 各聚合项的 detail overlay
- background / inbox / team / worktree 条目详情
- 中断、关闭、删除等确认流程
- 少量候选项的快速选择器

overlay 的职责是“短时聚焦交互”，而不是代替 `Activity` 或 `Response` 这类持续可见信息区。相关规则见 `docs/specs/omega-tui-overlay-popups.md`。

## Task-to-Surface Mapping

| Capability | Bottom Status Bar | Activity View | Response | Todo |
|-----------|------------|---------------|----------|------|
| workflow | current step / warning | 未来可扩展 `Workflow` | 否 | 否 |
| tool execution preview | 否 | `Logs` | 否 | 否 |
| skills | count / warning | `Skills` | 可在回答中简述，但不重复完整列表 | 否 |
| delivery observability | token/tool/file compact summary | `Delivery` | turn 完成后统一 delivery summary message | 否 |
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
- 若当前存在 overlay，键盘路由应先由 overlay 消费，再决定是否关闭或吞掉事件，而不是继续透传到 panel。
- `15B-11` 搜索应先面向当前聚焦面板工作；Activity view 只需复用同一搜索框架，不单独设计。
- task-level delivery 统计优先放在底部状态带与 `Delivery` view / overlay 中，不额外创建第四块永久面板。

相关模态与配置规则见 `docs/specs/omega-tui-modal-keymap.md`，overlay 规则见 `docs/specs/omega-tui-overlay-popups.md`。

## Session Boundary Implications

为了保持边界清晰，后续 runtime-visible 能力应优先通过 `omega-session` 暴露“前端可消费但不带 widget 语义”的更新，而不是让 `omega-tui` 直接读取各个 crate 的内部 manager。

这里的 `omega-session` 指当前 TUI 主路径上的 session boundary，不应自动推断为 REPL 已经接入同一层。REPL 是否在后续复用 `omega-session`，属于未来的架构收敛动作，而不是当前已成立的事实。

建议遵守以下规则：

- `omega-core` 只产生领域数据，不理解 `Activity view`、badge 顺序、颜色和焦点。
- `omega-session` 负责把领域事件归一为稳定的前端更新协议。
- `omega-session` 应负责把工作流阶段变化归一为 typed update，而不是让 `omega-tui` 直接推断阶段。
- 未来更通用的跨模块前端协议应以 `docs/specs/omega-runtime-ui-message-contract.md` 为主规格，`omega-tui` 只消费 target/kind/source 清晰的 runtime UI envelope。
- `omega-theme` 负责承载共享视觉令牌，例如颜色、边框类型、状态语义色和组件级样式槽，避免这些定义继续散落在 `omega-tui` 的多个渲染函数里。
- `omega-tui` 负责决定这些协议映射到哪个 badge、哪个 Activity view、以及窄终端如何退化。

## Visual System Boundary

为避免未来继续在 `render.rs` 中直接堆叠 RGB 常量与局部样式判断，视觉系统应新增独立的 `omega-theme` crate，承担以下职责：

- 提供命名主题与默认主题入口，例如 `dark`、未来可能的 `light` 或 `high-contrast`。
- 提供用户主题配置加载入口，默认读取 `.omega/theme.toml`，并将其解析为对内置主题的安全覆盖。
- 提供语义令牌，而不是零散的“某个 widget 专用颜色常量”，例如 `mode.insert_border_fg`、`panel.focus_border_fg`、`status.warning_fg`、`input_context.label_fg`。
- 为 `omega-tui` 提供可组合的组件级主题片段，例如输入框、底部状态带、overlay、sidebar rail。

边界规则：

- `omega-theme` 可以依赖 `ratatui` 暴露面向当前 TUI 的样式令牌，但不拥有运行态状态或布局逻辑。
- `omega-tui` 继续负责根据 `App` 状态选择语义样式，不把 `InteractionMode`、焦点、overlay 状态等状态机逻辑下沉到主题包。
- `omega-theme` 负责 `.omega/theme.toml` 的发现、默认文件生成、解析、校验和回退策略；`omega-tui` 只消费解析后的主题对象与错误摘要。
- 新的视觉调整若只是改 token，应优先落到 `omega-theme`；只有涉及布局、交互或状态映射时才修改 `omega-tui`。

## Responsive Degradation

窄终端下遵循以下策略：

- 先保留 `Response` 和底部状态带摘要。
- `Todos` 已有规则继续保留为优先级最高的辅助信息。
- `Activity` 隐藏时，不丢失关键 runtime 状态，而是退化到底部状态带 badges + 关键事件 toast/日志摘要。
- 不允许出现“面板被隐藏但仍可获得焦点”的状态。

## Task Planning Impact

为避免未来主线任务完成后还要返工 TUI，建议在 `Activity` 之前先补上弹窗基础设施：

- `Task 15B-16A`: 为搜索、确认和详情查看建立统一 overlay / popup 基础设施。
- `Task 15B-16`: 为 `Activity` 面板与状态栏徽章建立统一基础，作为后续 skills/subagent/background/team/worktree 等能力接入 TUI 的统一承载层。
- `Task 15B-17`: 为输入上下文带与底部状态带建立稳定布局和状态槽，为后续会话统计与运行态摘要扩展提供底座。
- `Task 15E-1`: 为 `omega-tui` 抽离 `omega-theme` 主题包，把共享颜色、边框和组件视觉令牌集中管理，为后续 Markdown、高亮和会话统计等能力的视觉扩展提供稳定入口。
- `Task 15F-1`: 为 `omega-session` 引入可配置 `omega-workflow` 执行阶段模型，并把当前阶段接入底部状态栏。
- `Task 15F-3`: 为 workflow 与 future runtime-visible 模块建立统一 runtime UI message/effect contract。
- `Task 15B-18`: 已为 `omega-tui` 建立统一的 runtime UI sink/reducer，当前按协议 target/kind/source 消费上游消息，后续新能力应继续沿该 reducer 扩展，而不是重新回到 feature-by-feature 特例分支。
- `Task 15F-4 / 15F-5`: 为 scene-aware routing 建立 `scene-recognition -> select-workflow -> child workflow` 主路径，并把 scene / workflow routing 收敛到 `omega-session`。
- `Task 15B-19`: 为当前 scene、selected workflow 与 root/child workflow 切换结果提供稳定的底部状态带与 Activity 可见性。

这些任务都不必先于 `Task 5` / `Task 10` 的核心 crate 实现完成，但应在这些能力正式追求 TUI 可用体验前落地；其中 `Task 15B-16` 应建立在 `Task 15B-16A` 之上，`Task 15B-17` 应建立在 `Task 15B-16` 之上，`Task 15E-1` 应建立在 `Task 15D` 与 `Task 15B-17` 已明确 UI 边界和底部布局之后，`Task 15F-1` 应建立在 `Task 15D` 提供的 `omega-session` 更新边界之上。

## Technical Decisions

| Decision | Choice | Rationale |
|---------|--------|-----------|
| future sidebar model | collapsible `Sidebar` with `Todos` + `Activity` | 避免每个运行态能力都要求独立常驻面板，并让专注模式下可让出主空间 |
| logs placement | fold into `Activity` | 日志仍保留，但不再垄断整个下半侧栏 |
| status visibility | bottom badges first | 紧凑摘要比长文本更适合持续可见 |
| visual token ownership | centralize in `omega-theme` | 避免颜色 / 边框 / 状态语义继续散落在 `omega-tui` 渲染实现 |
| task visibility split | `Todo` vs `Tasks` 分离 | turn-local 计划与持久化任务语义不同 |
| session boundary | typed updates through `omega-session` | 保持 `omega-core` 前端无关 |

## Testing Strategy

- 规格层：后续所有带运行态可见性的 crate 在落地时，都应明确对应到底部状态带或 `Activity` view，而不是直接把文本塞进日志。
- TUI 层：新增 view 切换后，测试必须覆盖可见 view 状态、隐藏侧栏退化、badge 摘要正确性。
- 交互层：后续 `15B-11` / `15B-12` 的搜索和统计能力应基于此规格验证没有引入额外常驻面板膨胀。

---

### Change Log
- 2026-03-19: 新增跨任务的 TUI 运行态体验规格，统一规划 skills/subagent/compression/tasks/background/message/team/worktree 在 TUI 中的可见落点。
- 2026-03-19: 补充 overlay / popup 交互层定位，明确其作为短时浮动交互基础设施先于 Activity 详情交互落地。
- 2026-03-19: 补充输入上下文带与底部状态带规则，明确移除顶部密集 header，并将持续状态摘要下移到底部固定区域。
- 2026-03-19: 补充 `omega-theme` 主题包边界，规划把共享视觉令牌从 `omega-tui` 渲染实现中抽离。
- 2026-03-19: 补充 `omega-workflow` 可配置四阶段执行模型，规划把当前工作流阶段接入底部状态栏摘要。
- 2026-03-20: workflow 接入后明确 `Response` 与 `Activity & Logs` 的职责分工：前者承载用户可阅读的对话与 step 正文结果，后者承载 workflow 阶段切换、tool preview、todo 刷新与 tracing runtime activity。
- 2026-03-20: 补充统一 runtime UI message/effect contract 规划，要求未来 runtime-visible 模块通过稳定 target/kind/source 协议接入 TUI。
- 2026-03-20: v1.1 — 补充当前 richer shell / thin shell 分裂现实，明确本规格仅覆盖 `omega-tui` 路径；与 `omega-session`、`omega-core` 的现状依赖关系以 `omega-runtime-ui-message-contract.md` 的 current-state 图为准。
- 2026-03-20: 交互入口已收敛为 `omega-tui`；与已移除 `omega-repl` 路径有关的说明不再作为活跃约束。
