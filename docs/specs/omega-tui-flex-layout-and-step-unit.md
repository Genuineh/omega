---
content_revision: 174
created: 2026-06-05
generation_id: gen_000087_r000174
language: bilingual
last_verified_commit: d8c30e3e9e310ce38cffa965be4688ed55a87787
owner: omega-team
projection_version: 87
related_prds: "[]"
source_doc_id: "spec:docs-specs-omega-tui-flex-layout-and-step-unit"
source_path: docs/specs/omega-tui-flex-layout-and-step-unit.md
status: draft
supersedes: "[]"
updated: 2026-06-05
---

# Omega TUI Flex Layout + Step Unit 规范

## Overview

经过 `Task 15B-51`、`Task 15B-61 ~ 15B-64`、`Task 39 ~ 39I`、`Task 47 ~ 51` 几轮迭代，`omega-tui` 已经有 `Panel` / `Section` / `Card` / `Frame` 四个 component 抽象，但每个 panel 内部仍然 ad-hoc 地用 `Layout::default().constraints(...)` 拼装子区域，没有一套统一的「弹性布局」原语。`Response` 面板里 Step / FinalAnswer / Thinking / Command 等结构也仍然以「多行 inline 渲染」为主，详情通过「body 段折叠 / 展开」展示。

本轮用户提出三个新约束：

1. **流程展示区 (Fixed) + 可变状态区 (Variable) + 详情弹窗 (Overlay)** 应该成为每个 step unit 的固定三段式。
2. **同一个 step 内部的多个 tool 调用的全部上下文应当保留在 panel 内部**（不是用 inline 列表展开），UI 上只露出**当前正在跑的那一个**。
3. **所有详情都进入弹窗**，弹窗内有结构化浏览（类似 DocumentNavigator 的 Rail + Content），不靠 inline 折叠。

把上述三条横向归纳，可以得到一个统一的设计语言：**任何 TUI 结构 = 固定区 + 可变区 + 详情弹窗；任何容器都通过同一套 flex 原语装配。** 本规格定义这套布局原语和 step unit 的具体落地形态。

## Goals

- 为 `omega-tui` 的渲染层提供一套 **flex-like 布局原语**（`FlexContainer` / `FlexSize` / `FlexChild`），让所有 panel / 弹窗 / 步骤单元用同一套抽象装配，而不是每个 panel 自己手写 `Layout::default().constraints(...)`。
- 把 `Response` 面板里的 **Step unit** 落地成「固定区（kind + state） + 可变区（当前 tool / 当前 status） + 详情弹窗（完整 tool / subflow / scene / output）」三段式。
- 其它 8 个 `MsgKind`（FinalAnswer / Thinking / Command / User / Agent / Error / Separator / Routing）也套用同一套三段式骨架，差异只在 header 颜色 / glyph / 可变区的内容。
- 详情全部走 **弹窗**，弹窗内复用现有 `OverlayState` / `DocumentNavigatorOverlay` 的 Rail + Content 模式，**支持嵌套**（Step 详情里选中一个 tool 弹 ToolRunDetail）。
- 提供至少 3 个回归测试：(a) 单个 Step unit 的固定+可变两行布局，(b) Step unit 详情弹窗显示 rail 列表 + content 区，(c) AgentResponse 嵌套多个 step unit 时空白行节奏正确。

## Non-Goals

- 不重写现有的 `Panel` / `Section` / `Card` / `Frame` 抽象；flex 原语作为新一层叠加，旧的 chrome 构造继续工作。
- 不在这一轮把 `Status` / `Sidebar` / `Input` 也改成 flex 布局；只把 `Response` 内的 step unit 和新的 `StepDetailOverlay` 用上 flex。
- 不重写 `omega-session` 端的 `ToolRun` / `StepSubflow` 数据模型；UI 端只读不改 typed contract。
- 不在弹窗里直接渲染 provider 私有协议或 XML 风格的 tool-call 标记。
- 不要求 T-52 ~ T-57 这一组任务一次性合并到一个 PR；可以拆 PR 但每 PR 必须保持编译 + 测试绿。

## Architecture

### A. Flex Layout Primitives (Task 52)

新文件 `crates/omega-tui/src/render/flex.rs`：

```rust
/// Flex direction: how children stack inside a container.
pub enum FlexDirection { Row, Column }

/// Size policy: how a child claims space within a flex container.
pub enum FlexSize {
    /// Take only what the content needs.
    Fixed,
    /// Take exactly N rows/cols.
    Length(u16),
    /// Take a fraction of the remaining space (0.0–1.0).
    Fraction(f32),
    /// Take all remaining space.
    Fill,
}

/// A child of a flex container.
pub struct FlexChild {
    pub size: FlexSize,
    pub render: Box<dyn FnOnce(&mut Frame, Rect)>,
}

pub struct FlexContainer {
    pub direction: FlexDirection,
    pub gap: u16,
    pub children: Vec<FlexChild>,
}

impl FlexContainer {
    /// Compute the per-child rects and render each. The math is the
    /// same one used by CSS flexbox:
    /// 1. Allocate `Fixed` and `Length` sizes first.
    /// 2. Sum the requested `Fraction` weights.
    /// 3. Distribute remaining space proportionally, rounding down.
    /// 4. If rounding leaves leftover rows/cols, hand them to the
    ///    last `Fraction` or `Fill` child.
    pub fn render(&self, frame: &mut Frame, area: Rect) -> Vec<Rect>;
}
```

特性：

- `gap` 在每对相邻 child 之间插入空行/空列（用 panel 背景色填充）。
- `Fixed` 模式调用一个 `content_size` callback 让 child 报出自己的 intrinsic size，再在剩余空间里分配（这个 callback 在简单场景下用 1 row 即可，复杂场景可以传）。
- 不引入 trait object 之外的动态分发；`FlexChild` 用 `Box<dyn FnOnce>` 是为了避免泛型传染。

### B. Step Unit (Task 53)

新文件 `crates/omega-tui/src/render/step_unit.rs`，导出 `StepUnit`：

```rust
pub struct StepUnit {
    /// Always-shown header row: kind glyph + formatted title + state.
    pub fixed: FlexChild,         // FlexSize::Length(1)
    /// Current-state row: which tool is running, or N/M summary.
    pub variable: FlexChild,      // FlexSize::Length(1)
    /// Optional detail request (None = no popup available).
    pub detail_target: Option<StepDetailRequest>,
}

pub struct StepDetailRequest {
    pub section_id: String,
    pub kind: MsgKind,
}
```

`StepUnit::render(&self, frame, area)` 用 `FlexContainer` 把 fixed + variable 两行垂直排好。`fixed` 用 `header_color` 上色（沿用 T-51 的 per-kind 配色），`variable` 用 `colors.text` 普通色 + 数据层 body_indent。

### C. Per-MsgKind Unit Adaptation (Task 54)

把 9 个 `MsgKind` 全部映射成 `StepUnit` 的实例化：

| MsgKind | fixed 区内容 | variable 区内容 | 详情弹窗 |
|---|---|---|---|
| `Step` | `◉ step workflow_id Section ●` | running tool name / `5/5 tools complete` summary | StepDetail (Tools / Subflows / Scene / Output) |
| `FinalAnswer` | `◆ final workflow_id Section ●` | `回答预览（前 80 字符）` | StepDetail (Output / Scene) |
| `Thinking` | `◦ reasoning workflow_id Section ●` | `▸ summary line` | StepDetail (Output / Thinking transcript) |
| `Command` | `◆ command builtin Section ●` | `running N lines` 或 `complete N lines` | StepDetail (Output / Tool runs) |
| `User` | `▶ Hello, agent.` (no header) | 整段 text 自身 | DetailOverlay (full text) |
| `Agent` | `Hello, human` (no header) | 整段 text 自身 | DetailOverlay (full text) |
| `Error` | `✗ Something broke.` (no header) | 整段 text 自身 | DetailOverlay (full text) |
| `Separator` | `─` divider | (none) | (none) |
| `Routing` | `route workflow Section ●` | `scene / items` meta lines | DetailOverlay (meta lines) |

「no header」类（User / Agent / Error / Separator）走 `FlexSize::Fixed` 模式（自适应 1 行），Variable 区缺省；详情弹窗通过按 Enter 触发。

### D. 详情弹窗 (Task 55)

新增 `OverlayState::StepDetail(StepDetailOverlay)`，结构复刻 `DocumentNavigatorOverlay` 的双栏：

```rust
pub struct StepDetailOverlay {
    pub origin_panel: Panel,
    pub section_id: String,
    pub title: String,              // "Step: workflow_id/Section"
    pub rail: Vec<StepDetailRailItem>,
    pub selected: usize,
    pub content: StepDetailContent,
    pub scroll: usize,
}

pub struct StepDetailRailItem {
    pub label: String,              // "Tools (3)" / "Subflows (2)" / "Scene" / "Output" / "Diagnostics"
    pub kind: StepDetailRailKind,
}

pub enum StepDetailRailKind { Tools, Subflows, Scene, Output, Diagnostics }

pub enum StepDetailContent {
    Tools(Vec<ToolRunSummary>),
    Subflows(Vec<SubflowSummary>),
    Scene(SceneContext),
    Output(Vec<String>),            // raw text lines
    Diagnostics(StepDiagnostics),
}
```

弹窗布局（用 FlexContainer 装配）：

```
┌─ Step: workflow_id / Section ────────────────────┐
│ [Flex: Row, gap=2]                                │
│   │ [Flex: Column, width=24]                      │
│   │   > Tools (3)                                 │  ← Rail
│   │     Subflows (2)                              │
│   │     Scene                                     │
│   │     Output                                    │
│   │     Diagnostics                               │
│   │                                               │
│   [Flex: Column, Fill, gap=1]                     │
│     # Tool 1: search_knowledge (complete)         │  ← Content
│     invocation: ...                               │
│     result: ...                                   │
│     [gap]                                         │
│     # Tool 2: file_write (running)                │
│     ...                                           │
│                                                   │
│ [Flex: Length(1)]                                 │
│   Enter=open tool detail  Esc=back  ↑/↓=rail nav  │  ← Footer
└───────────────────────────────────────────────────┘
```

嵌套：rail 选中 Tools 后按 Enter 打开某个 ToolRun 的 `DetailOverlay`（**复用现有** `open_tool_run_detail` 机制，不重写）；Esc 一层一层退。

### E. AgentResponse 容器 (Task 56)

`render_response_panel` 改成：

```
[Flex: Column, gap=1]                     ← 整个 response panel 内容
  unit_1  (Step unit, 2 rows)
  unit_2  (Step unit, 2 rows)
  ...
```

每个 unit 的 render 走 `StepUnit::render`；unit 之间靠 `gap=1` 留 1 行空（原 blank_list_item 逻辑）。response panel 自身继续用 `PanelChrome` 包外框（不变）。

### F. Wire Enter → 详情 (Task 57)

`ResponseLineAction::OpenStepDetail(section_id)` 新增，event handler 在 cursor 落在某个 step unit 的 variable 行上 + 按 Enter 时 push `StepDetailOverlay`。其他 unit（User/Agent/Error）走现有 `OpenDetailOverlay` 路径（已经是弹窗了），无需新逻辑。

## Data-Model

不变。复用现有：

- `ResponseDisplayLine` (T-39 引入) — 仍然是数据流的基础。
- `ResponseCard` / `ResponseCardSection` — 仍然由 data layer 装配。
- `tool_runs: Vec<ToolRun>` / `step_subflows: Vec<StepSubflowStatus>` — 在 `App` 上仍然全量保存；新 UI 只读不改。
- `OverlayState` — 扩展一个 `StepDetail(StepDetailOverlay)` 变体。

新增的 typed contract（在 `omega-session` 里）：

```rust
/// Snapshot of one step unit's tools/subflows/etc. that the overlay
/// renders. Built on demand by `App::snapshot_step_detail(section_id)`.
pub struct StepDetailSnapshot {
    pub section_id: String,
    pub title: String,
    pub tools: Vec<ToolRunSummary>,
    pub subflows: Vec<SubflowSummary>,
    pub scene: Option<SceneContext>,
    pub output: Vec<String>,
    pub diagnostics: Option<StepDiagnostics>,
}
```

- `ToolRunSummary` / `SubflowSummary` 是新结构，从 `ToolRun` / `StepSubflowStatus` 投影。
- `SceneContext` 来自现有 `step_diagnostics` / `context_diagnostics`。
- `StepDiagnostics` 已经存在（`omega-session`）。

## Testing

按每 Task 配 4 类测试：

1. **FlexContainer unit test** (T-52)
   - 3 个 Length child → 高度总和 = container 高度
   - 1 个 Length(2) + 1 个 Fill → Fill 拿到 (container - 2)
   - 1 个 Fraction(0.5) + 1 个 Fraction(0.5) → 各拿一半
   - 0 高度 container → 返回空 Vec<Rect>

2. **StepUnit unit test** (T-53)
   - 完整 unit 渲染：fixed + variable 两行
   - 0 高度 area → 返回空
   - detail_target = None → 不渲染 detail handle

3. **Per-MsgKind unit test** (T-54)
   - 9 个 kind 各 1 个 snapshot，确认 fixed/variable 内容正确
   - Step 跑状态 vs 结束状态时 variable 区内容不同

4. **StepDetailOverlay integration test** (T-55)
   - 打开 StepDetail → rail 显示 ["Tools (N)", "Subflows (M)", "Scene", "Output", "Diagnostics"]
   - 切换 rail → content 区显示对应内容
   - 在 Tools rail 按 Enter → 嵌套 ToolRunDetail
   - Esc 退到外层 StepDetail，Esc 再退关闭弹窗

5. **AgentResponse 容器 integration test** (T-56)
   - 推 3 个不同 kind 的消息 → AgentResponse 内 3 个 unit 排成 Flex column
   - unit 之间 1 行 gap
   - 滚动到底部自动锚定到最新 unit

6. **Wire Enter test** (T-57)
   - cursor 在 Step unit 的 variable 行 + Enter → push StepDetailOverlay
   - cursor 在 User/Agent/Error 行 + Enter → push DetailOverlay（既有路径）
   - 弹窗中 Esc → 回到外层

回归测试：T-52 ~ T-57 完成后所有原 215 个 omega-tui 测试仍然 100% 绿。

## Change Log

- 2026-06-05: 初稿。本规格定义 `omega-tui` 渲染层新增的 flex-like 布局原语、StepUnit 三段式组件、StepDetailOverlay 双栏弹窗，以及对 `AgentResponse` 容器的重组方案。配套 Task 52 ~ 57 落地。
