# miko_next 项目规则

## 对话规则与工作规范

1. **语言要求**：思考和回答使用中文，语气简洁、专业。
2. **避免信息过载**：需要分析大量代码 / 数据并提取结论时，使用子代理。
3. **方案选择**：涉及不在上下文中的领域知识、需要做选型时，停下来给出建议并交给用户选择，不要直接生成代码。
4. **提案前审视**：提案前从「做减法」的角度重新考虑一次。
5. **网络问题需介入**：网络异常时停下来提醒用户。

## 项目上下文

- 本项目为 **miko_next 重建仓**：从零建立的「通用引擎 + 可插拔剧本」。
- 旧实现与行为基准：`../umaai-rs`（旧仓）、`../umaai-rs-ref`（master 只读工作树，**移植源**）。
- 单一事实源文档在 `docs/`；重建方案见 [`docs/plan/miko_next_plan.md`](docs/plan/miko_next_plan.md)，未决项见 [`docs/notes/open_items.md`](docs/notes/open_items.md)。

## 相关文档

- [docs/plan/](docs/plan/miko_next_plan.md)：重建方案（v2 权威 + v1 参考）
- [docs/game/](docs/game/README.md)：游戏机制（规则 / 实现细节 / 术语 / 固有效果）
- [docs/changelog.md](docs/changelog.md)：变更日志（极简，每条一行）
- [docs/issues.md](docs/issues.md)：问题记录（状态 + 排查过程）
- [docs/project_context.md](docs/project_context.md)：项目上下文（最新代码结构）
- [docs/notes/](docs/notes/open_items.md)：开发笔记与待决事项

## 文档规则

以下三个文档随代码同步维护，均在提交准备阶段统一更新（见「Git 操作」）：

1. **changelog**（`docs/changelog.md`）：**极简记录**。每条修改一行，只写「改了什么」，不含具体代码、变量名或数值。
2. **issues**（`docs/issues.md`）：记录**问题状态与排查过程**（现象 / 根因 / 结论 / 处理结果），避免重复踩坑。
3. **project_context**（`docs/project_context.md`）：记录**最新版代码结构**（工作空间 / crate / 模块 / 配置文件 / 数据约定 / 开发环境等）。只描述当前现状，不含旧结构或修改过程。

## 安全注意事项

1. **隐私保护**：生成的代码和文档不应透露用户信息或绝对路径，使用相对路径或占位符。
2. **必须由用户确认的操作**：读取/修改根目录或系统文件、改写工作区以外的文件、远程连接、扫描局域网等，先简短解释并交用户确认。
3. **符号链接必须检查绝对路径**，确认落在工作区内。
4. **删除文件前先列出文件列表**，检查后再删。

## Rust 编码规范

### 依赖管理
1. 禁止直接手改 `Cargo.toml` 的 `dependencies`，用 `cargo add` 添加；workspace 子 crate 的依赖同步到 workspace 依赖。
2. 核心层 `umaai-core` 只允许轻量依赖（`serde` / `thiserror` 类）。

### 错误处理
1. `Result` 优先；可用 `Result` 的场合不用 `unwrap`。
2. `Result` 类型优先级：当前文件已有的类型 → `anyhow::Result` → 标准 `Result`。
3. 取 `Option` 内部值时优先 `ok_or_else` 转 `Result`。
4. 仅测试 / 临时代码允许 `unwrap` / `panic`，并加 linter 标注。

### 代码结构
1. **新类型原则**：固定结构的数据建新类型，避免裸 Tuple。
2. **文档化注释**：所有新增函数 / 宏 / 结构体必须有可用于 rustdoc 的注释。注释需简短，一般不超过10行；文件头部注释不超过15行；注释必须对应代码现状，历史修改应写入changelog文档，不可挂在注释上
3. 变量命名简短；长词可用缩写。

### 测试规范
1. **分层与断言**：单元测试（内联，私有细节 / 纯函数）用 `assert`；**集成测试（`tests/`）不用 `assert`**，用 `print` 类输出 + `Result`返回值供 Agent 检查；baseline 测试用「比对后返回 `Err` / 非零退出码」自动判定。
2. **失败信号**：集成测试统一经 `umaai-tests` 的 `Checker`（打印 + 聚合 + 返回 `Err`），保证 `cargo test` 有失败信号。
3. 跨 crate 的集成 / 端到端测试放 `crates/umaai-tests/tests/`；共享夹具经 testkit 暴露，禁止 `#[path = "..."]` 跨目录引入。
4. 新功能需在当前文件或对应 `tests/` 补测试，并随实现更新 `test_overview` 文档。
5. 测试工作目录为 workspace 根；需要时用公共函数定位。
6. **Release 模式**：所有 test 与 binary 用 Release 编译。

### 代码质量
1. 嵌入代码（如 XML / JSON 内联）需仔细检查语法。
2. 工具类方法符合标准 Trait（`From` / `TryFrom` / `Deref` / `Display` 等）时优先实现。
3. **`cargo fmt` 可运行**：工具链统一 stable（见 `rust-toolchain.toml`），格式规则见 `rustfmt.toml`；改动代码后可自行执行 `cargo fmt`。

## 工具使用

- PowerShell 下可用：`cargo nextest`、`tokei`、`grep`。

### Git 操作
1. **不自动提交 / 推送**：Agent 不主动 `git commit`、`git push`。先确认用户已完成全部想做的调整，再进入提交准备与提交。
2. **提交前准备**：提交准备阶段统一更新三个文档——`docs/changelog.md` / `docs/issues.md` / `docs/project_context.md`（规则见「文档规则」），由用户确认后再 `git commit`。
3. **提交范围**：commit 需涵盖工作树下由 Agent 与用户做的全部修改。
4. **推送由用户手动执行**：`git commit` 后不推送，push 一律交由用户手动完成。
5. 遵循 Git 安全协议，避免破坏性操作。

## 数据约定

- 运行时数据统一放**单一数据根** `gamedata/`（只读输入）；剧本数据不进本仓。
- 加载走 `umasim` 的单一 `data` 模块；**数据目录由宿主注入**，引擎不硬编码路径。
- 运行输出（`logs/` 等）不入数据根。
