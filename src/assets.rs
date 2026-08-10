use std::borrow::Cow;

use anyhow::Result;
use gpui::{AssetSource, SharedString};

pub struct Assets;

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
            _ => None,
        };

        Ok(bytes.map(Cow::Borrowed))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        if path == "icons" {
            Ok([
                "window-close.svg",
                "window-maximize.svg",
                "window-minimize.svg",
                "window-restore.svg",
            ]
            .into_iter()
            .map(SharedString::from)
            .collect())
        } else {
            Ok(Vec::new())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Assets;
    use gpui::AssetSource;

    #[test]
    fn embeds_all_title_bar_icons() {
        for path in [
            "icons/window-close.svg",
            "icons/window-maximize.svg",
            "icons/window-minimize.svg",
            "icons/window-restore.svg",
        ] {
            let icon = Assets.load(path).expect("asset lookup should succeed");
            assert!(icon.is_some(), "missing embedded asset: {path}");
        }
    }
}
