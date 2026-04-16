---
content_revision: 101
created: 2026-03-19
generation_id: gen_000015_r000101
last_verified_commit: N/A
owner: omega-team
projection_version: 15
related_prds: []
source_doc_id: "spec:docs-specs-omega-tui-input-status-layout"
status: implemented
supersedes: []
updated: 2026-04-10
---

# Omega TUI Input And Bottom Status Layout Specification

## Overview

当前 `omega-tui` 的底部区域仍沿用“输入框下方 hint bar + 顶部 header 状态条”的双源布局。随着 `Sidebar`、`Overlay`、后续 runtime badges 与会话统计继续增长，这种布局会把输入上下文、快捷键提示、模型状态和系统摘要拆散到屏幕两端，既增加视觉往返，也让后续状态接入缺少稳定落点。

本规格定义一套新的底部布局：把原先输入框下方承载提示与状态的内容，上移为“输入框上方、位于左列 `Response` 下方的固定上下文带”；再把原先顶部 header 中真正需要持续显示的底部状态信息拆成两层，下移为“共享边框输入壳层底部的一行 `Input info bar`”与“全宽页面底部的固定状态带”。最新实现里，`Sidebar` 已保持右列全高，`Context bar` 固定为两行并允许换行，输入壳层总高 `9`（上部输入区 + 输入/状态间留白 + 下部一行 info bar + 底部留白）；三条 bar 的背景也已与 `Agent Response` 的 dark panel surface 对齐。

## Goals

- 把与当前输入直接相关的提示、leader 状态、短消息提示聚合到输入框上方，减少视线在顶部和底部之间来回跳转。
- 移除顶部大块 header，把运行态和 token 摘要下移到输入框信息栏，把 mode 保留在底部固定状态带。
- 为后续 `skills/subagent/background/message/team/worktree` 等能力接入底部状态显示预留稳定扩展点。
- 保持 `Response` / `Sidebar` 主体区不因状态扩展而继续碎片化。

## Non-Goals

- 不在本规格中重新设计 `Sidebar`、`Overlay` 或 Activity 内容本身。
- 不要求本次同时落地 token 统计、轮次统计、后台任务摘要等具体业务状态，只要求先定义承载位。
- 不恢复顶部 `Omega Agent | KM | Focus | Mode` 这种密集 header 形式。

## Layout Model

终端主布局现收敛为两层：

1. 整体纵向：`Main shell -> Bottom status bar`
2. `Main shell` 横向：左列 `Response -> Input context bar -> Input shell`，右列 full-height `Sidebar`
3. `Overlay`: 若激活则浮于以上区域之上

### Input Context Bar

该区域位于左列 `Response` 和输入框之间，用于承载：

- leader pending 提示
- 当前输入模式下的快捷键提示
- 与当前输入或当前聚焦区域直接相关的短消息
- 需要立刻被用户感知、但不应长期占据底部状态带的瞬时反馈

规则：

- 优先显示当前交互上下文，而不是系统级长周期状态。
- 文案应短、面向当前操作，避免堆叠多个 badge。
- 当前实现固定为 `2` 行，并允许在长度不足时自动换行；不再要求强制单行。
- 共享 `Input shell` 上部输入区当前为 `4` 行可见 viewport：长文本应在壳层内软换行，`Shift+Enter` 可插入显式换行，`↑/↓` 应按可视行在多行内容内移动光标；内容超出可见高度时 viewport 必须跟随当前光标所在行滚动，而不能挤占或覆盖底部 `Input info bar`。

### Input Info Bar

该区域位于共享输入壳层底部、底部状态带上方，承担最短期的运行态信息。

当前实现承载：

- 当前模型名；若有 delivery token，则显示为 `model   12.3k`
- 最右侧 `state icon`：空闲时 `↑`，运行时使用单行压缩版单点 orbit 动画；glyph 会在 `● / ◉ / ◎ / ○ / ·` 间切换，但任一时刻只显示一个，并右对齐到壳层末端
- `delivery token summary`：仅保留 `k` 单位一位小数 token 值，例如 `12.3k`
- 该行位于输入壳层内部，左右各保留 `1` 列，且与上方输入区、下方边框之间各保留 `1` 行留白
- 各段之间只保留固定空格，不再使用点状分隔符
- 输入提交与换行语义拆开：`Enter` 继续提交，`Shift+Enter` 负责插入换行
- 当鼠标滚轮落在输入区矩形内时，只滚动输入 viewport，不应误滚 `Response` 或 `Sidebar` 面板

### Bottom Status Bar

该区域位于页面底部，替代旧顶部 header，承担持续性全局摘要。

当前实现保留：

- `mode`
- `flow / project / session / route / item` 这类全局摘要槽

明确移除：

- `Omega Agent`
- `KM`
- `Focus`
- `Mode` 从输入框信息栏移回底部状态带最左侧

这些信息如果仍有必要，应通过上下文带、overlay、局部标题或未来更轻量的交互方式表达，而不是重新挤回固定状态栏。

## Bottom Status Slot Model

为避免后续每加一个功能就继续拼接字符串，底部状态带应抽象为稳定的 slot 模型。

建议最小结构：

- `mode slot`: 当前交互模式
- `workflow slot`: 当前执行阶段，例如 `Explore` / `Plan` / `Execute` / `Report`
- `context slot`: `project / session / route / item` 等全局摘要
- `extension slots`: 预留给后续功能的附加状态项

扩展规则：

- 各 slot 只接收结构化状态项，不直接拼接完整展示字符串。
- 展示顺序固定，避免每个功能自由插队。
- 窄终端下优先保留 `mode slot`、`workflow slot` 与当前最关键的 context slot，其余 slot 可截断、折叠或隐藏。

## Interaction Rules

- `Input context bar` 负责“当前正在发生什么”，例如 leader 序列等待、当前模式下的快捷键提示、局部 notice。
- `Input info bar` 负责“输入邻近的短状态”，例如当前模型、token 摘要与右侧运行态图标。
- `Bottom status bar` 负责“系统当前处于什么全局上下文”，例如 mode、当前 workflow step、route、project 与 item 摘要。
- overlay 激活时，上下文带可切换为 overlay 专属提示；底部状态带仍保留基础状态摘要。
- `Sidebar` 收起或展开不应影响左列 `Context bar + Input shell` 与全宽 `Bottom status bar` 的结构。
- 输入区内部的滚动仅在 `Input shell` 上部 viewport 内发生，不应改变 `Input info bar` 和 `Bottom status bar` 的固定高度。
- 输入态的 `Up/Down` 与鼠标滚轮都应服务于输入 viewport 本身，而不是退化成 `Response` 面板滚动。

## Relationship To Existing Specs

- `docs/specs/omega-tui-runtime-experience.md` 应更新为：状态摘要的主要承载区位于输入框下方，而不是顶部 header。
- `docs/specs/omega-tui-collapsible-sidebar.md` 中提到的状态栏摘要，应理解为新的底部状态带，而非旧顶部 header。
- `docs/specs/omega-tui-overlay-popups.md` 中的短时提示，应优先复用输入上下文带，而不是污染底部状态带。
- `docs/specs/omega-theme-package.md` 应承接输入框、上下状态条、边框形态和状态语义色的共享视觉令牌，避免这些定义继续散落在 `omega-tui` 的单个渲染文件中。
- `docs/specs/omega-workflow-package.md` 应承接 workflow step 的定义、外部配置与阶段推进模型；底部状态带只消费结构化阶段摘要。

## Task Planning Impact

建议新增：

- `Task 15B-17`: `omega-tui` — 输入上下文带与底部状态带重构

建议依赖关系：

- `Task 15B-17` 依赖 `Task 15B-16`
- `Task 15B-12` 应建立在 `Task 15B-17` 之上，以便会话统计接入新的底部状态槽

推荐实现顺序：

1. 抽离底部状态模型与渲染入口
2. 把原顶部 header 的模型名与运行态迁到底部状态带
3. 把原输入下方 hint / notice 上移为输入上下文带
4. 清理旧 header 中遗留的 `KM / Focus / Mode / Omega Agent`
5. 让未来状态扩展改为接入 slot，而不是继续直接拼接文本
6. 让输入框、上下状态条和相关语义色迁移到 `omega-theme` 的组件级令牌定义中

## Technical Decisions

| Decision | Choice | Rationale |
|---------|--------|-----------|
| top header treatment | remove dense top header | 降低视觉噪音，避免顶部成为杂项信息堆积区 |
| prompt placement | move hints above input | 与当前输入动作就近，减少视线跳转 |
| status placement | place system summary below input | 为后续状态扩展提供稳定底座 |
| future extensibility | slot-based bottom status model | 避免后续继续手工拼接长字符串 |

## Testing Strategy

- `omega-tui` 单测：验证新布局下输入上下文带、输入框信息栏与底部状态带都拥有稳定高度。
- `omega-tui` 单测：验证旧顶部 header 被移除后，`mode` 位于底部状态带最左侧，模型名仅出现在输入框信息栏，而运行态图标位于输入框信息栏最右侧。
- `omega-tui` 单测：验证 leader pending / notice 提示进入输入上下文带，而不是底部状态带。
- `omega-tui` 单测：验证窄终端下底部状态带至少保留 `mode` 与关键全局摘要，输入框信息栏仍保留模型/token 和运行态图标。

---

### Change Log
- 2026-04-10: 现行实现已进一步收敛为 `Main shell(左列 Response/Context/Input shell + 右列 full-height Sidebar) -> Bottom status bar`；`Input shell` 现为共享边框输入容器，底部 `Input info bar` 带有上下左右内边距，承载 `model + token` 与右对齐 state icon，token 仅保留 `k` 值且段间只用固定空格分隔；`mode` 已回到底部状态带最左侧，底部状态带不再重复展示 model。输入区现补齐多行 viewport 语义：`Shift+Enter` 插入换行，`↑/↓` 按可视行移动光标，长输入会在壳层内软换行，并在内容超出 `4` 行可见高度时支持自动跟随与输入区内滚轮滚动。
- 2026-04-09: 现行实现已进一步收敛为 `Main shell(左列 Response/Context/Input + 右列 full-height Sidebar) -> Bottom status bar`；`Context bar` 固定两行并允许换行，`Input` 固定 6 行高，且 `Context bar` / `Bottom status bar` 背景都与 `Agent Response` panel 对齐。
- 2026-03-19: `Task 15B-17` 完成实现，主布局改为 `Main content -> Input context bar -> Input box -> Bottom status bar`，底部状态带以 slot 化 segment 渲染模型名与运行态。
- 2026-03-19: 新增输入上下文带与底部状态带布局规格，规划 TUI 底部区域的统一重构方向。
- 2026-03-19: 补充与 `omega-theme` 的关系，明确输入框和上下状态条的视觉令牌后续应由独立主题包统一管理。
- 2026-03-19: 补充 workflow slot 规划，要求底部状态带可显示当前执行阶段，并与 `omega-workflow` 的结构化阶段模型对齐。
