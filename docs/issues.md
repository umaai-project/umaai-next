# 问题记录

记载复杂问题的解决过程与根因，避免重复踩坑。

## 2026-10-17

### 搜索根必须取在决策点

- **现象**：搜索内核的真实 rollout 测试在人头分配处越界 panic（`distribution` 长度为 0）。
- **排查**：`newgame` 的初始局面停在回合 0 的 `Begin` 阶段，人头分布要到 `Distribute` 阶段才生成；测试直接拿初始局面跑训练动作，等于在非法搜索根上做 rollout。
- **结论**：测试夹具用真实 `run_stage` 推进到 `Train` 阶段再取根（生产搜索的调用点、人头分布已完成），crate 内与跨 crate 端到端夹具同法。

### 桩 rollout 的分数口径必须只依赖动作身份

- **现象**：候选顺序不变性测试失败，同一动作在正序/逆序下均分差 6000。
- **排查**：桩分数用「候选下标 × 1000」当基线，逆序后同一动作的下标变了，分数随之改变——失败是夹具造成的假信号，不是内核问题。
- **结论**：桩函数改由动作身份映射基数，候选下标只用于下标相关断言（如按序取种子）。

### `cargo check` 漏检测试代码

- **现象**：`cargo check -p <crate>` 通过，但 `cargo test` 编译失败（测试模块内的错误）。
- **排查**：`cargo check` 默认只检查 lib / bin target，不含 `#[cfg(test)]` 与 `tests/` 目录。
- **结论**：本地门禁统一用 `cargo check --all-targets` / `cargo clippy --all-targets`（与 CI 一致），避免测试代码漏检到 test 阶段才暴露。

## 2026-10-16

### 离线环境下的依赖添加

- **现象**：`cargo add serde_json` 退出 101，报 `rsproxy.cn:443 over proxy 127.0.0.1` 连接失败。
- **排查**：`cargo add` 需联网更新注册表索引；本地缓存中已有目标 crate 与锁文件解析结果。
- **结论**：加 `--offline` 即可走本地缓存完成添加。另需注意 `cargo add` 无 `--workspace` 标志（`--workspace` 会被当作依赖名），往 `[workspace.dependencies]` 登记需按 cargo 解析出的版本手工补一行，再把成员依赖改为 `workspace = true`。

### 核对依赖是否有新版的可行手段

- **现象**：`cargo info <pkg>` 退出 101，报 `crates-io is replaced with non-remote-registry source registry 'rsproxy-sparse'`。
- **排查**：该子命令只接受 crates-io 源，替换源下需显式 `--registry crates-io`（走直连）。
- **结论**：核对版本改用 `cargo update --dry-run --verbose`——它对「有更新但被版本要求挡住」的包打印 `Unchanged <pkg> vX (available: vY)`；未出现的包即为当前最新。

## 2026-10-15

### rand 依赖版本锚定（0.9.5）

- **现象**：`cargo add rand` 默认解析到 `0.10.3`；核心层接入 `rng.rs` 后 `use rand::RngCore` 编译失败。
- **排查**：rand 0.10 为破坏性改版——`RngCore` 改名 `Rng`（rand_core）且旧名标记 deprecated、扩展 trait `Rng` 改名 `RngExt`、`StdRng` 移除 `Clone`、`OsRng`→`SysRng`、`choose_multiple`→`sample` 等。生态已跟进（`rand_distr 0.6.0` 要求 `rand ^0.10`），故非「无法升级」。
- **结论**：核心层锚定 `rand 0.9.5`（0.9 线最新补丁，与旧仓解析口径一致），后续配套 `rand_distr 0.5.x`。理由：后续阶段要移植的是基于 0.9 API 的存量代码，升 0.10 需大面积改写（含 `StdRng` 不可 Clone 的结构性调整），且 0.10 带 `WeightedIndex` / `Uniform` 行为修复，会引入与旧仓的采样口径偏差，不利于固定种子对照。
- **补充：0.10 的 `StdRng` 无 Clone 等价物**
  - 成因：`StdRng` 底层由 `rand_chacha` 换成 RustCrypto 的 `chacha20` 类型，该类型不实现 `Clone`；rand 对 CSPRNG 也采取「不给 Clone，避免密钥流重复」的立场。仅 `StdRng` 受影响，`Xoshiro*` / `SmallRng` / `ThreadRng` 在 0.10 仍可 Clone。
  - 替代方案均非「克隆当前状态」：`SeedableRng::fork()` 是从当前 RNG 派生新子流且**会推进父流**；`from_seed` / `seed_from_u64` 需自留 seed 才能重建；或换用可 Clone 的 PRNG（数值会变）。
  - 影响面：旧仓把 `rng.clone()` 当「peek 不消耗」与「给子任务独立流」两种惯用法，散落上百处（含 `rng.clone().next_u64()` 探针、`RolloutSeeds::from_rng(&mut rng.clone())`、`eval_rng = rng.clone()`），无一对一替换。
  - 长期方向（若将来升级）：把上述惯用法迁到可 Copy 的自有 RNG（`SplitmixRng`）或 `Xoshiro256PlusPlus`；顺带摆脱 `StdRng` 不保证跨版本可复现的隐患。
- **状态**：已处理（正式更新文档时再补充细节）。
