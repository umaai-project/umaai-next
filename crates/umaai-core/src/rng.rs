//! 随机数受控原语
//!
//! 提供无状态随机流 [`SplitmixRng`]、类型隔离的三条规则流
//! [`TurnFixedRng`] / [`StrategyRng`] / [`EventRng`]，以及种子派生 [`derive_seed`] 与
//! 流标记 [`StreamTag`]：
//!
//! - 第 N 次随机 = `splitmix64(master + N·GAMMA)`，纯函数，不依赖此前消耗次数；
//! - 流值语义：Clone 即独立实例（counter 各自推进），是克隆局面后随机隔离的基础；
//! - 三条规则流类型不同：任何随机点接错流直接编译不过。

use rand::RngCore;

/// SplitMix64 黄金比例 gamma（标准常数，冻结不可改）
pub const GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

/// SplitMix64 finalizer 混淆常数 A（冻结不可改）
pub const MIX_A: u64 = 0xBF58_476D_1CE4_E5B9;
/// SplitMix64 finalizer 混淆常数 B（冻结不可改）
const MIX_B: u64 = 0x94D0_49BB_1331_11EB;

/// 决策流派生标记（批量对局局号派生用，冻结不可改）
pub const DECISION_TAG: u64 = 0xD3C1_51A8_0000_0001;
/// 策略流派生标记（回合策略流 master 用，冻结不可改）
pub const STRATEGY_TAG: u64 = 0x5374_7261_0000_0002;
/// 探测流派生标记（搜索 rollout 公共随机数骨架，冻结不可改）
pub const PROBE_TAG: u64 = 0x5072_6F62_0000_0003;
/// 事件流派生标记（回合开始事件链，冻结不可改）
pub const EVENT_TAG: u64 = 0x4576_656E_7400_0004;

/// SplitMix64 输出混合（finalizer），全仓库唯一权威哈希实现
///
/// 纯终混合，**不含** `seed += gamma` 那一步——调用方自行决定如何构造输入。
pub fn splitmix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(MIX_A);
    z = (z ^ (z >> 27)).wrapping_mul(MIX_B);
    z ^ (z >> 31)
}

/// 种子派生：各分量异或后单次 [`splitmix64`] 哈希
///
/// 形如 `splitmix64(base ^ i ^ TAG)`，是局号、回合流 master 与 rollout 骨架的统一派生入口。
pub fn derive_seed(base: u64, parts: &[u64]) -> u64 {
    let mut x = base;
    for &p in parts {
        x ^= p;
    }
    splitmix64(x)
}

/// 从父流取 1 个随机字，派生一条一次性的局部随机流
///
/// 用于把「消耗次数随局面变化」的子算法与父流隔离：父流**恒定消耗 1 次**，
/// 子算法内部抽多少次都只推进局部流，改子算法不再位移父流后续随机点。
/// `tag` 必须是冻结常量，且不同用途取不同值——同 tag 同父流值会得到同一条局部流。
///
/// # 已知耦合
///
/// 局部流 master 取自父流在派生点的**当前取值**：只隔离下游、隔离不了上游；
/// 需要与父流消耗次数无关的隔离时，改用 `(rule_master, turn, tag)` 直接派生。
pub fn fork_local_stream(parent: &mut impl RngCore, tag: u64) -> SplitmixRng {
    SplitmixRng::new(derive_seed(parent.next_u64(), &[tag]))
}

/// 流标记：随机流类别（冻结不可改）
///
/// - [`StreamTag::TurnFixed`]：回合固定流（回合内与策略无关的随机点）
/// - [`StreamTag::Strategy`]：策略流（策略动作落地产生的随机）
/// - [`StreamTag::Event`]：事件流（回合开始事件链：解锁判定 / 事件生成 / 事件应用）
/// - [`StreamTag::Probe`]：探测流（搜索 rollout 公共随机数骨架，预留）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamTag {
    /// 回合固定流
    TurnFixed,
    /// 策略流
    Strategy,
    /// 事件流（回合开始事件链）
    Event,
    /// 探测流（搜索预留）
    Probe,
}

impl StreamTag {
    /// 流标记数值（参与种子派生异或，冻结不可改）
    pub const fn tag(self) -> u64 {
        match self {
            Self::TurnFixed => 0,
            Self::Strategy => STRATEGY_TAG,
            Self::Event => EVENT_TAG,
            Self::Probe => PROBE_TAG,
        }
    }
}

/// 无状态随机流
///
/// 第 N 次随机 = `splitmix64(master + N·GAMMA)`，counter 从 0 起每次自增；
/// 与"此前消耗多少次"无关——Clone 即获得独立实例（counter 各自推进）。
///
/// # 用法
/// - 回合开始：`SplitmixRng::new(master)`（counter 归零）
/// - 搜索 rollout：`rng.reset(master_k)` 注入本轮公共骨架
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SplitmixRng {
    /// 流主种子（本流随机序列的确定性来源）
    master: u64,
    /// 本回合内已消费次数（第 N 次随机 = N）
    counter: u64,
}

impl SplitmixRng {
    /// 创建主种子为 `master`、计数为 0 的新流
    pub fn new(master: u64) -> Self {
        Self { master, counter: 0 }
    }

    /// 当前主种子（测试观测用）
    pub fn master(&self) -> u64 {
        self.master
    }

    /// 当前计数（= 下次随机的 N，测试观测用）
    pub fn counter(&self) -> u64 {
        self.counter
    }

    /// 重置为主种子 `master`、计数归零（回合切换 / rollout 注入）
    pub fn reset(&mut self, master: u64) {
        self.master = master;
        self.counter = 0;
    }
}

impl RngCore for SplitmixRng {
    fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    fn next_u64(&mut self) -> u64 {
        let v = splitmix64(self.master.wrapping_add(self.counter.wrapping_mul(GAMMA)));
        self.counter = self.counter.wrapping_add(1);
        v
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        for chunk in dest.chunks_mut(8) {
            let bytes = self.next_u64().to_le_bytes();
            chunk.copy_from_slice(&bytes[..chunk.len()]);
        }
    }
}

/// 回合固定流（类型隔离：与 [`StrategyRng`] 接错直接编译不过）
///
/// 每回合从 `(rule_master, turn)` 派生 master，counter 从 0 计数——与策略完全无关。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnFixedRng(SplitmixRng);

impl TurnFixedRng {
    /// 创建主种子为 `master` 的新回合固定流（counter 归零）
    pub fn new(master: u64) -> Self {
        Self(SplitmixRng::new(master))
    }

    /// 当前主种子（测试观测用）
    pub fn master(&self) -> u64 {
        self.0.master()
    }

    /// 当前计数（= 下次随机的 N，测试观测用）
    pub fn counter(&self) -> u64 {
        self.0.counter()
    }

    /// 重置为主种子 `master`、计数归零（回合切换）
    pub fn reset(&mut self, master: u64) {
        self.0.reset(master);
    }
}

impl RngCore for TurnFixedRng {
    fn next_u32(&mut self) -> u32 {
        self.0.next_u32()
    }

    fn next_u64(&mut self) -> u64 {
        self.0.next_u64()
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.0.fill_bytes(dest);
    }
}

/// 策略流（类型隔离：与 [`TurnFixedRng`] 接错直接编译不过）
///
/// 从 `(rule_master, turn, STRATEGY_TAG)` 派生 master；仅应用真实动作时消耗。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StrategyRng(SplitmixRng);

impl StrategyRng {
    /// 创建主种子为 `master` 的新策略流（counter 归零）
    pub fn new(master: u64) -> Self {
        Self(SplitmixRng::new(master))
    }

    /// 当前主种子（测试观测用）
    pub fn master(&self) -> u64 {
        self.0.master()
    }

    /// 当前计数（= 下次随机的 N，测试观测用）
    pub fn counter(&self) -> u64 {
        self.0.counter()
    }

    /// 重置为主种子 `master`、计数归零（回合切换）
    pub fn reset(&mut self, master: u64) {
        self.0.reset(master);
    }
}

impl RngCore for StrategyRng {
    fn next_u32(&mut self) -> u32 {
        self.0.next_u32()
    }

    fn next_u64(&mut self) -> u64 {
        self.0.next_u64()
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.0.fill_bytes(dest);
    }
}

/// 事件流（三流体系的第三轴）
///
/// 随机点：回合开始事件链（解锁判定、事件生成、事件应用）。
/// 事件**是否触发**依赖事件历史，但其随机本身与策略 / 局面无关——独立成轴后，
/// 事件历史的差异只影响事件流自身，不污染回合固定流与策略流。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventRng(SplitmixRng);

impl EventRng {
    /// 创建主种子为 `master` 的新事件流（counter 归零）
    pub fn new(master: u64) -> Self {
        Self(SplitmixRng::new(master))
    }

    /// 当前主种子（测试观测用）
    pub fn master(&self) -> u64 {
        self.0.master()
    }

    /// 当前计数（= 下次随机的 N，测试观测用）
    pub fn counter(&self) -> u64 {
        self.0.counter()
    }

    /// 重置为主种子 `master`、计数归零（回合切换）
    pub fn reset(&mut self, master: u64) {
        self.0.reset(master);
    }
}

impl RngCore for EventRng {
    fn next_u32(&mut self) -> u32 {
        self.0.next_u32()
    }

    fn next_u64(&mut self) -> u64 {
        self.0.next_u64()
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.0.fill_bytes(dest);
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    /// 层1：同一 (master, n) 两次计算同值（确定性）
    #[test]
    fn test_deterministic() {
        let mut a = SplitmixRng::new(42);
        let mut b = SplitmixRng::new(42);
        for n in 0..8 {
            assert_eq!(a.next_u64(), b.next_u64(), "master=42 第 {n} 次取值应相同");
        }
    }

    /// 层1：不同 master 序列不同（无相关性）
    #[test]
    fn test_masters_differ() {
        let mut a = SplitmixRng::new(1);
        let mut b = SplitmixRng::new(2);
        let sa: Vec<u64> = (0..8).map(|_| a.next_u64()).collect();
        let sb: Vec<u64> = (0..8).map(|_| b.next_u64()).collect();
        let same = sa.iter().zip(&sb).filter(|(x, y)| x == y).count();
        assert_eq!(same, 0, "不同 master 的前 8 值不应有位相同");
    }

    /// 层1：消费 k 次后从第 k+1 次继续（无状态）
    #[test]
    fn test_counter_continues() {
        let mut rng = SplitmixRng::new(7);
        let _first = rng.next_u64();
        let second = rng.next_u64();
        let mut skip = SplitmixRng::new(7);
        let _ = skip.next_u64();
        let after_skip = skip.next_u64();
        assert_eq!(second, after_skip, "第 2 次取值应与消费路径无关");
        assert_eq!(skip.counter(), 2);
    }

    /// 层1：Clone 值语义——克隆后各自推进互不影响（局面克隆隔离原子验证）
    #[test]
    fn test_clone_independent() {
        let mut a = SplitmixRng::new(99);
        let mut b = a;
        assert_eq!(a.next_u64(), b.next_u64());
        assert_eq!(a.next_u64(), b.next_u64());
        assert_eq!(a.counter(), 2);
        assert_eq!(b.counter(), 2);
    }

    /// 层1：不同 StreamTag 派生序列不重叠（流间隔离）
    #[test]
    fn test_stream_tags_isolated() {
        let base = 12345u64;
        let mut fixed = SplitmixRng::new(derive_seed(base, &[7]));
        let mut strategy = SplitmixRng::new(derive_seed(base, &[7, StreamTag::Strategy.tag()]));
        let sf: Vec<u64> = (0..8).map(|_| fixed.next_u64()).collect();
        let ss: Vec<u64> = (0..8).map(|_| strategy.next_u64()).collect();
        let overlap = sf.iter().filter(|x| ss.contains(x)).count();
        assert_eq!(overlap, 0, "不同 StreamTag 的序列不应重叠");
    }

    /// 层1：加法派生防撞车——stream(master,n) != stream(master^k, n^k)
    #[test]
    fn test_additive_no_xor_collision() {
        // XOR 派生下 stream(A, n) == stream(A^k, n^k)；加法派生应无此性质
        let mut a = SplitmixRng::new(0);
        let mut b = SplitmixRng::new(1);
        let _ = b.next_u64(); // b 消费 1 次 → 若 XOR 派生，a 第 0 次 == b 第 1 次
        assert_ne!(a.next_u64(), b.next_u64(), "加法派生不应出现 (master,n) 撞车");
    }

    /// 层1：类型隔离流可正常消费，包装层不改变底层序列，reset 归零
    #[test]
    fn test_typed_streams_work() {
        let mut fixed = TurnFixedRng::new(42);
        let mut strategy = StrategyRng::new(42);
        let f0 = fixed.next_u64();
        let s0 = strategy.next_u64();
        // 同为 SplitmixRng 包装：同 master 首值一致（隔离是编译期性质，非取值差异）
        assert_eq!(f0, s0);
        assert_eq!(f0, SplitmixRng::new(42).next_u64());
        assert_eq!(fixed.counter(), 1);
        // reset 后从 master 重新起算
        fixed.reset(7);
        assert_eq!(fixed.counter(), 0);
        assert_eq!(fixed.next_u64(), SplitmixRng::new(7).next_u64());
    }

    /// 层1：事件流可正常消费，reset 与同 master 新建一致
    #[test]
    fn test_event_rng() {
        let mut ev = EventRng::new(2024);
        assert_eq!(ev.counter(), 0);
        let first = ev.next_u64();
        assert_eq!(first, SplitmixRng::new(2024).next_u64());
        ev.reset(2024);
        assert_eq!(ev.next_u64(), first);
    }

    /// 层1：fill_bytes 按小端逐 8 字节铺满，且与 next_u64 序列一致
    #[test]
    fn test_fill_bytes() {
        let mut a = SplitmixRng::new(5);
        let mut b = SplitmixRng::new(5);
        let mut buf = [0u8; 17];
        a.fill_bytes(&mut buf);
        let expect: Vec<u8> = (0..3)
            .flat_map(|_| b.next_u64().to_le_bytes())
            .take(buf.len())
            .collect();
        assert_eq!(buf.to_vec(), expect);
        // 长度 8 的整数倍时也应铺满，且继续沿 next_u64 序列
        let mut full = [0u8; 16];
        a.fill_bytes(&mut full);
        let expect_full: Vec<u8> = (0..2).flat_map(|_| b.next_u64().to_le_bytes()).collect();
        assert_eq!(full.to_vec(), expect_full);
    }

    /// 局部流派生：父流恰好推进 1 次，不同 tag 得到互不相同的序列
    ///
    /// 层 1 原语自测：验证局部流本身是否确定、是否按 tag 分离，以及父流的消耗次数性质。
    #[test]
    fn test_fork_local_stream() {
        // 本地 tag 常量（原用途的分身局部流已移至剧本层，值沿用冻结常量）
        const TAG_A: u64 = 0x436C_6F6E_0000_0005;
        const TAG_B: u64 = 0x436C_6F6E_0000_0006;

        // ① 父流恰好推进 1 次
        let mut parent = SplitmixRng::new(0x1234_5678);
        let before = parent.counter();
        let mut local = fork_local_stream(&mut parent, TAG_A);
        println!("父流 counter: {before} -> {}", parent.counter());
        assert_eq!(parent.counter() - before, 1);

        // ② 局部流独立计数，抽多少次都不回流父流
        for _ in 0..20 {
            let _ = local.next_u64();
        }
        println!("局部流抽 20 次后，父流 counter 仍为 {}", parent.counter());
        assert_eq!(parent.counter() - before, 1);
        assert_eq!(local.counter(), 20);

        // ③ 确定性：同一父流状态 + 同一 tag => 同一局部流
        let mut p1 = SplitmixRng::new(0xABCD);
        let mut p2 = SplitmixRng::new(0xABCD);
        let mut l1 = fork_local_stream(&mut p1, TAG_A);
        let mut l2 = fork_local_stream(&mut p2, TAG_A);
        let (a, b) = (l1.next_u64(), l2.next_u64());
        println!("确定性: {a:#018x} vs {b:#018x}");
        assert_eq!(a, b);

        // ④ tag 分离：同一父流取值配不同 tag => 不同序列
        let mut p3 = SplitmixRng::new(0xABCD);
        let mut l3 = fork_local_stream(&mut p3, TAG_B);
        let d = l3.next_u64();
        println!("tag 分离: TAG_A={a:#018x} TAG_B={d:#018x}");
        assert_ne!(a, d);

        // ⑤ 父流位置分离：父流 counter 不同 => 局部流不同
        //    这也是本原语的已知耦合：局部流依赖父流在派生点的取值，
        //    因此它只隔离「下游」，隔离不了「上游此前消耗了几次」。
        let mut p4 = SplitmixRng::new(0xABCD);
        let _ = p4.next_u64();
        let mut l4 = fork_local_stream(&mut p4, TAG_A);
        let e = l4.next_u64();
        println!("父流位置分离: counter=0 时 {a:#018x}，counter=1 时 {e:#018x}");
        assert_ne!(a, e);
    }
}
