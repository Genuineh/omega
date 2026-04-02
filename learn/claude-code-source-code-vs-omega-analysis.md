---
status: active
last_verified_commit: N/A
owner: omega-team
created: 2026-04-01
updated: 2026-04-01
version: v1.0
scope: source-analysis
---

# Claude Code Source Code vs Omega 架构对比分析

## 概述

本文基于源码静态分析，对 `learn/claude-code-source-code` 与当前 Omega 系统做六个维度的对比：

1. 提示词系统与 tool prompt
2. tool 设计与使用
3. LLM 输出稳定解析
4. 核心执行流程
5. memory 与项目存储
6. 状态与权限管理

本文不包含运行时 benchmark，也不做主观“整体谁更强”的简单结论。更准确的结论是：两套系统的设计目标不同。

- Claude Code 更像“通用交互式 coding agent 平台”，优势在 prompt 体系成熟、权限链路完整、工具生态丰富。
- Omega 更像“工作流驱动的可验证执行系统”，优势在 step contract、结构化输出验证、运行态诊断和可测试性。

后续执行计划见：`learn/omega-learning-roadmap-from-claude-code.md`。
其中与 tool / prompt 子系统直接对应的正式实现规格见：`docs/specs/omega-tool-prompt-optimization.md`。

## 一页结论

| 维度 | 更强一方 | 原因 |
| --- | --- | --- |
| 提示词系统成熟度 | Claude Code | 分层 prompt、更成熟的 command prompt / tool prompt / CLAUDE.md / hook 体系 |
| tool 结果契约与运行态集成 | Omega | `ToolResult` / `ToolErrorKind` / `ToolRun` 更结构化，UI 与 runtime 边界更清晰 |
| 输出稳定解析 | Omega | 有显式 schema 验证、repair / regenerate 策略、step output diagnostics |
| 通用交互执行流 | Claude Code | 更像完整 agent shell，支持权限、MCP、remote、tasks、CLI/TUI 一体化 |
| 工作流执行纪律 | Omega | step-based orchestration 明确，scene/workflow/step/tool visibility 边界更稳 |
| memory / project context 产品化 | Claude Code | `CLAUDE.md`、memory files、session history、resume 更完整 |
| 可观察性与诊断 | Omega | `ContextDiagnostics`、`StepDiagnostics`、runtime envelope、tool lifecycle 更强 |
| 权限与审批 | Claude Code | 有专门 permission context、deny rules、classifier、prompt fallback |

## 1. 提示词系统与每个 tool 的 prompt 是做什么的？

### Claude Code 的做法

Claude Code 的 prompt 系统是分层装配的，不是单一大 system prompt。

核心证据：
- `learn/claude-code-source-code/src/constants/prompts.ts`
- `learn/claude-code-source-code/src/utils/systemPrompt.ts`
- `learn/claude-code-source-code/src/utils/queryContext.ts`
- `learn/claude-code-source-code/src/utils/messages/systemInit.ts`
- `learn/claude-code-source-code/src/constants/systemPromptSections.ts`
- `learn/claude-code-source-code/src/commands/init.ts`

它大致分为几层：

1. 基础 system prompt
   作用是定义 agent 的基本身份、全局原则、默认交互风格。

2. system prompt sections
   作用是把不同来源的指令拆成 section，例如基础规则、工具说明、语言偏好、动态上下文。这样便于缓存和拼装。

3. command prompt
   例如 `/init` 这类命令会有专门 prompt，指导 agent 按特定流程完成任务，而不是仅靠用户一句自然语言。

4. tool-specific prompt
   每个工具会暴露自己的使用说明，告诉模型这个工具何时使用、如何传参、约束是什么。
   这部分本质上是“tool affordance 文档”，帮助模型学会正确调用工具。

5. 项目 prompt / memory prompt
   `CLAUDE.md`、memory files、session history 被注入 prompt，作为项目长期约束和上下文。

这些 prompt 的主要作用不是“让模型更聪明”，而是降低模型在工具选择、上下文理解、项目约束遵守上的误差。

### Omega 的做法

Omega 的提示词系统是“基础 prompt + workflow prompt + skill prompt + step prompt”。

核心证据：
- `AGENTS.md`
- `crates/omega-app/src/lib.rs`
- `crates/omega-skills/src/lib.rs`
- `crates/omega-workflow/src/defaults.rs`
- `crates/omega-session/src/runner.rs`
- `docs/specs/omega-agent-spec.md`
- `docs/specs/omega-scene-routing.md`

Omega 的特点：

1. 基础 prompt 很薄
   更像“系统底座”，而不是一套很重的完整产品 prompt。

2. workflow/scene 驱动 prompt 选择
   不是所有请求都走同一提示模板，而是先 routing，再决定进入哪个 workflow，再落到哪个 step。

3. skill 注入是重要层
   skill 是可组合的 instruction bundle，比较像 Claude Code 中的 section + repo instruction 的混合体。

4. step prompt 的约束更强
   某个 step 要求什么输入、输出什么格式、能见哪些工具，约束更显式。

### 对比与优劣

Claude Code 更强的地方：
- prompt 分层更成熟，工具 prompt 体系更完整。
- `CLAUDE.md` / hook / skill / command 这些产品面已经形成闭环。
- 对“通用 coding agent”场景的覆盖更广。

Omega 更强的地方：
- prompt 不是唯一约束来源，workflow 和 output contract 会在运行态兜底。
- step 粒度更清晰，更适合需要稳定执行和强约束的流程。

结论：
- 如果目标是“通用、灵活、覆盖面广”的 agent 体验，Claude Code 的 prompt 体系更好。
- 如果目标是“可验证、可控、可诊断”的 workflow agent，Omega 的 prompt 体系虽然没那么产品化，但更稳。

## 2. Claude Code 中 tool 的设计和使用，对比我们有什么优劣？

### Claude Code 的做法

核心证据：
- `learn/claude-code-source-code/src/Tool.ts`
- `learn/claude-code-source-code/src/tools.ts`
- `learn/claude-code-source-code/src/services/tools/toolOrchestration.ts`
- `learn/claude-code-source-code/src/services/tools/toolExecution.ts`
- `learn/claude-code-source-code/src/services/tools/StreamingToolExecutor.ts`
- `learn/claude-code-source-code/src/tools/BashTool/`

Claude Code 的 tool 设计特点：

1. registry-based
   工具集中注册，builtin tools 很多，MCP tools 也能并入统一调用面。

2. tool prompt 非常重要
   模型不只是“知道有个 tool”，而是拿到完整的 tool usage guidance。

3. execution orchestration 比较成熟
   区分可并发和独占工具，有专门的 streaming executor 和 orchestration 层。

4. permission-aware
   工具可见性和实际执行是两层控制，不只是“注册了就能跑”。

它的缺点也很明显：
- tool result contract 没有 Omega 这么“显式 typed + runtime-visible”。
- 很多能力建立在产品逻辑和 prompt discipline 上，而不是统一结构化 runtime contract 上。

### Omega 的做法

核心证据：
- `crates/omega-tools/src/lib.rs`
- `crates/omega-tools-builtin/src/lib.rs`
- `crates/omega-core/src/agent.rs`
- `crates/omega-core/src/tool_factory.rs`
- `crates/omega-session/src/tool_catalog.rs`
- `crates/omega-workflow/src/policy.rs`
- `docs/specs/omega-tool-system-upgrade.md`

Omega 的 tool 设计特点：

1. `ToolHandler` trait + `ToolResult`
   这让工具结果不是随意字符串，而是明确有：
   - `output`
   - `preview`
   - `metadata`
   - `truncated`
   - `error_kind`

2. `ToolErrorKind` 很关键
   直接把错误分成 validation / policy / execution / timeout，这对后续 repair、UI 展示、日志诊断都非常有价值。

3. step-scoped visibility
   工具能否被看到、在哪个 step 可见，是 workflow policy 的一部分。

4. runtime UI 是工具一等公民
   `ToolRun` 生命周期会进入 runtime message pipeline 和 TUI，而不是附属日志。

Omega 的短板：
- 工具生态还不如 Claude Code 丰富。
- 工具 prompt/usage guidance 体系还比较薄。
- 权限与政策更多是静态 workflow gating，不如 Claude Code 那种交互式权限体系灵活。

### 对比与优劣

Claude Code 更强：
- 工具体系更像成熟产品，发现、说明、权限、并发执行做得更完整。
- MCP 与 builtin tool 混编能力更成熟。

Omega 更强：
- 工具结果契约更干净、更工程化。
- 工具执行和 UI/diagnostics 的边界更清晰。
- 更适合把 tool 作为 workflow runtime 的一部分，而不是聊天中的临时能力。

结论：
- Claude Code 赢在“工具平台化”。
- Omega 赢在“工具契约化”。

## 3. Claude Code 对 LLM 回答如何保证稳定解析？对比我们有什么优劣？

### Claude Code 的做法

核心证据：
- `learn/claude-code-source-code/src/tools/SyntheticOutputTool/SyntheticOutputTool.ts`
- `learn/claude-code-source-code/src/utils/permissions/yoloClassifier.ts`
- `learn/claude-code-source-code/src/constants/xml.ts`
- `learn/claude-code-source-code/src/utils/xml.ts`
- `learn/claude-code-source-code/src/skills/bundled/updateConfig.ts`

Claude Code 主要用了三种办法：

1. 工具输入 schema 校验
   对 tool input 会做 JSON schema / AJV 校验。

2. XML tag 包裹特殊输出
   对 classifier、terminal output、task notification 等使用 XML-like tag，降低语义歧义。

3. 对部分场景做结构化抽取
   比如 `<reason>`、`<thinking>`、`<block>` 这类标签解析。

但它的核心弱点是：
- 它并没有像 Omega 一样，在“主工作流输出”层面构建统一的 schema validate → repair → regenerate 管道。
- 如果输出不合法，更多是报错、包装或回退，而不是强力修复。

### Omega 的做法

核心证据：
- `crates/omega-session/src/output.rs`
- `crates/omega-session/src/runner.rs`
- `crates/omega-workflow/src/model.rs`
- `crates/omega-session/src/runtime_ui.rs`
- `crates/omega-context/src/lib.rs`
- `docs/specs/omega-step-lifecycle-hooks.md`
- `docs/specs/omega-runtime-ui-message-contract.md`

Omega 在这块明显更强，核心机制是：

1. JSON candidate extraction
   模型输出里即使包了一层 prose，也会尝试提取 JSON 候选。

2. schema validation
   验证是否满足 step contract。

3. recovery policy
   支持 `RegenerateOnly` 和 `RepairThenRegenerate`。

4. diagnostics 全程记录
   每次失败、repair、recovery decision 都有结构化 diagnostics。

5. step-specific output model
   `plan`、`execute`、`report` 等步骤都有明确输出结构，而不是统一让模型自由发挥。

### 对比与优劣

Claude Code 的优点：
- 对工具输入和特殊控制面已经有结构化约束。
- XML 标签对 prompt / parser 边界很有帮助。

Claude Code 的缺点：
- 主回答层面缺乏统一的稳定解析与修复闭环。
- 更依赖 prompt discipline，而不是 runtime contract。

Omega 的优点：
- 对“模型输出是否可用”这件事处理得更系统化。
- 能把格式错误从“产品问题”降级为“可诊断、可恢复的执行问题”。

Omega 的缺点：
- 目前 repair pass 次数保守，灵活性有限。
- 语义级验证仍然偏少，主要还是 schema 级别。

结论：
在“稳定解析”这一维，Omega 明显更好。这是 Omega 当前最值得保留并继续强化的核心优势之一。

## 4. Claude Code 的核心执行流程，对比我们的 step 核心执行流程有什么优劣？

### Claude Code 的做法

核心证据：
- `learn/claude-code-source-code/src/QueryEngine.ts`
- `learn/claude-code-source-code/src/query.ts`
- `learn/claude-code-source-code/src/query/config.ts`
- `learn/claude-code-source-code/src/query/tokenBudget.ts`
- `learn/claude-code-source-code/src/query/stopHooks.ts`
- `learn/claude-code-source-code/src/Task.ts`
- `learn/claude-code-source-code/src/tasks/`

Claude Code 的核心执行流更像通用 agent loop：

1. 组装 system prompt + messages
2. 调 API
3. 如果返回 tool_use，则执行工具
4. 把 tool result 回注模型
5. 循环直到 stop reason 结束

它的优点：
- 通用性高。
- 适合开放式交互。
- task、remote agent、tool loop 能自然融合。

它的缺点：
- 主流程是 agent loop first，不是 workflow first。
- 对复杂业务流程，控制力和可解释性不如 step model。
- 结构化结果约束不够强时，容易变成“靠 prompt 驱动的聪明循环”。

### Omega 的做法

核心证据：
- `crates/omega-session/src/lib.rs`
- `crates/omega-session/src/runner.rs`
- `crates/omega-session/src/routing.rs`
- `crates/omega-core/src/agent.rs`
- `crates/omega-session/src/ui_emit.rs`
- `crates/omega-workflow/src/lib.rs`
- `docs/specs/omega-runtime-message-pipeline.md`
- `docs/specs/omega-scene-routing.md`

Omega 的执行流是：

1. turn 进入 session
2. scene / workflow routing
3. 进入 step runner
4. step 配置 context / skills / tools / contracts
5. step 内部调用 agent loop
6. tool lifecycle 和 response sections 发到 runtime pipeline
7. validate output
8. 决定下一 step / retry / finish

它的优点：
- 更清晰的 orchestration boundary。
- 每个 step 都可以单独验证、诊断、限制工具。
- 更适合复杂工作流和稳定交付。

它的缺点：
- 灵活性不如 Claude Code。
- 一个 step 太重时，会显得原子粒度过粗。
- 交互体验不像 Claude Code 那样天然是“连续 agent shell”。

### 对比与优劣

Claude Code 更强：
- 通用 agent loop 更成熟，适合开放式探索与复杂实时交互。
- 任务系统、remote、CLI/TUI 产品感更强。

Omega 更强：
- step workflow 的边界清晰、诊断能力强、可测试性好。
- 执行结果更容易追责和复盘。

结论：
- 如果要做开放式 coding copilot，Claude Code 这一套更顺手。
- 如果要做“有明确阶段、有结构化交付、有稳定执行”的系统，Omega 这一套更优。

## 5. Claude Code 的 memory 和项目存储系统，对比我们有什么优劣？

### Claude Code 的做法

核心证据：
- `learn/claude-code-source-code/src/memdir/memdir.ts`
- `learn/claude-code-source-code/src/memdir/paths.ts`
- `learn/claude-code-source-code/src/memdir/findRelevantMemories.ts`
- `learn/claude-code-source-code/src/services/SessionMemory/sessionMemory.ts`
- `learn/claude-code-source-code/src/utils/sessionStorage.ts`
- `learn/claude-code-source-code/src/context.ts`
- `learn/claude-code-source-code/src/commands/init.ts`

Claude Code 的 memory 体系更接近产品化：

1. `CLAUDE.md`
   项目长期指令入口。

2. memory files
   可以自动或手工维护，按相关性筛选进 prompt。

3. session storage / transcript
   有跨 session 恢复和历史持久化能力。

4. `/init`
   会主动帮项目建立 `CLAUDE.md`、skills、hooks。

它的优点：
- 项目 onboarding 非常强。
- 用户容易理解和使用。
- 跨 session continuity 更完整。

它的缺点：
- memory 更偏 prompt product，而不是 execution system 的一等数据结构。
- 冲突、治理、结构化诊断能力有限。

### Omega 的做法

核心证据：
- `crates/omega-memory/src/lib.rs`
- `crates/omega-context/src/lib.rs`
- `crates/omega-context/src/document_model.rs`
- `crates/omega-session/src/session_state.rs`
- `crates/omega-session/src/runtime_ui.rs`
- `crates/omega-todo/src/lib.rs`
- `docs/specs/omega-context-management.md`

Omega 更偏“上下文工程系统”：

1. summary compaction
   有明确排序和压缩策略。

2. context budget / cache diagnostics
   可以观察上下文效率，而不只是把记忆塞进 prompt。

3. document abstraction
   长期目标是把文档、memory、search 统一到 context system。

4. todo / step summaries / routing state
   更强调执行态上下文，而不是项目说明文档本身。

Omega 的缺点：
- 缺少一个像 `CLAUDE.md` 这么自然、用户友好的项目入口。
- TODO 和长期 memory 的持久化产品体验还不够强。
- 跨 session continuity 不如 Claude Code 完整。

### 对比与优劣

Claude Code 更强：
- 项目记忆产品化、可落地、易理解。
- session history / memory files / CLAUDE.md 组成了完整闭环。

Omega 更强：
- context budget、compaction、diagnostics 更工程化。
- 记忆不是黑箱，有明确的数据面和可观察性。

结论：
- Claude Code 在“记忆产品体验”上更好。
- Omega 在“上下文工程与诊断能力”上更好。

## 6. Claude Code 的状态和权限管理，对比我们是否有什么优劣？

### Claude Code 的做法

核心证据：
- `learn/claude-code-source-code/src/Tool.ts`
- `learn/claude-code-source-code/src/utils/permissions/permissions.ts`
- `learn/claude-code-source-code/src/utils/permissions/PermissionMode.ts`
- `learn/claude-code-source-code/src/utils/permissions/yoloClassifier.ts`
- `learn/claude-code-source-code/src/services/mcp/channelPermissions.ts`
- `learn/claude-code-source-code/src/types/textInputTypes.ts`

Claude Code 的权限系统比 Omega 更完整，特点是：

1. 有显式 permission context
2. 有 deny rules / allow rules
3. 有 interactive prompt fallback
4. 有 classifier 自动判断高风险工具调用
5. remote / MCP 侧也有 permission bridge

这是一套真正意义上的“权限与审批产品”。

缺点：
- 逻辑复杂。
- classifier 这类动态审批机制会带来额外不确定性和成本。
- 用户体验上会有更多审批摩擦。

### Omega 的做法

核心证据：
- `crates/omega-workflow/src/policy.rs`
- `crates/omega-workflow/src/model.rs`
- `crates/omega-session/src/tool_catalog.rs`
- `crates/omega-session/src/runner.rs`
- `crates/omega-core/src/agent.rs`
- `crates/omega-core/src/helpers.rs`

Omega 现在更接近“policy gating”，而不是完整权限系统：

1. 哪些工具在某个 workflow / step 可见，是静态声明的。
2. hidden tool 会被拒绝，并标成 `ToolErrorKind::Policy`。
3. 没有交互式权限审批，也没有真正的用户/角色维度权限模型。

优点：
- 简单、稳定、可预测。
- 对 workflow system 足够清晰。

缺点：
- 灵活性差。
- 缺少一次性授权、会话授权、按操作审批等机制。
- 不适合多用户、多角色、远程代理协作场景。

### 对比与优劣

Claude Code 更强：
- 是完整权限系统。
- 覆盖 deny/allow/ask/classifier/remote bridge。

Omega 更强：
- 策略更简单，失败类型更干净。
- 对单机会话式 workflow 更容易保持一致性。

结论：
Claude Code 在权限/审批产品设计上明显更强，Omega 当前更多是 workflow policy，不是完整 permission framework。

## 我们系统可以学习的地方

以下是对 Omega 最有价值的学习点，按优先级排序。

### P0. 增加项目级长期指令入口

建议：为 Omega 增加类似 `CLAUDE.md` 的项目级常驻上下文文件，并与现有 `AGENTS.md`、skills、workflow 配置做清晰分工。

为什么值得学：
- 当前 Omega 的 repo instructions 很强，但更偏“开发者约束”，不够像“运行时项目记忆入口”。
- 一个用户可直接维护的项目记忆入口，会显著改善 onboarding 和跨 session continuity。

建议边界：
- `AGENTS.md` 继续作为仓库级工程规范。
- 新增项目记忆文件作为 runtime context 的用户入口。
- 两者不要混用。

### P0. 强化工具使用说明层

建议：为每个 Omega tool 增加更明确的 usage guidance，而不是只给 schema 和名字。

为什么值得学：
- Claude Code 在这方面很成熟，模型更容易选对工具、减少误用。
- 这会直接改善 tool selection 质量，而不必修改执行内核。

建议实现：
- 在 `ContextToolRegistry` / tool catalog 输出中加入 tool guidance。
- 把“何时用、何时不要用、典型输入模式”作为一等字段。

### P0. 构建真正的项目 memory 产品面

建议：把当前 `omega-memory` / `omega-context` 的工程能力，包装成更明确的用户可见对象：项目记忆、会话记忆、团队记忆。

为什么值得学：
- Claude Code 把 memory 做成了用户能理解的产品概念。
- Omega 现在 memory 很强，但更多是系统内部能力，不够可直接操作。

### P1. 增加权限系统，而不只是 policy gating

建议：在现有 workflow policy 上，增加审批层，而不是替换 policy。

推荐方向：
- 静态 policy 继续保留，作为第一层硬边界。
- 在硬边界之内，再引入 ask / allow-once / allow-for-turn / deny 等细粒度权限。
- 将来再考虑 classifier 或 remote permission bridge。

为什么值得学：
- 这能让 Omega 从“单机可控 workflow agent”向“可协作 agent platform”演进。

### P1. 为 prompt 系统增加 cacheable section 与更清晰的外部化配置

建议：把部分 workflow/skill prompt 从 Rust 默认值中外移，并显式标注哪些 section 是稳定可缓存的。

为什么值得学：
- Claude Code 的 prompt 分层更利于演进和缓存。
- Omega 虽然 prompt layering 清晰，但 prompt 资产还不够产品化。

### P1. 引入更成熟的 command-level workflow

建议：在现有 workflow 之外，增加类似 `/init` 这类高价值 command prompt 工作流。

为什么值得学：
- Claude Code 的 `/init` 不是简单命令，而是一个成熟 onboarding pipeline。
- Omega 也适合把高价值常见任务收敛成专用 workflow，而不总是从自由问答起步。

### P2. 增加跨 session 恢复与持久 transcript 产品面

建议：将当前 runtime envelope / diagnostics / session state 扩展为真正可恢复、可检索、可回放的会话存储。

为什么值得学：
- Claude Code 的 session storage / transcript 对恢复很有帮助。
- Omega 已经有 turn envelope 和 deterministic test seam，天然适合做 replayable session store。

### P2. 借鉴 remote permission / remote task 设计，但不要照搬 classifier

建议：如果未来 Omega 要做 subagent/team/remote，更值得先学 Claude Code 的 remote permission bridge，而不是先学 classifier。

为什么：
- classifier 会引入额外不确定性。
- remote permission bridge 是更基础、更稳的能力。

## 不建议照搬的地方

### 1. 不要把 Omega 的 step contract 弱化成纯 agent loop discipline

原因：
- 这是 Omega 当前最核心的系统优势。
- 一旦退回到“主要靠 prompt 和 tool loop 保证输出正确”，系统会更灵活，但稳定性和可诊断性会下降。

### 2. 不要过早引入复杂 classifier 审批

原因：
- Claude Code 的 classifier 是产品规模下的权衡，不是所有系统都值得承担的复杂度。
- Omega 当前更需要的是清晰的静态 policy + 简单审批层，而不是再加一个高不确定性判断器。

### 3. 不要把 memory 全部产品化到牺牲 diagnostics

原因：
- Omega 的 context diagnostics 是难得的工程优势。
- 更好的方向是“把工程能力包装成产品面”，而不是为了看起来简单把系统变成黑箱。

## 最终判断

如果问题是“谁更好”，我的结论是按维度分：

- 在 prompt 产品化、工具平台化、权限系统、项目记忆入口上，Claude Code 更成熟。
- 在结构化输出稳定性、workflow 执行纪律、运行态诊断、tool result contract 上，Omega 更强。

如果问题是“Omega 应该学什么”，最值得学的不是 Claude Code 的全部，而是这四件事：

1. 项目级长期指令入口
2. 更成熟的 tool usage guidance
3. 用户可见的 memory 产品面
4. 分层权限系统

如果问题是“Omega 最不该丢掉什么”，答案也是四件事：

1. step-based workflow orchestration
2. schema validate + repair/regenerate output pipeline
3. structured tool result contract
4. runtime diagnostics / envelope architecture

---

### Change Log
- 2026-04-01: 增补到正式实现规格的入口，明确 tool / prompt 子系统的后续落地以 `docs/specs/omega-tool-prompt-optimization.md` 为准。
- 2026-04-01: 初版完成，基于 Claude Code 源码与 Omega 当前实现做六维对比，并提炼可学习项。
