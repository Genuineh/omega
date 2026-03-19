---
status: draft
owner: omega-team
created: 2026-03-19
updated: 2026-03-19
version: 0.1
supersedes: []
related_prds: []
---

# Omega Theme Package Specification

## Overview

当前 `omega-tui` 的视觉样式主要集中在 `render.rs` 的 `ColorScheme` 与若干局部 `Style` 拼装逻辑里。这个方式在早期迭代足够快，但随着输入区、底部状态带、overlay、sidebar、Markdown 渲染、代码高亮和未来会话统计逐步增加，颜色、边框、状态语义色与组件级视觉规则会继续散落，最终让“改样式”演变成在多个渲染函数里搜索 RGB 常量和局部判断。

本规格规划新增 `omega-theme` crate，把共享视觉令牌和组件级样式定义从 `omega-tui` 中抽离出来。目标不是把布局和交互逻辑搬走，而是让所有“视觉决策”拥有清晰的所有权和稳定入口。

## Goals

- 为 `omega-tui` 提供单一的主题来源，集中管理颜色、边框、语义状态色、间距和组件视觉槽。
- 让输入框、上下状态条、sidebar、overlay、Markdown/代码块等后续视觉扩展使用同一套命名令牌，而不是继续堆叠局部常量。
- 保持 `omega-tui` 只负责状态到视觉语义的映射，不负责维护整套样式常量表。
- 为未来命名主题或高对比度主题预留结构，而不要求首期就实现多主题切换。

## Non-Goals

- 不在本任务中重构 `omega-tui` 的布局结构或事件模型。
- 不要求首期支持用户可配置主题文件、热切换主题或主题市场。
- 不把运行时状态机逻辑下沉到主题包，例如 `InteractionMode`、焦点、overlay 生命周期仍属于 `omega-tui`。
- 不要求一开始就覆盖仓库内所有 crate；首个消费者仅为 `omega-tui`。

## Problem Statement

当前存在的结构性问题：

- 颜色和样式常量集中在单个渲染文件，演进到更多 widget 后会形成新的“视觉 God object”。
- 组件视觉规则缺少命名边界，例如“输入框边框”和“插入模式边框”之间只有直接颜色选择，没有共享的组件主题模型。
- 同一视觉语义可能在多个位置重复定义，例如 focus、mode、warning、divider、muted text。
- 后续 Markdown 渲染和代码高亮如果直接接入 `omega-tui`，会继续放大样式散落问题。

## Proposed Crate

新增：`crates/omega-theme`

职责：

- 定义命名主题，例如 `OmegaTheme::dark()`。
- 定义语义令牌，例如 `status.running_fg`、`mode.insert_fg`、`surface.panel_border_dim`。
- 定义组件级主题片段，例如 `InputTheme`、`StatusBarTheme`、`SidebarTheme`、`OverlayTheme`。
- 暴露 `omega-tui` 可直接消费的样式结构。

非职责：

- 不持有 `App`。
- 不读取 keymap、session、todo、message 或 background 等运行态数据。
- 不负责布局计算、焦点路由或任何 widget 渲染顺序。

## Boundary Design

### Ownership

- `omega-theme`: 拥有视觉令牌、组件主题模型、默认主题构造与未来主题扩展。
- `omega-tui`: 拥有布局、状态机、面板路由以及“当前状态选用哪种语义样式”的判断。
- `omega-session` / `omega-core`: 不感知任何主题或样式细节。

### Dependency Direction

- `omega-tui` -> `omega-theme`
- `omega-theme` -> `ratatui`（允许首期直接依赖，降低适配层复杂度）
- 其他运行态 crate 不应依赖 `omega-theme`

### Architectural Rule

新增视觉调整时遵守：

- 如果只是改颜色、边框、divider、强调色、组件默认外观，优先改 `omega-theme`。
- 如果需要根据运行态选择某个组件主题，决策逻辑留在 `omega-tui`。
- 如果需要新组件的视觉抽象，先在 `omega-theme` 增加组件级主题结构，再让 `omega-tui` 接线。

## API Shape

首期建议结构：

```rust
pub struct OmegaTheme {
    pub surfaces: SurfaceTokens,
    pub text: TextTokens,
    pub status: StatusTokens,
    pub input: InputTheme,
    pub status_bar: StatusBarTheme,
    pub context_bar: ContextBarTheme,
    pub sidebar: SidebarTheme,
    pub overlay: OverlayTheme,
}

impl OmegaTheme {
    pub fn dark() -> Self;
}

pub struct InputTheme {
    pub border_type: BorderType,
    pub text_fg: Color,
    pub normal_border_fg: Color,
    pub insert_border_fg: Color,
    pub cursor_fg: Color,
    pub cursor_bg: Color,
}
```

设计原则：

- API 按语义和组件分组，不按“当前文件里有哪些字段”分组。
- 避免 `input_border`, `input_border_insert`, `input_border_normal`, `status_label_dim_2` 这类平铺扩张式字段。
- 保持名称稳定，使后续视觉重构尽量不影响调用方结构。

## Migration Plan

### Phase 1: Token Extraction

- 创建 `omega-theme` crate。
- 迁移 `ColorScheme` 中通用视觉令牌与组件级配置。
- 保持 `omega-tui` 外部行为不变，只替换样式来源。

### Phase 2: Component Theming

- 为输入框、输入上下文带、底部状态带、sidebar、overlay 建立独立主题片段。
- 把当前 `render.rs` 中散落的 `Style::default().fg(...).bg(...)` 聚合为可复用的组件级方法或字段。

### Phase 3: Advanced Consumers

- 为 `Task 15B-8` Markdown 渲染定义标题、列表、引用、代码块令牌。
- 为 `Task 15B-9` 代码高亮定义基础语法类别映射和 fallback 颜色。
- 为 `Task 15B-12` 会话统计和 badge 扩展定义更多状态语义色。

## Task Planning

建议在 `docs/TODO.md` 中新增：

- `Task 15E-1`: `omega-theme` — 主题与样式令牌包

建议依赖：

- `Blocked by`: `Task 15D`
- `Blocks`: `Task 15B-8`, `Task 15B-9`, `Task 15B-12`

理由：

- `Task 15D` 先完成 UI 边界剥离，避免主题包过早和非 UI 逻辑混在一起。
- Markdown、代码高亮和统计 badge 都会显著增加视觉复杂度，适合作为主题包落地后的受益方。

## Technical Decisions

| Decision | Choice | Rationale |
|---------|--------|-----------|
| crate name | `omega-theme` | 与现有 workspace 命名一致，语义直接 |
| ratatui dependency | allow in `omega-theme` | 当前唯一消费者是 TUI，先降低抽象成本 |
| token granularity | semantic + component-level | 避免只有原始颜色表，也避免 widget 逻辑下沉 |
| theme count in phase 1 | one named theme (`dark`) | 先解决所有权，再扩展多主题 |
| runtime mapping | keep in `omega-tui` | 主题包不拥有应用状态机 |

## Risks

- 如果过早把 API 抽得太细，会演变成新的样式样板代码负担。
- 如果只迁颜色表、不迁组件级主题，调用端仍会残留大量局部样式拼装。
- 如果把运行态判断也塞进 `omega-theme`，会破坏 UI 边界并降低可测试性。

## Testing Strategy

- `omega-theme` 单测：验证默认主题能构造出完整令牌集。
- `omega-tui` 回归测试：迁移到 `omega-theme` 后，现有输入区、状态带、sidebar 和 overlay 的视觉语义测试保持通过。
- 针对未来 Markdown / 高亮接入，优先增加“语义样式选择正确”的测试，而不是依赖截图式验证。

---

### Change Log
- 2026-03-19: 初版规格，规划新增 `omega-theme` crate，集中管理 `omega-tui` 的共享视觉令牌与组件级主题定义。