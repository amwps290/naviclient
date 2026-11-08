//! Home listening feature: resume the current song, or else the latest album.

use crate::app::prelude::*;
use crate::app::style::cover_tint::{cover_tint, tint_background, TintSurface};

#[derive(Clone)]
enum FeatureTarget {
    NowPlaying,
    Album(Album),
}

/// Element IDs stay separate per target so GPUI never reuses state across them.
struct FeatureIds {
    container: &'static str,
    cover: &'static str,
    play: &'static str,
    open: &'static str,
}

struct FeatureContent {
    target: FeatureTarget,
    ids: FeatureIds,
    eyebrow: SharedString,
    title: String,
    subtitle: String,
    cover_id: Option<String>,
    play_icon: PlayerIcon,
    play_label: SharedString,
    open_label: SharedString,
}

impl NavidromeApp {
    fn open_feature(&mut self, target: &FeatureTarget) {
        match target {
            FeatureTarget::NowPlaying => self.open_now_playing(),
            FeatureTarget::Album(album) => self.open_album(album.clone()),
        }
    }

    fn play_feature(&mut self, target: &FeatureTarget) {
        match target {
            FeatureTarget::NowPlaying => self.toggle_playback(),
            FeatureTarget::Album(album) => self.play_album(album.clone()),
        }
    }

    fn home_feature_content(&self) -> Option<FeatureContent> {
        if let Some(song) = &self.state.now_playing {
            let playback = self.audio.state();
            let is_playing = playback.active && !playback.paused;
            return Some(FeatureContent {
                target: FeatureTarget::NowPlaying,
                ids: FeatureIds {
                    container: "home-continue-listening",
                    cover: "home-feature-cover",
                    play: "home-feature-playback",
                    open: "home-feature-now-playing",
                },
                eyebrow: t!("Continue listening").into(),
                title: song.title.clone(),
                subtitle: if song.album.trim().is_empty() {
                    song.artist.clone()
                } else {
                    format!("{} · {}", song.artist, song.album)
                },
                cover_id: song.cover_art.clone(),
                play_icon: if is_playing {
                    PlayerIcon::Pause
                } else {
                    PlayerIcon::Play
                },
                play_label: if is_playing { t!("Pause") } else { t!("Play") }.into(),
                open_label: t!("Open player").into(),
            });
        }

        let album = self
            .state
            .home_albums
            .albums(HomeAlbumSection::Recent)
            .first()?
            .clone();
        Some(FeatureContent {
            ids: FeatureIds {
                container: "home-recent-feature",
                cover: "home-feature-album-cover",
                play: "home-feature-play-album",
                open: "home-feature-open-album",
            },
            eyebrow: t!("Recently played").into(),
            title: album.name.clone(),
            subtitle: album.artist.clone(),
            cover_id: album.cover_art.clone(),
            play_icon: PlayerIcon::Play,
            play_label: t!("Play album").into(),
            open_label: t!("Go to album").into(),
            target: FeatureTarget::Album(album),
        })
    }

    /// Home 的主焦点：优先继续当前播放；无播放会话时展示最近专辑。
    pub(in crate::app) fn render_home_feature(
        &self,
        compact: bool,
        cx: &Context<Self>,
    ) -> Option<gpui_kit::AnyElement> {
        let content = self.home_feature_content()?;
        let p = palette(cx);
        let tint = if matches!(content.target, FeatureTarget::NowPlaying) {
            cover_tint(
                TintSurface::HomeFeature,
                self.now_playing_cover_palette(),
                cx.theme().background,
                cx.theme().is_dark(),
            )
        } else {
            self.tint_for_cover(TintSurface::HomeFeature, content.cover_id.as_deref(), cx)
        };
        let art_size = if compact {
            tokens::HERO_COVER_COMPACT
        } else {
            tokens::HERO_COVER
        };
        let artwork = if let Some(song) = self
            .state
            .now_playing
            .as_ref()
            .filter(|_| matches!(content.target, FeatureTarget::NowPlaying))
        {
            self.render_song_cover_styled(
                song,
                art_size,
                tokens::radius::COVER,
                cover_shadow(false, cx),
            )
        } else {
            self.render_cover_styled(
                content.cover_id.as_deref(),
                art_size,
                tokens::radius::COVER,
                cover_shadow(false, cx),
            )
        };
        let target_cover = content.target.clone();
        let target_play = content.target.clone();
        let target_open = content.target;

        Some(
            h_flex()
                .id(content.ids.container)
                .w_full()
                .h(px(if compact {
                    tokens::HERO_CARD_COMPACT_HEIGHT
                } else {
                    tokens::HERO_CARD_HEIGHT
                }))
                .when(compact, |this| this.p_4().gap_4())
                .when(!compact, |this| this.p_6().gap_6())
                .items_center()
                .rounded(px(tokens::radius::LARGE))
                .bg(tint_background(tint, p.sunken, 118.0))
                .child(
                    div()
                        .id(content.ids.cover)
                        .flex_none()
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.open_feature(&target_cover);
                            cx.notify();
                        }))
                        .child(artwork),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(tokens::META))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(p.text_secondary)
                                .child(content.eyebrow),
                        )
                        .child(
                            serif_heading(
                                content.title,
                                if compact {
                                    tokens::CARD_TITLE_COMPACT
                                } else {
                                    tokens::CARD_TITLE
                                },
                                cx,
                            )
                            .truncate(),
                        )
                        .child(
                            div()
                                .text_size(px(tokens::META))
                                .text_color(p.text_secondary)
                                .truncate()
                                .child(content.subtitle),
                        )
                        .child(
                            h_flex()
                                .pt_2()
                                .gap_2()
                                .items_center()
                                .child(
                                    round_play_button(
                                        content.ids.play,
                                        content.play_icon,
                                        content.play_label,
                                        if compact {
                                            tokens::PLAY_BUTTON_COMPACT
                                        } else {
                                            tokens::PLAY_BUTTON
                                        },
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.play_feature(&target_play);
                                            cx.notify();
                                        },
                                    )),
                                )
                                .child(
                                    Button::new(content.ids.open)
                                        .label(content.open_label)
                                        .icon(AppIcon::ChevronRight)
                                        .ghost()
                                        .small()
                                        .rounded(px(tokens::radius::CONTROL))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.open_feature(&target_open);
                                            cx.notify();
                                        })),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }
}
