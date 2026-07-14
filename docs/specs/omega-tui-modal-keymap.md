---
content_revision: 174
created: 2026-03-19
generation_id: gen_000087_r000174
language: bilingual
last_verified_commit: d8c30e3e9e310ce38cffa965be4688ed55a87787
owner: omega-team
projection_version: 87
related_prds: "[]"
source_doc_id: "spec:docs-specs-omega-tui-modal-keymap"
source_path: docs/specs/omega-tui-modal-keymap.md
status: active
supersedes: "[]"
updated: 2026-04-10
---

# Omega TUI Modal Keymap Specification

## Overview

`Task 15B-13` 已经为 `omega-tui` 建立了第一版模态快捷键基础设施，但 2026-04-03 的输入问题复盘表明，v0.1 仍停留在“Normal-only leader + Insert raw text fallback”的保守模型。该限制现已通过 v0.3 落地修正：insert-mode mapping 与 leader 序列共享同一条 buffered route，并在超时或失配后把前缀安全回放为原始文本。

为避免把状态机、配置解析、映射解析和 TUI 事件处理全部塞回 `omega-tui`，快捷键系统仍应保持 `omega-keymap` / `omega-tui` 的分层边界，但两者之间的 contract 需要升级：从“单次按键解析”扩展到“可缓冲、可回放、可超时结算”的输入序列解析。

## Goals

- 为 `omega-tui` 提供统一的 leader 键快捷键入口，替代不断增长的硬编码快捷键分支。
- 引入 `Normal` / `Insert` 两种交互模式，使导航类操作与文本输入类操作明确分离。
- 支持 mode-aware、focus-aware、context-aware 的快捷键匹配规则。
- 支持从工作目录下 `.omega/keymap.toml` 启动加载用户快捷键设置，并在缺失时回退到内置默认映射。
- 已存在的工作区 `.omega/keymap.toml` 应视为“覆盖层”而不是“整份替换”：当内置默认 keymap 新增键位时，旧工作区文件必须自动继承这些新增默认，除非用户显式用同条件绑定覆盖它们。
- 将快捷键定义、加载、验证、解析与动作匹配逻辑放入独立 crate，降低 `omega-tui` 的输入系统复杂度。
- 允许 `Insert` 模式声明前缀序列，在 `space`、`jk` 等常见场景下同时满足“先尝试映射”和“失败后回放原始文本”。
- 为 pending 输入序列定义显式 timeout replay 语义，避免把空格 leader 与自由输入永久做成二选一。

## Non-Goals

- 不在本规格中实现完整 Vim 语义，不引入 `Visual`、命令行或宏录制模式。
- 不要求首版支持运行时热重载 `.omega/keymap.toml`。
- 不在本次设计中覆盖终端所有按键差异或平台特定组合键兼容问题。
- 不让独立 keymap crate 直接处理 TUI 绘制、焦点状态或 widget 内部滚动。
- 不追求逐项复刻 Neovim 的所有 mapping 选项；首期只解决 insert-mode prefix、timeout replay 与文本回填的核心语义。

## Architecture

### Components

- `omega-keymap`：独立 crate，负责默认 keymap、配置文件加载、schema 校验、按键序列解析、条件匹配与动作解析。
- `omega-tui`：负责维护当前交互模式、焦点面板、当前是否存在可输入上下文，并把 `crossterm` 事件交给 `omega-keymap` 解析。
- pending input buffer：由 `omega-tui` 持有的短生命周期输入缓冲，记录“当前是否在等待一个可能形成动作的 insert-mode 前缀”。
- `.omega/keymap.toml`：用户可编辑的快捷键设置文件，位于运行目录下 `.omega/`。
- status/hint renderer：由 `omega-tui` 负责，根据当前模式、leader pending 状态和匹配失败原因给出提示。

### Responsibility Split

`omega-keymap` 应只理解“按键序列 + 条件 -> 动作”，不理解具体 widget、面板尺寸、光标坐标或 session 生命周期。

`omega-tui` 应只提供运行时上下文，例如：

- 当前模式：`Normal` / `Insert`
- 当前焦点：`Response` / `Todo` / `Logs` / `SidebarRail` / 未来 `Activity` / `Overlay`
- 当前上下文是否允许输入
- 当前是否处于 leader pending 状态
- 当前是否处于 insert pending text sequence 状态
- 当前 pending sequence 在超时时需要回放的原始文本

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

### Neovim-Like Prefix Input Model

v0.2 不再把 leader 仅限于 `Normal` 模式，而是把按键处理拆成两条语义：

1. command sequence：若输入流在当前模式/焦点下匹配某个快捷键前缀，则进入 pending 状态，继续等待后续按键。
2. text fallback：若 pending 序列在超时前未形成动作，或最终确认是文本输入，则将缓存的字符按原顺序回放到输入框。

默认模式切换仍保留下面这组用户语义，但实现方式要升级为“可缓冲、可回放”的输入结算：

- `leader j k`：从 `Normal` 进入 `Insert`
- `Esc`：从 `Insert` 返回 `Normal`
- insert-mode 前缀：允许在 `Insert` 中声明诸如 `jk -> enter_normal_mode` 或 `space j k -> enter_normal_mode` 之类的序列
- `space`：在 `Insert` 中既可以只是普通空格，也可以作为一个 pending prefix 的首键；若后续未补全成映射，则必须自动回放为空格文本

这与 Neovim 的核心差异点一致：前缀键不是立即决定为“文本”或“命令”，而是在一个很短的超时窗口内先作为 pending sequence 保存，最终再结算为动作或文本。

### Input Pipeline Phases

键盘事件在 `omega-tui` 中应按以下顺序结算：

1. overlay route：若 overlay 激活，先交给 overlay 本地消费。
2. explicit pending route：若已经存在 leader pending 或 insert pending sequence，则尝试追加当前按键并继续解析。
3. direct keymap route：若当前按键本身可作为某个模式下的前缀或单键动作，则进入 pending 或直接命中。
4. text commit route：仅当上面三步都失败，并且当前上下文允许输入时，才将字符立即写入输入框。
5. timeout flush route：当 pending sequence 超时，若该序列带有 text fallback，则把缓存文本回放到输入框；否则仅取消 pending 并提示。

这意味着 `Insert` 模式不再采用“未命中就立刻写字符”的单层兜底，而是先经过一层短暂的 prefix buffer。

### Mode-Scoped Actions

快捷键匹配必须支持以下约束：

- `mode`：仅在 `Normal` 或 `Insert` 模式触发
- `focus`：仅在某个面板或 rail 聚焦时触发
- `input_capable`：仅在当前上下文允许输入时触发
- `leader_required`：是否需要 leader 前缀
- `text_fallback`：若序列失配或超时，是否应把已输入字符回放到输入框
- `timeout_ms`：该绑定是否使用自定义前缀等待窗口，未显式配置则继承 leader/default timeout

优先级建议：

1. 精确匹配 `mode + focus + input_capable + keys`
2. 匹配 `mode + focus + keys`
3. 匹配 `mode + keys`
4. 匹配全局 fallback

若存在冲突，加载阶段应直接报配置错误，而不是运行时随机选择。

当 overlay 激活时，键盘路由应遵循 overlay-first 原则：先由 overlay 本地交互消费，再决定是否触发关闭或全局取消；底层 panel 的导航、leader 动作和 insert pending buffer 都不应继续生效。相关交互层规则见 `docs/specs/omega-tui-overlay-popups.md`。

## Keymap File

### Location

默认路径：`.omega/keymap.toml`

启动行为：

- 若文件存在，则读取并校验
- 若文件存在，则以“内置默认 + 工作区覆盖”的方式合并加载，避免旧默认文件吞掉后续新增默认绑定
- 若文件缺失，则创建默认 `.omega/keymap.toml` 文件并加载它
- 若文件格式错误，则记录错误、向用户显示提示，并回退到内置默认 keymap

覆盖规则：

- `leader` 配置若在工作区文件中出现，则替换默认 leader，并让默认绑定也按新的 leader 重新解析。
- `bindings` 以 `keys + mode + focus + input_capable` 为覆盖键；工作区文件中命中的绑定会替换默认同条件绑定。
- 工作区文件未声明的新默认绑定必须自动保留，避免历史默认文件把后续新增快捷键静默吃掉。

### Suggested Format

```toml
[leader]
key = "space"
timeout_ms = 300

[[bindings]]
keys = "esc"
action = "enter_normal_mode"
mode = "insert"

[[bindings]]
keys = "leader j k"
action = "enter_insert_mode"
mode = "normal"
input_capable = true

[[bindings]]
keys = "j k"
action = "enter_normal_mode"
mode = "insert"
text_fallback = true

[[bindings]]
keys = "space j k"
action = "enter_normal_mode"
mode = "insert"
text_fallback = true

[[bindings]]
keys = "leader tab"
action = "focus_next_panel"
mode = "normal"

[[bindings]]
keys = "leader up"
action = "scroll_panel_up"
mode = "normal"

[[bindings]]
keys = "leader down"
action = "scroll_panel_down"
mode = "normal"

[[bindings]]
keys = "leader c"
action = "interrupt_turn"

[[bindings]]
keys = "leader q"
action = "quit"
```

### Validation Rules

- `keys` 必须能被解析为合法按键序列。
- `action` 必须属于受支持动作集合。
- 同一条件集下不得定义重复映射。
- `enter_insert_mode` 必须显式标注 `input_capable = true`，避免把不可输入上下文配置成可切换插入模式。
- insert-mode 多键绑定若会吞掉可打印字符，必须显式声明 `text_fallback = true` 或等价策略，避免用户在超时/失配时丢字。

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
    pub text_fallback: Option<bool>,
    pub timeout_ms: Option<u64>,
}

pub struct PendingInputSequence {
    pub events: Vec<KeyEvent>,
    pub fallback_text: String,
    pub started_at: std::time::Instant,
    pub mode: InteractionMode,
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
- `PendingInputSequence`
- `NoMatch`
- `InvalidInContext`
- `ReplayAsText(String)`

这样 `omega-tui` 才能决定状态栏提示、超时取消与错误反馈，而不是把所有失败都吞掉。

## Implementation Plan

### Phase 1: `omega-keymap` Sequence Semantics

- 已完成：解析结果从“单次命中”扩展为“命中 / pending / replay”。
- 已完成：binding schema 新增 `text_fallback` 与可选 `timeout_ms`。
- 已完成：prefix 优先级明确为“更长的 insert-mode mapping 优先于单字符直写，但失配时必须 replay”。

### Phase 2: `omega-tui` Buffered Router

- 已完成：`handle_unmatched_key()` 的 insert 兜底已改为 buffer-aware route。
- 已完成：app state 新增通用 pending sequence buffer，并在 tick/render 中处理 timeout flush。
- 已完成：running、overlay、focus 切换与 pending buffer 的边界已收口，模式切换与文本回放不会被运行态吞掉。

### Phase 3: Config Migration And UX

- 已完成：默认 `.omega/keymap.toml` 改写为 v0.2+ 示例，insert-mode 支持 `leader j k` + `text_fallback = true`。
- 已完成：status / hint bar 会展示 pending prefix 与 timeout replay 提示。
- 已完成：校验器会为需要迁移的 v0.1 insert multi-key binding 提供显式 `text_fallback` 诊断。

## Integration With Existing Tasks

`Task 15B-13` 应成为以下任务的交互基础设施前置项：

- `Task 15B-10`：输入历史，需要明确在哪个模式和焦点下触发
- `Task 15B-11`：面板内搜索，需要 leader 命名空间与模式约束
- `Task 15B-16A`：浮动弹窗 / overlay 基础设施，需要统一 leader 路由与 modal 焦点语义
- `Task 15B-12`：可调面板与会话统计，需要统一映射和模式隔离
- `Task 15B-16`：可收起 Sidebar 与 Activity 入口，需要依赖统一快捷键解析层

## Technical Decisions

| Decision | Choice | Rationale |
|---------|--------|-----------|
| keymap package | new `omega-keymap` crate | 将配置加载、匹配和验证从 `omega-tui` 中剥离 |
| mode model | `Normal` + `Insert` only | 先解决导航与输入冲突，不提前引入更多模态复杂度 |
| user config path | `.omega/keymap.toml` | 明确、可发现、与仓库级本地设置目录一致 |
| mode switch mapping | `leader j k` + `Esc`, with insert `text_fallback` | `Normal` 继续通过 `leader j k` 进入 `Insert`；`Insert` 同时支持 `Esc` 与 replayable `leader j k` 返回 `Normal`，既保留传统退出键，也允许 Neovim-like 前缀语义 |
| runtime ownership | mode/focus in `omega-tui`, resolution in `omega-keymap` | 保持边界清晰，避免独立 crate 反向依赖 UI |

## Testing Strategy

- `omega-keymap` 单测：验证默认 keymap 加载、`.omega/keymap.toml` 覆盖、冲突检测、非法 action 拒绝。
- `omega-keymap` 单测：验证 insert-mode prefix 在 `Matched / PendingInputSequence / ReplayAsText` 三种结算之间的优先级与超时语义。
- `omega-tui` 单测：验证 `leader j k` 进入 `Insert`、`Esc` 返回 `Normal`、`jk` insert mapping 返回 `Normal`、以及 `space` 前缀在超时后能回放为空格。
- `omega-tui` 单测：验证 running、overlay、focus 切换和 pending flush 不会吞掉模式切换或普通文本。
- `omega-tui` 单测：验证仅在 `Insert` 模式下接受自由文本字符输入，并且 pending sequence 最终按顺序回放。
- 手动验证：运行 `cargo run -p omega-tui`，确认 `.omega/keymap.toml` 缺失、合法、非法三种场景下都能启动并给出正确提示。

---

### Change Log
- 2026-03-19: 新增 TUI 模态快捷键规格，定义 `Normal`/`Insert` 模式、leader 模式切换、`.omega/keymap.toml` 配置和独立 `omega-keymap` crate 边界。
- 2026-04-03 v0.2: 根据 insert/normal 切换问题复盘，把输入系统规划升级为 Neovim 风格的 buffered prefix pipeline：insert-mode 允许前缀映射，未成形序列支持 timeout replay 为原始文本，并新增 `Task 15B-13A/B/C` 作为后续实现拆分。
- 2026-04-03 v0.3: `Task 15B-13A/B/C` 已落地；`omega-keymap` 支持 replayable pending sequence、`text_fallback` 与 binding 级 timeout，`omega-tui` 改为通用 pending buffer，workspace keymap 与回归测试矩阵同步迁移完成。
