# 测试一览（umaai-next）

> 条数与用例名以源码及 `cargo test --workspace --release -- --list` 为准，本文件只记分层、职责索引、降级矩阵与基线口径。

## 1. 测试分层与集成位置

| 层 | 位置 | 判定方式 |
|---|---|---|
| L1 单元 | 各 crate `src/` 内联 `#[cfg(test)]` | `assert` |
| L2 模块集成 | `umaai-core/tests/data_load.rs`、`umaai/tests/{channel,config,luck,protocol,runner}.rs`、`umaai-update/tests/update_flow.rs` | `print` + `Checker`/`Checks`（返回 `Result`） |
| L3 跨 crate 端到端 | `umaai-tests/tests/{basic_game,cli_smoke,rule_streams,runner_basic,search_smoke,session_basic,smoke,status_score,tools_smoke}.rs` | `print` + `Checker`（返回 `Result`） |
| L4 baseline | `umaai-tests/tests/baseline.rs` + `umaai-tests/baselines/*.json` | 逐项容差比对（返回 `Err`） |

- **只含内联单测的 crate**：`umasim`（另含 1 条 `` `ignore` `` doctest）、`umaai-cli`、`umaai-tools`（工具端到端统一放 `umaai-tests/tests/tools_smoke.rs`）。
- **配置与数据约定**：默认层 `<数据根>/ai_config.toml`，覆盖层 `<宿主目录>/ai_config.toml`（宿主目录由入口注入，运行目录不得残留覆盖层污染）；`DataSource` 同时覆盖 `DirSource` 与 `MemorySource`。
- **搜索夹具根**：搜索相关用例的根局面必须经真实 `run_stage` 推进到 `Distribute` 之后的首个训练决策点（生产搜索的调用点）；直接拿回合 0 的 `Begin` 局面 rollout 属非法搜索根。

## 2. 常用运行与降级矩阵

```powershell
# 全量测试与架构检查（Release + offline 为项目约定）
cargo test --workspace --release --offline
deslop-please

# 仅列用例清单 / 跑默认跳过项（如真 watcher、真通道、ignored doctest）
cargo test --workspace --release --offline -- --list
cargo test --workspace --release --offline -- --ignored --nocapture

# 三档可选特性降级验证（确保不引入对应重依赖且编过/测过）
cargo check -p umaai --no-default-features                                 # 关 file-channel 与 parallel（无 notify / rayon）
cargo test -p umasim -p umaai --release --offline --no-default-features    # 关 parallel（无 rayon，串行降级）
cargo test -p umaai-update --release --offline --no-default-features       # 关 http（无 ureq，全走 TestTransport）

# 独立剧本仓模板守门（不进主 workspace）
cargo fmt --manifest-path templates/umaai-scenario/Cargo.toml --all --check
cargo clippy --manifest-path templates/umaai-scenario/Cargo.toml --workspace --all-targets --offline -- -D warnings
cargo test --manifest-path templates/umaai-scenario/Cargo.toml --workspace --release --offline
```

## 3. L4 基线重录与反自证

- **重录命令**：`cargo run --release -p umaai-tools --bin umaai-tools -- baseline --update`（单跑校验去掉 `--update`，可用 `--only <基线名>` 收窄）。
- **一基线一 JSON**：`input` 自持（不读配置层）、`expect` 逐项（裸值精确 / `{value, tol}` 带容差）、`meta` 记版本与日期**不含路径**；`baseline.rs` 内置反自证守门（改动任一期望即红）。
- 默认行为不等价的改动重录基线后，须在 [`changelog.md`](changelog.md) 记录原因与影响面。
