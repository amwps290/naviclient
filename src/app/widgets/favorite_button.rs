//! Shared favorite toggle button for songs, artists and albums.

use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn favorite_button(
        &self,
        kind: FavoriteKind,
        id: &str,
        cx: &Context<Self>,
    ) -> Button {
        self.favorite_button_sized(kind, id, tokens::ICON_BUTTON_XS, cx)
    }

    pub(in crate::app) fn favorite_button_sized(
        &self,
        kind: FavoriteKind,
        id: &str,
        size: f32,
        cx: &Context<Self>,
    ) -> Button {
        let key = FavoriteKey::new(kind, id);
        let starred = self.state.favorites.is_starred(&key);
        let pending = self.state.favorites.is_pending(&key);
        let kind_name = match kind {
            FavoriteKind::Artist => "artist",
            FavoriteKind::Album => "album",
            FavoriteKind::Song => "song",
        };

        icon_button(
            SharedString::from(format!("favorite-{kind_name}-{id}")),
            if starred {
                AppIcon::HeartFilled
            } else {
                AppIcon::Heart
            },
            if starred {
                t!("Remove from favorites")
            } else {
                t!("Add to favorites")
            },
            size,
        )
        .opacity(if pending { 0.6 } else { 1.0 })
        .text_color(if starred {
            palette(cx).brand
        } else {
            palette(cx).text_secondary
        })
        .on_click(cx.listener(move |this, _, _, cx| {
            this.toggle_favorite(key.clone());
            cx.notify();
        }))
    }
}
