use std::borrow::Cow;

use anyhow::Result;
use gpui::{AssetSource, SharedString};
use gpui_component::IconNamed;

const ICON_FILES: [&str; 9] = [
    "window-close.svg",
    "window-maximize.svg",
    "window-minimize.svg",
    "window-restore.svg",
    "media-previous.svg",
    "media-play.svg",
    "media-pause.svg",
    "media-next.svg",
    "media-stop.svg",
];

pub struct Assets;

#[derive(Clone, Copy)]
pub enum PlayerIcon {
    Previous,
    Play,
    Pause,
    Next,
    Stop,
}

impl IconNamed for PlayerIcon {
    fn path(self) -> SharedString {
        match self {
            Self::Previous => "icons/media-previous.svg",
            Self::Play => "icons/media-play.svg",
            Self::Pause => "icons/media-pause.svg",
            Self::Next => "icons/media-next.svg",
            Self::Stop => "icons/media-stop.svg",
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
