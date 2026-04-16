---
content_revision: 101
created: 2026-04-14
generation_id: gen_000016_r000101
last_verified_commit: N/A
owner: omega-team
projection_version: 16
related_prds:
  - docs/specs/omega-context-management.md
  - docs/specs/omega-command-system.md
  - docs/specs/omega-project-plan-system.md
  - docs/specs/omega-project-system.md
source_doc_id: "spec:docs-specs-omega-structured-document-system"
status: active
supersedes: []
updated: 2026-04-15
---

# Omega Structured Document System Specification

## Overview

当前仓库的 `docs/` 仍以 markdown 文件直接作为 source of truth。这种方式在规模较小时足够直接，但已经开始暴露三个持续问题：

1. 文档格式只能靠约定和 health check 兜底，AI 没有一条“必须按结构写入”的强约束路径。
2. 当前展示层和数据层耦合，任何一次结构重排、索引重写或目录整理都需要直接编辑大量 markdown。
3. `.claude/skills` 中与文档相关的 skill 仍然围绕“直接编辑 markdown”展开，无法利用统一 schema、统一 task 结构和统一生成链。

本规格引入新的 **Structured Document System v2**：

- `docs-data/` 作为项目文档的 canonical data layer。
- `docs/` 退化为 generated presentation layer，可由工具随时重建。
- 文档治理不再只是“检查 markdown 是否像规范”，而是通过 `omega-plan` + docs tool surface 强制 AI 先写结构化记录，再生成 markdown。

该系统不是替代现有 `omega-document`/`omega-context`，而是在其上补一条更严格的文档管理主路径。首轮只规划并搭建新合同，不立即迁移现有 `docs/`；现有文档的 source extraction 和 parity migration 必须放在 rollout 最后。

## Implementation Status

截至 `2026-04-14`，`Task 19A ~ 19D` 已完成并成为现行基线：

- `omega-project-layout` 已定义 `docs-data/` canonical layout，包括 `manifest.json`、`records/*.jsonl`、`tasks/doc-tasks.jsonl`、`relations/links.jsonl` 与 `render/render-state.json`。
- `omega-document` 已实现 structured document record/task/relation schema、`docs-data` JSONL persistence、deterministic markdown renderer、projection validator 与 markdown source extractor。
- `omega-document` 的文档任务记录已对齐 `omega-plan` status/priority/kind，并在 apply 路径同步到 project plan store，形成文档任务的强 task contract。
- `omega-context` 的 `manage_document` tool 已扩展为强制结构化写入面：`upsert_record`、`upsert_task`、`upsert_relation`、`render_projection`、`validate_projection`、`extract_source`。
- `omega-session` 已补齐 `/document render`、`/document validate`、`/document extract` 命令面，供仓库内人工操作与 runtime-visible validation 使用。

截至 `2026-04-14`，`Task 19E ~ 19F` 也已完成：

- `.claude` 下与文档相关的 skills 已迁移到 structured docs v2 规则，默认要求先写 doc task / doc record，再 render 和 validate。
- 当前 `docs/` 目录已经通过 cutover runner 提取进 `docs-data/`，并由 renderer 重新生成展示层 markdown。
- `docs-data/tasks/doc-tasks.jsonl` 已成为 docs track 的 canonical task ledger，后续文档系统工作应继续在这一层推进。

截至 `2026-04-15`，`Task 19G ~ 19K` 也已完成：

- `omega-doc-cli` 已成为 repo-external docs workflow 的实际入口，当前已提供 foundation commands、id-based query 以及 record/task/relation/archive/remove mutation surface。
- `AGENTS.md` 与 `docs-cli-workflow` skill 已切到 CLI-first docs mutation guidance。
- docs/docs-data version contract 已写入 manifest、render state、generated docs metadata，以及 `omega-doc doctor|get|validate` output。
- 最终 CLI-only workflow cutover 已完成：正常文档 mutation 默认通过 `omega-doc` 执行，直接 markdown/docs-data edits 仅保留给 emergency projection repair。

截至 `2026-04-15`，`Task 4H ~ 4K` 也已完成：

- `/plan` canonical persistence 已收口到 `docs-data/tasks/*`，project plan、doc task 与 `docs/TODO.md` projection 现在共享同一 repo knowledge base。
- 剩余 `.omega/plans/` compatibility/import layer 已移除；project plan persistence 现在只通过 `docs-data/tasks/*` 暴露。
- 当前 structured docs system 已不再保留与 `/plan` dual storage 相关的 open follow-up。

## Goals

- 为 repo 内文档建立强 schema 的 canonical data layer，而不是继续让 markdown 直接承担真实写入面。
- 让 AI 管理文档时默认使用 `omega-plan` 驱动的强制工具路径，而不是自由编辑 `docs/**/*.md`。
- 统一文档任务、文档记录、文档关系、渲染产物和校验结果的数据格式。
- 把 `docs/` 切成两层：data layer 用于持久化和协作，presentation layer 用于阅读和展示。
- 为后续把现有 docs 全量转入新系统提供 parity-first 迁移路径，确保迁移前后内容语义一致。
- 把 `.claude` 下与文档相关的 skills 改成“围绕结构化文档系统工作”的规范，而不是继续围绕直接编辑 markdown。

## Non-Goals

- 不在本轮直接把当前所有 `docs/**/*.md` 一次性迁移到新格式。
- 不要求 Phase 1 就删除现有 markdown 手工编辑能力；对正常文档工作流，CLI-only cutover 已完成，手工修补仅保留给 emergency projection repair。
- 不把所有 repo 文件都转成 JSONL；本规格只约束文档系统、文档任务和 skill 文档治理面。
- 不把 generated `docs/` 变成一次性静态导出后不可读的产物；展示层仍需保持人类可读、git diff 可审查。

## Desired End State

### Two-Layer Docs Model

```text
<project-root>/
  docs-data/
    manifest.json
    records/
      specs.jsonl
      prds.jsonl
      guides.jsonl
      decisions.jsonl
      whitepapers.jsonl
      archive.jsonl
    tasks/
      doc-tasks.jsonl
    relations/
      links.jsonl
    render/
      render-state.json
  docs/
    README.md
    TODO.md
    specs/
    prds/
    guide/
    decisions/
    whitepapers/
    archive/
```

规则：

- `docs-data/` 是 canonical source of truth。
- `docs/` 是 generated presentation layer；允许人工阅读和 review，但不再是 AI 默认写入真源。
- 对 `docs/` 的任何变更都应可由 `docs-data/` + renderer 重建。

### Core Principle

对 AI 来说，文档管理必须默认变成：

```text
plan/select task
  -> 通过 omega-document-cli mutation surface 写 docs-data
  -> 运行 omega-document-cli render/generate
  -> 运行 omega-document-cli validate
```

读取/查询仍可直接读文件，或通过 stable id 做精确查询；需要被禁止的是无约束 mutation，而不是正常 inspection。

而不是：

```text
直接 apply_patch 若干 markdown 文件
```

## Data Layer Contract

### Document Record

每类文档以 JSONL 记录存入 `docs-data/records/*.jsonl`：

```json
{
  "doc_id": "spec:omega-project-plan-system",
  "doc_type": "spec",
  "slug": "omega-project-plan-system",
  "title": "Omega Project Plan System Specification",
  "status": "draft",
  "owner": "omega-team",
  "created": "2026-04-13",
  "updated": "2026-04-14",
  "version": "0.7",
  "source_path": "docs/specs/omega-project-plan-system.md",
  "frontmatter": {
    "last_verified_commit": "N/A",
    "supersedes": [],
    "related_prds": [
      "docs/prds/omega-project-plan-management.md"
    ]
  },
  "sections": [
    {
      "section_id": "overview",
      "heading": "Overview",
      "body_markdown": "..."
    }
  ],
  "relations": [
    {
      "kind": "references",
      "target": "spec:omega-command-system"
    }
  ],
  "render": {
    "template": "spec-v1",
    "presentation_path": "docs/specs/omega-project-plan-system.md"
  }
}
```

要求：

- 一条记录对应一个 presentation doc。
- frontmatter 与正文 section 分离存储，不允许把整个 markdown blob 作为唯一字段塞回 JSONL。
- `doc_id` 在仓库内稳定，不受文件重命名影响。

### Document Task Record

文档改造任务本身也进入强 schema，存入 `docs-data/tasks/doc-tasks.jsonl`，并由 `omega-plan` 持有 task graph：

```json
{
  "task_id": "DOC-0019A",
  "title": "Define docs-data contract",
  "status": "pending",
  "priority": "p1",
  "task_kind": "docs-system",
  "doc_scope": ["spec", "prd", "guide", "skill"],
  "depends_on": [],
  "acceptance": [
    "docs-data layout is specified",
    "renderer input/output contract is specified"
  ],
  "presentation_links": [
    "docs/specs/omega-structured-document-system.md"
  ]
}
```

规则：

- 文档任务状态使用 canonical enum，不允许自由文本。
- 文档任务的 owner、acceptance、dependency chain 必须可被 `omega-plan`/TUI/command surface 直接消费。
- markdown 中的 `### Task ...` 只能是 generated projection，而不是唯一真源。

## Presentation Layer Contract

### Renderer Responsibilities

renderer 从 `docs-data/` 生成 `docs/`，最少承担以下职责：

- 按模板渲染 frontmatter。
- 按固定 section 顺序生成 markdown。
- 自动生成 repo 内部 cross-link。
- 生成 `docs/README.md` 的阅读入口和索引摘要。
- 生成 `docs/TODO.md` 的 open-work projection。
- 保持输出稳定排序，减少无意义 diff。

### Render Invariants

- 同一份 `docs-data/` 输入必须生成稳定、可重复的 markdown 输出。
- render 过程不得悄悄丢失 section 内容或 frontmatter 字段。
- parity validation 必须能比较“原有 markdown 语义”与“新生成 markdown 语义”是否一致。

## Mandatory Tooling For AI

### Why `omega-plan` Owns The Task Contract

文档治理不是单纯的 file mutation 问题，而是“任务 -> 结构化记录 -> 渲染 -> 验证”的闭环。因此必须把文档任务纳入 `omega-plan`，让 AI 先通过任务系统声明和推进工作，而不是直接散写 markdown。

### Required Tool Families

首轮需要新增或升级以下强制工具面：

1. `plan_document_change`
   - 创建/更新文档任务
   - 绑定目标文档 record / acceptance / dependencies
   - 输出 `omega-plan` task mutation，而不是编辑 markdown
2. `manage_doc_record`
   - 对 `docs-data/records/*.jsonl` 做 typed create/update/archive
   - 校验 required fields、section shape、status enum、relation targets
3. `render_docs_projection`
   - 从 `docs-data/` 生成 `docs/`
   - 支持全量和增量 render
4. `validate_docs_projection`
   - 校验 generated output、broken refs、frontmatter completeness、schema validity、render parity
5. `extract_doc_source`
   - 仅用于 migration 阶段
   - 把现有 markdown 提取成 structured record，再经 renderer 回写到新层

### AI Write Policy

当目标属于文档系统时：

- AI 默认不直接编辑 `docs/**/*.md`。
- AI 必须先创建或选中文档任务，再通过结构化 docs tools 操作 data layer。
- 只有 renderer 负责生成或更新展示层 markdown。
- 若用户明确要求修一个紧急展示层 typo，可允许例外，但需要在任务日志中标记为 temporary/manual patch。

## Repo Layout Rules

### New Directories

- `docs-data/records/`: canonical structured doc records
- `docs-data/tasks/`: 文档任务和迁移任务记录
- `docs-data/relations/`: doc-to-doc / doc-to-task relation ledger
- `docs-data/render/`: renderer state、manifest、parity snapshot

### Existing `docs/` Rules After Cutover

- `docs/` 仍保持当前用户可读目录结构，不直接暴露 JSONL 给日常阅读。
- `docs/archive/` 继续存在于展示层，但其 canonical archive metadata 来自 data layer。
- `docs/TODO.md` 继续作为 open-work projection，而不是文档系统唯一任务源。

## Skill Migration

### Scope

`.claude/skills/` 下至少以下与文档相关的 skills 必须迁移：

- `docs-general`
- `docs-guide`
- `docs-prds`
- `docs-specs`
- `docs-todo`
- `docs-logs`

### New Skill Rules

文档 skill 需要从“编辑 markdown 的写作建议”升级成“驱动结构化文档系统的操作规范”：

- 先确定 doc task / doc record，再写内容。
- 修改文档时先更新 data layer，再 render presentation layer。
- 变更完成后运行 parity / health validation。
- 只有 migration 和 emergency patch 场景允许直接触碰 generated `docs/`。

## Migration Strategy

### Principle

现有文档迁移必须放在最后，且必须以 parity-first 为前提。

也就是说，先完成：

1. data schema
2. tools
3. renderer
4. validator
5. skills migration

然后才允许开始批量 extraction / cutover。

### Rollout Phases

| Task | Scope | Notes |
|------|-------|-------|
| `Task 19A` | 定义 `docs-data/` schema、repo layout、record/task/relation contract | 已实现为 `docs-data/` canonical layout 与 public schema |
| `Task 19B` | 基于 `omega-plan` 的文档任务 contract 与 mandatory docs tools | 已实现为 `manage_document` structured actions + plan sync |
| `Task 19C` | docs renderer / generator，把 `docs-data/` 投影到 `docs/` | 已实现为 deterministic `render_projection` path |
| `Task 19D` | parity validator、source extraction、migration diff tooling | 已实现为 `validate_projection` + `extract_source` |
| `Task 19E` | `.claude` 文档相关 skills 迁移到新规范 | 已实现；文档技能默认先写 structured source，再 render/validate |
| `Task 19F` | 现有 `docs/` 全量迁移与 cutover | 已完成；当前仓库 docs 已切到 `docs-data/` canonical + generated `docs/` |
| `Task 19G ~ 19H` | repo-external CLI foundation + query/mutation surface | 已完成；`omega-doc` 已提供 render/validate/extract/cutover/doctor、get/list、upsert/archive/remove |
| `Task 19I` | CLI-first skills/AGENTS guidance | 已完成；repo guidance 已把 CLI mutation 设为默认工作流 |
| `Task 19J` | docs/docs-data version contract | 已完成；manifest/render-state/generated docs 与 CLI validate/doctor 已具备版本对应关系 |
| `Task 19K` | CLI-only docs workflow cutover | 剩余最终 cutover：把手工 markdown/docs-data mutation 进一步收紧到 emergency-only 例外 |

## Acceptance Standards

只有满足以下条件，才允许开始 `Task 19F`：

- `docs-data/` schema 已冻结到可执行版本。
- AI 可通过强制工具完成文档任务闭环，而不依赖直接编辑 markdown。
- renderer 可稳定生成当前目录结构下的 presentation docs。
- parity validator 能对 representative docs 给出可信一致性结论。
- `.claude` 文档相关 skills 已改写为新规范。

## Testing Strategy

- schema tests: record/task/relation JSONL parse + validation
- renderer tests: 同一输入生成稳定 markdown
- parity tests: 旧 markdown -> extracted record -> generated markdown 语义一致
- tool tests: AI-facing docs tools 拒绝非法 status、非法 section、缺失 owner/frontmatter 的写入
- migration tests: representative spec/prd/guide/adr/todo sample round-trip
- skill regression: 文档 skill 指令不再默认要求“直接编辑 docs/markdown”

## Related Specs

- `docs/specs/omega-context-management.md`
- `docs/specs/omega-project-plan-docs-data-convergence.md`
- `docs/specs/omega-document-cli.md`
- `docs/specs/omega-document-projection-versioning.md`
- `docs/specs/omega-command-system.md`
- `docs/specs/omega-project-plan-system.md`
- `docs/specs/omega-project-system.md`

---

### Change Log

- 2026-04-15: v0.8 — `Task 19I ~ 19J` completed: repo guidance now defaults docs mutation to `omega-doc`, and docs/docs-data revision correspondence is recorded in manifest/render-state/generated docs plus CLI validation surfaces. Remaining structured-docs follow-up is narrowed to `Task 19K`.
- 2026-04-15: v0.7 — `Task 19G ~ 19H` completed: `omega-document-cli` now exists as a real crate with foundation commands, id-based query, and controlled mutation surface; remaining docs workflow follow-up is narrowed to `Task 19I ~ 19K`.
- 2026-04-14: v0.6 — expanded post-cutover follow-up from `Task 19G` to `Task 19G ~ 19K`: docs mutations should converge on `omega-document-cli`, queries remain direct-read or id-based lookup, and generated docs need an explicit version contract back to docs-data.
- 2026-04-14: v0.5 — added `Task 4H ~ 4K` as a follow-up to converge `/plan` canonical persistence on `docs-data/`, so document tasks and project plan no longer live in separate repo-local storage systems.
- 2026-04-14: v0.4 — added `Task 19G` as a post-cutover follow-up to plan a repository-external structured docs CLI that reuses `omega-document` outside session runtime.
- 2026-04-14: v0.3 — `Task 19E ~ 19F` completed: document-related `.claude` skills now follow structured docs v2, `docs-data/tasks/doc-tasks.jsonl` exists as canonical task ledger, and the current `docs/` tree has been migrated and regenerated from `docs-data/`.
- 2026-04-14: v0.2 — `Task 19A ~ 19D` completed and moved to baseline: `docs-data/` layout, structured record/task/relation schema, `manage_document` structured actions, `/document render|validate|extract`, and parity-first extraction/validation are implemented.
- 2026-04-14: v0.1 — 初版规格，定义 structured document system v2：`docs-data/` data layer、generated `docs/` presentation layer、`omega-plan` 驱动的文档任务 contract、mandatory docs tools、skill migration，以及“现有 docs 最后迁移”的 rollout 原则。
