---
content_revision: 117
created: 2026-03-19
generation_id: gen_000033_r000117
last_verified_commit: N/A
owner: omega-team
projection_version: 33
related_prds: []
source_doc_id: "spec:docs-specs-omega-tui-todo-sidebar-layout"
status: implemented
supersedes: []
updated: 2026-03-19
---

# Omega TUI Todo Sidebar Layout

## Overview

`omega-tui` 当前需要把任务状态从普通工具输出中提升为稳定可见的 UI 信息。为此，右侧辅助区不再只有单一 Logs 面板，而是改为上方 `Todos`、下方 `Logs` 的上下分栏。

## Goals

- 让 `todo` 工具的当前状态在 TUI 中持续可见，而不是只混在 Response 面板里。
- 保留日志面板，但把它下沉到右侧底部，避免与任务状态竞争同一块区域。
- 维持现有 `omega-session` / `omega-tui` 边界，不把 TUI widget 语义回灌到 `omega-core`。
- 保持当前键盘滚动和鼠标滚动模型可继续工作。

## Non-Goals

- 不在本次设计中实现 todo 的交互编辑。
- 不引入新的持久化格式或 session 存储层。
- 不改变 Response 主面板的消息语义。

## Layout

主布局仍为三段：顶部状态栏、中部主体、底部输入区与提示栏。

中部主体调整为：

- 左侧：`Response`
- 右侧上半：`Todos`
- 右侧下半：`Logs`

响应式规则：

- 终端宽度 `< 60` 时，隐藏整个右侧辅助栏，仅显示 `Response`
- 终端宽度 `60-99` 时，主体保持 `70/30`
- 终端宽度 `>= 100` 时，主体保持 `60/40`
- 右侧栏可见时，内部采用 `Todos 38% / Logs 62%` 的纵向切分
- 右侧栏隐藏后，焦点自动退回 `Response`，`Tab` 不再切到不可见面板；状态栏继续显示紧凑 todo 摘要，避免任务状态完全消失

## Data Flow

`todo` 数据来源不是直接读取 `omega-todo` 内部状态，而是由 `omega-session` 在工具执行回调中转发成功的 `todo` 工具输出。

协议扩展：

```rust
pub enum SessionUpdate {
    ToolCallPreview { ... },
    TodoSnapshot {
        turn_id: u64,
        rendered: String,
    },
    AssistantText { ... },
    TurnFinished { ... },
}
```

行为规则：

- 仅当 `todo` 工具成功执行时发送 `TodoSnapshot`
- `todo` 工具返回 `Error:` 时，保留现有 todo 面板内容不覆盖
- TUI 将 `rendered` 规范化为本地 `todo_lines` + 摘要统计，尾部 `(x/y completed)` 汇总不再重复占用列表正文
- 初始状态下若尚未收到 todo 快照，面板显示 `No todo snapshot yet.` / `Call the todo tool to track the current task.`
- 若 `todo` 工具明确返回空列表 `No todos.`，面板显示 `Todo list is empty.` / `Call the todo tool when the task splits into steps.`
- 当前 turn 运行中但尚未收到新 todo 快照时，Todo 面板标题与状态栏摘要标记为 stale，提示用户当前任务列表仍是上一轮结果

## Interaction Model

焦点顺序：

- `Response -> Todos -> Logs -> Response`

滚动规则：

- `Tab` 在三个可见面板间循环焦点
- `Up/Down` 对当前焦点面板滚动
- 鼠标滚轮根据 `Rect` 命中区域路由到 `Response` / `Todos` / `Logs`

## Technical Decisions

| Decision | Choice | Rationale |
|---------|--------|-----------|
| Todo 数据来源 | `SessionUpdate::TodoSnapshot` | 保持 `omega-core` 前端无关，不让 TUI 直接持有 `TodoManager` |
| 右侧布局 | 上 Todo、下 Logs | 任务状态比运行日志更值得持续占据固定位置 |
| 焦点模型 | 三面板循环 | 与现有单焦点滚动模型一致，避免额外交互模式 |
| 错误时 todo 刷新策略 | 保留旧状态 | 避免一次无效更新清空用户对当前任务状态的可见性 |

## Testing Strategy

- `omega-session`：验证新增 `TodoSnapshot` 协议不影响原有 turn 生命周期
- `omega-tui`：验证成功 todo 快照会更新 `todo_lines`
- `omega-tui`：验证空 todo 快照会切换到可操作空状态文案
- `omega-tui`：验证运行中新 turn 未同步 todo 时会标记 stale
- `omega-tui`：验证窄终端下隐藏侧栏后，焦点循环不会落到不可见面板
- `omega-tui`：继续保留现有 stale update 与等待中提交保护测试
- 手动验证：`cargo run -p omega-tui`，触发一次 `todo` 工具后检查右侧上方出现任务列表、下方仍能持续滚动日志

---

### Change Log
- 2026-03-19: 新增 Todo/Logs 侧栏布局规格，并与 `omega-session` 的 `TodoSnapshot` 更新协议对齐。
- 2026-03-19: 补充 Todo 面板联调完成后的行为，包括可操作空状态、运行中 stale 标记、汇总信息外提，以及窄终端隐藏侧栏时的焦点退化规则。
