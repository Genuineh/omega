---
status: active
owner: omega-team
created: 2026-03-20
updated: 2026-03-20
version: 0.4
supersedes: []
related_prds: []
---

# Omega TUI Response And Thinking Experience Specification

## Overview

`Task 15B-20` 与 `Task 15B-21` 现已把 `omega-tui` 的 `Agent Response` 从“按到达顺序追加的文本行列表”推进为基于 response section 的结构化 timeline。当前主阅读区已经能区分 root routing、child workflow step、final answer 与 provider-exposed thinking，并复用 `Task 15F-6` 引入的 section identity / append / finalize contract。

同时，仓库里已经出现了 `omega-client::ContentBlock::Thinking`，而 `Task 15F-6` 已把它推进为主路径中的 typed stream foundation：`omega-client` 提供 `ChatEvent` / `chat_stream()`，`omega-core` 可逐事件向上游暴露 response，`omega-session` 已能把 text / thinking 归一为带 section identity 的 runtime UI effect。当前该路径已经在 `omega-tui` 中落地：thinking 会在流式阶段实时追加、在完成后默认折叠成摘要、并与最终回答保持分离；若用户不希望显示 reasoning，可通过 `.omega/tui.toml` 的 `[response].show_thinking = false` 关闭这类可见性。下一阶段的 step-tool / thinking 精修方向见 `docs/specs/omega-tui-step-tool-thinking-refinement.md`。

本规格记录当前已经落地的统一方向：`Response` 现已从平铺文本列表提升为结构化 turn timeline，并把“最终回答”和“provider-exposed thinking”明确拆开；thinking 只展示模型或 provider 明确返回的 reasoning/thinking 内容，不把系统隐藏推理或内部链路状态伪装成 thinking 输出。

## Goals

- 让 `Agent Response` 按 `scene -> workflow -> step -> final answer` 形成稳定、可扫描的视觉层级。
- 让用户能快速区分 root routing、child workflow step 正文、最终回答，以及未来的 thinking 内容。
- 支持 provider-exposed thinking 的实时显示，而不必等整轮结束后才看到结果。
- 保持 `omega-session` 拥有 response/thinking 语义归一，`omega-tui` 只消费结构化 runtime UI contract。
- 在窄终端下提供合理退化策略，不让新信息架构把主阅读区挤爆。

## Non-Goals

- 不展示系统隐藏推理、内部 CoT 或并未由 provider 明确返回的 reasoning 内容。
- 不要求第一版就支持完整 Markdown 富文本、代码高亮或多列对比布局。
- 不把 tool preview、todos、trace log 再次塞回 `Response`；这些仍属于 `Activity` / `Todo`。
- 不要求第一版就支持跨 turn 的 response 历史折叠树；首轮只覆盖当前 turn 的结构化呈现。

## Problem Statement

当前体验的主要缺口：

- `Response` 仍是线性消息列表，step 只能通过 `[Plan] ...` 这类前缀弱表达，阅读成本高。
- root workflow 与 child workflow 已在状态栏和 Activity 中可见，但 `Response` 自身没有与之对应的结构层次。
- 最终 assistant answer 会被夹在若干 step 正文之后，用户很难快速定位“最终可采纳结果”。
- 当前 `LlmClient` 没有流式事件接口；即使 provider 支持 thinking，也无法实时送到 UI。
- 如果未来把 thinking 直接当普通文本追加到 `Response`，会进一步恶化可读性，并且模糊“过程内容”和“最终回答”的边界。

## UX Direction

### Response Surface Model

`Response` 应从“消息行列表”演进为“当前 turn 的结构化 timeline”。推荐最小结构：

- `Turn header`: 当前 turn 的 scene / selected workflow 摘要。
- `Routing summary`: root workflow 的 `scene-recognition -> select-workflow`，默认可折叠。
- `Step blocks`: child workflow 中每个 step 一个稳定 block。
- `Thinking block`: 当 provider 返回 reasoning/thinking 时，挂在当前活跃 step 或 final answer 之下。
- `Final answer block`: 始终作为 turn 末尾的显式结果块，默认展开。

### Step Block Rules

每个 step block 至少应包含：

- step label
- workflow id / workflow role (`root` / `child`)
- scene 或 route 上下文摘要
- step 状态（pending / streaming / completed / failed）
- step 正文内容区

推荐默认行为：

- root routing blocks 默认折叠，只保留一行摘要。
- child workflow 当前活跃 step 默认展开。
- 已完成 step 默认保持展开，但允许后续通过交互折叠。
- final answer block 始终展开，优先保证可见。

### Thinking Presentation Rules

thinking 只在以下前提下显示：

- provider 返回显式 reasoning/thinking block 或 thinking delta。
- 当前用户配置允许显示 thinking。

thinking 的展示规则：

- 与 final answer 分离，不混在同一正文块里。
- 默认以较弱视觉层级显示，避免抢占最终回答的主阅读权重。
- streaming 中保持实时追加；完成后可折叠为摘要。
- 若 provider 未返回 thinking，则不显示任何“伪 thinking 占位文案”。

## Architecture Direction

### Layer Ownership

- `omega-client`: 提供流式 chat 接口，并把 provider response 拆为 typed stream events。
- `omega-core`: 在 agent loop 中消费 stream events，并保持现有非流式路径可用。
- `omega-session`: 把 stream events 归一为 runtime UI contract，决定 thinking 属于哪个 response section。
- `omega-tui`: 维护 response timeline view state，渲染 step blocks / thinking blocks / final answer block。

### Architectural Rule

- thinking 的“是否显示、显示到哪个 response block、何时折叠为摘要”必须通过 session-owned contract 表达，不能把判断逻辑散落在 TUI reducer 中。
- `omega-tui` 不直接读取 `omega-client::ContentBlock::Thinking`；它只消费 session 发出的 runtime UI envelope。
- 若 provider 不支持 streaming，仍必须保留一次性 response path；结构化 response 不能绑定到某一家模型能力上。

## Runtime Contract Direction

当前 `RuntimeUiMessage` 只有 `UiContent::Text(String)`，不足以表达“开始一个 response section、追加 delta、完成一个 block、thinking 属于哪个 block”。下一阶段推荐补齐两类能力：

### 1. Response Section Identity

每段 response-visible 内容需要稳定 section id，用于让 TUI 按 block 聚合，而不是按文本到达顺序平铺。

推荐方向：

- 新增 response section / response block 概念。
- 支持 begin / append / complete 语义，而不是每次只能发整段替换后的文本。

### 2. Streaming Content Kinds

需要显式区分至少三类内容：

- step narrative / step result
- final assistant answer
- provider-exposed thinking

推荐方向可以是：

- 扩展 `UiMessageKind`
- 或扩展 `UiContent` 为更强的 typed payload
- 或两者同时扩展，但最终目标是让 TUI 不必靠字符串前缀识别 thinking / final / step delta

## Client And Session Direction

### Client Layer

当前 `omega-client` 只有：

- `chat(request) -> ChatResponse`

若要支持实时 thinking，需要新增流式接口，例如：

- `chat_stream(request) -> Stream<Item = ChatEvent>`

`ChatEvent` 推荐至少覆盖：

- text delta
- thinking delta
- tool use start / complete
- message complete
- usage / stop reason

### Session Layer

`omega-session` 推荐新增 streaming-aware response assembler：

- 为当前活跃 step 创建 response section
- 在模型 streaming 时持续把 text/thinking delta 转成 runtime UI envelope
- 在 step 完成或 tool loop 结束时 finalize section
- 在 final answer 阶段创建明确的 final answer section

## TUI Direction

### View Model

`omega-tui` 当前 `output_msgs: Vec<Msg>` 不再足以表达结构化 response。推荐方向：

- 新增 response timeline state
- 每个 section/block 有独立 id、title、status、body 和可选 thinking 子块
- 支持当前活跃 block 的 streaming append

### Rendering Direction

推荐逐步演进，而不是一次性做复杂组件库：

1. 先把平铺列表改为“按 block 渲染的列表”。
2. 每个 block 内部仍可先用纯文本和简单边框。
3. thinking block 先用弱化色和折叠摘要，不强依赖 Markdown。
4. final answer block 后续再叠加 Markdown / code highlight。

### Interaction Direction

需要预留但不要求首轮全部实现：

- 折叠 / 展开 routing block
- 折叠 / 展开 thinking block
- 快速跳到 final answer
- 在 response 内按 block 搜索，而不是只按文本行搜索

## Responsive Degradation

窄终端下，Response 需要优先保证：

- final answer 可读
- 当前活跃 step 可见
- thinking 不挤压最终回答

推荐退化规则：

- root routing block 自动折叠
- 已完成 thinking block 默认折叠成一行摘要
- 当前活跃 thinking 若过长，仅显示尾部流式窗口 + 已折叠头部计数

## Risks

| Risk | Level | Mitigation |
|------|-------|------------|
| 把 thinking 当成普通文本混入 Response | High | 明确单独 thinking block 与 typed contract |
| TUI 为了新体验直接读取 client/provider 类型 | High | 继续坚持 session-owned runtime UI contract |
| 只做 TUI 设计，不补 streaming backend | High | 把 client/core/session 流式路径拆成独立前置任务 |
| 结构过重导致窄终端不可读 | Medium | 先做 block list，再做 richer widget；定义明确退化规则 |

## Implementation Plan

### Phase 1: Streaming And Contract Foundation

- [x] 为 `omega-client` / `omega-core` / `omega-session` 建立流式 response / thinking 事件路径。
- [x] 为 runtime UI contract 增加 response section identity 与 append/complete 语义。

### Phase 2: Structured Response Timeline

- [x] 把 `omega-tui` 的 `Response` 从平铺文本列表重构为 step-aware timeline。
- [x] 让 root routing、child workflow step 和 final answer 有稳定 block 结构。

### Phase 3: Thinking Visibility

- [x] 将 provider-exposed thinking 作为实时 block 接入 Response。
- [x] 补齐折叠、完成态摘要与配置开关。

## Affected Roadmap Tasks

- `Task 15F-6`: 流式 response / thinking runtime contract
- `Task 15B-20`: 结构化 Agent Response timeline
- `Task 15B-21`: thinking stream 展示与交互控制

## Testing Strategy

- `omega-client`: stream event parsing / provider compatibility tests
- `omega-core`: stream-aware agent loop tests
- `omega-session`: response section assembly、thinking routing、finalization tests
- `omega-tui`: block rendering、stream append、collapse state、窄终端退化 tests

---

### Change Log

- 2026-03-20: 初版规格，规划结构化 Response timeline 与 provider-exposed thinking stream 的统一实现方向。
- 2026-03-20: `Task 15F-6` 完成，补齐 `ChatEvent` / `chat_stream()` 与 response section runtime contract；后续重点转移到 `omega-tui` 消费这些 section event。
- 2026-03-20: `Task 15B-20` 完成，`omega-tui` 已消费 response section effect 并将 `Routing / Step / FinalAnswer` 渲染为结构化 timeline；后续重点聚焦 `Thinking` section 的可见性与折叠交互。
- 2026-03-20: `Task 15B-21` 完成，`omega-tui` 已消费 `Thinking` section 并提供实时追加、完成态默认折叠、`Enter/x` 手动展开以及 `.omega/tui.toml` 的 `[response].show_thinking` 开关。