---
status: draft
owner: omega-team
created: 2026-03-19
updated: 2026-03-19
version: 0.1
supersedes: []
related_prds: []
---

# Omega TUI Modal Keymap Specification

## Overview

`Task 15B-13` 不再只是给高级功能补一个 leader 键前缀，而是要为 `omega-tui` 建立一套完整的模态交互与快捷键基础设施。该基础设施需要同时解决四个问题：一是把高级快捷键统一收敛到 leader 入口；二是引入类似 Vim 的 `Normal` / `Insert` 模式；三是让快捷键只在特定模式和上下文中触发；四是把快捷键配置从硬编码迁移到 `.omega` 目录下的可加载设置文件。

为避免把状态机、配置解析、映射解析和 TUI 事件处理全部塞回 `omega-tui`，快捷键系统应拆为独立 crate 管理，供 `omega-tui` 在启动时加载并在运行时查询。

## Goals

- 为 `omega-tui` 提供统一的 leader 键快捷键入口，替代不断增长的硬编码快捷键分支。
- 引入 `Normal` / `Insert` 两种交互模式，使导航类操作与文本输入类操作明确分离。
- 支持 mode-aware、focus-aware、context-aware 的快捷键匹配规则。
- 支持从工作目录下 `.omega/keymap.toml` 启动加载用户快捷键设置，并在缺失时回退到内置默认映射。
- 将快捷键定义、加载、验证、解析与动作匹配逻辑放入独立 crate，降低 `omega-tui` 的输入系统复杂度。

## Non-Goals

- 不在本规格中实现完整 Vim 语义，不引入 `Visual`、命令行或宏录制模式。
- 不要求首版支持运行时热重载 `.omega/keymap.toml`。
- 不在本次设计中覆盖终端所有按键差异或平台特定组合键兼容问题。
- 不让独立 keymap crate 直接处理 TUI 绘制、焦点状态或 widget 内部滚动。

## Architecture

### Components

- `omega-keymap`：独立 crate，负责默认 keymap、配置文件加载、schema 校验、按键序列解析、条件匹配与动作解析。
- `omega-tui`：负责维护当前交互模式、焦点面板、当前是否存在可输入上下文，并把 `crossterm` 事件交给 `omega-keymap` 解析。
- `.omega/keymap.toml`：用户可编辑的快捷键设置文件，位于运行目录下 `.omega/`。
- status/hint renderer：由 `omega-tui` 负责，根据当前模式、leader pending 状态和匹配失败原因给出提示。

### Responsibility Split

`omega-keymap` 应只理解“按键序列 + 条件 -> 动作”，不理解具体 widget、面板尺寸、光标坐标或 session 生命周期。

`omega-tui` 应只提供运行时上下文，例如：

- 当前模式：`Normal` / `Insert`
- 当前焦点：`Response` / `Todo` / `Logs` / `SidebarRail` / 未来 `Activity`
- 当前上下文是否允许输入
- 当前是否处于 leader pending 状态

这种拆分保持了输入系统与 UI 状态的边界：配置与匹配规则可测试、可复用，而具体模式切换和动作执行仍留在前端层。

## Interaction Model

### Modes

首版定义两个模式：

- `Normal`：默认导航/操作模式。面板切换、滚动、sidebar 展开折叠、搜索、布局调整、Activity view 切换等操作均在此模式触发。
- `Insert`：文本输入模式。仅在当前焦点存在可输入控件时允许进入；字符输入、光标移动、删除、提交等文本编辑操作在此模式工作。

规则：

- 应用启动默认进入 `Normal`。
- 当用户请求进入 `Insert` 但当前焦点不支持输入时，模式不切换，并在状态栏/提示栏给出反馈。
- 当输入控件失焦或被隐藏时，`omega-tui` 应把模式自动回退到 `Normal`，避免停留在无效 `Insert` 模式。

### Leader-Based Mode Switching

默认模式切换使用 leader 前缀下的 `j` / `k` 映射：

- `leader j`：切换到 `Normal`
- `leader k`：在当前存在可输入上下文时切换到 `Insert`

之所以使用 leader 命名空间而不是裸按键，是为了避免与终端快捷键和输入内容冲突，同时保持后续高级操作的一致认知模型。

### Mode-Scoped Actions

快捷键匹配必须支持以下约束：

- `mode`：仅在 `Normal` 或 `Insert` 模式触发
- `focus`：仅在某个面板或 rail 聚焦时触发
- `input_capable`：仅在当前上下文允许输入时触发
- `leader_required`：是否需要 leader 前缀

优先级建议：

1. 精确匹配 `mode + focus + input_capable + keys`
2. 匹配 `mode + focus + keys`
3. 匹配 `mode + keys`
4. 匹配全局 fallback

若存在冲突，加载阶段应直接报配置错误，而不是运行时随机选择。

## Keymap File

### Location

默认路径：`.omega/keymap.toml`

启动行为：

- 若文件存在，则读取并校验
- 若文件缺失，则使用内置默认 keymap
- 若文件格式错误，则记录错误、向用户显示提示，并回退到内置默认 keymap

### Suggested Format

```toml
[leader]
key = "space"
timeout_ms = 900

[[bindings]]
keys = "leader j"
action = "enter_normal_mode"
mode = "insert"

[[bindings]]
keys = "leader k"
action = "enter_insert_mode"
mode = "normal"
input_capable = true

[[bindings]]
keys = "leader f"
action = "panel_search"
mode = "normal"

[[bindings]]
keys = "leader h"
action = "history_previous"
mode = "normal"
focus = "response"
```

### Validation Rules

- `keys` 必须能被解析为合法按键序列。
- `action` 必须属于受支持动作集合。
- 同一条件集下不得定义重复映射。
- `enter_insert_mode` 必须显式标注 `input_capable = true`，避免把不可输入上下文配置成可切换插入模式。

## API Specification

### Core Types

```rust
pub enum InteractionMode {
    Normal,
    Insert,
}

pub enum KeyFocus {
    Response,
    Todo,
    Logs,
    SidebarRail,
    Activity,
    InputField,
}

pub struct KeyContext {
    pub mode: InteractionMode,
    pub focus: KeyFocus,
    pub input_capable: bool,
    pub leader_pending: bool,
}

pub struct KeyBinding {
    pub sequence: KeySequence,
    pub action: KeyAction,
    pub mode: Option<InteractionMode>,
    pub focus: Option<KeyFocus>,
    pub input_capable: Option<bool>,
}
```

### Actions

首版至少应支持以下动作：

- `enter_normal_mode`
- `enter_insert_mode`
- `focus_next_panel`
- `toggle_sidebar`
- `panel_search`
- `history_previous`
- `history_next`
- `resize_sidebar_wider`
- `resize_sidebar_narrower`
- `cancel_pending_sequence`

### Loading API

```rust
pub struct KeymapManager {
    // default keymap + user overrides
}

impl KeymapManager {
    pub fn load(root: &Path) -> Result<Self>;
    pub fn resolve(&self, context: &KeyContext, event: KeyEvent) -> KeyResolution;
}
```

`resolve` 的结果应区分：

- `Matched(action)`
- `PendingLeader`
- `NoMatch`
- `InvalidInContext`

这样 `omega-tui` 才能决定状态栏提示、超时取消与错误反馈，而不是把所有失败都吞掉。

## Integration With Existing Tasks

`Task 15B-13` 应成为以下任务的交互基础设施前置项：

- `Task 15B-10`：输入历史，需要明确在哪个模式和焦点下触发
- `Task 15B-11`：面板内搜索，需要 leader 命名空间与模式约束
- `Task 15B-12`：可调面板与会话统计，需要统一映射和模式隔离
- `Task 15B-16`：可收起 Sidebar 与 Activity 入口，需要依赖统一快捷键解析层

## Technical Decisions

| Decision | Choice | Rationale |
|---------|--------|-----------|
| keymap package | new `omega-keymap` crate | 将配置加载、匹配和验证从 `omega-tui` 中剥离 |
| mode model | `Normal` + `Insert` only | 先解决导航与输入冲突，不提前引入更多模态复杂度 |
| user config path | `.omega/keymap.toml` | 明确、可发现、与仓库级本地设置目录一致 |
| mode switch mapping | `leader j` / `leader k` | 与用户要求一致，并避免裸键与输入冲突 |
| runtime ownership | mode/focus in `omega-tui`, resolution in `omega-keymap` | 保持边界清晰，避免独立 crate 反向依赖 UI |

## Testing Strategy

- `omega-keymap` 单测：验证默认 keymap 加载、`.omega/keymap.toml` 覆盖、冲突检测、非法 action 拒绝。
- `omega-keymap` 单测：验证 mode/focus/input_capable 条件匹配优先级。
- `omega-tui` 单测：验证 `leader j` / `leader k` 的模式切换与不可输入上下文回退。
- `omega-tui` 单测：验证仅在 `Insert` 模式下接受自由文本字符输入。
- `omega-tui` 单测：验证 leader pending 超时取消后不会错误触发动作。
- 手动验证：运行 `cargo run -p omega-tui`，确认 `.omega/keymap.toml` 缺失、合法、非法三种场景下都能启动并给出正确提示。

---

### Change Log
- 2026-03-19: 新增 TUI 模态快捷键规格，定义 `Normal`/`Insert` 模式、leader 模式切换、`.omega/keymap.toml` 配置和独立 `omega-keymap` crate 边界。