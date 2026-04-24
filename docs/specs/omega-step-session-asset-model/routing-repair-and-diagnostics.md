---
content_revision: 120
created: 2026-03-25
generation_id: gen_000046_r000120
last_verified_commit: N/A
owner: omega-team
projection_version: 46
related_prds: []
source_doc_id: "spec:docs-specs-omega-step-session-asset-model-routing-repair-and-diagnostics"
status: active
supersedes: []
updated: 2026-03-25
---

# Omega Routing, Repair, And Diagnostics

## Overview

本文覆盖 routing convergence、structured output repair、context observability 以及后续迁移与验证要求。

## Structured Output Repair Strategy

仅靠校验失败后的 blind retry 不足以处理高 token、强结构 step。推荐恢复策略为 `RepairThenRegenerate`：

1. Primary generation: 正常 step prompt、正常 tool policy
2. Repair pass: 带 validation error、previous response preview、required contract 的轻量修复轮次
3. Full regenerate: repair 仍失败时才回退到完整重跑

对应 runtime policy 可表达为：

```rust
enum OutputRecoveryMode {
    RegenerateOnly,
    RepairThenRegenerate,
}
```

## Config Surface And Runtime Ownership

- workflow config 只声明 `recovery_mode` 这类稳定 policy
- `omega-session` 负责构造 repair envelope、恢复 checkpoint、切换可见工具、记录 attempt kind
- `validation_error`、`previous_response_preview` 等都属于 runtime 产物，不应静态写入配置

## Repair Prompt Contract

repair pass 应使用独立 envelope，而不是复用原 step prompt：

```xml
<output_repair step_id="plan">
mode: repair_structured_output
error_kind: schema_invalid
validation_error: expected object at $
required_contract: return exactly one valid JSON object
</output_repair>
```

关键约束：repair pass 只消费已有上下文，且只产出 JSON，不产生新的用户可见正文。

## Runtime Integration Direction

`omega-session` 需要把输出恢复拆成 `primary | repair | regenerate` 三类 attempt，并在 diagnostics 中保留：

- `validation_error`
- `previous_response_preview`
- `extracted_json_preview`
- `attempt_kind`

## Step Boundary Rule

只要当前 step 还没有产出合法 structured output，就绝不能 advance 到下一 step。repair pass 属于当前 step 的内部恢复流程，不是新 step，也不是对下游 step 的补救。

## Context Observability

当 workflow context 演进为 step data contract + structured outputs，就必须可观测。当前诊断面应至少回答：

- 这个 step 读了哪些 summaries / structured inputs
- 写入了哪些 `step_outputs` / todo changes
- output contract 是否通过、失败几次、当前处于哪种 attempt

截至当前基线，tracing/jsonl 与 TUI diagnostics 已能提供首轮输入/输出/contract 状态钻取。

## Routing State Convergence

`RoutingContext` 应替代独立的 `WorkflowRoutingState` 成为唯一路由状态容器。`WorkflowTurnRunner` 读写 routing 时只能通过 `SessionContext.routing`，child workflow delegation 也从这里取值，不再允许双写。

## Forward Direction: Scene-Aware Workflow Delegation

scene-aware routing 的长期要求是：workflow selection 本身也被表达为 step，而不是平行预处理逻辑。

- `select-workflow` 仍由通用 step runner 执行，并同时承担 scene recognition 与 workflow selection
- step completion 可表达 `StartWorkflow { workflow_id }`
- workflow stack / active workflow state 继续归 session 拥有

## Context Direction

下一轮 context 演进不应跳成完整 artifact 平台，而应继续围绕：

- typed routing context
- step summaries
- step outputs
- diagnostics / compression / retention

## Migration Plan

### Phase 1: 全 step 最小循环

所有内建 root/chat/feature steps 统一为 bounded agent loop。

### Phase 2: Session-owned Step Context

为每个 step 产出 summary，并显式写回 `SessionContext`。

### Phase 3: Step Data Contract Framework

引入 `input_contract` / `output_contract`、JSON 提取与恢复策略。

### Phase 4: Feature Workflow Schema Binding And Todo Integration

让 feature workflow 的 explore/plan/execute/report 正式消费结构化上下文与 todo 映射。

### Phase 5: Context Observability And Compression

在 diagnostics 稳定后继续演进 retention/compression，而不是重开一套新的 context 主路径。

## Risks

- session 资产层若内联过多逻辑，会退化成 God Object
- routing 若继续保留双状态容器，会导致 root/child handoff 双写失真
- data contract 若缺少 repair/diagnostics，结构化输出会回退成脆弱的 blind retry

## Testing Strategy

- `omega-workflow`: 配置解析与默认 contract 单测
- `omega-session`: asset resolution、context assembly、repair/validation、routing convergence 单测
- `omega-core`: dynamic tool visibility 单测
- `omega-tui`: diagnostics 与状态栏/response timeline 可见性单测

---

### Change Log

- 2026-03-25: 从入口规格中拆出 routing、repair、diagnostics、migration 与验证要求。
