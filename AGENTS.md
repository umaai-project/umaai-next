# umaai-next 项目规则

## 对话与工作规范

1. **语言**：使用中文回答，保持简洁。
2. **大量分析用子代理**：分析大量代码或数据时，派发子代理提取结论。
3. **选型交由用户**：涉及未确认的领域知识或方案选型时，先给出选项供用户决定再实施。
4. **提案先做减法**：提出方案前先从「做减法」的角度审视。
5. **编码前确认方案**：每阶段实际编码前，先总结方案并向用户确认，确认后再动手。

## 项目指针与范围

- **架构与落点**：新增模块、调整 crate 依赖、定位代码或处理特征门控前，先读 [ARCHITECTURE.md](ARCHITECTURE.md)。
- **文档与提交同步**：更新文档或进入提交准备前，先读 [docs/README.md](docs/README.md)。
- **剧本仓边界**：具体剧本实现与数据不进本仓，新剧本从 `templates/umaai-scenario/` 复制；跨剧本通用件按「带默认实现、非破坏」加钩子（见 [docs/scenario/README.md](docs/scenario/README.md)）。
- **数据约定**：`gamedata/` 为只读静态输入，不写死路径，统一经 `umaai-core::Data` 由宿主注入。

## 编码、测试与工具纪律

- **Rust 规范**：`Result` 优先（非测试与非临时代码禁用 `unwrap` / `panic`，越界直接报错）；固定结构建新类型；新增公开 API 须配只描述当前行为与契约的 `rustdoc`（简短，不挂历史修改）；`Deref` 底座公开方法不得与契约 trait 同名。
- **测试规范**：单元测试内联 `assert`；集成测试经 `Checker` / `Checks` 返回 `Result`；Release 档跑 test 与 binary；测试与验收闭环须一并运行 `deslop-please`。
- **架构检查（`deslop-please`）**：与 `cargo` 共用 `target/` 锁，顺序执行；报错须从第一性原理整体修复（以深模块与窄接口隐藏复杂度，而非机械挪动绕过）：
  - `module-cycle` / `tangle`：厘清目标抽象分层与各模块归属。
  - `field-leak` / `shallow-module`：审视字段与函数的存在意义及调用模式，整体收口为窄接口而非逐点修补。
  - 重构前先画结构图，向用户说明调整前后的模块关系。
- **Cargo 纪律**：在仓根直接运行 `cargo`；同一 `target` 一次只跑一个 cargo；日常 `fmt --all --check`、`clippy --workspace --all-targets -- -D warnings`、`test` 加 `--offline`；改 registry（`add` / `fetch`）先请求提权；**确认进入提交准备前，保持工作树格式原样（不运行会改写文件的 `cargo fmt` 或 `--fix`）**。
- **Git 提交**：待用户确认全部调整后再进入提交准备（涵盖工作树全部修改）；禁止主动 `commit` 或 `push`。

## Agent skills

- **领域与 ADR**：探索代码库、核对领域术语或记录架构决策时，读 [docs/agents/domain.md](docs/agents/domain.md)。
- **Issue、分诊与路线图**：操作 GitHub Issue、映射分诊标签或用 Wayfinder 拆解大型任务路线图时，读 [docs/agents/issue-tracker.md](docs/agents/issue-tracker.md)、[docs/agents/triage-labels.md](docs/agents/triage-labels.md)、[docs/agents/wayfinder.md](docs/agents/wayfinder.md)。
- **追问与一手调研**：执行多轮设计追问或后台一手资料调研时，读 [docs/agents/grilling.md](docs/agents/grilling.md)、[docs/agents/research.md](docs/agents/research.md)。
- **会话交接**：上下文过长或需交接给下一位 Agent 时，读 [docs/agents/handoff.md](docs/agents/handoff.md)。
