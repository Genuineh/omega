---
content_revision: 120
created: 2026-04-14
generation_id: gen_000046_r000120
last_verified_commit: N/A
owner: omega-team
projection_version: 46
related_prds:
  - docs/specs/omega-structured-document-system.md
  - docs/specs/omega-command-system.md
  - docs/specs/omega-project-plan-system.md
source_doc_id: "spec:docs-specs-omega-document-cli"
status: draft
supersedes: []
updated: 2026-04-15
---

# Omega Document CLI Specification

## Overview

当前 structured docs v2 已经具备两条可用路径：

- session 内的 `/document render|validate|extract`
- 仓库外的临时 `structured_docs_cutover` example

它们已经证明 `omega-document` 的 backend contract 可复用，但还不足以成为稳定的 repository-external automation surface。当前缺口是：用户在普通 shell、脚本或 CI 中，仍然没有一个正式的 binary 可以对任意 `--root` 执行 structured docs 工作流，也没有稳定的 machine-readable output、root validation、exit code contract 和 doctor surface。

本规格规划新增一个专门的 repo-external CLI，暂定名 `omega-doc-cli`。它的职责不是重写文档系统，而是把现有 `omega-document::OmegaDocument::manage_document(DocumentOp)` 收口成稳定的外部入口，让 structured docs 的 canonical workflow 能在 Omega session runtime 之外独立执行。

## Implementation Status

截至 `2026-04-15`，`Task 19G ~ 19K` 已完成并成为当前基线：

- `crates/omega-doc-cli` 已落地为独立 workspace crate，并提供 `omega-doc` binary。
- foundation commands 已可用：`doctor`、`render`、`validate`、`extract`、`cutover`。
- query/mutation surface 已可用：`get`、`list`、`record upsert`、`task upsert`、`relation upsert`、`archive`、`remove`。
- `structured_docs_cutover` example 已切到共享 backend helper，不再和 CLI 各维护一份 cutover flow。
- `AGENTS.md` 与 `docs-cli-workflow` skill 已切到 CLI-first docs mutation guidance。
- docs/docs-data version contract 已落地到 manifest、render state、generated docs metadata，以及 CLI `doctor` / `get` / `validate` 输出。
- 最终 docs workflow cutover 已完成：正常文档 mutation 默认只允许通过 `omega-doc` 进行，直接 markdown/docs-data edits 仅保留给 emergency projection repair。

## Goals

- 提供一个 repository-external CLI，支持针对任意 `--root` 执行 structured docs 主要工作流。
- 把 `render`、`validate`、`extract`、`cutover` 与 `doctor` 收口到稳定 subcommand，而不是继续依赖 example 或 session 内命令。
- 为文档 add/update/archive/remove 与 doc task / relation mutation 提供统一 CLI write surface。
- 为 doc/task lookup 提供 id-based query surface，使查询既可直接读取文件，也可通过 stable id 获取精确信息。
- 复用 `omega-document` backend 与 `DocumentOp` contract，不再创建第二套文档引擎。
- 为 shell automation 和 CI 提供稳定的 human-readable output、JSON output 与 exit code 语义。
- 明确 repo root、`docs-data/` layout、source path 与 validation failure 的诊断面。

## Non-Goals

- 不替代 session 内 `/document ...` 命令；后者仍服务于会话内交互式使用。
- 不把 `omega-doc-cli` 做成新的文档业务层；record/task/relation/render/validate 逻辑继续归 `omega-document` 拥有。
- 不在 Phase 1 把旧的 document indexing、query 或 health workflows 一并迁入该 CLI。
- 不再为正常文档工作流保留 bootstrap 式手工 mutation 路径；直接 markdown/docs-data 修补只保留给 emergency projection repair。

## Architecture

### Ownership

| Layer | Responsibility |
|------|----------------|
| `omega-doc-cli` | 参数解析、root 解析、subcommand 编排、输出格式化、exit code 映射 |
| `omega-document` | `DocumentOp`、structured docs schema、render、validate、extract、cutover helper |
| `omega-session` | 继续持有 session 内 `/document` 命令，不作为外部 CLI 依赖 |

### Dependency Direction

```text
omega-doc-cli
  -> omega-document

omega-doc-cli
  -X-> omega-session
  -X-> omega-app
  -X-> omega-tui
```

约束：

- CLI 不依赖 session runtime、TUI state 或 `/document` parse surface。
- CLI 不直接读写 `docs-data/*.jsonl` 细节，只通过 `OmegaDocument` backend 操作。
- `cutover` 的顺序编排应下沉到 `omega-document` helper 或共享 runner，避免 CLI/example 各维护一份流程。

### Crate Shape

建议新增 workspace crate：`crates/omega-doc-cli`

建议最小结构：

```text
crates/omega-doc-cli/
  Cargo.toml
  src/
    main.rs
    cli.rs
    output.rs
    exit_codes.rs
```

## Command Surface

### Phase 1 Commands

```text
omega-doc render [DOC_ID...] --root <path> [--check|--plan|--apply] [--json]
omega-doc validate [DOC_ID...] --root <path> [--json] [--strict]
omega-doc extract <SOURCE...> --root <path> [--doc-type <type>] [--check|--plan|--apply] [--json]
omega-doc cutover <SOURCE...> --root <path> [--doc-type <type>] [--json]
omega-doc doctor --root <path> [--json]
```

规则：

- `render` 默认 `--apply`，语义与 session 内 `/document render` 对齐。
- `extract` 支持 `--doc-type <type>`，并保持 `check|plan|apply` 三态。
- `validate` 只读，不接受 mutation mode。
- `cutover` 固定执行 `extract -> render -> validate`，任一步失败即终止。
- `doctor` 只负责环境与 layout 检查，不做 mutation。

### Implemented Query And Mutation Commands

```text
omega-doc get <doc-id|task-id> --root <path> [--json]
omega-doc list <docs|tasks|relations> --root <path> [--type <...>] [--json]
omega-doc record upsert --root <path> --input <json> [--check|--plan|--apply]
omega-doc task upsert --root <path> --input <json> [--check|--plan|--apply]
omega-doc relation upsert --root <path> --input <json> [--check|--plan|--apply]
omega-doc archive <doc-id> --root <path> [--reason <...>] [--check|--plan|--apply]
omega-doc remove <doc-id|task-id|relation-id> --root <path> [--check|--plan|--apply]
```

规则：

- `get` 提供按 stable id 精确读取，作为 direct file read 的 authoritative 补充。
- `list` 只负责枚举和过滤，不承担 mutation。
- `record/task/relation upsert` 成为 docs add/update 的标准 write path。
- `archive/remove` 成为 docs delete 类操作的唯一受控入口，不再允许无记录地直接删 markdown/JSONL。

## Read And Mutation Policy

### Query Rules

- 查询和阅读允许直接读取 generated docs、`docs-data/*.jsonl` 或相关代码文件。
- 当需要精确定位 canonical object 时，优先支持 `omega-doc get <id>`。
- “读”不要求一律走 CLI；CLI 的价值在于 id-based lookup 和 stable machine output，而不是阻止正常文件阅读。

### Mutation Rules

默认工作流是：

```text
inspect
	-> omega-doc record/task/relation/archive/remove ...
	-> omega-doc render
	-> omega-doc validate
```

规则：

- 文档 add/update/archive/remove、doc task 维护和 relation 维护应通过 `omega-doc` 完成。
- generated `docs/` 不应再作为默认 mutation surface。
- 直接编辑 markdown 或直接编辑 `docs-data/*.jsonl` 只保留给 emergency projection repair，并且同一次变更里必须 rerender、validate、doctor。
- 当需要确认版本对应关系或仓库 readiness 时，应继续运行 `omega-doc doctor` 或 version-aware `omega-doc validate`，而不是只凭时间戳或 git diff 推断。

## Skills And Agent Guidance

CLI-first workflow 需要同步给 agent guidance：

- `AGENTS.md` 应明确文档 mutation 走 `omega-document-cli`。
- 文档相关 skills 应引导“query 可直接读，mutation 必须走 CLI，再 render/validate”。
- 参考 skill 应提供常见 flow：按 id 获取、修改 record/task、重新生成、验证版本和 projection。

## Versioning Integration

`omega-document-cli` 当前已消费 docs/docs-data version contract：

- mutation 会推进 docs-data revision。
- render 会写入 projection revision / generation id。
- validate 会区分 generated docs 的 content drift 与 version/generation drift。
- get 与 doctor 会暴露 `content_revision`、`projection_version` 与 `generation_id`。

版本合同本身单独定义在 `docs/specs/omega-document-projection-versioning.md`。

### Command Mapping

| CLI command | Backend operation |
|------------|-------------------|
| `render` | `DocumentOp::RenderProjection` |
| `validate` | `DocumentOp::ValidateProjection` |
| `extract` | `DocumentOp::ExtractSource` |
| `cutover` | shared cutover runner calling `extract -> render -> validate` |
| `doctor` | root/layout validation + manifest/docs-data presence checks |

## Output Contract

### Human Output

默认输出优先服务于人类终端使用：

- 当前 step 名称
- backend 返回的 message
- warnings 数量与关键摘要
- validation failure 时的 missing / mismatch / broken relation 摘要

### JSON Output

`--json` 必须提供稳定、脚本可消费的结果。

建议 shape：

```json
{
  "command": "cutover",
  "root": "/repo",
  "ok": true,
  "steps": [
    { "name": "extract", "ok": true },
    { "name": "render", "ok": true },
    { "name": "validate", "ok": true }
  ],
  "warnings": [],
  "validation": null
}
```

规则：

- JSON output 必须保留 backend message，而不是只输出布尔值。
- validation 失败时必须保留 missing file、mismatch 和 broken relation 细节。
- JSON shape 应避免直接暴露 TUI/session 概念。

## Exit Codes

建议首轮采用稳定小集合：

| Code | Meaning |
|------|---------|
| `0` | 成功 |
| `1` | validation failed 或命令输入非法 |
| `2` | root/layout 不合法，无法把目标识别为 structured docs repo |
| `3` | apply 阶段写入失败 |
| `4` | 仅在 `--strict` 下，warnings 被提升为失败 |

## Technical Decisions

| Decision | Choice | Rationale |
|---------|--------|-----------|
| backend owner | reuse `omega-document` directly | 避免外部 CLI 与 session runtime 各维护一套文档逻辑 |
| runtime dependency | no `omega-session` dependency | 外部自动化不应绑定 TUI/session 生命周期 |
| cutover implementation | shared runner, not duplicated example logic | 避免 CLI/example 流程漂移 |
| root model | explicit `--root <path>` | 允许对任意 repo 执行，而不是只依赖当前 cwd |
| output mode | human default + `--json` | 同时满足交互式 shell 和 CI 脚本 |
| shipped scope | `doctor/render/validate/extract/cutover/get/list/upsert/archive/remove` | `Task 19G ~ 19K` 已把 foundation、query/mutation surface、guidance enforcement、version contract 和最终 cutover 全部落到实际 CLI 基线 |
| read policy | direct read allowed, CLI id lookup preferred for precision | 阅读不应被 CLI 人为阻塞，但 canonical object lookup 应有 stable id surface |
| mutation policy | CLI-only by default, emergency-only manual exception | 正常文档 mutation 统一走受控 contract，只保留 projection repair 例外面 |

## Testing Strategy

- parser tests: subcommand、`--root`、mutation mode、`--doc-type` 与 invalid args。
- integration tests: 基于 temp repo 执行 `extract -> render -> validate` 全链路。
- doctor tests: 缺失 `docs/`、缺失 `docs-data/`、坏 root、manifest 缺失时给出可信错误。
- JSON output tests: 字段稳定、validation detail 完整、warning 可消费。
- failure-path tests: broken relation、missing generated file、write failure 的 exit code 正确。

## Rollout Plan

### Task 19G: Completed Foundation

- 新增 `omega-doc-cli` crate。
- 提供 subcommand parser、`--root` 解析和 `doctor`。
- `render`、`validate`、`extract`、`cutover` 已接入 shared backend helper。

### Task 19H: Completed Query And Mutation Surface

- 增加 `get/list` 与 `record/task/relation` mutation commands。
- 增加 `archive/remove` 命令，让 docs add/update/archive/remove 都有受控 CLI contract。
- 补齐 CLI 集成测试，并验证真实 repo 上的 `doctor/list/get/archive --check` smoke path。

### Task 19I: Completed CLI-First Guidance And Enforcement

- `AGENTS.md` 与 `docs-cli-workflow` skill 已明确“query 可直接读，mutation 走 CLI，再 render/validate”的默认工作流。
- 手工 markdown/docs-data 修改已降级为 emergency projection repair 例外。

### Task 19J: Completed Docs/Data Version Contract

- manifest 已记录 `content_revision`、`projection_version` 与 `last_generation_id`。
- render state 和 generated docs metadata 已写入 revision/generation linkage。
- `render`、`validate`、`get`、`doctor` 已消费并暴露这套 version contract。

### Task 19K: Completed CLI-Only Workflow Cutover

- 文档 mutation 的最终例外面已收紧到 emergency-only projection repair。
- 正常文档工作现在默认通过 `omega-doc` 完成，并在同一变更中 rerender 和 validate。

## Related Specs

- `docs/specs/omega-structured-document-system.md`
- `docs/specs/omega-document-projection-versioning.md`
- `docs/specs/omega-command-system.md`
- `docs/specs/omega-project-plan-system.md`

---

### Change Log

- 2026-04-15: v0.4 — `Task 19I ~ 19J` completed: CLI-first docs guidance is now the default repo policy, and `omega-doc` now surfaces docs/docs-data revision correspondence through render-state metadata plus `doctor` / `get` / `validate`.
- 2026-04-15: v0.3 — `Task 19G ~ 19H` completed: the `omega-doc-cli` crate now ships the foundation commands plus id-based query and mutation commands, and the cutover example reuses the shared backend helper.
- 2026-04-14: v0.2 — extended the CLI plan to cover id-based query, mutation surface, CLI-first skills/AGENTS guidance, and the docs/docs-data version contract required for a full workflow cutover.
- 2026-04-14: v0.1 — 初版规格，规划一个 repository-external structured docs CLI，复用 `omega-document` backend 为 shell/CI 提供 `render` / `validate` / `extract` / `cutover` / `doctor`。
