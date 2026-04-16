---
content_revision: 101
created: 2026-03-19
generation_id: gen_000016_r000101
last_verified_commit: N/A
owner: omega-team
projection_version: 16
related_prds: []
source_doc_id: "spec:docs-specs-omega-tui-collapsible-sidebar"
status: implemented
supersedes: []
updated: 2026-03-19
---

# Omega TUI Collapsible Sidebar Specification

## Overview

当前 `omega-tui` 已经有右侧 `Todos` + `Logs` 分栏，但它仍然更像两个独立面板，而不是一个统一的辅助工作区。为了让后续 `Activity` 体系有稳定落点，并让窄屏或专注输入时的视觉负担可控，右侧区域需要升级为一个可整体收起的侧边栏 shell。

本规格定义一个两级收起模型：首先可以通过快捷键整体收起或展开右侧侧边栏；其次在侧边栏内部，`Todos` 与 `Logs` 也可以各自折叠为顶部图标入口。展开状态下，两个区域在同一侧边栏内做垂直弹性排列，而不是继续被视为互不相关的固定面板。

## Goals

- 将当前 `Todos` + `Logs` 提升为统一的右侧侧边栏，而不是两个松散拼接的 widget。
- 支持通过快捷键一键收起/展开整个侧边栏，让用户能在窄屏或专注写作时把主空间让给 `Response`。
- 支持在侧边栏内部把 `Todos` 与 `Logs` 折叠为顶部小图标，并在图标与展开视图之间快速切换。
- 在展开状态下保持 `Todos` 与 `Logs` 的垂直弹性布局，使剩余空间由当前可见内容自然分配。
- 为后续 `Activity` 面板扩展提供壳层模型，让未来 `Skills`、`Delegations`、`Background` 等视图复用同一侧边栏交互。

## Non-Goals

- 不在本规格中直接实现新的日志/待办数据协议；已有 `TodoSnapshot` 与日志更新协议保持不变。
- 不要求本次同时完成鼠标拖拽改宽、复杂动画或多列嵌套导航。
- 不在本次设计中引入完整图标主题系统；初版允许使用 ASCII/Unicode 简化图标。
- 不把侧边栏内部视图选择逻辑下沉到 `omega-core` 或 `omega-session`。

## Sidebar Model

### Sidebar Shell

右侧辅助区从“两个固定面板”升级为单一 `Sidebar` 容器，结构分为三层：

- `Header rail`：位于侧边栏顶部，承载 `Todos`、`Logs` 以及未来运行态视图的轻量入口与紧凑状态。
- `Body`：显示当前展开的 section 内容；若多个 section 同时展开，则在此区域内垂直弹性排列。
- `Collapsed shell state`：当整个侧边栏被收起时，仅保留主面板与状态栏摘要，不再渲染右侧内容区域。

### Section States

每个 section 具有以下状态：

- `Expanded`：参与侧边栏主体布局并显示完整内容。
- `Collapsed`：仅保留顶部图标入口与简短徽记，不占据主体高度。
- `Hidden by shell`：当整个侧边栏收起时，section 不单独保留可见区域。

初始默认状态：

- `Sidebar`: expanded
- `Todos`: expanded
- `Logs`: expanded

## Layout Rules

### Expanded Sidebar

当侧边栏处于展开状态时：

- 左侧仍为 `Response` 主面板。
- 右侧为单一较大的 `Sidebar` 面板。
- `Header rail` 固定在 `Sidebar` 顶部，使用单行轻量文本入口显示各 section 的名称与摘要，入口之间按 tab 顺序横向排列。
- `Body` 在 rail 下方渲染已展开的 section。

### Section Expansion Behavior

- 当 `Todos` 与 `Logs` 都展开时，`Body` 采用纵向弹性分配。
- 默认比例保持接近当前体验：`Todos 40% / Logs 60%`。
- 若其中一个 section 内容明显更短，允许在满足最小高度后把剩余空间让给另一个 section。
- 当仅有一个 section 展开时，它占据整个 `Body`。
- 不允许在侧边栏展开时两个 section 同时都被折叠为空白主体；若用户试图折叠最后一个展开 section，则保持该 section 展开，并向状态栏给出提示。

### Collapsed Sidebar

- 侧边栏整体收起后，`Response` 占据全部主体宽度。
- 状态栏继续显示紧凑 todo 摘要与关键运行态徽章，避免辅助信息完全不可见。
- 焦点必须自动回退到 `Response`，不可落到已隐藏的侧边栏或其内部 section。

## Interaction Model

### Keyboard Actions

定义以下逻辑动作：

- `toggle_sidebar`: 收起或展开整个右侧侧边栏。
- `toggle_todos`: 在侧边栏展开时展开或折叠 `Todos` section。
- `toggle_logs`: 在侧边栏展开时展开或折叠 `Logs` section。
- `focus_sidebar_rail`: 把交互焦点移到侧边栏顶部 rail。
- `cycle_sidebar_section`: 在 rail 内切换当前高亮的 section 文本入口。
- `activate_sidebar_section`: 展开当前高亮 section，或把焦点跳转到该 section。

当前默认交互：

- `←/→`：在 rail 中切换 `Todos` / `Logs`
- `Enter`：激活当前 section
- `x`：折叠或展开当前 section

快捷键绑定策略：

- 该组动作的目标实现应接入 `Task 15B-13` 的模态 keymap 层，而不是继续增加全局裸快捷键。
- `toggle_sidebar`、`focus_sidebar_rail`、section 切换与展开动作应默认放在 `Normal` 模式的 leader 命名空间下。
- 若分阶段实现需要短期直接绑定，应仅作为过渡默认映射存在，并在 `omega-keymap` 接入后统一收口。

相关模式与配置规则见 `docs/specs/omega-tui-modal-keymap.md`。

### Focus Rules

推荐焦点层次：

- `Response`
- `Sidebar rail`
- `Todos` section
- `Logs` section

规则：

- 当侧边栏收起时，焦点序列自动退化为仅 `Response`。
- 当某个 section 折叠时，它不再直接参与滚动焦点循环，只保留在 `Sidebar rail` 中可选。
- `Tab` 负责在顶层焦点区间切换；rail 内的 section 切换走独立按键或左右方向键。

### Mouse Rules

- 点击侧边栏顶部图标可切换对应 section 的高亮或展开状态。
- 鼠标滚轮只作用于当前命中的展开 section；折叠图标不接收滚动。
- 若未来支持拖拽宽度调整，应由 `Task 15B-12` 在不破坏本规格层次的前提下追加。

## Responsive Behavior

- 终端宽度 `< 60` 时，侧边栏强制收起，只保留状态栏摘要。
- 终端宽度 `60-99` 时，`Sidebar` 使用较窄宽度，但保持 rail 可见。
- 终端宽度 `>= 100` 时，`Sidebar` 可使用更宽比例，以承载双 section 同时展开。
- 若高度不足以同时舒适展示双 section，应优先保留 rail，并允许其中一个 section 自动折叠为图标态。

## Relationship To Activity Panel

本规格是 `docs/specs/omega-tui-runtime-experience.md` 的具体落地补充。

演进顺序建议为：

1. 先把当前 `Todos` + `Logs` 迁移到统一 `Sidebar` shell。
2. 让 `Logs` 成为第一个可切换的 `Activity` view。
3. 后续再把 `Skills`、`Delegations`、`Background`、`Inbox` 等视图增量加入同一顶部 rail，而不是新增常驻面板。

需要进一步查看某个 Activity 条目详情、执行确认或做短时选择时，不应继续挤占 rail 或新增固定 section，而应优先通过 overlay / popup 打开浮动详情交互。相关规则见 `docs/specs/omega-tui-overlay-popups.md`。

这样可以确保当前需求不是一次性特例，而是未来运行态信息架构的稳定入口。

## Technical Decisions

| Decision | Choice | Rationale |
|---------|--------|-----------|
| sidebar ownership | `omega-tui` local layout state | 侧边栏开合、顶部 rail 与弹性布局都是纯 UI 语义 |
| section source data | keep existing session updates | 不为本次壳层重构改动 `omega-session` 协议 |
| collapsed representation | top rail text tabs with compact badges | 既保留可发现性，也避免折叠后信息完全消失 |
| empty-body prevention | keep at least one section expanded | 避免出现打开侧边栏却只有空壳的死区 |
| future extensibility | reuse rail for Activity views | 为后续运行态视图提供统一承载面 |

## Task Planning Impact

建议把该需求并入 `Task 15B-16` 的首期范围，但前面应先完成 overlay 基础设施：

0. `Overlay foundation`：由 `Task 15B-16A` 提供统一浮动弹窗能力，承载后续 detail / confirm / picker 交互。

1. `Sidebar shell foundation`：统一右侧侧边栏容器、整体收起快捷键、顶部 rail、`Todos`/`Logs` section state。
2. `Activity view migration`：保持 `Todos`、`Logs` 作为统一 rail 内的运行态入口，并在后续逐步加入 `Skills`、`Delegations`、`Background`、`Inbox` 等更多视图。

如果实现过程中发现交互复杂度明显上升，可以将 `Sidebar shell foundation` 再拆成单独子任务，但不建议把顶部 rail 与整体收起能力推迟到 `Activity` 视图全部接入之后。

## Testing Strategy

- `omega-tui` 单测：验证 `toggle_sidebar` 后布局和焦点会正确退化与恢复。
- `omega-tui` 单测：验证 `Todos` / `Logs` 折叠后只保留 rail 文本入口，不参与主体高度分配。
- `omega-tui` 单测：验证单 section 展开时会占满 `Sidebar` 主体。
- `omega-tui` 单测：验证试图折叠最后一个展开 section 时不会出现空主体。
- `omega-tui` 单测：验证窄终端下侧边栏强制收起后，焦点与快捷键行为仍一致。
- 手动验证：运行 `cargo run -p omega-tui`，检查侧边栏整体收起、section 文本入口切换、双展开弹性布局与窄终端退化。

---

### Change Log
- 2026-03-19: 新增可收起侧边栏规格，定义统一 `Sidebar` shell、顶部 rail、section 折叠态与整体收起交互。
- 2026-03-19: `Task 15B-16` 已落地，`omega-tui` 现支持统一 `Sidebar` shell、轻量单行 rail、`Todos | Logs` 入口、最后一个 section 防空折叠，以及直接显示 `Logs` 标题。
