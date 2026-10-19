# 自动更新方案（P6c′ 定稿）

> 2026-10-19 用户拍板。本文是「自动更新」的单一事实源，取代 [`miko_next_plan.md`](miko_next_plan.md) §7 的方向性表述；
> 阶段归属见同文件 §9 的 **P6c′**，未决项见 [`../notes/open_items.md`](../notes/open_items.md)。
>
> 一句话：**先把「更新能力」做成零引擎依赖的公共库 `umaai-update`（清单协议 + 传输 + 校验 + 安装），
> 入口接线 / 自更新 / 发布侧改造落方案待开工**——库先立住，谁做入口都只是接一根线。

---

## 1. 形态（已拍板）

| 项 | 定稿 |
|---|---|
| 落点 | 新增 crate **`umaai-update`**（workspace 第 7 个成员），**零引擎依赖**、不在引擎依赖链上；验收条款由「6 个 crate」改为「7 个 crate」 |
| 本轮范围 | **只交付库**；入口接线（`umaai update`）、自更新、发布侧改造只落方案（§6–§8） |
| 更新源 | **编译期常量**，由产品 crate 给出（`Source::new("<基址>")`）；库不内置域名、不读配置文件、不提供改源开关（CLI / 环境变量都不加） |
| 频道 | `ai_data`（AI 数据）、`ura_data`（小黑板数据）**分别更新**：各自清单、各自版本、各自成败，单频道失败不阻断其他频道 |
| 默认层配置 | `ai_config.toml`（数据根内的默认层）**随 `ai_data` 一起更新**；宿主目录的覆盖层不在数据根内，不受影响 |
| 小黑板目标 | 统一到**发布目录的 `.portable`**（宿主注入；`%LOCALAPPDATA%` 形态废弃，不做平台探测） |
| 传输 | 只有两种：**HTTP**（`HttpTransport`，ureq + rustls，可 `default-features = false` 剪掉）与**本地测试模式**（`TestTransport`，内存假源）；不做本地目录源——宿主真要离线导入就自己 `impl Transport` |
| 清单协议 | 新 JSON **`update.json`**（逐文件 sha256）；不复用旧 `version.toml` |
| 判定 | **无状态**：清单登记的 sha256 与本地逐文件比对（缺失 / 大小不符 / 哈希不同 = 待更新），不写本地版本文件 |
| 安装口径 | 全部下载到 `<工作目录>/.umaai-update/<频道>/` → **全部校验通过** → 备份旧文件并替换（任一失败回滚）→ **成功后删除临时目录**；失败保留临时目录待排查 |
| 安全档 | HTTPS + sha256；**不做**清单签名、**不做**自更新前 Authenticode 验签（延后，见 §10） |
| 配置层 | 不加 `[update]` 段 |
| 启动检查 | 主线 CLI **不默认联网**；库提供 `check`，是否在启动时检查由真实产品入口决定（§7 原「启动时轻量检查 + 提示」按此收口） |
| 剧本 AI 本体 | 由**剧本仓**在自己的 bin 里声明频道（数据目录 / 自身 exe）；本仓不内置具体剧本条目，也不反向依赖剧本仓 |

---

## 2. 依据：决定设计的事实

### 2.1 旧形态（现役 `uma-autoupdate`，独立 GUI 仓）

| 项 | 事实 | 新方案怎么处理 |
|---|---|---|
| 形态 | Rust + iced GUI，Windows-only，与产品 exe 并列 | 放弃 GUI（plan §0 决策 10），能力沉到库 |
| 清单 | `version.toml` 分段（`auto_update` / `ura_data` / `ai_data`），`package*` / `ver` 声明了但从不读 | 新协议 `update.json`，字段只用得上才留 |
| 更新源 | `https://cdn2.viktorlab.cn/uma` 两处硬编码 + 固定 `Referer` | 源常量归产品 crate，请求头随 `Source` 携带 |
| 判定 | 只比 `filelist[0]` 的 sha1；`date` / `ver` / `index` 都不参与（`index` 仅 UI 排序） | **逐文件** sha256，全表覆盖 |
| 校验 | `verify()` 恒返回 `true`，哈希不符只 `warn!` | 校验不过即失败，且**在动目标目录之前** |
| 下载 | 逐文件写到 CWD `<file>.autoupdate`，失败文件不会重下，无超时设置 | 下载到工作目录下的临时目录；超时可配；失败即整频道失败 |
| 安装 | 全部下完 copy 覆盖 | 备份 + 替换 + 回滚；成功后清临时目录 |
| 小黑板目标 | 只认 `./.portable`，否则 `bail!`（与 URA 自身的 `%LOCALAPPDATA%` 回退不一致） | 统一 `.portable`（发布目录内），去掉平台探测 |
| 自更新 | rename → `.old`、拉起新 exe、`exit(0)`、下次启动清 `.old`（exe 名硬编码） | 机制沿用，参数化（§7） |
| 分发侧 | `make_version.py` 只刷 `date` + 首个文件 sha1；`filelist` 手工维护；`coscmd upload -rs . /uma/` | 清单由**同一份 schema 生成**（库的 `manifest::build`），消除手工漂移 |

### 2.2 消费面

- **AI 数据**（`ai_data` 频道）：数据根 5 张表（`umaDB` / `cardDB` / `text_data_dict` / `events` / `constants`）+ `manifest.json`（`schema_version` 是数据/程序一致性的唯一门控，`umaai-core::data::manifest::SCHEMA_VERSION`）+ `ai_config.toml` 默认层。旧分发仓 `ai_data/` 只有 3 个 JSON（缺 `events.json` / `constants.json` / `manifest.json` / 默认层配置），旧仓归档文档已记录该缺口。
- **小黑板数据**（`ura_data` 频道）：11 个 `.br` 是**纯 brotli 压缩 JSON**（无容器、无内部版本），消费方只有小黑板本体（`Database.cs` 的 9 个文件名常量），AI 侧不读。其中 `events.br` / `factor_effects.br` 在工作区找不到读取方，`success_events.br` 未入旧清单——**说明旧 filelist 是手工维护的产物**。小黑板自身还有一条 GitHub Assets 更新线，本频道是它的镜像。
- **剧本 AI 本体**：旧形态**完全没有版本通道**（`version.toml` 三处副本都不描述 `umaai.exe`；crate 版本 `0.14.2`、发布包名 `0.21`、更新器 `0.2.0/0.2.1` 三套号互不相干；无 CI、无打包脚本）。纳入更新是**补缺口**，不是可选增强。

### 2.3 新仓约束（沿用，不改）

- 数据根路径宿主注入、配置两份、`manifest.schema_version` 门控、不做热更新（plan §5）；
- 引擎不碰 IO / 网络：更新是**宿主职责**，故库不能进 `umaai-core`（依赖白名单）也不能进引擎稳定面（`umaai-core` / `umasim` / `umaai`）；
- 入口当前无子命令（P5′ 定稿「等 P6′ 有 update / 探针再引入」）；
- 校验与展示分离：库不打印任何东西，进度经回调交宿主。

---

## 3. 库设计（P6c′-a，已落地）

### 3.1 模块与 API

```
crates/umaai-update/src/
    lib.rs        公开面与用法示例（doctest）
    error.rs      UpdateError（Io / Http / Manifest / Hash / Schema / SchemaMissing / Rollback）
    hash.rs       sha256_hex / sha256_file（64 KiB 分块）/ is_sha256_hex
    manifest.rs   Manifest / FileEntry / PROTOCOL_VERSION / validate_path / build（发布侧）
    channel.rs    Source（源常量 + 请求头）/ Channel（本地声明）/ Target（安装目标）
    transport.rs  Transport trait + HttpTransport（特性 http，默认开）
    testing.rs    TestTransport（内存假源：serve / fail / requests）
    updater.rs    check / update / Options / Progress / 报告类型
```

公开入口（`crates/umaai-update/src/lib.rs`）：

```rust
// 源与频道
Source::new(base)?.with_header("Referer", site)
Channel::data(id, name, dir)?.expect_schema(SCHEMA_VERSION)   // id 只允许 [A-Za-z0-9_-]
Target::Dir(PathBuf)                                          // #[non_exhaustive]，自更新频道启用时新增变体

// 检查（只读）与更新
check(&source, &channels, &transport) -> CheckReport          // 逐频道 UpdateResult<CheckOutcome>
update(&source, &channels, &transport, &Options::new(work_dir), &mut |Progress| {}) -> UpdateReport

// 发布侧
manifest::build(dir, BuildOptions::new("2026.10.19").with_schema_version(1))? -> Manifest
Manifest::to_json()?                                          // 带缩进 + 换行
```

- 报告是**逐频道 `Result`**：`CheckReport { channels }` / `UpdateReport { channels }`，`failed()` / `pending()` / `updated()` / `all_ok()` 汇总，单频道失败不影响其他频道。
- 库**零引擎依赖**，也不碰终端；除 workspace 既有的 `serde` / `serde_json` / `thiserror` 外，只新增 `sha2` 与（可选，`http` 特性）`ureq`。

### 3.2 清单协议（`<基址>/<频道 id>/update.json`）

```json
{
  "protocol": 1,
  "version": "2026.10.19",
  "date": "2026-10-19 12:00:00",
  "schema_version": 1,
  "files": [ {"path": "umaDB.json", "size": 139788, "sha256": "<64 位小写十六进制>"} ]
}
```

纪律四条：

1. `protocol` 只认自己支持的值（不认识即拒绝并提示升级程序），保证老客户端不会误读新协议；
2. 未知字段**宽容忽略**（协议前向演进），缺必需字段即报错；
3. `files[].path` 必须是安全相对路径：拒绝绝对路径、`..`、`.`、空段、反斜杠、盘符冒号；路径重复即报错；
4. 清单**不携带安装路径**——装到哪由本地 `Channel` 决定（源被攻破也写不出目标目录之外）。

`manifest::build` 递归扫描发布目录、跳过 `update.json` 自身、按路径字典序排序，故**同一目录两次生成结果逐位一致**；符号链接等非普通文件直接报错。

### 3.3 判定与安装口径

```
check:  拉清单 → 校验 → schema 门控 → 逐文件（大小 + sha256）比对本地 → pending
update: 拉清单 → 同上算 pending → 全部下载到 <工作目录>/.umaai-update/<频道>/files/
        → 全部 sha256 校验通过 → 逐文件（备份旧文件 → 暂存复制 → rename 就位）
        → 任一失败：按备份回滚已替换项（回滚不完整则报 Rollback）
        → 全部成功：删除 <频道> 临时目录，空了再删临时目录根
```

- 落文件用「同目录暂存名 + rename」，避免目标被写成半截；
- 临时目录**只清自己那一层**，不碰工作目录里别的东西；
- 失败时临时目录保留（下次运行先清理同名目录），便于排查；
- 数据 schema 不一致（门控失败）在**下载之前**拦截，一个字节都不动。

### 3.4 传输

- `HttpTransport`：ureq 3.4 + rustls，阻塞式，连接 5s / 整体 120s 默认超时（`with_timeouts` 可收紧），UA = `umaai-update/<版本>`；非 2xx 报错，不把错误页当内容。
- `TestTransport`：内存假源（`serve` / `serve_str` / `fail` / `requests`），库自身集成测试、宿主自测、离线演练都用它。
- 宿主自备传输：实现 `Transport` 一个方法即可（集成测试里有示例）。

### 3.5 发布侧复用同一 schema

`manifest::build` 与客户端 `Manifest::parse` 是同一份类型定义：发布侧生成、客户端解析，不存在「两边各写一遍」的漂移面——旧仓 `make_version.py` 手工 `filelist` 的坑由此关闭。

---

## 4. 拍板记录（2026-10-19）

| # | 决策 | 结果 |
|---|---|---|
| D1 | 库落点 | **新 crate `umaai-update`**（第 7 个成员，改验收条款）；不放 `umaai-cli`（样例面不承诺）、不放引擎链 |
| D2 | 源可覆盖性 | **纯编译期常量**：库不提供 CLI / 环境变量改源；测试与离线走 `Transport` 注入 |
| D3 | 本轮范围 | **C：只做库**；入口接线、自更新、发布侧落方案文档 |
| D4 | 启动检查 | **主线 CLI 不默认联网**；`check` 交产品入口自己决定 |
| D5 | 清单协议 | **新 JSON `update.json`**（逐文件 sha256、无状态判定） |
| D6 | 安装口径 | **全下到工作目录下的临时目录 → 校验 → 替换 → 成功后删除**（失败保留）；备份 + 回滚照做 |
| D7 | 频道内容 | `ai_config.toml` 默认层**随 `ai_data` 更新**（覆盖层不受影响） |
| D8 | 配置段 | **不加 `[update]`**（开关留给入口层，等真实产品要持久开关再按「带默认实现、非破坏」加） |
| D9 | 安全档 | **HTTPS + sha256**；不做清单签名（B）、不做自更新前 Authenticode 验签（C） |
| D10 | 剧本频道 | 库留扩展（`Channel` 由宿主传入），剧本仓自己声明；本仓不内置 |
| D11 | 小黑板文件集 | 以 **URA 实际读取的 9 个文件**为准（`events_male` / `events_female` / `names` / `skill_data` / `talent_skill_sets` / `factor_ids` / `skill_upgrade_speciality` / `wins_saddle` / `succession_relation`）；`events.br` / `factor_effects.br` / `success_events.br` 不纳入 |
| — | 小黑板目标 | 统一 **发布目录的 `.portable`**，`%LOCALAPPDATA%` 形态废弃 |
| — | 传输形态 | 只保留 **HTTP + 本地测试模式**，不做本地目录模式 |

---

## 5. 阶段与交付物

| 阶段 | 交付 | 状态 |
|---|---|---|
| **P6c′-a** | 公共库 `umaai-update`：清单协议（生成 / 解析 / 校验）、传输（HTTP + 测试）、`check` / `update`、备份回滚、临时目录口径、L1 20 条 + L2 8 条 | ✅ 已落地（2026-10-19） |
| **P6c′-b** | 入口接线：`umaai-cli` 引入子命令，`update` 用法与呈现（§6） | 待开工 |
| **P6c′-c** | 自更新频道：`Target::SelfExe` + Windows 替换时序 + 防循环（§7） | 待开工 |
| **P6c′-d** | 发布侧：CDN 目录与 `update.json` 生成流程、`ai_data` 补齐、发布步骤文档（§8） | 待开工 |
| — | 剧本频道 | 剧本仓侧；本仓只交付库与接入说明 |

---

## 6. 入口接线设计（P6c′-b，待开工）

- `umaai-cli` **首次引入子命令**（P5′ 已预留）：`umaai update`
  - `--check` 只报告（人读表 + 机读单行，沿用工具面口径）；
  - `--channel ai_data,ura_data` 选频道（缺省全部内置频道）；
  - `--yes` 免确认；无 `--yes` 时先列可更新项再问一次；
  - `--from-dir <目录>` 离线演练（内部用自备传输，不是库的本地目录模式）；
  - `--data-root` 复用现有开关；新增 `--ura-root`（缺省按发布目录约定取 `<发布目录>/.portable`，显式可覆盖）；
  - `--no-update` 语义：入口默认不联网，本开关留给真实产品的启动检查。
- 频道声明（代码常量）：
  - `ai_data` → `--data-root`，`expect_schema(umaai_core::data::manifest::SCHEMA_VERSION)`；
  - `ura_data` → `--ura-root`，无 schema 门控。
- 呈现复用 `umaai-cli` 的 `render` 件；进度经 `update` 的回调打印。
- 退出码：全部成功 0；有频道失败 1（报告逐条点名）。

## 7. 自更新设计（P6c′-c，待开工）

- `Target` 增 `SelfExe` 变体（`#[non_exhaustive]`，非破坏）；频道 id 沿用 `auto_update`（与旧 CDN 目录同名）。
- Windows 时序（沿用旧仓验证过的做法）：下载新 exe → 校验 → 当前 exe `rename` 为 `.old` → 新 exe 就位 → 拉起新进程（带「已更新」标记，防更新循环）→ 本进程退出；下次启动清理 `.old`。
- 非 Windows：rename 语义可用（macOS 需注意代码签名失效，记为风险）。
- **版本口径**：`update.json.version` 只作显示（建议日期码，如 `20261019`），判定仍走哈希——旧仓 `date` 字符串比较无调用点、`ver` 从不读，不必重蹈。
- **剧本 AI 本体**：剧本仓 bin 用同一频道机制声明自己的 exe 与源；本仓只提供能力与接入说明。
- 验签（D9）：暂不做；将来加「安装前校验 Authenticode 指纹」（现役发行包已有自签 `CN=UmaAI` + 指纹白名单脚本）属非破坏增强。

## 8. 发布侧设计（P6c′-d，待开工）

- **CDN 目录沿用旧 key**（`ai_data/`、`ura_data/`、`auto_update/`），每个目录新增 `update.json`；旧客户端只读根目录 `version.toml`，**新旧客户端可并存过渡**。
- **生成**：发布侧跑 `manifest::build`（或后续做一个薄工具子命令）产出 `update.json`；`filelist` 手工维护的形态作废。
- **`ai_data` 内容补齐**：5 张表 + `manifest.json` + `ai_config.toml` 默认层（`scenario_*.json` 属旧形态遗留，不进本频道，剧本数据归剧本仓）。
- **`ura_data` 内容**：按 D11 的 9 个文件（不再维护无读取方文件）。
- **发布步骤文档**：现状是零自动化（`.github/workflows` 空目录、无打包 / 上传 / 签名脚本），至少要写出「构建 → 签名 → 生成清单 → 上传 → 复核」的手工步骤；签名仍用现役自签 Authenticode + 指纹白名单。

---

## 9. 守门与验收

- **L1（内联，20 条）**：协议版本 / 字段 / 路径安全 / 哈希格式 / 重复路径、生成清单的确定性与清单自身跳过、符号链接拒绝、URL 拼装与百分号编码、频道 id 白名单、已知 sha256 向量与跨块文件哈希、临时目录与暂存名、本地一致性判定、schema 门控三态。
- **L2（`crates/umaai-update/tests/update_flow.rs`，8 条）**：首次全量更新与临时目录清理、只拉变化文件（请求清单留证）、下载损坏不动目标目录、频道互不阻断、schema 门控不落盘、替换失败回滚、进度回调起止、宿主自备传输。
- **效果**：`cargo test -p umaai-update --release` 全绿；`clippy -D warnings`（含 `--no-default-features`，守 HTTP 可剪）无 warning；`fmt` 干净。
- CI 的 workspace 档位自动覆盖新 crate；**不进 L4 基线**（属编排 / IO 面，不是模拟数值面）。
- 网络不入测试：真实源冒烟只在手工档（需要时自行 `HttpTransport` + 真实基址）。

## 10. 不做的事（减法）

1. 不做独立 GUI 更新器（plan §0 决策 10）；
2. 不做进程内热更新（plan §5）；
3. 不做本地目录传输模式（宿主真需要就自己 `impl Transport`）；
4. 不做清单签名、不做安装前 Authenticode 验签（D9 延后）；
5. 不做整包（zip）与增量二进制 diff——逐文件天然增量；
6. 不做断点续传、多源镜像、内网自动回退；
7. 不写本地版本状态文件（无状态判定已能自愈）；
8. 不加 `[update]` 配置段；
9. 不做静默自动应用（提示 + 显式更新）；
10. 本仓不内置剧本频道条目。

## 11. 风险与待决

| # | 项 | 状态 |
|---|---|---|
| 1 | 发布侧没有任何自动化与流程文档——库做完也可能没有可消费的源 | P6c′-d 的前置工作 |
| 2 | 小黑板目标目录改为统一 `.portable`：安装版（`%LOCALAPPDATA%`）用户需要显式指路径 | 已拍板，入口层给 `--ura-root` 兜底 |
| 3 | Windows 自更新被杀软 / 占用干扰；macOS 替换后签名失效 | P6c′-c 开工时验证 |
| 4 | 协议演进：新字段客户端宽容、`protocol` 升级拒绝并对齐发布侧 | 已在协议里留位 |
| 5 | 基线 `meta.data_schema_version` **只写不校验**：数据更新后基线可能悄悄过期 | 记入 `open_items`，与 P6c′ 独立 |
| 6 | 库的公开面尚无真实消费者（入口未接） | P6c′-b 落地时按真实用法复核 API |
