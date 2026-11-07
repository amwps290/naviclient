# `src/app` 布局重构设计

日期：2026-11-05
状态：待审阅

## 背景

`src/app/` 下约 140 个模块平铺，文件名靠前缀（`playback_*`、`server_*`、`shortcut_*`）隐含分组，
新代码该放哪里只能靠猜。`app.rs` 同时承担模块清单和共享导入，子模块通过 `use super::*` 拿到全部依赖。

这是重构系列的第一个子项目（布局整理）。数据正确性修复和性能结构调整是后续独立的子项目。

## 目标

看目录就能知道代码属于哪个功能、新代码该放哪里，且不改变任何行为。

## 范围

**包含：**

- 把 `src/app/` 下的模块按功能移入子目录并去掉重复前缀
- 新增 `app/prelude.rs`，承接 `app.rs` 顶部的共享导入
- 为移动所必需的可见性和路径改写
- 更新 `README.md`、`DEVELOPMENT_PLAN.md` 和源码注释中的旧路径

**不包含：**

- 把 `pub(in crate::app)` 收紧为目录内可见
- 去掉 glob 导入、改成显式导入
- 整理 `src/audio`、`src/models`、`src/config` 等其他目录
- 修复 bug、拆分 View、任何性能改动

## 工作方式约束

- 直接在工作区修改，不创建分支，不提交。本 spec 和后续计划也只写入、不提交。
- 工作区开始时已有 323 个未提交改动。因为没有 git 回退点，开始前先在仓库外打备份包。
- 使用普通 `mv`，不使用 `git mv`，不改动暂存区。
- 不使用子代理。

## 目标结构

```text
src/app.rs           NavidromeApp 结构体、少量 impl、18 个 mod 声明
src/app/prelude.rs   共享导入
src/app/tests.rs     应用级集成测试（位置不变）
src/app/<功能>.rs    功能入口：只有 mod 声明和必要的重导出
src/app/<功能>/      功能内的模块
```

### 规则

1. 目录深度最多两层：`app/<功能>/<文件>.rs`。
2. 测试跟随所属文件：`src/app/playback_start/tests.rs` 变成 `src/app/playback/start/tests.rs`。
3. 功能入口文件不放业务代码。
4. 被多个功能使用的组件放进 `widgets/`。
5. `app/tests.rs` 留在原位，只更新其中的路径。

### 模块映射

旧模块名均相对于 `src/app/`。

| 目录 | 新名称 | 旧模块 |
|---|---|---|
| `shell` | `startup` | `startup` |
| | `state` | `state` |
| | `render` | `render` |
| | `content` | `content_view` |
| | `navigation` | `navigation` |
| | `view_state` | `view_state` |
| | `view_helpers` | `view_helpers` |
| | `polling` | `polling` |
| | `messages` | `message_polling` |
| | `redraw` | `redraw_state` |
| | `titlebar` | `titlebar_view` |
| | `sidebar` | `sidebar_view` |
| `widgets` | `scrolling_text` | `scrolling_text_view` |
| | `scrolling_text_state` | `scrolling_text_state` |
| | `favorite_button` | `favorite_button_view` |
| | `skeleton` | `library_skeleton_view` |
| | `page_controls` | `library_view` |
| | `metadata_links` | `metadata_links` |
| `playback` | `advance` | `playback` |
| | `start` | `playback_start` |
| | `transport` | `playback_transport` |
| | `lifecycle` | `playback_lifecycle` |
| | `logic` | `playback_logic` |
| | `modes` | `playback_modes` |
| | `occurrence` | `playback_occurrence` |
| | `display` | `playback_display` |
| | `preload` | `playback_preload` |
| | `preload_state` | `preload_state` |
| | `reconnect` | `playback_reconnect` |
| | `session` | `playback_session` |
| | `session_state` | `session_state` |
| | `listening` | `listening` |
| | `listening_state` | `listening_state` |
| `player` | `bar` | `player_view` |
| | `controls` | `player_controls_view` |
| | `progress` | `player_progress_view` |
| | `now_playing` | `now_playing_view` |
| | `lyrics` | `lyrics` |
| | `lyrics_state` | `lyrics_state` |
| | `mini_view` | `mini_player_view` |
| | `mini_window` | `mini_window` |
| | `mini_window_state` | `mini_window_state` |
| `sound` | `volume` | `volume` |
| | `volume_state` | `volume_state` |
| | `volume_view` | `volume_view` |
| | `equalizer` | `equalizer` |
| | `equalizer_state` | `equalizer_state` |
| | `equalizer_view` | `equalizer_view` |
| | `loudness` | `loudness` |
| | `loudness_state` | `loudness_state` |
| | `output` | `audio_output` |
| `queue` | `actions` | `queue` |
| | `edits` | `queue_edits` |
| | `logic` | `queue_logic` |
| | `navigation` | `queue_navigation` |
| | `view` | `queue_view` |
| `songs` | `list` | `song_list` |
| | `scope` | `song_scope` |
| | `selection` | `song_selection` |
| | `menu` | `song_menu` |
| | `playback` | `song_playback` |
| | `row_actions` | `song_row_actions` |
| | `row_view` | `song_row_view` |
| | `view` | `song_view` |
| | `info_view` | `song_info_view` |
| `library` | `refresh` | `library` |
| | `helpers` | `library_helpers` |
| | `navigation` | `library_navigation` |
| | `playback` | `library_playback` |
| | `requests` | `library_requests` |
| | `paged_collection` | `paged_collection` |
| | `album_details` | `album_details` |
| | `album_paging` | `album_paging` |
| | `album_grid` | `album_grid_view` |
| | `artist_details` | `artist_details` |
| | `artist_library` | `artist_library` |
| | `artist_grid` | `artist_grid_view` |
| | `genre_library` | `genre_library` |
| | `browse_view` | `browse_view` |
| | `detail_view` | `detail_view` |
| | `favorites` | `favorites` |
| `home` | `albums` | `home_library` |
| | `view` | `home_view` |
| | `feature_view` | `home_feature_view` |
| | `section_view` | `home_section_view` |
| `search` | `submit` | `search` |
| | `state` | `search_state` |
| | `view` | `search_view` |
| `playlists` | `edits` | `playlists` |
| | `loading` | `playlist_loading` |
| | `library` | `playlist_library` |
| | `edit_state` | `playlist_edit_state` |
| | `dialog` | `playlist_dialog` |
| | `dialog_view` | `playlist_dialog_view` |
| | `list_view` | `playlist_list_view` |
| `recommendations` | `context` | `recommendations` |
| | `auto_continue` | `auto_continue` |
| | `auto_continue_state` | `auto_continue_state` |
| | `daily_mix` | `daily_mix` |
| | `daily_mix_state` | `daily_mix_state` |
| | `daily_mix_view` | `daily_mix_view` |
| | `radio` | `radio` |
| | `radio_state` | `radio_state` |
| `covers` | `loading` | `covers` |
| | `state` | `cover_state` |
| | `textures` | `cover_textures` |
| | `texture_state` | `cover_texture_state` |
| | `artwork` | `artwork_helpers` |
| | `view` | `artwork_view` |
| `server` | `connection` | `connection` |
| | `connection_state` | `server_connection` |
| | `cache` | `server_cache` |
| | `edit` | `server_edit` |
| | `form_view` | `server_form_view` |
| | `lifecycle` | `server_lifecycle` |
| | `list_view` | `server_list_view` |
| | `profile_edit` | `server_profile_edit` |
| | `profiles` | `server_profiles` |
| | `removal_state` | `server_removal_state` |
| | `remove_dialog` | `server_remove_dialog` |
| | `settings_view` | `server_settings_view` |
| `settings` | `preferences` | `preferences` |
| | `state` | `settings_state` |
| | `view` | `settings_view` |
| | `overlay` | `settings_overlay` |
| | `navigation_view` | `settings_navigation_view` |
| | `appearance_view` | `appearance_settings_view` |
| | `desktop_view` | `desktop_settings_view` |
| | `playback_view` | `playback_settings_view` |
| `shortcuts` | `dispatch` | `shortcuts` |
| | `bindings` | `shortcut_bindings` |
| | `config` | `shortcut_config` |
| | `edit` | `shortcut_edit` |
| | `repeat` | `shortcut_repeat` |
| | `row_view` | `shortcut_row_view` |
| | `settings_view` | `shortcut_settings_view` |
| `desktop` | `notifications` | `desktop` |
| | `state` | `desktop_state` |
| | `media_commands` | `media_commands` |
| | `media_command_state` | `media_command_state` |
| | `media_controls` | `media_controls` |
| | `platform_window` | `platform_window` |
| `import` | `orchestration` | `import` |
| | `flow` | `import_flow` |
| | `view` | `import_view` |

## 技术方案

### 共享导入

`app.rs` 顶部的 `use` 和对内部类型的 `use`、`pub(crate) use` 移入 `app/prelude.rs`，
以 `pub(in crate::app) use` 重导出。生产文件的 `use super::*;` 统一改为
`use crate::app::prelude::*;`。

测试文件的 `use super::*;` 指向被测模块，不做修改。

`msg.rs` 和 `main.rs` 依赖的 `crate::app::*` 公开路径保持不变，继续由 `app.rs` 重导出。

### 可见性

文件移入子目录后，`pub(super)` 的含义从“`app` 内可见”变成“所在目录内可见”。为保持现有可访问范围，
生产文件顶层的 `pub(super)` 统一改为 `pub(in crate::app)`。测试文件里的 `pub(super)` 相对于被测模块，
不做修改。

### 路径改写

一次性 Python 脚本放在 `/tmp`，不进入仓库。它读取上面的映射表，并且：

1. 移动 `旧名.rs` 和 `旧名/`（含测试）到新位置。
2. 改写 `super::旧名`、`super::super::旧名`、`crate::app::旧名` 为新的完整路径。
3. 改写经 glob 导入的裸模块路径 `旧名::`，并排除前面是 `::` 或 `.` 的情况。
4. 按新深度修正 `include_bytes!` 和 `include_str!` 的相对路径。
5. 更新功能入口文件和 `app.rs` 的 `mod` 声明。

全部目录移动完后，同一目录内的引用从 `crate::app::<功能>::<模块>` 缩短为 `super::<模块>`，
与现有写法保持一致；跨目录引用保留完整路径，让功能之间的依赖一眼可见。

脚本改写不了的情况按编译错误手工修复。

### 迁移顺序

每一步结束都必须通过验证才能进入下一步：

1. 备份到 `~/naviclient-backup-20261105.tar.gz`，包含 `src`、`locales`、`assets`、`docs`、`Cargo.toml`、
   `Cargo.lock`、`build.rs`、`README.md`、`DEVELOPMENT_PLAN.md`、`PRODUCT_PLAN.md`。
2. 引入 `prelude.rs`，替换 `use super::*`。
3. 改写可见性。
4. 按依赖从少到多依次移动目录：`import`、`search`、`home`、`shortcuts`、`server`、`settings`、`desktop`、
   `playlists`、`recommendations`、`queue`、`songs`、`covers`、`library`、`sound`、`player`、`playback`、
   `widgets`、`shell`。
5. 把 `technical_info_chip` 移到 `player/bar.rs`，精简 `app.rs`。
6. 更新文档和源码注释中的旧路径。

## 验证

每一步运行：

```sh
cargo check --all-targets
cargo test
cargo clippy --all-targets
cargo fmt --check
```

允许使用 `cargo fmt` 修正因路径变长造成的换行。

## 完成标准

- `cargo test` 报告 1095 个测试通过，0 失败（基线：1095 passed）。
- 重构前后的测试名集合，在去掉模块路径、只比较最后一段后完全一致。
- `cargo clippy --all-targets` 零警告，`cargo fmt --check` 无差异。
- `src/app/` 根目录只包含 18 个功能目录及其入口文件，加上 `prelude.rs` 和 `tests.rs`。
- 生产代码的改动仅限路径、可见性、导入和 `technical_info_chip` 的位置，没有逻辑改动。
- `README.md`、`DEVELOPMENT_PLAN.md` 和源码注释不再引用已移动的旧路径。

## 风险

| 风险 | 处理 |
|---|---|
| 裸路径改写误伤同名标识符，如 `state`、`import`、`queue` | 只匹配 `旧名::` 形式并排除 `::` 和 `.` 前缀，漏网的由编译器发现 |
| 同一目录下出现重名模块 | 映射表已逐目录检查，没有重名 |
| 新名称与 `prelude` 里 glob 导入的名字冲突，如 `playback`、`config` | 改写时使用完整路径，冲突由编译器发现后手工修复 |
| 中途失败无法回退 | 用备份包恢复 |
| 测试被意外漏掉 | 比较重构前后的测试名集合 |
| `cfg(windows)` 代码在 Linux 上不参与编译，本机缺 mingw，也无法交叉检查 | 这些代码只用到 prelude 里的名称；本地人工核对，最终由 CI 的 `windows-latest` 验证 |
