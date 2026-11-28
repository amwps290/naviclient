# 本地优先与私有存储演进规划（Local-First & Private Storage Roadmap）

> 创建日期：2026-11-28  
> 适用范围：Navidrome Client（Rust + GPUI 桌面端）  
> 目标定位：从「Navidrome / Bilibili 多源客户端」演进为 **本地优先（Local-First）、原生支持多种私有存储、用户完全掌握个人音乐资产** 的高性能桌面音乐播放器。

---

## 1. 当前基线与核心缺口

截至 commit `f8fa442`，项目已完成播放核心、Gapless、10 段均衡器、ReplayGain、逐字/双语歌词及时间校准、Vinyl Salon 全屏、迷你胶囊播放器、系统托盘/媒体控制、内存诊断面板以及 `MusicSource`（OpenSubsonic / Bilibili）抽象。

对照「**本地优先 + 私有存储**」的目标定位，当前存在三类关键缺口：

1. **缺少一等本地音源与通用私有存储**：`ProviderKind` 仅有 `OpenSubsonic` 与 `Bilibili`，尚不支持直接扫描本地音乐文件夹，也未接入 WebDAV / Alist / Jellyfin 等常见私有存储形态。
2. **离线能力停留在被动缓存层**：`StreamCache` 目前是 4 GiB 被动 LRU 缓存，曲库元数据强依赖服务端实时在线，无法主动“离线保留（Pin）”专辑/歌单，也无法在断网或 NAS 休眠时完整浏览、搜索并播放已离线曲目。
3. **个人音乐资产与高频听歌体验尚不完整**：缺少跨源本地歌单（`.m3u8` 导出/导入）、本地隐私听歌统计、一键配置/偏好备份迁移，以及桌面悬浮歌词、歌曲表多列排序、睡眠定时器、Crossfade/倍速播放与文件夹浏览等高频能力。

---

## 2. 总体演进路线图与分阶段优先级

遵循「**先夯实本地存储与离线底座 → 再扩展私有存储接入 → 再沉淀个人资产 → 最后补齐高频体验**」的顺序，拆分为 4 个阶段、12 个可独立交付的模块：

| 阶段 | 编号 | 模块 | 优先级 | 依赖 |
| :--- | :--- | :--- | :--- | :--- |
| **Phase 1：本地优先底座** | LP-01 | 本地音乐文件夹音源（`ProviderKind::LocalFolder`） | P0 | 无（直接复用 `MusicSource`） |
| | LP-02 | 主动离线下载（Pin）与离线可浏览/播放模式 | P0 | 无 |
| **Phase 2：私有存储扩展** | LP-03 | WebDAV / Alist 私有存储音源（`ProviderKind::WebDav`） | P1 | 复用 LP-01 索引器与标签解析 |
| | LP-04 | 文件夹目录视图（Folder Browse） | P1 | OpenSubsonic 原生接口 + LP-01 / LP-03 |
| **Phase 3：个人音乐资产** | LP-05 | 跨源本地歌单与 `.m3u8` 导入/导出 | P1 | 建议在 LP-01 后，也可独立先行 |
| | LP-06 | 本地隐私听歌统计与回顾（Listening Insights） | P2 | 复用现有 `ListeningHistory` |
| | LP-07 | 个人配置、歌词偏好与歌单一键备份/迁移 | P2 | 无 |
| **Phase 4：高频体验补齐** | LP-08 | 桌面悬浮歌词窗口（Desktop Overlay Lyrics） | P1 | 复用现有 `mini_window` 与 `word_lyrics` |
| | LP-09 | 歌曲列表多列排序与局部过滤 | P2 | 无 |
| | LP-10 | 睡眠定时器（Sleep Timer） | P2 | 无 |
| | LP-11 | 切歌淡入淡出（Crossfade）与倍速播放 | P2 | Audio Worker 管线扩展 |
| | LP-12 | 单曲另存到本地 & 本地库标签/封面轻编辑 | P3 | LP-01 / LP-02 |

---

## 3. 模块详细设计规格

---

### LP-01：本地音乐文件夹音源（`ProviderKind::LocalFolder`）

#### 目标
让用户无需搭建任何服务器，直接将一个或多个本地目录（含外置硬盘 / 挂载的 SMB 目录）添加为标准音乐源，享有与 OpenSubsonic 一致的专辑/艺术家/流派/搜索/播放/歌词体验。

#### 核心架构与数据模型
1. **Provider 扩展（`src/models/source.rs`、`src/source.rs`）**
   - `ProviderKind` 新增 `LocalFolder`。
   - `SourceProfile` 新增：
     ```rust
     LocalFolder {
         id: String,
         name: String,
         roots: Vec<PathBuf>,
         #[serde(default = "default_true")]
         watch_changes: bool,
     }
     ```
   - `SourceCapabilities`：`library: true`, `search: true`, `favorite_read/write: true`（本地收藏持久化），`playlist_read/write: true`（接入 LP-05），`lyrics: true`（支持同目录 `.lrc` 与内嵌标签歌词），`scrobble: false`, `transcoding: false`。
   - `PlaybackResource` 新增 `LocalFile { path: PathBuf, content_length: Option<u64> }` 分支，Audio Worker 直接以 `File` 打开并支持零开销精确 Seek，不再走 HTTP 与 `StreamCache`。

2. **元数据扫描与本地索引（新模块 `src/source/local/`）**
   - 依赖选型：使用 `lofty` 解析 FLAC / MP3 / M4A (ALAC/AAC) / OGG / OPUS / WAV 标签（Title、Artist、AlbumArtist、Album、Track、Disc、Year、Genre、Duration、Bitrate、SampleRate、ReplayGain、内嵌封面、内嵌歌词 USLT/SYLT）。
   - 稳定 ID 生成：以「根目录索引 + 规范化相对路径」的哈希作为 `Song.id`，保证重扫或重启后队列、收藏、歌词偏好与播放位置不失效；Album ID 由 `(AlbumArtist || Artist, Album)` 派生，Artist ID 由规范化 `Artist` 派生。
   - **持久化索引**：在 `config_dir()/sources/<cache_namespace>/library-index.bin`（或 SQLite `rusqlite`）保存 `file_path -> (mtime, size, Song, cover_hash)`：
     - 启动时先秒级加载持久化索引立即可浏览；
     - 后台执行增量扫描：比对 `(mtime, size)`，仅重新解析新增/变更文件并清理已删除条目；
     - 扫描进度通过 `Msg` 上报给 UI（标题栏或设置页展示“正在扫描 320 / 1840”并支持取消）。
   - **封面提取策略**：按 `cover_hash` 去重缓存到磁盘，优先读取同目录 `cover.{jpg,png}` / `folder.{jpg,png}`，其次提取音频文件内嵌图；写入前通过现有 `normalize_cover_bytes` 限制长边 `≤ 512px`，避免高分辨率内嵌图撑爆内存。
   - **本地歌词读取**：播放时优先查找同目录同名 `.lrc` / `.txt`（UTF-8 / GBK 自动探测），其次读取音频内嵌歌词，仍未命中时无缝回落到现有在线歌词服务。

3. **文件监听与错误隔离**
   - 可选引入 `notify` 监听根目录文件变更（防抖 2 秒后触发局部增量更新）。
   - 单文件损坏、权限不足或符号链接循环不得中断整库扫描，汇总到可查看的扫描告警列表。

4. **验收标准**
   - 10,000 首 FLAC/MP3 曲库二次启动 < 200 ms 可浏览；
   - 增删改文件后增量重扫只处理变更项；
   - 缺失标签的文件回退使用文件名与上级目录名展示；
   - 卸载/拔出外置盘时给出清晰离线提示而不误清空历史收藏。

---

### LP-02：主动离线下载（Pin）与离线可浏览/播放模式

#### 目标
把现有被动 LRU 音频缓存升级为**可控离线库**：用户可将专辑、歌单或单曲标记为「离线保留」，断网或家里 NAS 不可达时客户端仍可正常浏览、搜索并播放所有离线内容。

#### 核心架构与数据模型
1. **缓存分层（`src/audio/stream_cache.rs` + 新模块 `src/offline/`）**
   - 将现有缓存目录拆分为两类策略：
     - `cache/`：继续作为自动 LRU 临时音频缓存（受 `audio_cache_limit_mb` 限制）；
     - `offline/`：受保护的持久离线目录（**永远不被自动 LRU 清理**），由独立的 `offline-manifest.json` 维护清单（`MediaKey -> OfflineEntry { song_snapshot, cover_key, lyrics_cached, audio_path, quality, bytes, pinned_at, status }`）。
   - **去重与零拷贝迁移**：若用户 Pin 一首已在 `cache/` 完整缓存的歌曲，直接原子 `rename`（跨卷则复制后删除）进 `offline/`，无需重复下载。
   - **播放优先级**：`StreamCache::open_with_policy` 优先检查 `offline/`（命中即零网络播放），未命中再查 `cache/`，最后走网络流。

2. **后台下载调度器（`OfflineManager`）**
   - 并发限流（默认 2 并发，避免抢占当前在线播放带宽；支持暂停/继续/取消与断点续传 `.part`）；
   - 下载单曲时同步拉取封面图（经 `normalize_cover_bytes`）与歌词文档，保证彻底断网也能显示封面和同步歌词；
   - 支持配置「离线下载音质」（默认跟随当前播放音质或单独指定原品质/320k）。

3. **离线模式与元数据快照**
   - 在现有 `server_cache` 基础上，为每个 `OpenSubsonic` 源维护轻量化离线视图索引（所有已 Pin 的歌曲 + 所属专辑/艺人/歌单分组）；
   - 新增「离线模式」开关（侧边栏底部状态区 + 断网/连接超时横幅一键切换）：
     - 离线模式下自动屏蔽远端网络请求，首页/专辑/歌单/搜索直接查询本地离线清单；
     - 未离线曲目在列表中置灰不可点，已离线曲目带精致小云朵/勾选徽标可直接播放。
   - **离线操作延迟队列（Outbox）**：断网期间产生的 Scrobble 与收藏切换写入 `pending-sync.json`，恢复连接后后台自动重放同步。

4. **UI 入口**
   - 专辑详情页 / 歌单详情页操作栏新增「离线下载」按钮（下载中显示环形进度，完成后变为「已离线」并可点击管理/移除）；
   - 歌曲右键菜单新增「离线下载 / 移除离线文件」；
   - 设置 → 播放与缓存页新增「离线存储管理」（显示已离线曲目数、占用空间、按专辑/歌单清理、一键清理全部离线文件）。

5. **验收标准**
   - 开启离线模式并断开网络，已 Pin 专辑可完整浏览、搜索、播歌、看封面与同步歌词；
   - 触发普通播放缓存满额淘汰时，已 Pin 文件 100% 不受影响；
   - 应用异常退出重启后，未完成任务自动从断点恢复或清晰标记失败可重试。

---

### LP-03：WebDAV / Alist 私有存储音源（`ProviderKind::WebDav`）

#### 目标
支持直接挂载群晖/飞牛/TrueNAS WebDAV、Nextcloud、Alist / OpenList 等私有云盘目录作为音乐源，无需额外部署 Navidrome。

#### 核心设计
1. **配置与凭证（`src/models/source.rs`、`src/secrets/`）**
   - `SourceProfile::WebDav { id, name, base_url, root_path, username, credential_key, route_id }`；
   - 密码存入系统钥匙串（复用 `src/secrets/`），网络路由复用现有 `NetworkSettings`（支持直连或指定代理）。
2. **目录扫描与元数据索引**
   - 使用标准 WebDAV `PROPFIND`（`Depth: 1` 逐层遍历，限流 4 并发）拉取音频文件列表、`content-length` 与 `etag / lastmodified`；
   - 音频元数据提取策略（两级优化避免整文件下载）：
     - 第一步：通过 HTTP `Range` 请求只读取文件头部（如前 128 KB）与尾部（如后 64 KB，用于 ID3v1/APEv2/MP4 moov）尝试解析标签；
     - 第二步：对 `moov` 在中间或不支持部分解析的格式，回退用目录结构（`艺人/专辑/01 - 歌名.flac`）推断元数据，并允许用户手动触发完整标签解析；
   - 索引持久化结构直接复用 LP-01 的 `library-index`。
3. **流式播放与缓存**
   - 通过现有 `HttpResource`（带 Basic Auth / Bearer 请求头与 `Range` 支持）接入 Audio Worker 与 `StreamCache` / LP-02 离线下载，天然获得无缝播放、拖动和离线保留能力。

---

### LP-04：文件夹目录视图（Folder Browse）

#### 目标
为习惯按 NAS 目录树整理曲库（如 `华语/周杰伦/2003-叶惠美/` 或 `Hi-Res/古典/`）的用户提供原生目录浏览体验。

#### 核心设计
- **OpenSubsonic**：接入 `getMusicFolders` + `getIndexes` + `getMusicDirectory`，懒加载子目录；
- **LocalFolder / WebDAV**：直接基于本地索引构建虚拟目录树；
- **UI**：
  - 侧边栏曲库区新增「文件夹」视图（受 `SourceCapabilities::folders` 控制，Bilibili 源自动隐藏）；
  - 顶部面包屑导航（点击任意祖先目录快速跳转），支持「播放此目录全部曲目」与「递归加入队列」；
  - 目录内子文件夹卡片置顶、当前目录音频文件复用现有虚拟化歌曲列表 `render_song_list`。

---

### LP-05：跨源本地歌单与 `.m3u8` 导入/导出

#### 目标
解除歌单对单一 Navidrome 服务端的强绑定，允许用户创建属于客户端本地的自由歌单，支持把 **本地文件 + Navidrome + WebDAV + Bilibili** 曲目混编在一起，并可随时导出/导入标准 `.m3u8`。

#### 核心设计
1. **持久化存储**
   - 保存于 `config_dir()/local-playlists/<playlist_id>.json`（原子写入）；
   - 每条曲目保存 `{ media_key: MediaKey, snapshot: Song, added_at }`：即使某个远端源暂时离线或断开，歌单仍能显示完整歌名/歌手/时长，并在源恢复后自动可播。
2. **跨源播放**
   - 现有 `QueueItem` 已携带 `Song.source`，切歌时 `NavidromeApp` 按曲目自身的 `source` 分发对应 `MusicSource` 解析音频，因此跨源队列/歌单天然可行，只需确保后台解析在未切换当前浏览源时也能拿到对应音源句柄。
3. **`.m3u8` 导入与导出**
   - 导出：支持导出标准 `#EXTM3U` / `#EXTINF:<secs>,<artist> - <title>`（本地音源写绝对/相对路径，远端音源写 `naviclient://` URI 注释或回退匹配元数据）；
   - 导入：解析 `.m3u` / `.m3u8`，对本地路径直接绑定 `LocalFolder` 曲目，对未知路径复用现有 `src/import/` 的曲目匹配器在当前库中自动匹配。

---

### LP-06：本地隐私听歌统计与回顾（Listening Insights）

#### 目标
完全在本地记录与汇总用户的真实聆听习惯（不上传任何外部服务），提供精致的「听歌统计」视图。

#### 核心设计
1. **事件日志持久化（`src/listening/stats.rs`）**
   - 现有 `PlaybackTimeline` 已有精确的有效播放时长判定，在满足有效播放阈值（如播放 ≥ 30 秒或 ≥ 50% 时长）时向 `config_dir()/listening-log.jsonl` 追加一行轻量事件：`{ ts, source_id, song_id, title, artist, album, genre, duration_secs, listened_secs }`；
   - 按月/按年懒聚合并缓存汇总指标，避免日志增长后打开页面变慢。
2. **统计维度与视觉呈现**
   - 时间范围切换：近 7 天 / 近 30 天 / 今年 / 全部；
   - 顶部 KPI 卡片：总聆听时长、播放曲次、独立曲目/艺人/专辑数、日均收听；
   - 排行与分布：常听歌曲 Top 20（可直接一键「生成播放队列」）、常听艺人、流派分布条、24 小时聆听时段热力条。

---

### LP-07：个人数据一键备份与迁移

#### 目标
解决多台电脑（如 Linux 与 Windows 双端）或重装系统时的配置、歌词校准和个人资产迁移痛点。

#### 核心设计
- **导出包格式**：标准 `.zip`（或结构化 JSON Bundle），包含版本头 `schema_version` 与时间戳，内容可选勾选：
  1. 应用基础配置与快捷键（`config.json`，**默认剥离明文密码与 B 站 Cookie，保证安全可分享/存网盘**）；
  2. 自定义均衡器预设；
  3. 手动选词记录与每首歌的歌词时间偏移（`lyrics-cache` 中的用户偏好部分）；
  4. 跨源本地歌单（LP-05）；
  5. 本地听歌统计日志（LP-06）。
- **导入策略**：提供「合并（保留较新项）」与「覆盖」两种模式，导入前自动在本地备份一份旧快照以便回滚。
- **UI 入口**：设置 → 桌面 / 日志或新增「备份与恢复」分组卡片，提供「导出备份…」「从备份恢复…」。

---

### LP-08：桌面悬浮歌词窗口（Desktop Overlay Lyrics）

#### 目标
工作、写代码或浏览网页时，在桌面常驻一行/两行透明置顶歌词，支持逐字高亮、锁定鼠标穿透和快捷控制。

#### 核心设计
1. **窗口实现（复用 `src/app/player/mini_window.rs` 基础设施）**
   - 创建独立的无边框透明置顶 GPUI 子窗口（`WindowKind::PopUp` / `always_on_top`），记忆窗口位置、宽度与字体大小；
   - 支持双行显示（当前行逐字高亮 + 下一行或当前行译文）与单行极简模式；
   - 悬停时浮现半透明工具条（上一首 / 播放暂停 / 下一首 / 字体大小 ± / 锁定穿透 / 关闭）；
   - **鼠标穿透（Click-Through）**：
     - Windows：切换 `WS_EX_TRANSPARENT | WS_EX_LAYERED`；
     - Linux：通过 Wayland/X11 input region 置空或托盘菜单解锁；
     - 锁定时在系统托盘菜单与主窗口播放栏提供「解锁桌面歌词」入口，防止用户锁死无法拖动。
2. **设置项与快捷键**
   - 支持全局快捷键一键开关桌面歌词与切换锁定状态；
   - 设置 → 歌词页新增字体大小、描边/阴影强度、单双行与颜色主题选项。

---

### LP-09：歌曲列表多列排序与局部过滤

#### 目标
补齐专辑详情、收藏页、歌单详情与搜索结果中歌曲表格的交互短板。

#### 核心设计
- **可点击表头排序**：在 [src/app/songs/view.rs](file:///home/hn/code/rust/naviclient/src/app/songs/view.rs) 表头的 `#`（默认序/音轨号）、`Title`、`Artist`、`Time` 上支持点击切换 `默认 → 升序 → 降序`（带精致上下箭头指示），仅作用于浏览视图，队列抽屉保持播放顺序不可误打乱；
- **非破坏性视图映射**：沿用歌单搜索已有的 `original_index` 映射机制，确保排序后点击播放、多选、右键菜单与删除仍精准指向原始曲目。

---

### LP-10：睡眠定时器（Sleep Timer）

#### 目标
睡前或专注听歌时自动平滑停止播放。

#### 核心设计
- **触发模式**：15 / 30 / 45 / 60 分钟、自定义分钟数、以及「播完当前曲目后暂停」；
- **结束前柔和淡出**：倒计时最后 10 秒线性降低输出音量至 0 后执行暂停，并自动恢复原音量设定（不破坏用户 `config.volume`）；
- **UI 入口**：底部播放条右侧或托盘菜单触发紧凑 Popover；激活时在播放条显示倒计时微徽标（如 `24:10`），悬停可加时 `+10m` 或一键取消。

---

### LP-11：切歌淡入淡出（Crossfade）与倍速播放

#### 目标
提升连续听歌的流畅感，并满足 B 站长音频/播客/练歌的变速需求。

#### 核心设计
1. **Crossfade（`src/audio/`）**
   - 在 Audio Worker 中支持双解码通道短时混音（可配 `0s / 2s / 4s / 6s / 8s`，默认关闭）；
   - **智能规避**：同专辑连续音轨（Gapless 专辑）自动关闭 Crossfade，防止破坏现场专辑/概念专辑的无间隙衔接；仅在跨专辑、随机播放、歌单或手动切歌时生效。
2. **倍速播放（`0.5x ~ 2.0x`）**
   - 接入 `rubato` 或变速不变调（WSOLA / SoundTouch 风格重采样器）作为 EQ 前置 DSP 节点，实时同步 `PlaybackTimeline` 的进度推进速率，保证逐字歌词在倍速下依然精准对齐；
   - 提供预设档位（`0.75x / 1.0x / 1.25x / 1.5x / 2.0x`），非 `1.0x` 时在播放条展示速度徽标。

---

### LP-12：单曲另存到本地 & 本地库标签/封面轻编辑

#### 目标
完善私有库曲目落盘与本地库整理闭环。

#### 核心设计
- **另存为本地文件**：对 OpenSubsonic / WebDAV 曲目在右键菜单提供「导出到本地文件夹…」，复用已下载缓存或拉取原音质文件，并按 `{Artist}/{Album}/{Track:02} - {Title}.{ext}` 模板保存；
- **轻量标签编辑（仅 `LocalFolder`）**：在曲目信息弹窗（`info_view.rs`）中支持切换编辑模式，使用 `lofty` 安全回写标题、艺人、专辑、音轨号、年份、流派与替换内嵌封面，写完自动触发单文件索引刷新。

---

## 4. 全局工程约束与验收底线

后续逐步推进上述模块时，需统一遵守以下约束：

1. **配置与缓存向后兼容**：所有新增 `Config` / `SourceProfile` / `PlaybackSession` 字段均加 `#[serde(default)]`，保证老版本用户升级无缝迁移，不丢失历史偏好与缓存。
2. **Provider 边界清晰**：任何页面组件不得硬编码 `if provider == ...` 分散业务逻辑，一律通过 `SourceCapabilities` 与 `PlaybackPolicy` 驱动 UI 显隐。
3. **跨平台一致性**：Windows 与 Linux 双端均需保证路径规范化、文件锁（重命名/覆盖前关闭句柄）、钥匙串安全与高 DPI 视觉一致；不支持某特性的平台须优雅降级。
4. **内存与性能预算**：本地扫描、离线下载、封面提取全部在后台限流执行，封面统一经 `normalize_cover_bytes` 限幅；单模块合入后需通过 `cargo test -j 8`、`cargo clippy -j 8 --all-targets` 与三语 locale 完整性测试（`i18n_tests`）。
5. **独立小步提交**：每个 `LP-xx` 模块按「核心状态/逻辑 + 单测 → UI 集成 + 视觉验收」拆分独立提交，便于回溯与逐项在 Windows/Linux 实测。
