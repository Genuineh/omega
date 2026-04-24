---
content_revision: 120
created: 2026-03-19
generation_id: gen_000046_r000120
last_verified_commit: N/A
owner: omega-team
projection_version: 46
related_prds: []
source_doc_id: "spec:docs-specs-omega-tui-overlay-popups"
status: implemented
supersedes: []
updated: 2026-03-19
---

# Omega TUI Overlay Popup Specification

## Overview

`omega-tui` 目前已经有主响应区、Todo/Sidebar、状态栏和模态 keymap，但仍缺少一层适合“短时、聚焦、后续交互”的浮动界面载体。若继续把搜索、确认、详情查看、条目选择、轻量表单等交互硬塞进常驻面板，主界面会越来越拥挤；若全部退化成日志或状态栏提示，又会让用户无法在当前上下文内完成后续操作。

本规格定义一套统一的 overlay / popup 系统：在同一个终端 frame 内，以浮动窗口的形式显示短时交互内容，并通过统一的焦点捕获、键盘路由、遮罩和尺寸策略承载后续交互能力。

## Goals

- 为 `omega-tui` 提供统一的浮动弹窗基础设施，承载搜索、确认、详情查看、picker 和轻量输入。
- 让短时交互不必抢占常驻面板布局，避免侧边栏和主面板继续膨胀。
- 明确 overlay 的焦点、键盘、鼠标和关闭规则，避免各功能各做一套临时实现。
- 保持 `omega-session` 和其他核心 crate 前端无关，把弹窗语义留在 `omega-tui` 本地。
- 为后续 `Task 15B-11` 搜索、`Task 15B-16` Activity 详情交互和未来确认类流程提供统一承载层。

## Non-Goals

- 不实现操作系统级新窗口、终端分屏或独立子进程 UI。
- 不支持任意深度的 modal stack；首版不追求复杂的多层嵌套弹窗。
- 不把所有复杂表单都迁进弹窗；重度编辑仍应留在主输入区或专用面板。
- 不在本规格中定义具体业务数据协议，例如 background 任务详情字段或 inbox message schema。

## Architecture

### Components

- `OverlayManager`：`omega-tui` 本地状态，负责当前活动 overlay、打开/关闭生命周期和焦点捕获。
- `OverlaySurface`：对单个弹窗的抽象，包含标题、尺寸策略、阻塞性、关闭动作和内容类型。
- `OverlayRenderer`：在主 frame 之上绘制遮罩、边框、标题和 overlay 内容。
- `OverlayActionRouter`：在 overlay 激活时优先消费键盘/鼠标事件，再决定是否向底层 panel 透传。

### Core Model

首版建议采用“单活动 overlay”模型，而不是完整堆栈：

- 任一时刻最多只允许一个 blocking overlay 处于激活状态。
- 若当前已有 overlay，再次打开新 overlay 时，默认应拒绝、替换或显式关闭旧 overlay，而不是无限叠加。
- `Esc`、确认动作和外层关闭动作都应通过同一生命周期路径回收 overlay 状态。

这能显著降低实现复杂度，并避免焦点、背景快捷键和尺寸计算迅速失控。

## Overlay Types

首版不要求全部实现，但基础设施应能覆盖以下类型：

- `SearchPopup`：面向 `Task 15B-11` 的搜索输入框和结果摘要。
- `ConfirmDialog`：确认中断、删除、关闭等短时二选一动作。
- `DetailDialog`：展示 background/inbox/team/worktree/activity 条目的详细信息。
- `PickerPopup`：在少量候选项之间做快速选择，例如 Activity view 或 section 目标。
- `InputPrompt`：轻量单字段输入，不取代主输入框，只用于短时配置或命名。

## Layout Rules

### Placement

- overlay 应绘制在主布局之上，而不是重新切分整个 terminal。
- 默认居中显示，保留四周可见背景，帮助用户保持上下文感知。
- 背景使用轻量遮罩或 dim 处理，提示当前交互已被 overlay 捕获。

### Size Strategy

- 首版使用预设尺寸档位：`small`、`medium`、`large`。
- 宽度和高度都应受终端尺寸限制，避免在窄终端下溢出。
- 当终端过窄时，overlay 可退化为近似全宽，但仍保留明确边框和标题。

### Background Interaction

- blocking overlay 激活时，底层 panel 不接收滚动、焦点切换或 leader 快捷键。
- 背景内容继续可见，但应被视为只读上下文，而不是可交互区域。

## Interaction Model

### Focus Ownership

- overlay 激活时，交互焦点从 `Response` / `Sidebar` / `Activity` 切换到 overlay。
- `Tab`、方向键、确认和取消动作在 overlay 内部解析，不再参与底层 panel 循环。
- overlay 关闭后，焦点应恢复到打开它之前的顶层区域。

### Keyboard Routing

- `Task 15B-13` 的 keymap 层应成为 overlay 的统一键盘入口。
- 当 overlay 激活时，事件顺序应为：overlay-local handling -> global cancel/close -> drop，不应继续下沉到底层 panel。
- `Esc` 默认表示关闭当前 overlay；若 overlay 明确要求二次确认，则通过内部状态处理，而不是把 `Esc` 透传给主界面。
- 带文本输入的 overlay 应拥有独立输入捕获，不应与主输入框共享字符消费。

### Mouse Routing

- 点击 overlay 内部元素只作用于 overlay。
- 点击遮罩区域的行为由 overlay 类型决定：
  - `ConfirmDialog` 默认不应通过外部点击关闭。
  - `DetailDialog` 可选支持点击遮罩关闭。
- 鼠标滚轮不应穿透到背景 panel。

## Relationship To Existing TUI Surfaces

- `Response`：继续承载主对话与主要结果，不用于短时确认或搜索输入。
- `Sidebar / Activity`：继续承载持续可见的运行态信息；需要 drill-down 时再打开 `DetailDialog`。
- status bar：只展示摘要与提醒，不承载复杂交互；需要进一步操作时可提示打开 overlay。

换言之，overlay 解决的是“短时浮动交互”，而不是“新的常驻信息区”。

## Relationship To Roadmap Tasks

推荐把 overlay 基础设施作为独立前置任务，插在 `Task 15B-13` 与后续高阶交互之间：

- `Task 15B-11` 搜索：推荐基于 `SearchPopup` 实现，而不是挤占常驻输入区域。
- `Task 15B-16` Activity 面板：列表继续留在侧边栏，详情、确认、快速切换则通过 overlay 完成。
- 未来 background / inbox / team / worktree 的细节查看，应优先落到 `DetailDialog`，而不是新增固定面板。

## Task Planning Impact

建议新增：

- `Task 15B-16A`: `omega-tui` — 浮动弹窗 / Overlay 基础设施

建议依赖关系：

- `Task 15B-16A` 依赖 `Task 15B-13`
- `Task 15B-11` 依赖 `Task 15B-16A`
- `Task 15B-16` 依赖 `Task 15B-16A`

推荐顺序：

1. `Task 15B-13`：统一 keymap / mode / leader 路由
2. `Task 15B-16A`：overlay 弹窗基础设施
3. `Task 15B-16`：Activity 面板与状态徽章基础
4. `Task 15B-11`：搜索 popup 与面板内搜索交互

## Technical Decisions

| Decision | Choice | Rationale |
|---------|--------|-----------|
| popup model | single active overlay | 先控制复杂度，避免 modal stack 失控 |
| ownership | `omega-tui` local UI state | overlay 是纯前端布局与交互语义 |
| key routing | overlay-first on top of keymap | 保持输入边界统一，不回到硬编码分支 |
| detail strategy | drill-down via popup, not new panel | 防止面板数量膨胀 |
| narrow terminal fallback | shrink overlay, keep border/title visible | 保持可用性与上下文感 |

## Testing Strategy

- `omega-tui` 单测：验证 overlay 打开后底层 panel 不再接收焦点与滚动事件。
- `omega-tui` 单测：验证 `Esc` 关闭 overlay 后焦点恢复到打开前区域。
- `omega-tui` 单测：验证带输入的 overlay 不会把字符泄漏到主输入框。
- `omega-tui` 单测：验证窄终端下 overlay 仍能在限制尺寸内渲染。
- 手动验证：运行 `cargo run -p omega-tui`，确认搜索、详情和确认类交互在浮动窗口中可完成闭环。

---

### Change Log
- 2026-03-19: 新增 overlay / popup 规格，作为 `omega-tui` 短时浮动交互的统一基础设施。
- 2026-03-19: `Task 15B-16A` 已实现单活动 overlay 状态、遮罩渲染、搜索 popup、确认弹窗与 overlay-first 事件路由。
