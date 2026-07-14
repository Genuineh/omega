---
content_revision: 174
created: 2026-06-05
generation_id: gen_000087_r000174
language: bilingual
last_verified_commit: d8c30e3e9e310ce38cffa965be4688ed55a87787
owner: omega-team
projection_version: 87
related_prds: "[]"
source_doc_id: "adr:docs-decisions-009-tui-flex-layout-primitives"
source_path: docs/decisions/009-tui-flex-layout-primitives.md
status: accepted
supersedes: "[]"
updated: 2026-06-05
---

# 009: TUI Flex Layout Primitives + Step Unit 三段式

## Status

Accepted (2026-06-05).

## Context

`omega-tui` 渲染层经过 4 轮迭代（`Task 15B-51`、`Task 15B-61~64`、`Task 39~39I`、`Task 47~51`）已经有 `Panel` / `Section` / `Card` / `Frame` 四个 component 抽象和 per-kind header 配色，但每个 panel 内部仍然 ad-hoc 用 `Layout::default().constraints(...)` 拼装子区域。

`Response` 面板里 Step / FinalAnswer / Thinking / Command 等结构目前以「多行 inline 渲染」为主，详情靠「body 段折叠 / 展开」展示。多轮 review 后用户提出三个新约束：

1. **流程展示区 (Fixed) + 可变状态区 (Variable) + 详情弹窗 (Overlay)** 应该成为每个 step unit 的固定三段式。
2. **同一个 step 内部的多个 tool 调用的全部上下文应当保留在 panel 内部**（不展开），UI 上只露出**当前正在跑的那一个**。
3. **所有详情都进入弹窗**，弹窗内用结构化浏览（类似 `DocumentNavigatorOverlay` 的 Rail + Content），不靠 inline 折叠。

横向归纳后，我们得到一个统一的设计语言：**任何 TUI 结构 = 固定区 + 可变区 + 详情弹窗；任何容器都通过同一套 flex 原语装配。** 这就是本 ADR 接受的核心抽象。

## Decision

### 1. 新增 Flex Layout Primitives

新文件 `crates/omega-tui/src/render/flex.rs`，定义：

```rust
pub enum FlexDirection { Row, Column }
pub enum FlexSize { Fixed, Length(u16), Fraction(f32), Fill }
pub struct FlexChild { pub size: FlexSize, pub render: Box<dyn FnOnce(&mut Frame, Rect>> }
pub struct FlexContainer { pub direction: FlexDirection, pub gap: u16, pub children: Vec<FlexChild> }
```

布局算法：先分配 `Length` 确定的行/列；剩余空间按 `Fraction` 权重比例分配（向下取整）；余数交给最后一个 `Fraction` 或 `Fill` 子项。`gap` 在相邻 child 之间插入 1 行/列空白（用 panel 背景色填充）。

### 2. Step Unit = Fixed + Variable

新文件 `crates/omega-tui/src/render/step_unit.rs`，导出 `StepUnit`：

```rust
pub struct StepUnit {
    pub fixed: FlexChild,                  // FlexSize::Length(1), kind glyph + title + state
    pub variable: FlexChild,               // FlexSize::Length(1), current tool / summary
    pub detail_target: Option<StepDetailRequest>,
}
```

`StepUnit::render` 用 `FlexContainer` 把 fixed + variable 两行垂直排好。`fixed` 用 `header_color` 上色（沿用 T-51 的 per-kind 配色），`variable` 用普通 `text` 色 + 数据层 body_indent。

### 3. 9 个 MsgKind 全部映射成 StepUnit 实例化

| MsgKind | fixed 区 | variable 区 | 详情弹窗 |
|---|---|---|---|
| `Step` | `◉ step workflow_id Section ●` | running tool / `N/M tools complete` | StepDetail (Tools / Subflows / Scene / Output / Diagnostics) |
| `FinalAnswer` | `◆ final workflow_id Section ●` | 回答预览前 80 字符 | StepDetail (Output / Scene) |
| `Thinking` | `◦ reasoning workflow_id Section ●` | `▸ summary` | StepDetail (Output) |
| `Command` | `◆ command builtin Section ●` | `running N lines` / `complete N lines` | StepDetail (Output) |
| `User/Agent/Error` | (badge + 整段 text 自身，1 行) | 缺省 | DetailOverlay (full text) |
| `Separator` | `─` divider (1 行) | 缺省 | 无 |
| `Routing` | `route workflow Section ●` (1 行) | meta lines (1 行) | DetailOverlay (meta) |

「no header」类 (User/Agent/Error/Separator) 走 `FlexSize::Fixed` 模式（1 行）；详情弹窗通过 Enter 触发（User/Agent/Error 走现有 `OpenDetailOverlay` 路径）。

### 4. StepDetailOverlay 双栏弹窗

新增 `OverlayState::StepDetail(StepDetailOverlay)`，结构复刻 `DocumentNavigatorOverlay`：

```rust
pub struct StepDetailOverlay {
    pub origin_panel: Panel,
    pub section_id: String,
    pub title: String,
    pub rail: Vec<StepDetailRailItem>,   // Tools / Subflows / Scene / Output / Diagnostics
    pub selected: usize,
    pub content: StepDetailContent,      // 按 rail 选中项填充
    pub scroll: usize,
}
```

弹窗内部用 FlexContainer 装配：Row [Column(Rail, width=24) + Column(Content, Fill)] + Column(Footer, Length(1))。

嵌套：rail 选中 Tools 后按 Enter 打开某个 ToolRun 的 `DetailOverlay`（**复用现有** `open_tool_run_detail` 机制，不重写）；Esc 一层一层退。

### 5. AgentResponse = Flex Column of StepUnits

`render_response_panel` 内部改用 `FlexContainer { direction: Column, gap: 1 }` 装配所有 unit；unit 之间靠 gap=1 留 1 行空（原 `blank_list_item()` 逻辑）。

外层 `PanelChrome` 不变。

### 6. Enter → 详情

新增 `ResponseLineAction::OpenStepDetail(section_id)`；cursor 在 Step unit 的 variable 行 + Enter → push `StepDetailOverlay`。其他 kind 继续走 `OpenDetailOverlay`（既有路径，不变）。

## Alternatives Considered

### 替代 A：把 Layout::default().constraints 直接升级为 helper

保留 `ratatui::Layout`，加一个 `flex_layout!` macro 写 `Length / Fill / Fraction` 语法糖。**否决**：macro 不便于在 `FlexChild` 列表里混用 widget 和自定义 render closure，扩展性差。

### 替代 B：把 Step unit 完全重写成 struct，不走 Flex 原语

直接在 `StepUnit::render` 里写 `Layout::default().direction(Column).constraints([Length(1), Length(1)]).split(area)`。**否决**：8 个其他 kind 都要写类似的 1-2 行 split；flex 原语是统一抽象，长期收益更高（以后 Status / Sidebar / Input 也可以逐步接入）。

### 替代 C：详情继续用 inline body，不做弹窗

保留 T-51 的「单行 step header + 详情在 body 行」结构。**否决**：用户明确要求「所有详情都进弹窗」+「弹窗内结构化浏览」。inline body 在长 step（10+ tools）下要么折成很多行，要么用户按 Enter 展开，本质上还是单层结构化。

### 替代 D：详情弹窗不走 OverlayState，直接挂在 App 上

把 StepDetail 作为 `App` 的新字段 `step_detail: Option<StepDetailOverlay>`。**否决**：现有 `OverlayState` + `push_overlay` / `pop_overlay` 已经有 stack + 嵌套 + 焦点路由；新字段等于重做一遍。

## Consequences

### 正面

- **统一抽象**：所有 panel / 弹窗 / 步骤单元用同一套 `FlexContainer` 装配，新人看一个 `flex.rs` 就能理解整个 TUI 的布局。
- **可嵌套**：`StepUnit` 内部固定 + 可变两行；`StepDetailOverlay` 内部 Rail + Content；`DocumentNavigatorOverlay` 也可以后续迁到 flex。任意层级一致。
- **测试友好**：flex 布局数学纯函数（输入 rect + children 输出 Vec<Rect>），单测覆盖率高；StepUnit / Overlay 也是纯 render 函数，不依赖 IO。
- **不破坏现有契约**：`Panel` / `Section` / `Card` / `Frame` 都不动；旧的 215 个 omega-tui 测试不需修改。

### 负面

- **新增一层抽象**：写 panel 要先理解 `FlexChild` / `FlexSize` 语义；新人 onboarding 略复杂。
- **Box<dyn FnOnce> 引入动态分发**：render closure 不能复用，但每帧创建 `Box` 代价小（panel 数量个位数到几十个），可接受。
- **数据模型不集中**：弹窗需要的 `StepDetailSnapshot` 是 on-demand 投影，data layer 已经有 80% 现成结构（`ToolRun` / `StepSubflowStatus` / `StepDiagnostics`），但需要新加一个 `App::snapshot_step_detail` 方法。

## Implementation Plan

按 Task 52 ~ 57 落地，每 Task 独立 PR：

| Task | 内容 | 依赖 | 估时 |
|---|---|---|---|
| T-52 | `crates/omega-tui/src/render/flex.rs` + 4 个 unit test | — | 0.5 day |
| T-53 | `crates/omega-tui/src/render/step_unit.rs` + Step 单元渲染 | T-52 | 0.5 day |
| T-54 | 9 个 MsgKind 的 StepUnit 适配 + per-kind snapshot | T-53 | 1 day |
| T-55 | `StepDetailOverlay` + `OverlayState::StepDetail` + Rail/Content 装配（Flex） | T-52 | 1 day |
| T-56 | `render_response_panel` 改用 `FlexContainer { Column, gap=1 }` | T-53, T-54 | 0.5 day |
| T-57 | `OpenStepDetail` action + Enter 路由 + 嵌套 ToolRun 复用 | T-55, T-56 | 0.5 day |

总计 ~4 个工作日，6 个独立 PR。

## References

- `docs/specs/omega-tui-flex-layout-and-step-unit.md` — 设计规格
- `docs/specs/omega-tui-visual-refresh.md` — TUI 视觉基线
- `docs/specs/omega-tui-step-tool-thinking-refinement.md` — 上一轮 step/thinking 精修
- `docs/decisions/008-tui-component-architecture-refactor.md` — Panel/Section/Card 抽象 ADR
- `docs/TODO.md` — Task 52 ~ 57
