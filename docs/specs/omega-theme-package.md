---
content_revision: 120
created: 2026-03-19
generation_id: gen_000046_r000120
last_verified_commit: N/A
owner: omega-team
projection_version: 46
related_prds: []
source_doc_id: "spec:docs-specs-omega-theme-package"
status: implemented
supersedes: []
updated: 2026-04-08
---

# Omega Theme Package Specification

## Overview

当前 `omega-tui` 的视觉样式主要集中在 `render.rs` 的 `ColorScheme` 与若干局部 `Style` 拼装逻辑里。这个方式在早期迭代足够快，但随着输入区、底部状态带、overlay、sidebar、Markdown 渲染、代码高亮和未来会话统计逐步增加，颜色、边框、状态语义色与组件级视觉规则会继续散落，最终让“改样式”演变成在多个渲染函数里搜索 RGB 常量和局部判断。

本规格规划新增 `omega-theme` crate，把共享视觉令牌和组件级样式定义从 `omega-tui` 中抽离出来。目标不是把布局和交互逻辑搬走，而是让所有“视觉决策”拥有清晰的所有权和稳定入口。

## Goals

- 为 `omega-tui` 提供单一的主题来源，集中管理颜色、边框、语义状态色、间距和组件视觉槽。
- 让输入框、上下状态条、sidebar、overlay、Markdown/代码块等后续视觉扩展使用同一套命名令牌，而不是继续堆叠局部常量。
- 保持 `omega-tui` 只负责状态到视觉语义的映射，不负责维护整套样式常量表。
- 支持通过用户可编辑的 `.omega/theme.toml` 对默认主题做仓库级或工作目录级覆盖，并在非法配置下安全回退。
- 为未来命名主题或高对比度主题预留结构，而不要求首期就实现多主题切换。
- 为 `Modern TUI / Rich CLI` 风格提供稳定的 token grammar，而不是只提供泛化的暗色配色表。

## Non-Goals

- 不在本任务中重构 `omega-tui` 的布局结构或事件模型。
- 不要求首期支持运行时热重载 `.omega/theme.toml` 或主题市场。
- 不把运行时状态机逻辑下沉到主题包，例如 `InteractionMode`、焦点、overlay 生命周期仍属于 `omega-tui`。
- 不要求一开始就覆盖仓库内所有 crate；首个消费者仅为 `omega-tui`。
- 不提供“随意上色”的自由调色盘接口；主题系统应优先保护信息层级和语义一致性。

## Design Language

`omega-theme` 的默认主题不应被理解成抽象的 `dark theme`，而应被理解成终端里的 `Dark Industrial Report Console`：

- 深色 surface 稳定，层级分明，但不靠纯黑和纯白硬顶对比。
- accent 克制，默认只服务 focus、status、section hierarchy 和关键数据。
- 正文尽量安静，把亮色预算留给 header、summary metrics、code-style token 和异常状态。
- 表格、列表、divider、badge 与 section title 都应有明确 token，而不是共享同一组文本颜色。

### Semantic Accent Roles

推荐至少拆出以下角色：

- `structure accent`: section title、card header、次级 divider。
- `metric accent`: 核心数字、百分比、时长、token 成本。
- `status accent`: success / running / warning / error / interrupted。
- `code accent`: 文件名、命令、代码、标识符。
- `muted support`: meta row、说明文、折叠摘要、弱提示。

这些角色的目标是避免“标题、状态、文件名、指标全都抢同一种亮色”。

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
- 定义默认用户主题文件模板与 `.omega/theme.toml` 加载入口。
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
- `omega-theme` -> `toml` / `serde`（用于解析 `.omega/theme.toml`）
- 其他运行态 crate 不应依赖 `omega-theme`

### Architectural Rule

新增视觉调整时遵守：

- 如果只是改颜色、边框、divider、强调色、组件默认外观，优先改 `omega-theme`。
- 如果需要根据运行态选择某个组件主题，决策逻辑留在 `omega-tui`。
- 如果需要新组件的视觉抽象，先在 `omega-theme` 增加组件级主题结构，再让 `omega-tui` 接线。

## Theme File

### Location

默认路径：`.omega/theme.toml`

设计沿用 `.omega/keymap.toml` 的本地配置目录约定，保持用户在工作目录下查找配置时的可发现性一致。

### Startup Behavior

- 若文件存在，则读取、解析并校验，再应用到内置默认主题之上。
- 若文件缺失，则创建默认 `.omega/theme.toml` 文件并加载它。
- 若文件格式错误或字段非法，则记录错误、向用户显示提示，并回退到内置默认主题。

### Merge Model

`.omega/theme.toml` 不要求声明完整主题，而是作为对内置主题的覆盖层：

- 未提供的字段继续使用默认主题值。
- 只允许覆盖公开的语义令牌和组件级主题字段。
- 不允许在配置文件中表达运行时状态判断、条件逻辑或布局控制。

这样可以避免用户配置因主题版本演进而频繁失效，也可以降低首次编辑门槛。

## Configuration Format

建议使用语义分段，而不是平铺颜色表：

```toml
theme = "dark"

[input]
border_type = "rounded"
text_fg = "#569cd6"
normal_border_fg = "#a3bbd6"
insert_border_fg = "#4ec9b0"

[context_bar]
label_fg = "#747e8c"
hint_fg = "#acb3bd"

[status_bar]
label_fg = "#747e8c"
divider_fg = "#626b78"
idle_fg = "#7bc78f"
running_fg = "#ffc468"

[surfaces]
focus_border_fg = "#4ec9b0"
border_dim_fg = "#303030"

[report]
section_header_fg = "#b7a7ff"
metric_emphasis_fg = "#ffb454"
code_fg = "#7fd1b9"
muted_meta_fg = "#8b93a1"
```

格式要求：

- 颜色优先支持 `#RRGGBB`；是否支持命名色可后续再议。
- 枚举型字段如 `border_type` 只接受受控值，例如 `plain`、`rounded`。
- 配置键名应与主题模型的语义字段一致，而不是直接暴露底层渲染实现的临时字段名。

## API Shape

首期建议结构：

```rust
pub struct OmegaTheme {
    pub surfaces: SurfaceTokens,
    pub text: TextTokens,
    pub status: StatusTokens,
    pub report: ReportTokens,
    pub input: InputTheme,
    pub status_bar: StatusBarTheme,
    pub context_bar: ContextBarTheme,
    pub sidebar: SidebarTheme,
    pub overlay: OverlayTheme,
}

pub struct ReportTokens {
    pub section_header_fg: Color,
    pub metric_emphasis_fg: Color,
    pub code_fg: Color,
    pub muted_meta_fg: Color,
    pub table_border_fg: Color,
    pub summary_badge_bg: Color,
}

impl OmegaTheme {
    pub fn dark() -> Self;
    pub fn load_or_create_default(path: &Path) -> Result<LoadedTheme>;
}

pub struct InputTheme {
    pub border_type: BorderType,
    pub text_fg: Color,
    pub normal_border_fg: Color,
    pub insert_border_fg: Color,
    pub cursor_fg: Color,
    pub cursor_bg: Color,
}

pub struct LoadedTheme {
    pub theme: OmegaTheme,
    pub source: ThemeSource,
    pub warnings: Vec<String>,
}

pub enum ThemeSource {
    BuiltinDefault,
    File(PathBuf),
    FileWithFallback(PathBuf),
}
```

设计原则：

- API 按语义和组件分组，不按“当前文件里有哪些字段”分组。
- 避免 `input_border`, `input_border_insert`, `input_border_normal`, `status_label_dim_2` 这类平铺扩张式字段。
- 保持名称稳定，使后续视觉重构尽量不影响调用方结构。
- 加载 API 返回主题来源和警告，便于 `omega-tui` 在状态栏、日志或 notice 中向用户解释当前主题状态。

## Validation Rules

- 未知字段默认视为错误，避免用户误以为配置已生效。
- 非法颜色值应给出精确错误信息，例如字段路径和原始值。
- 枚举值非法时应拒绝该配置并回退，而不是静默忽略。
- 配置迁移期若出现已弃用字段，可先产出 warning，再在后续版本删除。

## Migration Plan

### Phase 1: Token Extraction

- 创建 `omega-theme` crate。
- 迁移 `ColorScheme` 中通用视觉令牌与组件级配置。
- 增加 `.omega/theme.toml` 默认模板、解析模型和加载入口。
- 保持 `omega-tui` 外部行为不变，只替换样式来源。

### Phase 2: Component Theming

- 为输入框、输入上下文带、底部状态带、sidebar、overlay 建立独立主题片段。
- 把当前 `render.rs` 中散落的 `Style::default().fg(...).bg(...)` 聚合为可复用的组件级方法或字段。
- 让 `omega-tui` 在启动时消费 `LoadedTheme`，并把加载失败或 fallback 信息转成用户可见 notice / log。

### Phase 3: Advanced Consumers

- 为 `Task 15B-8` Markdown 渲染定义标题、列表、引用、代码块令牌。
- 为 `Task 15B-9` 代码高亮定义基础语法类别映射和 fallback 颜色。
- 为 `Task 15B-12` 会话统计和 badge 扩展定义更多状态语义色。
- 为 `Task 15B-56 ~ 15B-57` 的结构化报告输出补齐 section header、table、metric emphasis 和 quiet-meta token。

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
| user config path | `.omega/theme.toml` | 与 `.omega/keymap.toml` 保持一致，明确且可发现 |
| config merge model | overlay on builtin theme | 减少用户配置体积，并降低版本演进时的破坏性 |
| token granularity | semantic + component-level | 避免只有原始颜色表，也避免 widget 逻辑下沉 |
| theme count in phase 1 | one named theme (`dark`) | 先解决所有权，再扩展多主题 |
| runtime mapping | keep in `omega-tui` | 主题包不拥有应用状态机 |
| default visual language | `Dark Industrial Report Console` | 让默认 dark theme 有明确的审美和语义约束 |

## Risks

- 如果过早把 API 抽得太细，会演变成新的样式样板代码负担。
- 如果只迁颜色表、不迁组件级主题，调用端仍会残留大量局部样式拼装。
- 如果把运行态判断也塞进 `omega-theme`，会破坏 UI 边界并降低可测试性。
- 如果配置文件字段直接暴露底层实现细节，后续重构会把用户配置格式也锁死。
- 如果校验与 fallback 不清晰，用户会难以判断当前看到的是配置主题还是默认主题。

## Testing Strategy

- `omega-theme` 单测：验证默认主题能构造出完整令牌集。
- `omega-theme` 单测：验证 `.omega/theme.toml` 缺失、合法、非法三种场景下的创建、加载、校验与回退。
- `omega-theme` 单测：验证覆盖式合并不会清空未声明字段。
- `omega-theme` 单测：验证 report tokens 缺失时仍能回退到安全、可读的默认值，而不是把结构标题和指标高亮退化成普通正文色。

## Change Log

- 2026-04-08: 为主题系统补充 `Dark Industrial Report Console` 设计语言与 report token 方向，支撑结构化报告型 TUI 输出。
- `omega-tui` 回归测试：迁移到 `omega-theme` 后，现有输入区、状态带、sidebar 和 overlay 的视觉语义测试保持通过。
- 针对未来 Markdown / 高亮接入，优先增加“语义样式选择正确”的测试，而不是依赖截图式验证。
- 2026-03-19: 初版规格，规划新增 `omega-theme` crate，集中管理 `omega-tui` 的共享视觉令牌与组件级主题定义。
- 2026-03-19: 补充 `.omega/theme.toml` 用户配置加载设计，明确默认文件生成、覆盖合并、校验与回退策略。
