---
status: active
last_verified_commit: N/A
owner: omega-team
created: 2026-04-01
updated: 2026-04-01
version: v1.0
scope: roadmap
based_on:
	- learn/claude-code-source-code-vs-omega-analysis.md
---

# Omega 向 Claude Code 学习的改进路线图

## Overview

本文把前一份对比分析收束成 Omega 的可执行改进路线图。

其中针对工具子系统的正式实现规格已单独收敛到 `docs/specs/omega-tool-prompt-optimization.md`；本文保留更高层的阶段路线图，不重复展开工具能力面的详细 contract。

目标不是“把 Omega 做成 Claude Code”，而是有选择地吸收 Claude Code 在产品化、工具说明、项目记忆、权限链路上的优势，同时保留 Omega 现有的核心长板：

- step-based workflow orchestration
- structured output validation + repair/regenerate
- structured tool result contract
- runtime diagnostics / envelope architecture

这是一份架构与产品双重路线图，因此每个任务都同时考虑：

- 对现有 crate 边界的影响
- 对运行时稳定性的风险
- 与现有 workflow/step contract 是否冲突
- 是否能独立交付并验证

## Goals

- 为 Omega 增加更强的项目级长期上下文入口。
- 为工具系统补齐更成熟的使用说明和发现体验。
- 把现有 memory/context 工程能力产品化，而不是仅停留在内部机制。
- 在静态 workflow policy 之上增加细粒度权限层。
- 让 prompt 资产和高价值命令工作流更易演进、可配置、可缓存。
- 保持现有 step contract、diagnostics 和 deterministic execution 优势不被削弱。

## Non-Goals

- 不把 Omega 退化成 prompt-first 的通用 agent shell。
- 不弱化现有 step output contract 来换取“更灵活”的自由回答。
- 不直接照搬 Claude Code 的 classifier 审批系统。
- 不在第一阶段引入复杂远程协作协议或完整多用户 RBAC。
- 不把 context / memory 变成黑箱产品，牺牲现有 diagnostics 面。

## Guiding Principles

1. Policy first, permissions second
	 现有 workflow/step policy 继续做硬边界；新的权限层只能在硬边界内细化，不能取代硬边界。

2. Contract first, prompt second
	 prompt 可以增强，但不能代替 step input/output contract。

3. Productize existing strengths
	 优先把已有能力包装成更可用的产品面，而不是先新增一批全新底层。

4. Small vertical slices
	 每一阶段都必须能独立交付、测试、文档化，而不是等大重构完成。

## Roadmap Summary

| Phase | Priority | Theme | Main Outcome |
| --- | --- | --- | --- |
| Phase 0 | P0 | Protect current strengths | 明确哪些能力不能被新设计削弱 |
| Phase 1 | P0 | Project memory + tool guidance | 补齐项目记忆入口与工具说明层 |
| Phase 2 | P1 | Memory productization | 将 context/memory 变成用户可感知能力 |
| Phase 3 | P1 | Layered permissions | 在静态 policy 上叠加细粒度审批 |
| Phase 4 | P1 | Prompt asset externalization | prompt sections 与高价值命令工作流外部化 |
| Phase 5 | P2 | Session persistence + replay | 跨 session continuity 与回放能力 |
| Phase 6 | P2 | Remote coordination follow-up | 为将来 subagent/team/remote 打基础 |

## Phase 0: Protect What Omega Already Does Better

### Goal

在吸收 Claude Code 优点之前，先把 Omega 现有不可退化的架构边界写清楚。

### Task 0.1: Freeze Core Non-Regressions

- **Type**: Design
- **Complexity**: S
- **Dependencies**: None
- **Affected areas**: `docs/specs/`, `omega-session`, `omega-core`, `omega-tools`
- **Description**: 明确四条不可退化边界：step orchestration、structured output validation、structured tool result、runtime diagnostics。
- **Deliverable**: 一份补充规范，说明后续改造不得破坏的 contract。
- **Acceptance criteria**:
	- 明确列出不可退化能力。
	- 标注对应 crate 和主要接口。
	- 后续 roadmap 中每个任务都能引用该规范进行自检。

### Task 0.2: Add Comparison-Derived Decision Log

- **Type**: Documentation
- **Complexity**: S
- **Dependencies**: Task 0.1
- **Affected areas**: `docs/decisions/` or `learn/`
- **Description**: 记录“学什么”和“不学什么”的决策，防止后续讨论回到模糊状态。
- **Deliverable**: ADR 或短决策文档。
- **Acceptance criteria**:
	- 明确记录不照搬 classifier、不断言 prompt 替代 contract。
	- 记录保留 step model 的理由。

## Phase 1: Project Memory Entry And Tool Guidance

### Goal

优先落两个收益最高、风险最低的能力：项目长期记忆入口，以及更强的工具说明层。

### Task 1.1: Add Project Memory Entry File

- **Type**: Design + Implementation
- **Complexity**: M
- **Dependencies**: Task 0.1
- **Affected areas**: `omega-context`, `omega-app`, `omega-session`, docs
- **Description**: 新增项目级长期上下文入口，定位类似 Claude Code 的 `CLAUDE.md`，但与 `AGENTS.md` 分工清晰。
- **Proposed shape**:
	- `AGENTS.md`: 工程规则、贡献流程、仓库约束
	- `OMEGA.md` or `.omega/context.md`: 面向运行时 agent 的项目长期上下文
- **Deliverable**:
	- 文件约定与加载规则
	- context injection 逻辑
	- precedence 规则（repo prompt vs workflow prompt vs skill prompt）
- **Acceptance criteria**:
	- 新文件可被 `omega-context` 自动读取并进入 system blocks。
	- 与 `AGENTS.md` 的职责边界在文档中明确。
	- 有测试覆盖文件存在/不存在/超长截断场景。

### Task 1.2: Add Tool Guidance Field To Tool Catalog

- **Type**: Design + Implementation
- **Complexity**: M
- **Dependencies**: Task 0.1
- **Affected areas**: `omega-tools`, `omega-session`, `omega-context`, builtin tools
- **Description**: 为每个工具增加“何时用、何时不要用、典型输入模式”的 guidance 字段。
- **Deliverable**:
	- tool definition 扩展字段
	- context/tool registry 中的 prompt 展示格式
	- builtin tools guidance 初版
- **Acceptance criteria**:
	- model 可见的 tool definitions 中包含 guidance。
	- 至少核心 inspection/editing/todo/bash 工具完成 guidance 补齐。
	- 不影响 `ToolResult` / `ToolHandler` 兼容面。

### Task 1.3: Add Tool Selection Regression Tests

- **Type**: Testing
- **Complexity**: S
- **Dependencies**: Task 1.2
- **Affected areas**: `omega-session`, `omega-core`, docs/specs
- **Description**: 为工具说明层新增回归测试，证明 guidance 不改变 contract，但改善错误选择概率的场景覆盖。
- **Deliverable**: deterministic scenario tests
- **Acceptance criteria**:
	- 至少覆盖 read-only inspection、write path、todo management 三类典型误用场景。
	- 测试不依赖模糊文本匹配，仍基于结构化 output/visible tools。

## Phase 2: Productize Memory Without Losing Diagnostics

### Goal

把当前 `omega-memory` / `omega-context` 的工程能力提升为用户可理解、可操作的产品面。

### Task 2.1: Define Memory Tiers As First-Class Product Concepts

- **Type**: Design
- **Complexity**: M
- **Dependencies**: Task 1.1
- **Affected areas**: `omega-context`, `omega-memory`, docs/specs
- **Description**: 把长期项目记忆、会话记忆、执行态摘要、文档知识区分成清晰层级。
- **Deliverable**:
	- Memory model spec
	- ownership / retention / visibility rules
- **Acceptance criteria**:
	- 每层 memory 有明确来源、消费方、生命周期。
	- 明确哪些进 prompt，哪些只进 diagnostics/UI。

### Task 2.2: Add User-Facing Memory Management Surface

- **Type**: Implementation
- **Complexity**: L
- **Dependencies**: Task 2.1
- **Affected areas**: `omega-tools-builtin`, `omega-tui`, `omega-session`
- **Description**: 提供统一 memory 管理接口，使用户能查看、写入、删除项目/会话级记忆，而不是只靠内部 compaction。
- **Deliverable**:
	- memory tool or command surface
	- TUI visibility / diagnostics entry
	- persistence rules
- **Acceptance criteria**:
	- 用户可区分 session/project memory。
	- memory 变更在 diagnostics 中可见。
	- 不破坏现有 `ContextDiagnostics` 聚合模型。

### Task 2.3: Add Memory Conflict And Staleness Diagnostics

- **Type**: Implementation + Testing
- **Complexity**: M
- **Dependencies**: Task 2.1
- **Affected areas**: `omega-context`, `omega-session`, `omega-tui`
- **Description**: 增加 memory 冲突、过期、截断、优先级覆盖等诊断，而不是仅默默 compaction。
- **Deliverable**: diagnostics fields + UI rendering
- **Acceptance criteria**:
	- memory truncation/staleness/conflict 至少有结构化事件或字段可见。
	- 对已有 step diagnostics 无破坏。

## Phase 3: Layered Permissions On Top Of Static Policy

### Goal

保留 workflow policy 的硬边界，在边界内引入更细粒度的用户审批机制。

### Task 3.1: Define Permission Layer Model

- **Type**: Design
- **Complexity**: M
- **Dependencies**: Task 0.1
- **Affected areas**: `omega-workflow`, `omega-session`, `omega-core`, docs/specs
- **Description**: 在现有 tool visibility policy 之上定义第二层 permission model。
- **Proposed modes**:
	- `deny`
	- `ask-once`
	- `allow-for-turn`
	- `allow-for-session`
	- `always-allow` (repo-local config only)
- **Acceptance criteria**:
	- 明确 policy 和 permission 的 precedence。
	- 明确哪些工具永远不能被审批层放开。

### Task 3.2: Add Interactive Permission Prompts For Sensitive Tools

- **Type**: Implementation
- **Complexity**: L
- **Dependencies**: Task 3.1
- **Affected areas**: `omega-tui`, `omega-session`, `omega-tools-builtin`, `omega-core`
- **Description**: 对敏感工具增加交互式审批，优先覆盖 `bash` 与写路径工具。
- **Deliverable**:
	- permission state model
	- TUI prompt / notice
	- execution continuation rules
- **Acceptance criteria**:
	- policy blocked 工具仍直接拒绝。
	- permission-gated 工具可弹出 ask 流程。
	- denial / approval 均进入 runtime diagnostics。

### Task 3.3: Add Permission Audit Trail

- **Type**: Implementation + Testing
- **Complexity**: M
- **Dependencies**: Task 3.2
- **Affected areas**: `omega-session`, `omega-tui`, diagnostics
- **Description**: 记录工具尝试、批准、拒绝、范围（turn/session）等审计信息。
- **Acceptance criteria**:
	- 每次审批有结构化记录。
	- 可在 TUI 或日志中追溯工具为何被允许或拒绝。

## Phase 4: Externalize Prompt Assets And Add Command Workflows

### Goal

让 prompt 更易演进，并把高价值常见操作收敛成命令级工作流。

### Task 4.1: Externalize Prompt Sections

- **Type**: Design + Implementation
- **Complexity**: L
- **Dependencies**: Task 1.1
- **Affected areas**: `omega-app`, `omega-skills`, `omega-workflow`, config/docs
- **Description**: 把部分 base/workflow prompt sections 从 Rust 默认值外移到配置或文档资产。
- **Deliverable**:
	- prompt asset loading model
	- cacheable vs dynamic sections split
	- migration plan for existing defaults
- **Acceptance criteria**:
	- 至少 base prompt 与一个 workflow prompt 支持外部化加载。
	- 不影响现有 skill 注入和 routing。

### Task 4.2: Implement A High-Value `/init`-Style Workflow

- **Type**: Design + Implementation
- **Complexity**: L
- **Dependencies**: Task 1.1, Task 1.2
- **Affected areas**: `omega-workflow`, `omega-session`, `omega-tools-builtin`, docs
- **Description**: 为 Omega 增加类似 `/init` 的高价值 onboarding workflow，帮助新仓库快速建立项目上下文、工具策略、记忆入口。
- **Deliverable**:
	- dedicated workflow
	- deterministic output contract
	- docs for generated artifacts
- **Acceptance criteria**:
	- workflow 能读取关键仓库文件并产出项目上下文资产建议。
	- 输出仍遵守 Omega 的 step contract，不退化成自由回答。

## Phase 5: Session Persistence, Replay, And Recovery

### Goal

把现有 runtime envelope 和 deterministic seam 进一步做成真正可恢复、可回放的 session persistence。

### Task 5.1: Persist Turn And Runtime Envelope Journal

- **Type**: Design + Implementation
- **Complexity**: L
- **Dependencies**: Task 0.1
- **Affected areas**: `omega-session`, `omega-app`, `omega-tui`, docs/specs
- **Description**: 将 turn 生命周期和关键 runtime envelopes 持久化，形成最小可恢复日志。
- **Acceptance criteria**:
	- 可重放 turn timeline。
	- journal 保持 deterministic，不写入易漂移字段。

### Task 5.2: Add Resume And Replay UX

- **Type**: Implementation
- **Complexity**: M
- **Dependencies**: Task 5.1
- **Affected areas**: `omega-tui`, `omega-app`
- **Description**: 增加恢复最近 session、查看历史 turn、回放关键步骤的用户面。
- **Acceptance criteria**:
	- 至少支持最近 session resume。
	- replay 不影响现有 live runtime pipeline。

## Phase 6: Remote And Team Coordination Follow-Up

### Goal

为未来 subagent/team/remote 做准备，但避免过早引入复杂 classifier 或完整 RBAC。

### Task 6.1: Define Remote Permission Bridge Contract

- **Type**: Design
- **Complexity**: M
- **Dependencies**: Task 3.3
- **Affected areas**: `omega-subagent`, `omega-team`, `omega-message`, docs/specs
- **Description**: 设计远端代理请求本地审批的消息契约。
- **Acceptance criteria**:
	- 明确消息类型、审批状态、超时策略。
	- 与现有 runtime message boundary 不冲突。

### Task 6.2: Pilot Subagent Permission Handoff

- **Type**: Implementation
- **Complexity**: L
- **Dependencies**: Task 6.1
- **Affected areas**: `omega-subagent`, `omega-session`, `omega-team`
- **Description**: 在单一 subagent 路径上验证远端工具请求审批的最小闭环。
- **Acceptance criteria**:
	- 子代理不能绕过主 session policy。
	- 主 session 能看到并审批子代理的敏感工具请求。

## Recommended Execution Order

### Immediate

1. Task 0.1
2. Task 1.1
3. Task 1.2
4. Task 1.3

### Next

1. Task 2.1
2. Task 3.1
3. Task 4.1

### After Foundation Stabilizes

1. Task 2.2
2. Task 2.3
3. Task 3.2
4. Task 3.3
5. Task 4.2

### Later Expansion

1. Task 5.1
2. Task 5.2
3. Task 6.1
4. Task 6.2

## Suggested First Milestone

推荐第一里程碑只做三件事：

- 项目长期记忆入口
- tool guidance 字段
- 一份明确的非退化规范

这是因为这三项：

- 实现成本低于权限系统和持久会话
- 能显著改善产品体验
- 不会破坏现有 step/runtime contract
- 能为后续权限和 memory 产品化打基础

## Risks

- **Prompt creep**: 如果项目记忆入口、tool guidance、外部 prompt sections 缺少边界，prompt 复杂度会快速膨胀。
- **Policy erosion**: 如果权限层优先级设计不清，可能削弱现有 step-scoped hard boundary。
- **Memory black box**: 如果过度产品化 memory 而不暴露 diagnostics，会丢掉 Omega 当前的重要工程优势。
- **State sprawl**: session persistence 若直接落地过多运行时细节，容易让 replay/journal 变脆。

## Success Criteria

这个路线图成功的标志不是“Omega 更像 Claude Code”，而是：

1. Omega 的项目上下文入口更清晰，用户更容易上手。
2. 模型更容易正确选择工具，而不是依赖 prompt 运气。
3. memory/context 更可见、更可管理，但 diagnostics 没退化。
4. 敏感工具在 workflow hard boundary 之内获得细粒度审批能力。
5. step contract、tool result contract、runtime diagnostics 仍然保持现有强度。

---

### Change Log
- 2026-04-01: 明确工具子系统的正式实现规格已收敛到 `docs/specs/omega-tool-prompt-optimization.md`，路线图保留阶段级规划，不重复展开工具 contract 细节。
- 2026-04-01: 基于 `learn/claude-code-source-code-vs-omega-analysis.md` 产出 Omega 改进路线图，拆分为阶段、任务、依赖、交付物与验收标准。
