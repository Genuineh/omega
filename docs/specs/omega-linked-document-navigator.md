---
content_revision: 120
created: 2026-04-17
generation_id: gen_000046_r000120
last_verified_commit: N/A
owner: omega-team
projection_version: 46
related_prds: []
source_doc_id: "spec:docs-specs-omega-linked-document-navigator"
status: draft
updated: 2026-04-17
---

# Omega Linked Document Navigator Overlay Specification

## Overview

当前 `/plan` 与 `/document` 的内容查看仍以 picker 或纯文本 detail overlay 为主。用户一旦进入 detail overlay，就失去左侧上下文和关联内容目录，想跳到 sibling artifact、linked spec 或 query result 的其他命中项时，只能关闭弹窗再重新进入。本文定义一个共享的 linked document navigator overlay：在同一个 overlay 内固定左侧导航列、右侧正文列，让 `/plan` 与 `/document` 都能在不退出 detail surface 的前提下完成关联内容跳转。

## Goals

- 为 `/plan` 与 `/document` 提供同一套 detail-drill-down overlay contract。
- 在 overlay 内固定左侧导航列，让用户能持续看到当前上下文集合与直接关联项。
- 让右侧正文区支持 markdown/file/log 三类内容的稳定呈现，而不是退回长字符串。
- 保持 keyboard-first 交互：在不关闭 overlay 的前提下切换条目、滚动正文、返回上层入口。
- 让 runtime contract 与 TUI render 都能复用到后续 document-heavy operator flow，而不是只为某个命令族做特例。

## Non-Goals

- 不把当前需求做成新的常驻 sidebar 或全局 panel；它仍然是短时聚焦的 overlay。
- 不在首轮引入完整知识图谱浏览器或任意深度的 relation graph explorer。
- 不替换现有 `OperatorPickerRequest` 入口；picker 仍负责选择对象，navigator 负责在 detail 内持续跳转。
- 不要求首轮从任意源码文件自动推断复杂关系；没有结构化 relation 时允许退化为当前上下文集合导航。
- 不在首轮引入多 overlay stack；同一时刻仍保持单活动 overlay。

## Current Gap

- `omega-session::runtime_ui::UiContent` 当前只有 `Text` 与 `OperatorPicker`，detail overlay 无法承载固定目录与结构化正文。
- `/plan links` 当前先打开 `OperatorPickerRequest`，再由 `/plan view-file` 或 `open-link` 退回 `OverlayTarget::Detail + UiContent::Text`。
- `/document query` 当前把命中结果渲染成正文字符串；它可以回答“命中了什么”，但不能在同一 surface 内持续浏览这些命中项。
- `omega-tui` 的 `OverlayState::Detail` 仍是单列滚动文本；切到另一个关联对象时，当前 detail shell 与上下文选择都会消失。

## Experience Model

overlay 采用稳定的两列结构：

- 顶部 header：显示当前入口来源、当前条目标题、breadcrumb 与快捷键提示。
- 左侧 navigator rail：固定宽度，默认 28 到 32 列，不跟随正文滚动。
- 右侧 content pane：显示当前条目的正文与元数据，独立滚动。

左侧 rail 的最小分组：

- `Context`：当前 `/plan` task artifact 集合，或当前 `/document query` result set。
- `Related`：当前条目可解析到的 direct relation、artifact sibling、或当前 task 下的其他可跳转文档。
- `History`：本次 overlay 内已经访问过的条目；首轮可以只保留最近 5 个。

最小交互：

- `Up/Down` 或 `j/k`：在当前 focus 区域移动。
- `Tab`：在左侧 rail 与右侧正文之间切换 focus。
- `Enter`：打开左侧选中条目，并在同一个 overlay 内刷新右侧正文。
- `Esc`：关闭 overlay。

设计规则：用户从 `/plan` 或 `/document` 进入 navigator 后，不应该因为查看一个关联条目而被迫回到命令响应面板。

## Runtime Contract

建议把 navigator 作为新的结构化 `UiContent` 变体，而不是继续把所有 detail 内容压平为纯文本。

新增 contract 方向：

- `UiContent::DocumentNavigator(DocumentNavigatorRequest)`
- `DocumentNavigatorRequest` 至少包含：`navigator_id`、`title`、`origin`、`active_entry_id`、`entries`、`content`。
- `entries` 至少包含：`id`、`label`、`group`、`kind`、`preview`、`disabled_reason`。
- `content` 至少包含：`title`、`subtitle`、`body`、`body_kind`、`breadcrumbs`。

其中 `body_kind` 的首轮枚举即可覆盖：

- `markdown`
- `file`
- `log`

ownership 规则：

- `omega-session` 负责基于 `/plan` task links、`/document` query hits 与 structured doc relations 组装 `DocumentNavigatorRequest`。
- `omega-document` 继续拥有 relation / doc lookup / render-ready body 的来源逻辑。
- `omega-tui` 只拥有 navigator overlay state、focus、scroll 与渲染，不拥有 relation 推断。

in-place 刷新规则：

- 同一条 navigator drill-down 必须复用稳定的 `navigator_id`。
- 当 runtime 发送同一 `navigator_id` 的新 request 时，TUI 应更新当前 overlay 内容，而不是把它当成“关闭旧 overlay，再打开新 overlay”的两次独立流程。
- 这样可以在保持同一 overlay shell 的前提下切换正文，并保留 rail focus、history 与交互心智。

## Command Integration

`/plan` 接入规则：

- `/plan list` 与 `/plan links` 继续使用 picker 作为第一层选择入口。
- 当用户从 task links 进入某个 file/doc/log item 时，session 不再只发 plain-text detail，而是发 seeded navigator request。
- task artifact 是左侧 `Context` 组的初始集合；如果当前 active item 对应 structured doc，再把 direct doc relations 加入 `Related` 组。

`/document` 接入规则：

- `/document query` 仍可以先返回结果摘要或结果 picker，但选中某个命中后应进入 navigator overlay。
- query result set 是左侧 `Context` 组的初始集合；当前 active hit 若能映射到 structured doc，则把 direct relations 加入 `Related` 组。
- 如果命中只是普通文件或代码片段，没有 structured relations，也必须允许以“仅上下文集合导航”的模式工作，而不是报错。

边界规则：

- picker 负责“选哪个对象进入 detail”。
- navigator 负责“在 detail 中继续跳哪个关联对象”。
- command handler 不应把多列布局细节回流到字符串模板；它只负责提供 typed navigator request。

## Rollout Plan

### Phase 1: Shared contract and builder seam

- 在 `omega-session::runtime_ui` 中新增 `UiContent::DocumentNavigator` 及对应 request model。
- 在 `omega-session` 中新增 shared builder，用同一套 entry/content model 服务 `/plan` 与 `/document`。

### Phase 2: TUI overlay state and render

- 在 `omega-tui` 中新增 `OverlayState::DocumentNavigator`。
- 落地左 rail / 右 content 的两列渲染、focus 切换、滚动和 same-`navigator_id` in-place update。

### Phase 3: `/plan` integration

- 让当前 task artifact/file/log drill-down 改为发 navigator request。
- 以 task design links、implementation links 与 recent logs 作为初始 `Context`。

### Phase 4: `/document` integration

- 让 query-result drill-down 改为发 navigator request。
- 以 query result set 为初始 `Context`，并在 active structured doc 上展开 `Related`。

### Phase 5: Polish and follow-ups

- 补齐 overlay history、breadcrumb polish、mouse hit target 与 direct open affordance。
- 再决定是否增加 backlinks、more-like-this 或 richer metadata badges。

## Acceptance Criteria

- `/plan` 与 `/document` 至少共享一个 typed navigator request contract，而不是继续各自产生纯文本 detail。
- 在 navigator overlay 中，左侧 rail 保持固定，右侧正文独立滚动，并支持不关闭 overlay 的条目切换。
- `/plan` 能用 task artifact 集合作为初始导航上下文；`/document` 能用 query result set 作为初始导航上下文。
- 缺少 structured relation 时，overlay 仍能以单纯 context-set navigator 退化工作。
- keyboard-only 路径完整：rail 选择、pane focus 切换、正文滚动、关闭 overlay 都有明确按键。

## Change Log

- 2026-04-17: 初版规格，定义 `/plan` 与 `/document` 共用的 linked document navigator overlay、typed runtime contract、两列交互模型与分阶段 rollout。
