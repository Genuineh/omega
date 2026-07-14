---
content_revision: 174
created: 2026-06-05
generation_id: gen_000087_r000174
language: bilingual
last_verified_commit: d8c30e3e9e310ce38cffa965be4688ed55a87787
owner: omega-team
projection_version: 87
related_prds: "[]"
source_doc_id: "adr:docs-decisions-011-tui-chat-log-summary-cleanup"
source_path: docs/decisions/011-tui-chat-log-summary-cleanup.md
status: accepted
supersedes: "[]"
updated: 2026-06-05
---

# 011: Chat-Log 摘要清理 (T-47~51 与 T-52~62 冲突解决)

## Status

Accepted (2026-06-05).

## Context

经过 `Task 47 ~ 51`（卡片化）与 `Task 52 ~ 62`（Flex 原语 + StepUnit + StepDetailOverlay + ChatTurn）两轮升级，`Agent Response` 面板出现"信息重复展示"的 bug：

1. **`T-47 ~ 51` 设计意图**：每条 `MsgKind` 记录 = 主面板 1~2 行**摘要**（kind glyph + formatted title + state），详情在 `StepDetailOverlay` / `TurnDetailOverlay` 里。
2. **`T-53` 实施**：`StepUnit` 简化为 `Vec<Line<'static>>`，每行由 `response_card::build_response_lines(line, colors, inner_w)` 渲染。`build_response_lines` 对一个 `Step` 返回 1 行（header），对一个 `Step` 的 body line 也返回 1 行（含 data layer 的 `  Gather context` 2-char indent）。**这个函数返回的是 data layer 的完整输出，不是摘要**。
3. **`T-58` 实施**：`ChatTurn::render` 遍历 `agent_msgs`，对每条用 `build_response_lines` 渲染并装进 `FlexContainer` 子节点。**结果每个 sub-record 在主面板渲染了 header + body 共 2+ 行**。

### 实测验证

30-row terminal，1 turn = User "Tell me a joke" + Step "Search" + FinalAnswer "Answer"：

```
│· You                                          ← user bubble title
│  ▶ Tell me a joke                             ← user bubble body
│                                               ← gap
│◉ step  child:wf-1  Search  ●                  ← Step SUMMARY (1 row)
│                                               ← gap
│  Found 3 jokes.                               ← Step BODY (应在 popup)
│                                               ← gap
│◆ final  child:wf-1  Answer  ●                ← FinalAnswer SUMMARY
│                                               ← gap
│  │ Why did the chicken cross the road?...    ← FinalAnswer BODY (应在 popup)
```

6 行内容；body 行在主面板和 popup 里**重复展示**。用户的"摘要 + 弹窗"愿景被破坏。

## Decision

### 1. `StepUnit` 简化为摘要 + body 二元组

`StepUnit::lines: Vec<Line<'static>>` 改为：

```rust
pub struct StepUnit {
    /// 1-line summary for the chat log (T-63).
    pub summary_line: Line<'static>,
    /// Full body content for the popup (T-55 / T-61).
    pub body_lines: Vec<String>,
    /// Optional detail request for the Enter handler.
    pub detail_target: Option<StepDetailRequest>,
}
```

### 2. `build_subunit_summary` 新增

新函数（在 `chat_turn.rs` 或新的 `subunit_summary.rs`）：

```rust
pub fn build_subunit_summary(
    line: &ResponseDisplayLine,
    colors: &ColorScheme,
    inner_w: usize,
) -> Line<'static>;
```

返回 1 行摘要：kind glyph + formatted title + 可选 body 预览。**body 行不返回**。

### 3. `ChatTurn::render` 改用 summary

不再调 `build_response_lines`，改调 `build_subunit_summary`。每个 sub-record 只占 1 行。

### 4. 弹窗行为不变

`App::open_step_detail_overlay` / `open_turn_detail_overlay` 仍走 `m.text` 全量 → popup 仍是 source of truth。Body 仍然完整显示在弹窗里。

## Alternatives Considered

### 替代 A：保留 T-58 的 inline body，但缩短 body 行（截断到 30 字符）

减小主面板冗余感，但 body 仍部分暴露在主面板，且语义错位（"摘要应该是摘要"）。

### 替代 B：完全重写 `ChatTurn::render`，每个 sub-record 不再渲染 inline body，只用 summary

跟当前决策相同，但需要在 T-53 重构 `StepUnit` 时就做，而不是 T-63 单独做一次。

### 替代 C：保留 `build_response_lines` 但加一个 "summary-only" 模式

不动现有数据流，新增一个 `summary_mode: bool` 参数。复杂度更高，但更兼容旧调用点。否决：`build_response_lines` 当前只在 `chat_turn.rs` 和 `event/overlay_handlers.rs` 调，影响面小。

## Consequences

### 正面

- **Chat log 简洁**：每条记录只占 1 行摘要（+ kind 区分），4 行覆盖 user + 1 agent response。
- **Body 不再重复**：body 完全在 popup 里展示，主面板纯粹是 chat-log 索引。
- **T-47~51 设计意图被还原**：摘要 + 弹窗分离，恢复"fixed + variable + detail"三段式。
- **不破坏 popup 行为**：`App::open_step_detail_overlay` / `open_turn_detail_overlay` 不动，body 仍在弹窗里全量展示。

### 负面

- **修改 ChatTurn 主路径**：需要把 `build_response_lines` 调替换为 `build_subunit_summary`，影响 ~50 行代码。
- **测试需要更新**：原本期望"header + body 2 行"的 snapshot 测试需要改为"摘要 1 行"。

## Implementation Plan

| Task | 内容 | 依赖 | 估时 |
|---|---|---|---|
| T-63 | `StepUnit` 重构 + `build_subunit_summary` + `ChatTurn::render` 改用 summary | T-52~T-62 | 0.5 day |
| T-64 | Snapshot 测试替换 / 新增（1-row-per-sub-record） | T-63 | 0.25 day |
| T-65 | 弹窗 body 全量回归测试 | T-63 | 0.25 day |
| T-66 | 文档 + ADR 更新 | T-63 ~ T-65 | 0.25 day |

总计 ~1.25 工作日，4 个独立 PR。

## References

- `docs/specs/omega-tui-chat-log-summary-cleanup.md` — 设计规格
- `docs/specs/omega-tui-flex-layout-and-step-unit.md` — T-52~T-57 上一轮规格
- `docs/specs/omega-tui-chat-turn-history.md` — T-58~T-62 上一轮规格
- `docs/decisions/008-tui-component-architecture-refactor.md` — Panel/Section/Card 抽象
- `docs/decisions/009-tui-flex-layout-primitives.md` — Flex 原语
- `docs/decisions/010-tui-chat-turn-history.md` — ChatTurn
- `docs/TODO.md` — Task 63 ~ 66
