use std::borrow::Cow;

use anyhow::Result;
use gpui::{AssetSource, SharedString};
use gpui_component::IconNamed;

const ICON_FILES: [&str; 22] = [
    "window-close.svg",
    "window-maximize.svg",
    "window-minimize.svg",
    "window-restore.svg",
    "media-previous.svg",
    "media-play.svg",
    "media-pause.svg",
    "media-next.svg",
    "settings.svg",
    "refresh.svg",
    "shuffle.svg",
    "queue.svg",
    "heart.svg",
    "heart-filled.svg",
    "repeat.svg",
    "repeat-one.svg",
    "lyrics.svg",
    "chevron-right.svg",
    "arrow-left.svg",
    "vinyl-highlight.svg",
    "tonearm.svg",
    "tonearm-rest.svg",
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
    Lyrics,
    ChevronRight,
    ArrowLeft,
    VinylHighlight,
    Tonearm,
    TonearmRest,
}

#[derive(Clone, Copy)]
pub enum PlayerIcon {
    Previous,
    Play,
    Pause,
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
            Self::Lyrics => "icons/lyrics.svg",
            Self::ChevronRight => "icons/chevron-right.svg",
            Self::ArrowLeft => "icons/arrow-left.svg",
            Self::VinylHighlight => "icons/vinyl-highlight.svg",
            Self::Tonearm => "icons/tonearm.svg",
            Self::TonearmRest => "icons/tonearm-rest.svg",
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
            "icons/settings.svg" => Some(include_bytes!("../assets/icons/settings.svg")),
            "icons/refresh.svg" => Some(include_bytes!("../assets/icons/refresh.svg")),
            "icons/shuffle.svg" => Some(include_bytes!("../assets/icons/shuffle.svg")),
            "icons/queue.svg" => Some(include_bytes!("../assets/icons/queue.svg")),
            "icons/heart.svg" => Some(include_bytes!("../assets/icons/heart.svg")),
            "icons/heart-filled.svg" => Some(include_bytes!("../assets/icons/heart-filled.svg")),
            "icons/repeat.svg" => Some(include_bytes!("../assets/icons/repeat.svg")),
            "icons/repeat-one.svg" => Some(include_bytes!("../assets/icons/repeat-one.svg")),
            "icons/lyrics.svg" => Some(include_bytes!("../assets/icons/lyrics.svg")),
            "icons/chevron-right.svg" => Some(include_bytes!("../assets/icons/chevron-right.svg")),
            "icons/arrow-left.svg" => Some(include_bytes!("../assets/icons/arrow-left.svg")),
            "icons/vinyl-highlight.svg" => {
                Some(include_bytes!("../assets/icons/vinyl-highlight.svg"))
            }
            "icons/tonearm.svg" => Some(include_bytes!("../assets/icons/tonearm.svg")),
            "icons/tonearm-rest.svg" => Some(include_bytes!("../assets/icons/tonearm-rest.svg")),
            _ => None,
        };

        Ok(bytes.map(Cow::Borrowed))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        if path == "icons" {
            Ok(ICON_FILES.into_iter().map(SharedString::from).collect())
        } else {
            Ok(Vec::new())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Assets, ICON_FILES};
    use gpui::AssetSource;

    #[test]
    fn embeds_all_application_icons() {
        for filename in ICON_FILES {
            let path = format!("icons/{filename}");
            let icon = Assets.load(&path).expect("asset lookup should succeed");
            assert!(icon.is_some(), "missing embedded asset: {path}");
        }
    }
}
