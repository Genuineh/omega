---
status: active
last_verified_commit: N/A
owner: omega-team
created: 2026-04-07
updated: 2026-04-07
version: v1.0
scope: improvement-plan
based_on:
	- learn/hindsight-vs-omega-memory-analysis.md
	- learn/hindsight/hindsight-docs/docs/developer/retain.md
	- learn/hindsight/hindsight-docs/docs/developer/retrieval.md
	- learn/hindsight/hindsight-docs/docs/developer/reflect.mdx
	- learn/hindsight/hindsight-docs/docs/developer/observations.mdx
	- docs/specs/omega-context-management.md
	- docs/specs/omega-tui-document-memory-supervision.md
	- docs/specs/omega-command-system.md
	- crates/omega-context/src/lib.rs
	- crates/omega-document/src/lib.rs
	- crates/omega-memory/src/lib.rs
---

# Omega Memory Improvements Inspired by Hindsight

## Overview

本文不重复做系统对比，而是直接回答一个更具体的问题：

> 参考 Hindsight 后，Omega 的 `document + memory` 系统最值得补的改进是什么？

实现说明（2026-04-07）：这份文档保留为调研结论。面向仓库实施的修正版方案已收敛到 `docs/specs/omega-knowledge-evolution.md`。关键调整包括：先做 retention profiles 与 memory query，再做 observation layer；统一 recall planner 延后到 `MemoryService` 具备真实 query surface 之后；strategy-specific chunking 与 relation-aware retrieval 不再视为低风险首轮工作。

结论先说：

- Omega 不该把自己改成“另一个 Hindsight”。
- Omega 应该继续坚持 `repo-local source-of-truth + omega-context facade + workflow-first assembly + document governance`。
- 真正值得吸收的是 Hindsight 的三类高价值机制：
  1. mission-driven extraction
  2. evidence-backed consolidation
  3. unified multi-strategy recall

因此本文给出的改进建议都遵守两个约束：

1. **不破坏 Omega 当前架构长板**
2. **优先做对 coding agent 真有收益的能力**

如果只看 Hindsight 在“知识整理算法”和“检索算法”上的可迁移做法，见：`learn/hindsight-algorithms-for-omega-knowledge.md`。

## Design Principles

### 1. Source Of Truth Must Stay Local And Explicit

`files.jsonl`、真实文件树、document governance 结果、todo snapshots 仍是主真相。

任何新增的 observation / insight / project model 都只能是派生层，不能反向覆盖主存储。

### 2. Consolidated Knowledge Must Carry Evidence

如果 Omega 要增加类似 observation 的层，这些内容必须显式记录：

- 来自哪些 turn summaries / file records / chunks / governance events
- 最后验证时间
- 何时变 stale
- 被什么新证据修正过

否则它会变成“模型写的一段看起来正确的话”，而不是可验证的项目知识。

### 3. Repo-Local Coding Agent First

优先改造能帮助下面这些问题的能力：

- 项目架构快速恢复
- 团队编码约定记忆
- open threads / pending work continuity
- 检索结果更稳定地进入 workflow step
- 文档系统的长期演化与版本历史

人格化 disposition 不是当前重点。

## Recommended Improvements

## Improvement 1: Add A Derived Observation Layer Above Raw Memory And Document Facts

### What to add

在 `omega-context` 之下、但不替代 `omega-memory` / `omega-document` 的前提下，增加一个派生知识层，可以命名为：

- `project observations`
- `context observations`
- 或更保守的 `insight cards`

该层负责把以下原始材料综合成稳定项目知识：

- selected turn summaries
- document health / governance outcomes
- search hit patterns
- file/chunk facts
- operator usage activity

### Example outputs

- “这个仓库当前采用 Rust workspace + facade boundary，`omega-context` 是唯一对外入口。”
- “团队默认倾向 staged document governance，不鼓励黑盒多文件修改。”
- “当前 document backend 已有 active/pending version 机制，但 promotion 失败时仍以旧 active version 服务。”
- “最近几轮任务主要集中在 command system 与 document supervision。”

### Why it is valuable

这能把 Omega 从“有索引、有摘要”提升到“有高价值长期项目认知”。

### Why it is safe

只要 observation 保留 evidence refs，它就是可审计的派生层，而不是危险的黑箱真相层。

## Improvement 2: Add Mission Profiles For Different Memory Domains

### What to add

借鉴 Hindsight 的 `retain_mission` / `observations_mission` 思想，但改成适合 Omega 的 profile 配置，而不是通用 bank 配置。

建议至少有四类 profile：

1. `project_facts`
   保留架构边界、技术栈、crate ownership、目录规则、长期稳定约定。

2. `developer_preferences`
   保留用户偏好、代码风格要求、常见 review bar、工具使用偏好。

3. `open_threads`
   保留未完成任务、决策待确认项、当前 risk。

4. `ephemeral_debug`
   默认弱保留或直接忽略，避免把短期日志噪音当长期记忆。

### Why it is valuable

Omega 当前已经有 memory/doc 分层，但还没有明确声明“什么值得进入长期项目知识”。这会让长期记忆容易被低价值调试内容稀释。

### Suggested storage shape

不要引入 Hindsight 的 bank 概念原样照搬。更适合 Omega 的是：

- workspace-scoped domains
- optional user-scoped preference domain
- optional branch/task-scoped transient domain

## Improvement 3: Build A Unified Recall Planner Across Memory And Document

### Current gap

Omega 现在已经有：

- `search_codebase`
- `/document query`
- context summary selection
- supervision `current_hits`

但这些入口还没有形成“一个统一的 recall planner”。

### What to add

让 `omega-context` 暴露一个更高层的 recall contract，例如：

```rust
RecallRequest {
    goal: RecallGoal,
    max_tokens: u32,
    include_document: bool,
    include_memory: bool,
    include_observations: bool,
    filters: RecallFilters,
}
```

然后在内部统一调度：

- summary-based memory recall
- document keyword/semantic/hybrid recall
- future observation recall
- operator activity / health / version metadata recall

### Why it is valuable

这样 step context 组装、tool 调用、command 输出、supervision 命中摘要可以共享同一 recall policy，而不是各自拉一份近似逻辑。

### What not to do

不要为了模仿 Hindsight 而把 graph retrieval、cross-encoder、agentic reflect 一次性全部塞进来。先做统一 contract，再逐步提升内部策略。

## Improvement 4: Add Freshness And Contradiction Semantics To Derived Knowledge

### What to add

对于未来 observation / insight card，需要增加两类状态：

1. `freshness`
   - fresh
   - stale
   - invalidated

2. `evolution`
   - reinforced by new evidence
   - corrected by new evidence
   - superseded by new architecture state

### Example

旧观察：

> document backend 默认关闭，需要显式 feature 才启用。

新证据到来后，不是直接覆盖，而是变成：

> 该结论适用于旧版本；当前默认 app build 已接通 document backend，但 store 仍然惰性生成。

### Why it is valuable

这能让 Omega 记住“知识是如何变化的”，而不是只记住最新一句话。

这对架构迁移、规则变化、重构后的项目认知尤其重要。

## Improvement 5: Add Curated Project Models Instead Of Disposition-Driven Reflect

### What to add

不要把 Hindsight 的 disposition 原样引入 Omega。Omega 更需要的是稳定的“项目模型卡片”，而不是人格化 reasoning。

建议增加三类 curated cards：

1. `project-context-card`
   总结 tech stack、crate boundaries、关键协议、目录规则。

2. `developer-preferences-card`
   总结用户长期偏好和 review expectations。

3. `open-threads-card`
   总结尚未关闭的任务、风险和待确认决策。

### How to generate

- 可由命令触发生成或刷新
- 可在重要变更后自动标记 stale
- 可由 observation 层辅助生成

### Why this is better than disposition for Omega

对 coding agent 来说，稳定的项目卡片比“高 skepticism / 高 empathy”更有实用价值。

## Improvement 6: Expose Consolidated Knowledge Through Commands And Supervision

### What to add

既然 Omega 已经有 `/document` command 和 supervision 面，就不要把新能力藏成内部黑盒。

建议新增或预留：

- `/document insights`
- `/memory context`
- `/memory open-threads`
- `/memory refresh`

并在 supervision 中增加：

- current project observations count
- stale cards count
- last consolidation time
- evidence coverage quality

### Why it is valuable

这能延续 Omega 当前的优势：任何高价值状态都能被 operator 直接看到，而不是只在 agent 内部默默生效。

## Improvement 7: Add Explicit Noise Gates Before Long-Term Retention

### What to add

在记忆进入长期层之前做显式噪音门控，避免把 transient debugging output 变成长期污染。

建议信号：

- 是否出现文件/架构/约定类事实
- 是否是被重复提及的偏好或模式
- 是否进入 TODO / command / governance 的正式结果
- 是否只是单次失败日志或局部试错痕迹

### Why it matters

Hindsight 借助 mission 来减噪。Omega 的 coding-agent 场景里，噪音密度更高，因此更需要 retention gate。

## Improvement 8: Add Better Memory-Specific Observability

### Current strength

Omega 已经有很强的 supervision / diagnostics 基础。

### What is still missing

如果未来加入 observation 或 curated cards，需要新增更细的指标：

- extraction candidates accepted / dropped
- consolidation runs
- stale observations count
- corrected observations count
- recall source mix: summary vs document vs observation
- token share by recall source

### Why it is valuable

这样可以很快看出“是不是 observation 层在制造噪音”“是不是 recall 过度偏向 document 而忽略 memory”等问题。

## Proposed Adoption Order

## Phase 1: High ROI, Low Risk

1. 加 mission profiles
2. 加 noise gates
3. 统一 recall planner contract

这是最先值得做的，因为它们不会大幅改变现有数据模型，但会显著提高长期记忆质量。

## Phase 2: Derived Knowledge Layer

4. 增 observation / insight cards
5. 加 freshness / contradiction semantics
6. 先通过 command + supervision 暴露

这是把 Omega 从“可检索上下文”推向“可演化项目认知”的关键阶段。

## Phase 3: Advanced Retrieval

7. 在 recall planner 里增强 temporal / relation-aware retrieval
8. 视收益再评估轻量 graph layer

这一步要谨慎，不应为了追赶 Hindsight 而过度复杂化。

## What Omega Should Explicitly Avoid

1. **不要让 reflect-style reasoning 成为主入口**
   Omega 的主入口仍应是 workflow step、tool surface 和 command system。

2. **不要让 synthesized knowledge 直接参与 apply 类治理动作**
   observation 可以建议，但不能直接当作治理真相。

3. **不要引入重型服务化依赖作为本地默认路径**
   PostgreSQL/worker/control plane 模式不应替代 repo-local 默认。

4. **不要把 personality/disposition 产品化放在前面**
   对 Omega 来说，项目模型和知识质量比人格调优重要得多。

## Final Recommendation

如果只选三件最值得做的事，我建议是：

1. **给 Omega 的长期记忆增加 mission profiles 和噪音门控**
   先解决“记什么”。

2. **在 raw summaries / document facts 之上增加 evidence-backed observation 层**
   再解决“如何把零散事实变成稳定项目认知”。

3. **把 memory + document + observation 统一到一个 recall planner contract**
   最后解决“如何把这些知识稳定交付给 workflow、command 和 supervision”。

这条路线既能吸收 Hindsight 的高价值思想，又不会破坏 Omega 当前最重要的架构优势。