use std::borrow::Cow;

use anyhow::Result;
use gpui_kit::component::IconNamed;
use gpui_kit::{AssetSource, SharedString};

const ICON_FILES: [&str; 37] = [
    "window-close.svg",
    "window-maximize.svg",
    "window-minimize.svg",
    "window-restore.svg",
    "media-previous.svg",
    "media-play.svg",
    "media-pause.svg",
    "media-next.svg",
    "media-stop.svg",
    "mini-player.svg",
    "settings.svg",
    "refresh.svg",
    "shuffle.svg",
    "queue.svg",
    "heart.svg",
    "heart-filled.svg",
    "repeat.svg",
    "repeat-one.svg",
    "chevron-right.svg",
    "arrow-left.svg",
    "vinyl-highlight.svg",
    "vinyl-grooves.svg",
    "tonearm.svg",
    "tonearm-shade.svg",
    "tonearm-highlight.svg",
    "tonearm-stylus.svg",
    "volume-high.svg",
    "volume-low.svg",
    "volume-muted.svg",
    "chevron-up.svg",
    "chevron-down.svg",
    "house.svg",
    "mic-vocal.svg",
    "disc-album.svg",
    "genre-tags.svg",
    "eq.svg",
    "list-music.svg",
];

pub struct Assets;

#[derive(Clone, Copy)]
pub enum AppIcon {
    Close,
    Settings,
    Refresh,
    Shuffle,
    Queue,
    Heart,
    HeartFilled,
    Repeat,
    RepeatOne,
    ChevronRight,
    ChevronUp,
    ChevronDown,
    ArrowLeft,
    VinylHighlight,
    VinylGrooves,
    Tonearm,
    TonearmShade,
    TonearmHighlight,
    TonearmStylus,
    VolumeHigh,
    VolumeLow,
    VolumeMuted,
    MiniPlayer,
    Maximize,
    Home,
    Artists,
    Albums,
    Genres,
    Playlists,
    Eq,
}

#[derive(Clone, Copy)]
pub enum PlayerIcon {
    Previous,
    Play,
    Pause,
    #[allow(dead_code)]
    Stop,
    Next,
}

impl IconNamed for AppIcon {
    fn path(self) -> SharedString {
        match self {
            Self::Close => "icons/window-close.svg",
            Self::Settings => "icons/settings.svg",
            Self::Refresh => "icons/refresh.svg",
            Self::Shuffle => "icons/shuffle.svg",
            Self::Queue => "icons/queue.svg",
            Self::Heart => "icons/heart.svg",
            Self::HeartFilled => "icons/heart-filled.svg",
            Self::Repeat => "icons/repeat.svg",
            Self::RepeatOne => "icons/repeat-one.svg",
            Self::ChevronRight => "icons/chevron-right.svg",
            Self::ChevronUp => "icons/chevron-up.svg",
            Self::ChevronDown => "icons/chevron-down.svg",
            Self::ArrowLeft => "icons/arrow-left.svg",
            Self::VinylHighlight => "icons/vinyl-highlight.svg",
            Self::VinylGrooves => "icons/vinyl-grooves.svg",
            Self::Tonearm => "icons/tonearm.svg",
            Self::TonearmShade => "icons/tonearm-shade.svg",
            Self::TonearmHighlight => "icons/tonearm-highlight.svg",
            Self::TonearmStylus => "icons/tonearm-stylus.svg",
            Self::VolumeHigh => "icons/volume-high.svg",
            Self::VolumeLow => "icons/volume-low.svg",
            Self::VolumeMuted => "icons/volume-muted.svg",
            Self::MiniPlayer => "icons/mini-player.svg",
            Self::Maximize => "icons/window-maximize.svg",
            Self::Home => "icons/house.svg",
            Self::Artists => "icons/mic-vocal.svg",
            Self::Albums => "icons/disc-album.svg",
            Self::Genres => "icons/genre-tags.svg",
            Self::Playlists => "icons/list-music.svg",
            Self::Eq => "icons/eq.svg",
        }
        .into()
    }
}

impl IconNamed for PlayerIcon {
    fn path(self) -> SharedString {
        match self {
            Self::Previous => "icons/media-previous.svg",
            Self::Play => "icons/media-play.svg",
            Self::Pause => "icons/media-pause.svg",
            Self::Stop => "icons/media-stop.svg",
            Self::Next => "icons/media-next.svg",
        }
        .into()
    }
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        let bytes: Option<&'static [u8]> = match path {
            "icons/window-close.svg" => Some(include_bytes!("../assets/icons/window-close.svg")),
            "icons/window-maximize.svg" => {
                Some(include_bytes!("../assets/icons/window-maximize.svg"))
            }
            "icons/window-minimize.svg" => {
                Some(include_bytes!("../assets/icons/window-minimize.svg"))
            }
            "icons/window-restore.svg" => {
                Some(include_bytes!("../assets/icons/window-restore.svg"))
            }
            "icons/media-previous.svg" => {
                Some(include_bytes!("../assets/icons/media-previous.svg"))
            }
            "icons/media-play.svg" => Some(include_bytes!("../assets/icons/media-play.svg")),
            "icons/media-pause.svg" => Some(include_bytes!("../assets/icons/media-pause.svg")),
            "icons/media-next.svg" => Some(include_bytes!("../assets/icons/media-next.svg")),
            "icons/media-stop.svg" => Some(include_bytes!("../assets/icons/media-stop.svg")),
            "icons/mini-player.svg" => Some(include_bytes!("../assets/icons/mini-player.svg")),
            "icons/settings.svg" => Some(include_bytes!("../assets/icons/settings.svg")),
            "icons/refresh.svg" => Some(include_bytes!("../assets/icons/refresh.svg")),
            "icons/shuffle.svg" => Some(include_bytes!("../assets/icons/shuffle.svg")),
            "icons/queue.svg" => Some(include_bytes!("../assets/icons/queue.svg")),
            "icons/heart.svg" => Some(include_bytes!("../assets/icons/heart.svg")),
            "icons/heart-filled.svg" => Some(include_bytes!("../assets/icons/heart-filled.svg")),
            "icons/repeat.svg" => Some(include_bytes!("../assets/icons/repeat.svg")),
            "icons/repeat-one.svg" => Some(include_bytes!("../assets/icons/repeat-one.svg")),
            "icons/chevron-right.svg" => Some(include_bytes!("../assets/icons/chevron-right.svg")),
            "icons/chevron-up.svg" => Some(include_bytes!("../assets/icons/chevron-up.svg")),
            "icons/chevron-down.svg" => Some(include_bytes!("../assets/icons/chevron-down.svg")),
            "icons/arrow-left.svg" => Some(include_bytes!("../assets/icons/arrow-left.svg")),
            "icons/vinyl-highlight.svg" => {
                Some(include_bytes!("../assets/icons/vinyl-highlight.svg"))
            }
            "icons/vinyl-grooves.svg" => Some(include_bytes!("../assets/icons/vinyl-grooves.svg")),
            "icons/tonearm.svg" => Some(include_bytes!("../assets/icons/tonearm.svg")),
            "icons/tonearm-shade.svg" => Some(include_bytes!("../assets/icons/tonearm-shade.svg")),
            "icons/tonearm-highlight.svg" => {
                Some(include_bytes!("../assets/icons/tonearm-highlight.svg"))
            }
            "icons/tonearm-stylus.svg" => {
                Some(include_bytes!("../assets/icons/tonearm-stylus.svg"))
            }
            "icons/volume-high.svg" => Some(include_bytes!("../assets/icons/volume-high.svg")),
            "icons/volume-low.svg" => Some(include_bytes!("../assets/icons/volume-low.svg")),
            "icons/volume-muted.svg" => Some(include_bytes!("../assets/icons/volume-muted.svg")),
            "icons/house.svg" => Some(include_bytes!("../assets/icons/house.svg")),
            "icons/mic-vocal.svg" => Some(include_bytes!("../assets/icons/mic-vocal.svg")),
            "icons/disc-album.svg" => Some(include_bytes!("../assets/icons/disc-album.svg")),
            "icons/genre-tags.svg" => Some(include_bytes!("../assets/icons/genre-tags.svg")),
            "icons/list-music.svg" => Some(include_bytes!("../assets/icons/list-music.svg")),
            "icons/eq.svg" => Some(include_bytes!("../assets/icons/eq.svg")),
            _ => None,
        };

        match bytes {
            Some(bytes) => Ok(Some(Cow::Borrowed(bytes))),
            None => gpui_kit::assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(
            ICON_FILES
                .into_iter()
                .map(|filename| format!("icons/{filename}"))
                .filter(|name| name.starts_with(path))
                .map(SharedString::from),
        );
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::{Assets, ICON_FILES};
    use gpui_kit::AssetSource;

    #[test]
    fn embeds_all_application_icons() {
        for filename in ICON_FILES {
            let path = format!("icons/{filename}");
            let icon = Assets.load(&path).expect("asset lookup should succeed");
            assert!(icon.is_some(), "missing embedded asset: {path}");
        }
    }

    #[test]
    fn includes_kit_icons_without_replacing_application_icons() {
        use gpui_kit::assets::IconName;

        let search = IconName::Search.path();
        assert!(Assets.load(&search).unwrap().is_some());
        assert_eq!(
            Assets
                .load("icons/chevron-down.svg")
                .unwrap()
                .unwrap()
                .as_ref(),
            include_bytes!("../assets/icons/chevron-down.svg"),
        );
        let paths = Assets.list("icons/").unwrap();
        assert!(paths.contains(&search));
        assert!(paths.iter().any(|path| path == "icons/vinyl-grooves.svg"));
        assert_eq!(
            paths
                .iter()
                .filter(|path| *path == "icons/chevron-down.svg")
                .count(),
            1,
        );
    }
}
