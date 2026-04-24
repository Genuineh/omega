---
content_revision: 120
created: 2026-04-16
generation_id: gen_000046_r000120
last_verified_commit: N/A
owner: omega-team
projection_version: 46
source_doc_id: "prd:docs-prds-omega-benchmark-evaluation"
status: draft
updated: 2026-04-16
version: v0.1
---

# Omega Benchmark Evaluation

## Summary

为 Omega 新增一个独立于 `crates/` 的顶层项目 `omega-benchmark`，用统一的 benchmark suite、runner 和 scoring contract 评估三类能力：BFCL 风格工具调用、GAIA 风格通用助手任务，以及数据生成质量。目标不是再写一套零散测试，而是建立可重复、可比较、可回归的能力评估系统。

## Problem

当前仓库已经有 build、unit test 和 integration test，但这些验证大多回答的是实现是否正确，而不是 Omega 作为 agent 的能力是否稳定提升。我们现在缺少三类可重复证据：

- 工具调用是否真的能稳定选对工具、构造对参数、拒绝无关工具。
- 综合任务是否真的能完成多步问题，而不是只在局部测试里看起来可行。
- 生成类任务输出是否有质量、可比较、可抽样复核。

没有一条独立的 benchmark 主线，就很难回答模型切换、prompt 调整、tool contract 变更或 runtime 重构到底是提升了能力，还是只改变了局部实现细节。

## Users

- 需要持续比较不同模型、prompt 和 runtime 版本效果的 Omega maintainer
- 负责 workflow、tool、context、delivery summary 等核心能力的实现者
- 需要为回归、发布或实验结论提供可审计证据的评估与研究使用者

## Requirements

### Must Have (P0)

- `omega-benchmark` 必须作为顶层独立项目存在，不放入 `crates/` 目录。
- 系统必须覆盖三类 suite：BFCL 风格工具调用、GAIA 风格综合任务、数据生成质量评估。
- 每个 benchmark case 都必须有稳定 manifest、输入、评分规则和可复跑的 run config。
- benchmark 运行必须通过 frontend-neutral 的 Omega 执行边界完成，而不是依赖 TUI 组件或手工 UI 操作。
- 结果必须同时记录 correctness、latency、token usage、failure reason 和可回放 evidence。
- benchmark 结果必须能和已有 baseline 做机器可读比较。

### Should Have (P1)

- BFCL 风格 suite 应区分工具选择正确率、参数精确匹配、多工具/并行调用正确性和无关工具拒绝率。
- GAIA 风格 suite 应支持 exact match、quasi-exact match 与 evidence-aware completion 判定。
- 数据质量 suite 应支持 judge score、win rate 与人工抽样复核。
- 运行器应支持按 suite、track、模型和场景筛选批量执行。

### Nice to Have (P2)

- baseline regression gate 可进入 CI 或 release 检查。
- 支持从外部 benchmark 数据源导入或裁剪本地可分发子集。

## Design

- 技术规格：`docs/specs/omega-benchmark-system.md`
- 运行时边界：`docs/specs/omega-agent-spec.md`
- 交付统计与可观测性：`docs/specs/omega-task-delivery-observability.md`
- 项目入口说明：`omega-benchmark/README.md`

## Implementation Tasks

- `TASK-0019`: 建立 benchmark manifest、suite registry 与 CLI 主入口。
- `TASK-0020`: 建立 frontend-neutral 的 Omega benchmark target，统一收集 response、tool trace 与 delivery summary。
- `TASK-0021`: 落地 BFCL 风格工具调用评估与评分。
- `TASK-0022`: 落地 GAIA 风格综合任务评估与评分。
- `TASK-0023`: 落地数据生成质量评估，包括 judge、win-rate 与人工抽样接口。
- `TASK-0024`: 落地结果汇总、baseline 对比与回归报告。

## Acceptance Criteria

- 可以针对选定 suite 运行一次完整 benchmark，并生成 per-case 结果与总览摘要。
- 三条评估主线都有 v1 的 case manifest 与 scorer。
- benchmark 输出可以按模型、配置和 suite 维度比较回归与提升。
- `docs/TODO.md` 中能看到这条新 benchmark 轨道的开放任务。

## Open Questions

- v1 是否只支持 in-process 的 session/app 评估边界，还是需要同时提供更黑盒的 CLI/process adapter。
- 外部 benchmark 数据集的 license、切片和 repo 分发策略如何控制。
- 哪些数据质量 case 必须进入人工抽样，而不能只依赖 LLM judge。

---

### Change Log

- 2026-04-16: 初版 PRD，定义 `omega-benchmark` 的项目边界、评估主线和首轮任务拆解。
