---
baseline: omega-benchmark/baselines/run-20260417-040256.json
content_revision: 120
created: 2026-04-17
endpoint: "https://api.minimaxi.com/anthropic"
generation_id: gen_000046_r000120
model: MiniMax-M2.5
owner: omega-team
projection_version: 46
run_id: run-20260417-040256
source_doc_id: "guide:benchmark-report-20260417"
status: active
updated: 2026-04-17
---

# Omega Benchmark Report — 2026-04-17

## Run Summary

| 字段 | 值 |
|---|---|
| Run ID | `run-20260417-040256` |
| Model | MiniMax-M2.5 |
| Endpoint | `https://api.minimaxi.com/anthropic` |
| Timestamp | 2026-04-17T04:02:56 UTC |
| Total Cases | 12 |
| Passed | 4 |
| Failed | 8 |
| Errors | 0 |
| Timeouts | 0 |
| **Aggregate Score** | **0.444** |
| Total Latency | 645,793 ms (~10.8 min) |

## Suite Results

### assistant-basic [assistant]

| Metric | Score |
|---|---|
| task_completion | 1.000 ✓ |
| quasi_exact_match | 0.800 |
| evidence_completeness | 0.500 |
| exact_match | 0.000 |
| **Aggregate** | **0.692** |
| Cases | 4 total, 2 passed, 2 failed |

### data-quality-basic [data-quality]

| Metric | Score |
|---|---|
| judge_score | 0.000 |
| schema_validity | 0.000 |
| win_rate | 0.000 |
| **Aggregate** | **0.000** |
| Cases | 3 total, 0 passed, 3 failed |

### tool-basic [tool-calling]

| Metric | Score |
|---|---|
| irrelevance_rejection_rate | 1.000 ✓ |
| tool_selection_accuracy | 0.556 |
| argument_exact_match | 0.000 |
| parallel_call_validity | 0.000 |
| **Aggregate** | **0.511** |
| Cases | 5 total, 2 passed, 3 failed |

## Analysis

**assistant-basic (0.692)**: task_completion 满分说明模型能理解并完成任务目标。quasi_exact_match 0.8 说明语义正确，但 exact_match 为 0 表明输出格式不够严格（如包含多余文字、标点）。evidence_completeness 0.5 说明响应在该维度存在信息缺漏。

**data-quality-basic (0.000)**: 全部失败。judge_score 与 win_rate 为 0 可能因为当前 scorer 预期外部 judge 模型调用，但 benchmark 运行时未配置 judge target，导致 scorer 得到空响应后输出 0 分。schema_validity 为 0 说明结构化输出与期望 schema 不匹配，需要进一步检查 suite case 的 expected schema 设计与提示词。

**tool-basic (0.511)**: irrelevance_rejection 满分说明模型能正确拒绝不相关工具调用，这是重要的安全属性。tool_selection_accuracy 0.556 说明约半数 case 的工具选择正确。argument_exact_match 和 parallel_call_validity 为 0 说明参数格式或多工具并发调用路径仍需改善。

**token 统计**: `total_tokens: 0` 是因为 `OmegaTarget` 当前硬编码返回 0，token 统计路径未接入 session recorder。这是已知 gap，不影响 score 准确性。

## Next Steps

1. **data-quality scorer**: 检查 `judge_score` / `win_rate` scorer 实现，确认是否需要 judge target；若不需要外部 judge，改为 schema-only 评估模式。
2. **exact_match 改善**: 检查 assistant-basic 失败 case 的实际输出 vs 期望输出，判断是提示词问题还是模型格式倾向。
3. **argument_exact_match**: 查看 tool-basic 失败 case 的参数格式，考虑放宽匹配策略（如忽略字段顺序）或改进 system prompt。
4. **token 统计**: 在 `OmegaTarget::execute` 中从 `RuntimeMessageEnvelope` 提取实际 token 用量（当前 `total_tokens: 0` 是已知 gap）。
5. **baseline 对比**: 当前 run 已保存为 baseline `run-20260417-040256`，后续跑 `omega-bench compare <new-run-id>` 可得回归对比。
