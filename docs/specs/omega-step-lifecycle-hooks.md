---
status: draft
owner: omega-team
created: 2026-03-25
updated: 2026-03-25
version: 0.1
supersedes: []
related_prds: []
---

# Omega Step Lifecycle Hooks

## Overview

当前 Omega 已经具备 `step prompt + tool_request + input/output contract + max_iterations` 的基础工作流模型，也已经为 `feature` / `research` 补上了最小可用的 todo-driven execute repeat。但这条主路径仍然把“什么时候允许离开当前 step”写死在 runtime 里，导致不同工作场景一旦出现不同的执行判据，就只能继续往 `omega-session` 里叠加条件分支。

本规格定义下一阶段的收敛方向：不引入独立的 `runtime_policy` 对象，而是把 **step 生命周期** 和 **能否进入下一步的判定** 收敛为显式的 Rust hook 机制。workflow 继续声明 step 的静态结构，runtime 继续拥有最终编排权；用户可以在 `.omega/hooks/` 下编写 Rust hook，通过统一的单方法接口参与 step 生命周期，并在 `BeforeAdvance` 阶段决定是否允许进入下一步。

## Goals

- 为 step 建立统一的生命周期事件模型，而不是散落的 before/after 特例。
- 允许 workflow step 通过配置引用 `.omega/hooks/` 下的 Rust hook。
- hook 只需要实现一个方法，即可处理 before-step、after-step、tool 周期、advance gate 等生命周期事件。
- hook 可以读取 step 可见工具、session context、todo 状态，并拥有当前 step 生命周期内可持久的运行时存储。
- “是否允许进入下一步”由 runtime 在 hook 参与下判定；若不允许，则保持当前 step 重复执行直到达到 repeat 上限。
- 为相关逻辑提供 deterministic mock LLM harness，使 workflow 行为可以在不依赖真实模型稳定性的情况下验证。

## Non-Goals

- 不把 step 编排权下放给 prompt 或 tool 自行决定。
- 不让 tool 直接控制 workflow advance；tool 只产生事实，advance 由 runtime 判定。
- 不在 v1 支持任意全局 hook 事件总线或跨 turn 持久插件状态。
- 不在 v1 允许 hook 绕过 step 的可见工具集或直接突破 workspace 安全边界。
- 不在 v1 自动为 `.omega/hooks/` 做隐式编译；runtime 只负责发现并加载已构建好的 hook artifact。

## Design Principles

### No Separate Runtime Policy Layer

workflow step 只需要额外声明两类信息：

- 当前 step 绑定了哪些 hook
- 当前 step 最多允许因为 advance 被拒而重复多少次

是否进入下一步由 hook 和现有 output/input contract 共同决定；不再引入独立的 `runtime_policy` 配置对象。

### Lifecycle Is Explicit

`before_step`、`after_step`、`advance gate` 都属于同一个 step 生命周期，不应继续以若干散落布尔分支存在。runtime 必须把这些点收敛为统一的事件模型。

### Hooks Are Runtime Helpers, Not Hidden Orchestrators

hook 可以影响 step 是否继续停留在当前阶段，但不能绕过 session 的最终编排边界：

- required output contract 仍然优先于 hook
- session 仍然拥有 repeat budget 和最终 error/advance 决策
- hook 只能通过显式 API 读写上下文、todo、storage 和工具

## Architecture

### Components

- `omega-workflow`: 解析 step 级 `hooks` 与 `max_step_repeats` 配置。
- `omega-session`: 在 step 生命周期边界分发 hook 事件，收集 advance 决策，并维持 step-scoped runtime storage。
- `omega-hooks`（planned）: 负责 hook manifest 解析、ABI-safe 动态加载、hook dispatch 和 host-side adapters。
- `.omega/hooks/`: 用户编写的 Rust hook 源码与构建产物所在目录。
- deterministic workflow test harness（planned）: 为 hook/gate/repeat/repair 路径提供可预测的脚本化验证环境。

### Lifecycle Events

单个 step 的最小生命周期事件定义为：

1. `BeforeStep`
2. `BeforeModelTurn`
3. `AfterModelTurn`
4. `BeforeToolCall`
5. `AfterToolCall`
6. `BeforeAdvance`
7. `AfterStep`
8. `StepFailed`

v1 重点落地 `BeforeStep`、`AfterToolCall`、`BeforeAdvance` 与 `AfterStep`；其余事件保持接口预留，避免未来再次断裂式改动。

## Hook Programming Model

### Single-Method Contract

用户 hook 只需要实现一个入口方法：

```rust
pub trait StepHook {
    fn handle(&mut self, event: StepHookEvent, ctx: &mut StepHookContext) -> HookResult;
}
```

其中：

- `event` 描述当前处于 step 生命周期的哪个阶段
- `ctx` 提供受控的 runtime 视图与可变能力
- `HookResult` 用于报告 diagnostics、记录 storage 变化、以及在 `BeforeAdvance` 阶段表达 allow/deny

用户不需要分别实现 `before_step()`、`after_step()`、`should_advance()` 三个方法；只需在单方法里按 `event` 分支处理。

### Recommended Export Shape

为避免插件样板代码膨胀，host SDK 应提供导出宏，例如：

```rust
use omega_hook_sdk::{export_step_hook, HookResult, StepHookContext, StepHookEvent};

struct TodoManagedExecuteHook;

impl TodoManagedExecuteHook {
    fn handle(&mut self, event: StepHookEvent, ctx: &mut StepHookContext) -> HookResult {
        match event {
            StepHookEvent::BeforeStep => HookResult::continue_run(),
            StepHookEvent::BeforeAdvance => {
                if ctx.todo().has_open_items() {
                    ctx.deny_advance("todo items remain open");
                }
                HookResult::continue_run()
            }
            _ => HookResult::continue_run(),
        }
    }
}

export_step_hook!(TodoManagedExecuteHook);
```

## Hook Context Surface

`StepHookContext` 必须显式暴露以下能力：

- `workflow()` / `step()` / `iteration()`：当前执行位置与重复次数
- `session_context()`：只读的 `SessionContext` 快照
- `structured_input()` / `structured_output()`：当前 step 已知的结构化输入输出
- `todo()`：读取与更新当前共享 todo 状态的受控接口
- `tools()`：查看当前 step 可见工具，并通过 host 提供的方法调用这些工具
- `storage()`：当前 step 生命周期内的可变运行时存储
- `diagnostics()`：向 runtime/TUI 追加 hook 诊断信息
- `deny_advance(reason)`：仅在 `BeforeAdvance` 阶段生效，要求 runtime 保持当前 step

### Tool Visibility Rule

hook 只能调用当前 step 已经可见的工具。也就是说：

- `research.execute` hook 只能看到 research execute 允许的只读工具
- `feature.execute` hook 可以看到写工具，因为该 step 本身具备这些能力
- root routing hook 仍然受 root tool block 约束

hook 不应成为突破 tool policy 的后门。

### Runtime Storage Rule

v1 只提供 **step-scoped** runtime storage：

- 生命周期从 `BeforeStep` 开始，到 `AfterStep` / `StepFailed` 结束
- 会跨当前 step 的 repeated execute 保持
- 默认按 hook id namespaced，避免多个 hook 互相踩写
- 数据模型使用 JSON-like value map，避免 host/plugin 边界上的任意 Rust 类型耦合

这足以支撑：

- todo execute 中的 no-progress 计数
- 证据累积类 research hook
- gating 前的中间状态缓存

workflow-scoped 或 turn-scoped hook storage 留作后续扩展，而不是 v1 必选项。

## Advance Gate Model

### Base Rule

step 结束后，session 按以下顺序决定是否允许进入下一步：

1. 先检查 `output_contract`
2. 若 required structured output 尚未满足，则当前 step 不允许 advance
3. 若 output contract 已满足，则触发 `BeforeAdvance` hook 事件
4. 若任一 hook 拒绝 advance，则保持当前 step，进入下一轮 repeat
5. 若没有 hook 拒绝 advance，则允许进入下一 step

因此，advance gate 不是新的 policy 对象，而是当前 step 生命周期中的一个标准事件点。

### Repeat Budget

workflow step 新增：

- `hooks = ["..."]`
- `max_step_repeats = N`

当 `BeforeAdvance` 被拒绝时：

- 若 repeat 次数尚未达到上限，则重复当前 step
- 若已达到上限，则 session 以 step failure 结束，而不是静默跳到下一步

这让“保持当前步骤迭代直到满足条件，或者直到达到步级上限”成为统一机制。

### Why This Is Better Than Tool-Owned Flow Control

tool 可以更新 todo、写入上下文、产生验证结果，但不能直接决定 workflow advance。原因是：

- tool output 是事实，不是 orchestration 决策
- advance 判定需要聚合 output contract、todo 状态、storage、hook 诊断
- 把流程控制塞进 tool 会形成隐式 contract，难以测试和调试

## Hook Loading Model

### Workspace Layout

建议目录约定：

```text
.omega/
  hooks/
    todo_managed_execute/
      Cargo.toml
      src/lib.rs
      Hook.toml
      target/.../libtodo_managed_execute.so
```

`Hook.toml` 至少描述：

- hook id
- crate/package name
- compiled artifact path
- supported host API version

### ABI Strategy

运行时加载用户 Rust hook 不能依赖原生 Rust trait object ABI。v1 推荐使用 `abi_stable` 一类稳定 ABI 边界，而不是直接通过 `libloading + Box<dyn Trait>` 交换对象。

### Build Strategy

v1 只要求 runtime **加载** 已构建好的 hook artifact，不要求 host 在运行中自动调用 Cargo 编译 `.omega/hooks/`。后续如需补 build helper，可作为独立体验增强，而不是核心 contract 的一部分。

## Workflow Configuration Surface

step 配置未来可收敛为：

```toml
[[steps]]
id = "execute"
label = "Execute"
prompt = ".omega/prompt/step/execute.md"
loop_mode = "agent_loop"
max_iterations = 200
max_step_repeats = 8
hooks = ["todo_managed_execute"]
tool_request = { mode = "inherit" }
input_contract = { mode = "required", sources = ["plan"] }
output_contract = { mode = "optional", format = "json", schema_path = ".omega/schema/step/execute.json" }
```

这里没有额外的 `runtime_policy` 字段；step 的运行时完成语义来自：

- output/input contract
- hook 列表
- repeat 上限

## Deterministic Testing Strategy

LLM 不可控性不能成为 workflow runtime 行为不可验证的理由。为支撑 hook/gate/repeat 的迭代，测试层必须补一个稳定的 scripted mock harness。

### Target Capabilities

测试基建至少要能脚本化模拟：

- 正常 response / streaming response
- structured output valid / invalid / regenerate
- tool_use 序列
- execute no-progress / partial-progress / full-progress
- hook deny/allow advance
- repeat exhaustion
- scene-specific execute 差异（feature vs research）

### Recommended Shape

- 把当前 `omega-session` 内部 ad-hoc 的 `SequencedClient` / `IdleClient` 抽成稳定的测试支持模块或独立 crate
- 提供 `ScriptedLlmClient` builder，而不是继续在每个测试里手写 response 向量
- 为 hook host 提供两类测试：
  - in-process fake hook，用于快测 lifecycle dispatch
  - real compiled fixture hook，用于验证 loader/ABI/manifest

### Why This Matters

未来一旦引入 step hooks、advance gates、step-scoped storage，就不能只靠 prompt 文案和人工试跑验证 workflow 逻辑。deterministic mock harness 是保证 Task 10、Task 11 和未来 scene-specific execute 迭代稳定性的前置条件。

## Rollout Plan

1. 先完成大文件拆分（特别是 `omega-session` / `omega-workflow` / 相关 specs）
2. 抽离 deterministic mock LLM workflow harness
3. 在 `omega-workflow` 中引入 `hooks[]` 与 `max_step_repeats`
4. 实现 hook host、storage 与 lifecycle dispatch
5. 用 `BeforeAdvance` 替换当前零散的 execute repeat 条件
6. 为 `feature.execute` / `research.execute` 落地首个 `todo_managed_execute` hook

## Open Questions

- v1 是否需要 workflow-scoped hook storage，还是 step-scoped 已足够
- hook diagnostics 是否应进入 `RuntimeUiEffect::UpsertStepDiagnostics`，还是先走 Activity/Log
- build helper 是放进 `omega-tools-builtin`、单独 crate，还是晚些再做

---

### Change Log
- 2026-03-25: 初版规格，提出用单方法 Rust hook + lifecycle events + `BeforeAdvance` gate 统一 step 生命周期，并明确 deterministic mock LLM harness 是该方向的前置测试能力。