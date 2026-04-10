---
status: draft
last_verified_commit: N/A
owner: omega-team
created: 2026-04-10
updated: 2026-04-10
version: 0.1
supersedes: []
related_prds: []
---

# Omega Operator Picker Overlay Specification

## Overview

`omega-tui` 已经具备通用 overlay 基础设施，也已经有 `Picker` 浮层类型，但当前实现仍停留在“本地字符串列表 + Enter 后弹一条 status notice”的层级，尚不足以承载真正的 operator workflow。与此同时，`/session list` 仍把 session 列表直接输出到 `Agent Response`，用户无法在同一浮层里完成“选择对象 -> 查看详情 -> 执行动作”的闭环。

本规格定义一个可复用的 **operator picker overlay**：在现有单活动 overlay 基础设施上，提供结构化候选项、可见的动作热键、可选过滤输入，以及“把选中动作回收为现有 slash command / local UI action”的统一执行路径。`/session` 是首个消费者，但设计必须可复用到后续 `project` / `worktree` / `team` / `task` 等 operator surface。

## Goals

- 让用户在一个浮层中完成“浏览候选项、查看详情、执行动作”，而不是先读 Response 文本再手输 id。
- 为 `omega-tui` 提供一个可复用的 operator picker 组件，而不是每个命令族各自拼一套 popup。
- 保持领域动作仍由现有 command/runtime handler 拥有，避免把 `resume/archive/delete` 之类的业务语义复制到 TUI。
- 让 session、project、team 等后续 operator flow 可以复用统一的按键和 footer action 语义。
- 在 overlay 内明确展示可用热键组合，降低高频操作的认知成本。

## Non-Goals

- 不实现全局 command palette，也不替代已有 slash command parser。
- 不在首轮支持多层嵌套 picker stack；仍保持单活动 overlay 模型。
- 不把复杂富文本详情直接内嵌到 picker body；长详情仍优先进入 `Detail` overlay。
- 不让 TUI 直接调用 session/project 内部方法执行领域动作；动作执行仍应回收为 command 或等价 operator intent。

## Current Gap

当前实现状态存在三个约束：

1. `PickerOverlay` 只接受 `Vec<String>`，没有 item id、subtitle、badge、disabled state 或 action metadata。
2. `OverlayTarget::Picker` 只能通过 `UiContent::Text` 间接打开；runtime contract 尚无结构化 picker payload。
3. picker 的 `Enter` 只会关闭弹窗并写一条本地 status notice，无法把“查看详情”或“执行 resume”接回到真实 operator flow。

因此，若直接把 `/session list` 换成现有 picker，只会把“纯文本输出”换成“纯文本浮层”，并不能解决真正的操作闭环问题。

## Architecture

### Components

| Component | Responsibility |
|----------|----------------|
| `omega-session` / other producers | 生成结构化 operator picker request，声明候选项和可执行动作 |
| runtime UI contract | 把 picker 作为 typed overlay payload 暴露给 app/TUI，而不是退回 `UiContent::Text` |
| `omega-app` | 透传 picker request，不解释领域动作 |
| `omega-tui` operator picker | 渲染列表、过滤输入、footer action hints、选中态与空状态 |
| TUI action dispatch bridge | 将选中的 operator action 回收为现有 slash command 或等价本地 UI intent |

### Layering Rule

- **领域语义** 属于 `omega-session` / `omega-project` / 具体 command handler。
- **picker 展示与键盘交互** 属于 `omega-tui`。
- **动作触发路径** 必须通过统一 bridge 回到现有 operator surface，而不是让 TUI 直接拥有领域修改权限。

这能避免把 picker 做成新的 god object，也能保持所有变更继续复用 slash command 的验证、错误处理和测试路径。

## Data Model

建议新增结构化 picker payload，而不是继续复用 `UiContent::Text`：

```rust
pub struct OperatorPickerRequest {
    pub picker_id: String,
    pub title: String,
    pub empty_state: String,
    pub filter_enabled: bool,
    pub items: Vec<OperatorPickerItem>,
    pub primary_action: OperatorPickerAction,
    pub secondary_actions: Vec<OperatorPickerAction>,
}

pub struct OperatorPickerItem {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub badges: Vec<String>,
    pub preview: Option<String>,
    pub disabled_reason: Option<String>,
}

pub struct OperatorPickerAction {
    pub action_id: String,
    pub label: String,
    pub key_hint: String,
    pub intent: OperatorPickerIntent,
}

pub enum OperatorPickerIntent {
    OpenDetail,
    SubmitSlashCommand { command_template: String },
    RefreshPicker,
    ClosePicker,
}
```

设计约束：

- `command_template` 允许使用选中 item 的 `id` 进行模板替换，例如 `/session resume {id}`。
- `OpenDetail` 不直接塞入长文本；应通过 item id 触发 detail builder，再打开统一 `Detail` overlay。
- `disabled_reason` 允许当前选中项显示但禁止执行某些动作，例如 active session 上的 delete/archive。

## Keyboard Contract

建议把 operator picker 统一为下面这组交互：

- `Up/Down` 与 `j/k`: 移动选中项。
- `Enter`: 执行 primary action。对 session picker，默认是 `查看详情`。
- `/`: 当 `filter_enabled = true` 时进入浮层内过滤输入。
- `Ctrl-R`: 对选中项执行 `resume`。
- `Ctrl-A`: 对选中项执行 `archive`。
- `Ctrl-D`: 对选中项执行 `delete`。
- `Ctrl-N`: 执行不依赖当前选中项的全局动作，例如 `new session`。
- `Esc`: 关闭 picker 并恢复打开前焦点。

约束：

- 所有可执行动作都必须在 footer hint 中显式展示，不能只存在于隐藏快捷键里。
- 对不可执行动作，footer 仍可显示热键，但选中项上必须给出 disabled reason，而不是静默无效。

## Action Execution Model

首轮建议采用 **slash-command bridge**，而不是为每个 picker 单独发明新的 runtime mutation API：

1. picker action 解析到 `OperatorPickerIntent::SubmitSlashCommand { command_template }`
2. TUI 用当前选中 item 的 `id` 替换模板变量
3. 该动作经由现有 submit path 提交给 `omega-session` command handler
4. 成功结果不再追加普通 command response section，而是更新 overlay / status notice / restore hydration surface

原因：

- 继续复用既有命令验证和错误路径。
- 避免把 `resume/archive/delete` 的业务规则复制到 TUI。
- 后续无论 action 来自 slash 输入还是 picker 选择，都共享同一领域实现。

## Session As First Consumer

### Entry Rules

- `/session` 与 `/session list` 默认打开 session picker overlay，而不是把列表打印到 `Agent Response`。
- `/session resume` 在无参数时，打开同一个 picker，并默认把可恢复 session 排在前面。
- `/session info <id>` 默认打开 detail overlay，而不是输出普通 command section 文本。

### Session Picker Row Model

每个 session row 至少展示：

- `title`
- `session_id` 的短预览
- `status` (`active` / `idle` / `archived`)
- `resume_ready`
- `archived_turn_count`
- `last_user_turn_preview`

建议 badge 样式：

- `current`
- `resume-ready`
- `archived`
- `turns=<n>`

### Session Actions

对 session picker，建议动作为：

- `Enter`: 打开所选 session 的 detail overlay
- `Ctrl-R`: resume 所选 session；成功后关闭 picker，并进入 restore hydration
- `Ctrl-A`: archive 所选 session；成功后 picker 原地刷新
- `Ctrl-D`: delete 所选 session；先进入 confirm overlay，再执行删除，成功后 picker 原地刷新
- `Ctrl-N`: 创建新 session；成功后关闭 picker，并切换到新 session

### Success / Error Surfaces

- 成功的 `list/info/resume/archive/delete/new` 不应再向 `Agent Response` 追加普通 command section 文本。
- 成功结果通过三种 surface 表达：picker 刷新、detail overlay、status notice / restore hydration。
- 只有解析错误、权限拒绝或持久化失败等异常路径才退回 command error 或 confirm/error overlay。

## Technical Decisions

| Decision | Choice | Rationale |
|---------|--------|-----------|
| Generic foundation | 在现有 `PickerOverlay` 上升级为 typed operator picker | 复用现有 overlay、focus 和遮罩模型 |
| Domain execution | action -> slash-command bridge | 避免 TUI 复制领域逻辑 |
| Primary action | `Enter = detail` | 满足“先看详情，再执行动作”的 operator 心智 |
| Action hotkeys | `Ctrl-*` 组合 | 避免与过滤输入和普通字母选择冲突 |
| Refresh model | action success 后按 request 选择 refresh/close | 让 archive/delete/new 等动作都能在 picker 内形成闭环 |

## Testing Strategy

- `omega-session`：验证 `/session` 与 `/session list` 会产出 picker request，而不是 response 文本。
- `omega-app`：验证 typed picker request 能透传到 `TuiSurface`。
- `omega-tui`：验证 picker selection、footer hint、`Enter` detail、`Ctrl-R/A/D/N` 动作、过滤输入、以及关闭后的焦点恢复。
- integration：验证 session picker 触发的 `resume/archive/delete/new` 最终仍走现有 command handler，并正确刷新 picker 或触发 restore hydration。

## Task Planning Impact

建议把这项需求拆成：

1. `Task 15B-70`: 通用 operator picker overlay 与快捷键动作基础件。
2. `Task 17G`: `/session list` / `/session resume` 的 picker entry 与 typed request。
3. `Task 17H`: session detail / resume / archive / delete / new 的 picker 内闭环与回归测试。

---

### Change Log
- 2026-04-10: 初版规格，定义基于现有 overlay 基础设施的可复用 operator picker，以及 `/session` 作为首个消费者的交互契约。