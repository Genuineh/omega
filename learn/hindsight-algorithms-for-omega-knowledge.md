---
status: active
last_verified_commit: N/A
owner: omega-team
created: 2026-04-07
updated: 2026-04-07
version: v1.0
scope: algorithm-analysis
based_on:
	- learn/hindsight/hindsight-docs/docs/developer/retain.md
	- learn/hindsight/hindsight-docs/docs/developer/retrieval.md
	- learn/hindsight/hindsight-docs/docs/developer/observations.mdx
	- learn/hindsight/hindsight-docs/docs/developer/rag-vs-hindsight.md
	- learn/hindsight/hindsight-docs/docs/developer/configuration.md
	- learn/hindsight/hindsight-docs/docs/developer/monitoring.md
	- learn/hindsight/hindsight-docs/docs/developer/storage.md
	- docs/specs/omega-context-management.md
	- docs/specs/omega-tui-document-memory-supervision.md
	- docs/specs/omega-command-system.md
	- crates/omega-document/src/lib.rs
	- crates/omega-context/src/lib.rs
	- crates/omega-memory/src/lib.rs
---

# Hindsight Algorithms Omega Should Learn For Knowledge Organization And Retrieval

## Overview

这份文档只讨论一件事：

> Hindsight 里哪些“成熟且准确”的知识整理与检索算法思想，值得 Omega 吸收，用来提升文档管理和知识检索能力？

实现说明（2026-04-07）：这份文档保留为算法调研材料。面向仓库实施的修正版顺序见 `docs/specs/omega-knowledge-evolution.md`：先补 retention quality 与 memory query，再引入 observation layer，最后才做 unified recall planner；relation-aware retrieval、temporal retrieval 与 strategy-specific chunking 已后移到更晚阶段。

这里的“算法”不是要求逐行复刻 Hindsight 的内部实现。公开文档并没有暴露所有权重、表结构和调参细节，因此本文只提炼可以可靠确认的算法原则、关键处理流程和对 Omega 的迁移方式。

重点关注三类能力：

1. **知识整理**：怎样把原始信息变成长期稳定、可更新的知识。
2. **知识召回**：怎样在不同查询类型下更准确地找回真正相关的信息。
3. **知识运维**：怎样监控、更新和修正知识，而不是把索引当静态黑盒。

## Executive Summary

Hindsight 真正值得学习的，不是“它用了 PostgreSQL”或者“它有 reflect agent”，而是下面六个算法级原则：

1. **分层知识表示**：raw facts 与 consolidated knowledge 必须分层存放。
2. **mission-driven extraction**：整理知识前先定义保留目标，不能无差别入库。
3. **多策略并行检索**：semantic 不是唯一检索手段，keyword、relation、temporal 必须并行参与。
4. **融合后再精排**：先用 RRF 之类的弱排序融合多路候选，再对小候选集精排。
5. **知识具备 freshness 和 contradiction 语义**：知识不是写入即永恒，需要能被强化、修正、失效。
6. **token-budget-aware recall**：返回多少知识，不该只按 top-k，而应按给 agent 的上下文预算来裁剪。

对 Omega 来说，最值得优先落地的是其中四项：

1. mission-driven extraction
2. consolidated observation layer
3. hybrid + relation + temporal aware recall planner
4. freshness-aware project knowledge lifecycle

## 1. Layered Knowledge Representation

## Hindsight 的做法

Hindsight 不是把所有输入都当成同一种向量文本。它至少分成三层：

1. **raw facts / experiences**
2. **observations**
3. **mental models**

这三层的角色不同：

- raw facts 是证据层
- observations 是模式层
- mental models 是人工或高层策展层

这是一种很关键的知识整理算法思想：

> 不要让“原始证据”和“综合认知”混在同一个索引层。

## Omega 当前状态

Omega 现在已经有一定层次，但还不完整：

- `FileRecord` / `Chunk` / `TodoSnapshot` / turn summaries 属于原始或半原始层
- `DocumentHealthReport` / `DocumentStoreVersion` / supervision snapshot 属于运维投影层
- 缺少介于两者之间的“项目知识综合层”

## 应吸收的点

Omega 应增加一层轻量的 **project observations**，位于原始文件/摘要之上、但低于人工撰写 spec：

- 输入：turn summaries、document hits、governance events、store version changes、operator activity
- 输出：稳定项目认知卡片
- 要求：必须保留 evidence refs

## 为什么这会提升文档管理和检索

文档管理不只是“能搜到文件”，还包括：

- 当前有哪些长期有效约定
- 哪些规则已经被新证据修正
- 某项架构结论是否已失效

没有 observation 层，这些高价值结论只能散落在文档和历史对话里。

## 2. Mission-Driven Extraction

## Hindsight 的做法

Hindsight 在 retain 阶段不是无脑抽取，而是用 `retain_mission`、`observations_mission`、retain strategy 去控制：

- 什么要提取
- 什么降权
- 什么直接忽略
- 同一个 bank 面对不同内容时用什么抽取策略

这本质上是一个 **知识整理前的选择函数**。

## 算法价值

这件事非常关键，因为错误往往不是“搜不到”，而是“库里混了太多不该记的东西”。

mission-driven extraction 的核心收益是：

- 降低噪音
- 提高长期知识密度
- 防止短期过程日志污染长期记忆

## Omega 当前状态

Omega 现在对 document 的 ingest 已有结构化扫描和 `storeignore`，但对 memory 侧还缺明确的“长期保留策略”。

尤其在 coding-agent 场景下，下面这些内容很容易污染长期记忆：

- 一次性调试输出
- 中间试错日志
- 已被放弃的局部方案
- 临时 shell 命令噪音

## 应吸收的点

Omega 应新增 **memory retention profiles**，至少区分：

1. `project_facts`
2. `developer_preferences`
3. `open_threads`
4. `ephemeral_debug`

并且在进入长期层前做判定：

- 是否涉及稳定架构事实
- 是否被重复提及
- 是否进入 command/governance/todo 正式结果
- 是否只是 transient debugging residue

## 3. Multi-Strategy Retrieval Instead Of Single-Mode Search

## Hindsight 的做法

Hindsight recall 的关键不是“有向量检索”，而是：

- semantic
- keyword
- graph
- temporal

四路并行，再统一融合。

这解决了一个成熟系统必须面对的问题：

> 不同查询其实需要不同检索信号，单一 semantic similarity 无法稳定覆盖。

## 算法价值

不同信号解决的问题不同：

- **semantic**：概念近义、自然语言提问
- **keyword**：专有名词、crate 名、路径名、命令名、配置项
- **relation / graph**：跨文件、跨模块、跨实体的间接关联
- **temporal**：最近一次、某个阶段、重构前后、某月某轮变化

## Omega 当前状态

Omega 已有：

- keyword
- semantic
- hybrid fallback
- path/language/doc_type/status filters

这已经比普通 RAG 强，但还缺两类信号：

1. **relation-aware recall**
2. **temporal-aware recall**

## 应吸收的点

对 Omega，不需要直接上通用 knowledge graph。更适合的是 **graph-lite / relation-aware retrieval**：

- document crossrefs
- archive replacement links
- crate/module ownership links
- workflow step dependency links
- command/operator activity links
- file-to-file dependency or mention graph

temporal 检索也不需要做通用日历系统，可以先做面向工程场景的时间维度：

- last_used_at
- modified_at
- built_at / promoted_at
- before_refactor / after_refactor
- recent task window

## 4. Result Fusion Before Reranking

## Hindsight 的做法

Hindsight 不是把四种检索结果硬拼接，而是：

1. 多路候选并行召回
2. 用 **RRF** 融合排序
3. 再对候选集做 cross-encoder rerank

这是一条成熟检索系统常见但非常实用的路径。

## 算法价值

这比“单一分数直接排序”更稳，因为：

- 不同检索器分数不可直接比较
- 多路都命中的候选通常更可信
- 精排成本高，只应作用于较小候选集

## Omega 当前状态

Omega 代码里已经有 `HYBRID_RRF_K`，说明 hybrid fusion 的方向是对的，但当前主要还是 document search 层面的 keyword/vector 组合。

## 应吸收的点

Omega 应把融合策略提升到更高层的 recall planner，而不只停留在 document search：

- document keyword candidates
- document semantic candidates
- future observation candidates
- recent operator/history candidates
- selected summary candidates

先统一用 rank-based fusion，再对有限候选做更细的二次排序。

如果暂时不接 cross-encoder，也至少先把“融合与精排分层”建立起来。

## 5. Temporal Modeling Must Be Explicit

## Hindsight 的做法

Hindsight 明确区分：

1. **事情发生在什么时候**
2. **系统是什么时候知道这件事的**

这是很重要的知识整理算法，因为很多错误都来自把 `observed_at` 和 `effective_at` 混成一个时间戳。

## Omega 当前状态

Omega 已经有一些时间维度：

- file `modified_at`
- store `built_at` / `promoted_at`
- health check time
- operator `last_used_at`

但长期知识层还没有统一的时间语义。

## 应吸收的点

未来如果增加 project observations，建议至少保留两种时间：

- `source_observed_at`: 系统在哪个 turn / scan / command 中捕获到证据
- `effective_at`: 这条知识对应的真实工程状态从何时起生效

### Example

- “默认启用 document-backend” 这条知识
  - `source_observed_at`: 今天 scan 或文档读取时
  - `effective_at`: 某次实现变更进入主线时

这会让“最近学到的旧事实”和“最近发生的新事实”不再混淆。

## 6. Consolidation Must Handle Contradictions And Freshness

## Hindsight 的做法

Hindsight 的 observation consolidation 不只是“生成摘要”，还强调：

- 新证据会强化旧观察
- 矛盾证据会修正旧观察
- stale observations 会在 reflect 前重新验证

## 算法价值

成熟知识系统不能只会新增，还必须会：

- 更新
- 失效
- 重建

## Omega 当前状态

Omega 已经在 store/version/health 层有相似意识：

- active / pending version
- promotion failure
- archived version history
- health status lifecycle

但项目认知本身还没有这套 lifecycle。

## 应吸收的点

project observations 应具备：

- `fresh`
- `stale`
- `superseded`
- `corrected`

并且每次新证据进入时，不是直接覆盖，而是：

1. 检查是否强化当前观察
2. 检查是否矛盾
3. 记录修正原因
4. 必要时重建 observation

## 7. Retrieval Should Be Budget-Aware, Not Only Top-K

## Hindsight 的做法

Hindsight 明确把 recall 的输出控制在 token budget 内，而不是简单返回 top-k。

## 算法价值

对 agent 来说，最稀缺资源不是“返回了多少条结果”，而是：

- 最终 prompt 里还能装多少信息
- 哪些结果最值得占用有限 token

## Omega 当前状态

Omega 的 context assembly 本来就有 token budget 和 summary selection，这其实是非常好的基础。

## 应吸收的点

把这种 budget-aware 思路推广到统一 recall planner：

- 不只是 summary selection 讲预算
- document hits / observations / recent activity 也讲预算
- 最终返回给 step 的是 `best_evidence_within_budget`，不是 `top_10`

这会显著提高检索结果对 workflow 的实际可用性。

## 8. Graph Retrieval In Omega Should Be Lightweight And Domain-Specific

## 不建议照搬的部分

Hindsight 的 graph retrieval 很强，但 Omega 不适合一上来建通用实体图谱，因为代价太高，而且 coding-agent 的实体空间与通用对话不同。

## 建议的替代方案

做 **domain-specific relation graph**，优先纳入：

1. 文件引用关系
2. crate / module ownership
3. 文档 crossref / replaced_by / archived_by
4. command -> operator -> affected path
5. turn summary -> changed path -> document section

这足以覆盖大多数 coding-agent 的多跳场景，又不会引入过重抽象。

## 9. Knowledge Management Needs Strategy-Specific Chunking

## Hindsight 的做法

Hindsight 在 retain 侧有 chunk size、extraction mode、strategy selection，不是所有内容都按同一种方式切块。

## Omega 当前状态

Omega 的 document chunking 目前有固定字符目标，这在工程上足够简单，但对不同类型文档并不总是最优。

## 应吸收的点

Omega 应逐步从固定 chunk size 走向 **strategy-specific chunking**：

- code：按 AST / symbol / impl block
- spec / guide：按 heading / section
- ADR / PRD：按 decision block / rationale block
- TODO / logs：按 item / event block

这样会同时提升：

- keyword 命中质量
- semantic embedding 纯度
- future observation synthesis 的输入质量

## 10. Monitoring Must Be Retrieval-Stage Aware

## Hindsight 的做法

Hindsight 的 monitoring 文档把 recall、rerank、consolidation 都拆成独立观测 scope，这是成熟系统的标志。

## Omega 当前状态

Omega 的 supervision 已经不错，但更多聚焦结果态，而不是“检索和知识整理过程”本身的阶段细节。

## 应吸收的点

如果 Omega 引入 observation/consolidation/retrieval planner，需要新增阶段化指标：

- extraction candidates accepted / dropped
- relation expansion hits
- temporal filter hits
- fusion candidate count
- rerank candidate count
- stale observation revalidation count
- corrected observation count
- final recall token share by source

这会让调参从“感觉不准”变成“知道是哪一阶段不准”。

## Prioritized Algorithm Adoption For Omega

## Phase 1: Immediate And Safe

1. mission-driven extraction profiles
2. budget-aware unified recall planner
3. strategy-specific chunking for docs and code

这三项能直接提升文档管理和知识检索质量，而且不会破坏现有 store truth model。

## Phase 2: High Leverage

4. project observation layer with evidence refs
5. contradiction + freshness lifecycle
6. relation-aware lightweight graph expansion

这三项会把 Omega 从“会搜”提升到“会整理和持续更新项目知识”。

## Phase 3: Later, If Needed

7. stronger temporal query planner
8. candidate reranker beyond current hybrid fusion
9. curated project models refreshed from observations

这部分收益更高，但应建立在前两阶段稳定后再做。

## Final Distillation

如果把 Hindsight 的成熟经验压缩成一句话，那就是：

> 知识系统的准确性，不只来自更好的向量模型，而来自“分层表示 + 多路召回 + 融合精排 + 可更新知识生命周期”。

对 Omega 而言，这句话可以进一步翻译成更具体的工程原则：

1. **先决定什么该长期保留，再决定怎么索引。**
2. **先把原始证据和综合认知分层，再谈智能整理。**
3. **先统一 recall contract，再逐步增强 relation 和 temporal。**
4. **所有派生知识都必须带来源、带 freshness、可被修正。**

这几条比“上一个更强的 embedding 模型”更重要，也更符合 Omega 当前 document + memory 系统的演进方向。