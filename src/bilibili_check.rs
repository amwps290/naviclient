//! Explicit SSH diagnostics, before file logging, GUI and single-instance setup.

use crate::{
    config,
    source::{
        bilibili::{account::restore_session, BilibiliProvider},
        SourceProfile,
    },
};
use anyhow::{bail, Result};

pub(crate) fn try_run() -> bool {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some("--bilibili-check") {
        return false;
    }
    let result = options(&args[1..]).and_then(|options| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()?
            .block_on(run(options))
    });
    if let Err(error) = result {
        eprintln!("Bilibili 验证失败：{error}");
        std::process::exit(1);
    }
    true
}

#[derive(Default)]
struct Options {
    folder: Option<u64>,
    account_index: Option<usize>,
    decode: bool,
    play_seconds: Option<u64>,
}
fn options(args: &[String]) -> Result<Options> {
    let mut options = Options::default();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--decode" => options.decode = true,
            "--play-seconds" => {
                let seconds = args.next().ok_or_else(|| anyhow::anyhow!("--play-seconds 需要秒数（1–60）"))?.parse::<u64>()?;
                if !(1..=60).contains(&seconds) { bail!("播放秒数必须在 1–60 之间"); }
                options.play_seconds = Some(seconds);
            }
            "--folder" => options.folder = Some(args.next().ok_or_else(|| anyhow::anyhow!("--folder 需要收藏夹 ID"))?.parse()?),
            "--account-index" => options.account_index = Some(args.next().ok_or_else(|| anyhow::anyhow!("--account-index 需要账号序号"))?.parse()?),
            _ => bail!("用法：--bilibili-check [--account-index 1] [--folder ID] [--decode] [--play-seconds 3]"),
        }
    }
    Ok(options)
}

async fn run(options: Options) -> Result<()> {
    let config = config::load();
    let profiles: Vec<_> = config
        .sources
        .iter()
        .filter(|profile| matches!(profile, SourceProfile::Bilibili { .. }))
        .collect();
    if profiles.is_empty() {
        bail!("尚未配置 Bilibili 账号，请先扫码登录");
    }
    if profiles.len() > 1 && options.account_index.is_none() {
        bail!(
            "配置了 {} 个 Bilibili 账号，请使用 --account-index 选择序号",
            profiles.len()
        );
    }
    let profile = profiles
        .get(
            options
                .account_index
                .unwrap_or(1)
                .checked_sub(1)
                .ok_or_else(|| anyhow::anyhow!("账号序号从 1 开始"))?,
        )
        .ok_or_else(|| anyhow::anyhow!("账号序号不存在"))?;
    let route = config
        .route_for(&profile.id())
        .map_err(|_| anyhow::anyhow!("所选代理已不存在，请在设置中重新选择"))?;
    let session = restore_session(profile, &route).await?;
    println!("登录恢复：成功（系统凭据库回读及账号验证通过）");
    let provider = BilibiliProvider::new(profile.id(), session, route)?;
    let folders = provider.favorite_folders().await?;
    println!("自己的收藏夹：{} 个", folders.len());
    for (index, folder) in folders.iter().enumerate() {
        println!("  收藏夹 #{}，视频数={}", index + 1, folder.media_count);
    }
    let selected = options.folder.or_else(|| {
        folders
            .iter()
            .find(|folder| folder.media_count > 0)
            .map(|folder| folder.id)
    });
    let Some(selected) = selected else {
        println!("没有可验证的收藏视频。");
        return Ok(());
    };
    let tracks = provider.favorite_tracks(selected).await?;
    println!(
        "收藏夹加载：{} 页，{} 个视频，{} 个分 P，跳过 {} 个不可用条目",
        tracks.pages,
        tracks.videos,
        tracks.songs.len(),
        tracks.unavailable
    );
    if !options.decode && options.play_seconds.is_none() {
        println!("加上 --decode 可验证独立音频、PCM 解码和向前/向后跳转。");
        return Ok(());
    }
    // Skip unsupported content within a bounded sample, never fall back to video.
    let mut chosen = None;
    for song in tracks.songs.iter().take(10) {
        match provider.resolve_playback(song).await {
            Ok(resource) => {
                chosen = Some(resource);
                break;
            }
            Err(
                crate::source::bilibili::client::BilibiliError::UnsupportedAudio
                | crate::source::bilibili::client::BilibiliError::Unavailable
                | crate::source::bilibili::client::BilibiliError::PermissionDenied
                | crate::source::bilibili::client::BilibiliError::RegionRestricted,
            ) => continue,
            Err(error) => return Err(error.into()),
        }
    }
    let resource =
        chosen.ok_or_else(|| anyhow::anyhow!("前 10 个分 P 中没有可验证的独立 AAC-LC 音频"))?;
    println!(
        "独立音频：{}，声明带宽 {} bit/s，{} 个备用地址，时长 {} 秒",
        resource.quality.codec.as_deref().unwrap_or("unknown"),
        resource.quality.bandwidth.unwrap_or_default(),
        resource.backups.len(),
        resource.duration.unwrap_or_default().as_secs()
    );
    if options.decode {
        let decoded = resource.clone();
        let evidence =
            tokio::task::spawn_blocking(move || crate::audio::probe::decode(&decoded)).await??;
        println!(
            "应用解码：{} Hz，{} 声道，{} 个 PCM 样本，{} 次跳转成功",
            evidence.sample_rate, evidence.channels, evidence.samples, evidence.seeks
        );
    }
    if let Some(seconds) = options.play_seconds {
        output_probe(resource, seconds, config.output_device).await?;
    } else {
        println!("此验证未连接音频输出设备；实际出声仍需播放器试听。");
    }
    Ok(())
}

async fn output_probe(
    resource: crate::source::PlaybackResource,
    seconds: u64,
    device: Option<String>,
) -> Result<()> {
    use std::time::Duration;
    let cache = tempfile::tempdir()?;
    let audio = crate::audio::AudioHandle::start(cache.path().to_path_buf(), device)?;
    audio.set_volume(0.05);
    if !audio.play(
        resource.http.clone().with_backups(resource.backups.clone()),
        resource.cache_key(),
        resource.duration,
        1.0,
        Default::default(),
    ) {
        bail!("音频输出工作线程未接受播放请求");
    }
    println!(
        "正在验证运行播放器的主机音频输出（5% 音量，{} 秒）…",
        seconds
    );
    let result = async {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds + 30);
        loop {
            let state = audio.state();
            if let Some(error) = state.error {
                bail!("音频输出失败：{error}");
            }
            if state.worker_closed {
                bail!("音频输出工作线程已关闭");
            }
            if state.output_connected
                && state.active
                && state.position >= Duration::from_secs(seconds)
            {
                println!(
                    "输出通路：已连接，播放位置推进 {} 秒，设备采样率 {} Hz，{} 声道",
                    state.position.as_secs(),
                    state.output_sample_rate,
                    state.output_channels
                );
                println!("SSH 中无法确认听感；声音输出在运行播放器的主机。");
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                bail!("音频输出起播或播放位置推进超时");
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    .await;
    audio.stop();
    result
}
