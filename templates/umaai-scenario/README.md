# umaai-scenario（空剧本仓模板）

最小可编译的剧本骨架：复制本目录到自己的剧本仓、改名 `umaai-<剧本>` 后开工。
完整的接入纪律见引擎仓的 [`docs/scenario/README.md`](../../docs/scenario/README.md) 与
[`docs/scenario/contracts.md`](../../docs/scenario/contracts.md)。

## 上手三步

1. **复制 + 改名**：复制本目录到剧本仓，把 `Cargo.toml` 的 `name` 改成 `umaai-<剧本>`
   （如 `umaai-ramen`）；并入自己仓的 workspace 时删掉 `[workspace]` 表并登记 `members`。
2. **改依赖路径**：开发期 `path` 指向引擎仓（并列目录 / 仓内相对路径两种写法见下）；稳定期换
   git `tag`；下游宿主（Web / Android）加 `default-features = false`。
3. **填留白**：`list_actions`（自己的动作）、`generate_events`（自己的事件）、`do_train`（自己的
   数值）、`deyilv`（自己的得意率）、`ScenarioView`（自己的资源快照）。

## 依赖三档

```toml
# 1. 开发期：并列目录（引擎仓与剧本仓并列）
umaai-core = { path = "../umaai-next/crates/umaai-core" }
umasim     = { path = "../umaai-next/crates/umasim" }
umaai      = { path = "../umaai-next/crates/umaai" }
umaai-cli  = { path = "../umaai-next/crates/umaai-cli" }   # 仅终端入口需要（样例面）

# 2. 稳定期：同一 repo、同一 tag（cargo 只解析一份检出）
# umaai = { git = "<engine repo>", tag = "engine-v0.1.0" }

# 3. 下游（Web / Android）：自喂帧、无文件监听，并剪掉并行后端
# umaai = { git = "<engine repo>", tag = "engine-v0.1.0", default-features = false }
```

## 验收口径

- `cargo check` 通过（**不改引擎任何一行**）；
- 接入新剧本时引擎仓 `git diff` 为空——确需新能力时回引擎仓按「带默认实现、非破坏」加钩子；
- 三条随机轴（局面 / 事件 / 策略）+ 决策流只喂 `Trainer`；阶段自持、对外只投影 `StageId`；
- 剧本资源给前端走 `ScenarioView`（形态 A 五条纪律），不进契约 trait。
