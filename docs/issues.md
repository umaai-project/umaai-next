# 问题记录

记载复杂问题的解决过程与根因，避免重复踩坑。

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
