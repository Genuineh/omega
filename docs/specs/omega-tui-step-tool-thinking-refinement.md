---
content_revision: 117
created: 2026-03-20
generation_id: gen_000033_r000117
last_verified_commit: N/A
owner: omega-team
projection_version: 33
related_prds: []
source_doc_id: "spec:docs-specs-omega-tui-step-tool-thinking-refinement"
status: draft
supersedes: []
updated: 2026-03-20
---

# Omega TUI Step Tool And Thinking Refinement Specification

## Overview

`Task 15B-20` 与 `Task 15B-21` 已经把 `Response` 推进为按 `route / step / final / thinking` 组织的结构化 timeline，但日常使用中仍有两个明显缺口。第一，step 内的工具使用当前只以 `[tool] ...` 纯文本落到 `Activity(Log)`，用户很难把某次工具调用与当前 step 的正文、thinking 和结果快速对应起来。第二，thinking 虽然已经具备实时流式与折叠能力，但当前视觉层级仍然过弱，在深色主题下不够醒目，且截图中还出现了 provider 风格的原始 `<minimax:tool_call>` 标记泄漏进 step 正文的问题。

本规格定义下一阶段的精修方向：保持现有交互层边界不变，即完整工具 preview 仍属于 `Activity`，但在 `Response` 的 step block 内补上“结构化工具摘要与 drill-down 入口”；同时强化 thinking 的可读性和层级，让它在不抢走最终回答主阅读权重的前提下更容易看清。

## Goals

- 让用户在 step block 内快速看见“这个 step 调用了哪些工具、当前状态如何、最后产出了什么”。
- 保持完整 command/output 详情仍落在 `Activity` 或 `Overlay`，避免把 Response 重新塞回原始日志流。
- 强化 thinking 的视觉对比、完成态摘要和 streaming 态辨识度，解决“太弱、看不清”的问题。
- 消除 provider 风格原始 tool-call 标记泄漏到 step/ thinking 正文的问题，避免双重表达与阅读噪音。
- 继续保持 `omega-session` 拥有 runtime 语义归一，`omega-tui` 只消费 typed contract，而不是靠字符串解析推测工具结构。

## Non-Goals

- 不把完整工具输出重新挪回 `Response`，也不让 `Response` 退化为新的日志面板。
- 不要求这一轮就把 Sidebar 从 `Logs` 演进成完整多视图 `Activity` 工作区。
- 不在 TUI 里直接理解 provider 私有协议或 XML 风格 tool-call 语法。
- 不要求首轮就实现跨 turn 的工具历史索引或全局搜索能力。

## Current Issues

### 1. Step 内工具语义缺失

当前工具 preview 由 `omega-session` 通过 `UiTarget::Activity(ActivityTarget::Log)` + `UiSource::Tool` 发出，`omega-tui` 仅把它渲染成 `[tool] ...` 文本日志。这样虽然保持了边界正确，但用户必须在 `Response` 和侧栏日志之间来回跳转，才能回答以下问题：

- 当前 step 是否真的发起了工具调用？
- 发起了几次？
- 正在运行还是已经完成？
- 这次调用对当前 step 的产出贡献了什么？

### 2. Thinking 可见但辨识度不足

thinking 目前已经是独立 block，并支持实时追加与完成态折叠，但仍有以下问题：

- header/body 的颜色层级过弱，在深色背景下容易被 step 正文淹没。
- collapsed 摘要虽然存在，但可扫描性不够强，难以一眼识别这是 reasoning 而不是普通正文。
- streaming thinking 与 completed thinking 的视觉差异还不够明确。

### 3. Provider 原始 tool markup 泄漏

截图中的 `<minimax:tool_call>` / `<invoke ...>` 说明当前至少存在一种 provider 输出路径，会把原始工具调用标记混入 step 正文或 thinking 文本。即使工具实际已经通过结构化 `ToolUse` / `tool preview` 路径执行，这种原始标记仍会污染主阅读区，造成：

- step 内重复表达“工具调用发生了”
- provider 兼容层细节泄漏到最终 TUI
- thinking/step block 被低价值模板文本占据

## UX Direction

### Step Block 内新增 Tool Summary Lane

每个 step block 在正文区下方增加一个轻量 `Tool Summary Lane`，仅呈现与该 step 绑定的结构化工具运行摘要，而不是完整日志。推荐最小展示单元：

- tool name
- run status: `running / done / failed`
- invocation preview: 对 `bash` 展示命令摘要，对 `read_file` / `write_file` / `edit_file` 展示目标路径摘要
- result preview: 一行截断摘要，例如 `12 lines`, `wrote src/lib.rs`, `Error: path escapes workspace`

默认行为：

- 当前活跃工具调用默认展开为 1-2 行摘要
- 已完成工具调用默认折叠成单行 token
- 同一 step 中的连续同类调用允许聚合为 `read_file ×3` 这类摘要

### Activity 保留完整详情

完整 command/output 仍保留在 `Activity(Log)`；step 内工具 lane 只承担“建立关联”和“降低跳转成本”的职责。推荐支持两种 drill-down：

- 在 `Response` 中选中某条工具摘要后用 `Enter` 打开现有 `DetailOverlay`
- 或后续把对应条目在 `Activity` 中高亮/定位

### Stronger Thinking Hierarchy

thinking block 应保留“低于 final answer”的层级，但必须比当前更清晰：

- header 使用更强的语义标签，如 `reasoning` 或 `thinking live`
- header 与 body 使用高于当前 `border_dim` 的对比色
- streaming 态显示更明确的进行中状态
- completed 态 collapsed 摘要携带行数/片段信息，例如 `reasoning · 5 lines · outline answer...`
- thinking 在 step 内保持明显的缩进或左侧导轨，避免与 step 正文混成同一层

## Architecture Direction

### Boundary Rule

- `Response` 只显示 step-owned 的简化工具摘要和 reasoning，不显示完整原始日志。
- `Activity` 继续承载完整工具 preview / output。
- `omega-tui` 不应自行从 `[tool] ...` 字符串中反向推导结构；结构必须来自 `omega-session` 的 typed contract。

### Session / Runtime Contract Requirement

当前 `UiSource::Tool { tool_name } + UiMessageKind::Log + UiContent::Text` 不足以支撑好的 step 内工具体验，因为它缺少：

- stable tool run id
- parent step / response section 归属
- invocation preview 与 result preview 的结构化字段
- running / complete / failed 生命周期

下一阶段建议新增 step-scoped `ToolRun` contract，最小字段包括：

- `id`
- `parent_section_id` 或 `workflow_id + step_id`
- `tool_name`
- `status`
- `invocation_preview`
- `result_preview`
- `detail_lines`

推荐通过 `RuntimeUiEffect` 的 begin/update/complete 语义表达，而不是继续用纯文本 log 追加。`Activity(Log)` 仍可同时保留文本日志作为兼容输出，但 `Response` 的工具 lane 应只消费结构化 tool-run state。

### Provider Markup Sanitization

原始 `<minimax:tool_call>` 之类 provider 风格标记不应直接进入 Response。建议在 `omega-session` 侧加入一层“已知结构化 tool-use 的正文清洗”规则：当同一轮 / 同一步中已经收到结构化工具运行事件时，若文本 delta 只是在重复 provider 私有 tool-call 包装，则应在进入 response/thinking section 前被剥离或降级到 debug-only 日志。

## TUI Direction

### Task 15B-22: Step Tool Visibility

`omega-tui` 需要在现有 step block 中增加一个 tool summary lane，并维护最小的 step-local tool run view state。首轮不做复杂树状 UI，建议从以下形式开始：

- step header
- optional thinking block
- step body
- tool summary lines

### Task 15B-23: Stronger Thinking Readability

`omega-tui` 需要在当前 thinking block 基础上提升：

- 配色对比
- header 命名
- streaming / done / failed 差异
- collapsed 摘要信息量
- 与 step body 的视觉分层

这应被视为 `15B-21` 的可读性 refinement，而不是推翻现有 contract。

## Task Breakdown

### Task 15F-7: omega-session — 结构化 tool-run runtime contract 与 provider markup 清洗

- 为 step 内工具调用建立稳定的 typed runtime contract
- 让 tool run 可被绑定到具体 step / response section
- 加入已知 provider tool markup 的 response/thinking 清洗规则

### Task 15B-22: omega-tui — step 内工具使用可见性

- 在 step block 中渲染 tool summary lane
- 当前活跃工具调用可见，已完成工具调用默认压缩
- 为 detail overlay / Activity drill-down 预留稳定入口

### Task 15B-23: omega-tui — thinking 可读性强化

- 提升 thinking header/body 的视觉对比
- 强化 collapsed summary 与 streaming 状态表达
- 确保窄终端下依旧保持 final answer 优先级

## Testing Strategy

- `omega-session` 单测：验证 tool run 生命周期被正确映射到 typed runtime contract。
- `omega-session` 单测：验证已知 provider tool-call markup 不再泄漏到 response/thinking text。
- `omega-tui` 单测：验证 step block 中工具摘要的展开/折叠与状态切换。
- `omega-tui` 单测：验证 thinking 的强化样式不会影响 final answer 的优先可读性。
- 手动验证：运行 `cargo run -p omega-app`，确认 step 内能看见工具摘要、完整输出仍留在 Activity，thinking 在深色主题下明显更易读。

## Risks

| Risk | Level | Mitigation |
|------|-------|------------|
| 为了追求 step 内工具可见性而把完整工具日志塞回 Response | High | 只显示摘要，完整详情继续留在 Activity / Overlay |
| TUI 自己解析 `[tool] ...` 字符串恢复结构 | High | 新增 session-owned typed tool-run contract |
| thinking 强化后反而压过 final answer | Medium | 保持 final answer 最高对比度与默认展开优先级 |
| provider markup 清洗过度，误删正常正文 | Medium | 仅清洗已知重复包装模式，并以测试锁定边界 |

## Related Specs

- `docs/specs/omega-runtime-ui-message-contract.md`
- `docs/specs/omega-tui-response-thinking-experience.md`
- `docs/specs/omega-tui-runtime-experience.md`
- `docs/specs/omega-tui-overlay-popups.md`

---

### Change Log

- 2026-03-20: 初版规格，规划 step 内工具使用可见性、thinking 可读性强化，以及 provider 原始 tool-call markup 清洗方向。
