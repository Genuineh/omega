---
content_revision: 174
created: 2026-04-16
generation_id: gen_000087_r000174
language: bilingual
last_verified_commit: d8c30e3e9e310ce38cffa965be4688ed55a87787
owner: omega-team
projection_version: 87
related_prds:
  - docs/prds/omega-benchmark-evaluation.md
source_doc_id: "spec:docs-specs-omega-benchmark-system"
source_path: docs/specs/omega-benchmark-system.md
status: draft
supersedes: "[]"
updated: 2026-04-16
version: v0.1
---

# Omega Benchmark System Specification

## Overview

`omega-benchmark` 是一个位于仓库顶层、独立于 `crates/` 的 workspace member，用来评估 Omega 作为 agent 系统的真实能力，而不是继续把能力验证分散在单元测试、局部集成测试和人工试跑里。v1 覆盖三条主线：BFCL 风格工具调用、GAIA 风格综合任务，以及数据生成质量评估。静态 suite 与 baseline 定义留在 `omega-benchmark/` 下，生成态运行产物进入 repo-local state，而不是混进 docs 或现有 runtime crate。

## Goals

- 新增顶层独立项目 `omega-benchmark`，但继续接入根 workspace。
- 为 benchmark case、runner、scorer、report 建立稳定 contract。
- 统一评估工具调用、综合任务完成度和数据生成质量三类能力。
- 通过 frontend-neutral 的 Omega target 执行 benchmark，而不是耦合 TUI。
- 让结果具备可比较、可回放、可追踪的 artifact 结构。

## Non-Goals

- 不用 benchmark 取代现有 unit test、integration test 或 deterministic seam 测试。
- 不把 TUI 渲染、布局或交互细节纳入 benchmark 指标。
- 不在 v1 直接镜像完整外部数据集或构建公共 leaderboard。
- 不要求首轮就支持所有可能的 benchmark target；先保证 Omega 自身路径可评估。

## Project Boundary

| Layer | Responsibility |
|------|----------------|
| `omega-benchmark` | suite manifest、fixture 组织、runner、scorer、report、baseline compare |
| Omega runtime target | 提供 frontend-neutral 的执行边界、tool trace、response 和 delivery summary |
| `omega-tui` | 不进入 benchmark 主路径；UI 不是能力评估边界 |

核心规则：benchmark 必须把 Omega 当成被评估对象，而不是把 TUI 或人工交互当成测试 harness。

## Benchmark Tracks

### Tool Calling Track

- 参考 BFCL，覆盖 simple、multiple、parallel、irrelevant 等 case 族。
- 评分重点是工具选择、参数构造、并行/多工具调用结构和无关工具拒绝。

### General Assistant Track

- 参考 GAIA，覆盖需要多步推理、检索、文件处理和工具协作的综合任务。
- 评分重点是 task completion、exact or quasi-exact match、evidence completeness 和失败归因。

### Data Quality Track

- 覆盖训练数据、测试数据或结构化输出生成任务。
- 评分重点是 schema validity、judge score、win rate 和人工抽样通过率。

## Architecture

benchmark 系统建议收口为五个内聚部件：

- `SuiteRegistry`: 按 track 注册 suite id、manifest loader 和 scorer。
- `BenchmarkTarget`: 统一执行 Omega 请求并返回标准化结果。
- `CaseRunner`: 对单 case 负责 fixture 准备、执行、超时和重试策略。
- `ScoringPipeline`: 对不同 track 执行精确匹配、quasi match、judge 和聚合逻辑。
- `ReportStore`: 负责写入 per-case 结果、总览摘要和 baseline diff。

v1 的默认方向是先实现一个 session/app-owned、frontend-neutral 的 in-process target，直接复用现有 runtime message 与 delivery summary contract；黑盒 CLI or process adapter 作为后续增强，而不是前置依赖。

## Data Model

公共数据模型至少要覆盖：

- `BenchmarkSuiteManifest`: suite id、track、case list、default scorer、fixture root。
- `BenchmarkCase`: prompt、expected outcome、allowed tools、timeout、tags。
- `RunConfig`: model、workflow or prompt preset、tool budget、max turns、seed or deterministic flags。
- `CaseResult`: raw output、tool trace、delivery summary、latency、token usage、score breakdown、failure reason。
- `RunSummary`: suite-level aggregate、per-metric summary、baseline diff、artifact paths。

模型规则：不同 track 可以扩展 case payload，但最终都必须下沉到统一的 `CaseResult` 和 `RunSummary`。

## Runner And Scoring Flow

单次 benchmark run 的最小流程为：

1. 读取 suite manifest 与 run config。
2. 为 case 准备 fixture、输入文件和允许工具集。
3. 调用 `BenchmarkTarget` 执行 Omega。
4. 规范化收集 response、tool trace、delivery summary、token 和 latency。
5. 按 track 调用 scorer。
6. 输出 per-case result、run summary 和 baseline diff。

评分规则：

- Tool calling track 优先使用结构化 call comparison，而不是自由文本判断。
- General assistant track 允许 exact match 与 quasi-exact match 并存，但必须保留 evidence。
- Data quality track 默认采用 judge plus pairwise compare，再叠加人工抽样入口。

## Metrics And Artifacts

每次 run 至少输出以下指标：

- tool track: `tool_selection_accuracy`, `argument_exact_match`, `parallel_call_validity`, `irrelevance_rejection_rate`
- assistant track: `task_completion_rate`, `exact_match`, `quasi_exact_match`, `failure_rate`
- data track: `schema_validity`, `judge_score`, `win_rate`, `human_audit_pass_rate`
- common metrics: `latency_ms`, `total_tokens`, `tool_count`, `failure_reason`

产物布局建议：

- `omega-benchmark/suites/`: committed suite and fixture definitions
- `omega-benchmark/baselines/`: committed baseline summaries for regression compare
- `.omega-state/benchmark/runs/`: local generated run artifacts and detailed evidence

规则：commit 进入仓库的是 suite 和 baseline；大体积运行结果进入 repo-local state。

## Rollout Plan

### Phase 1

- 建立 `omega-benchmark` CLI、suite manifest 和 run summary schema。
- 建立 frontend-neutral Omega benchmark target。

### Phase 2

- 落地 BFCL 风格 tool calling suite 与 scorer。
- 沉淀第一批 golden baseline。

### Phase 3

- 落地 GAIA 风格综合任务 suite 与 scorer。
- 增加 evidence-aware completion summary。

### Phase 4

- 落地数据生成质量 suite，包括 judge、win-rate 和人工抽样接口。
- 增加 baseline diff、回归摘要和报告导出。

## Acceptance Criteria

- `omega-benchmark` 可以独立选择 suite 并执行 benchmark run。
- 同一条 run contract 可以覆盖 tool calling、general assistant 和 data quality 三个 track。
- benchmark 结果包含结构化 evidence，而不只是最终分数。
- baseline compare 能输出回归、持平和提升。
- 整个评估路径不依赖 TUI surface。

## Related Docs

- `docs/prds/omega-benchmark-evaluation.md`
- `docs/TODO.md`
- `docs/specs/omega-task-delivery-observability.md`

---

### Change Log

- 2026-04-16: 初版规格，定义 `omega-benchmark` 的项目边界、三条评估主线、artifact contract 和 rollout 方向。
