//! Provider ownership shared by transport, artwork and player-side operations.

use crate::app::prelude::*;
use crate::source::SourceId;

pub(in crate::app) fn configured_source(config: &Config, id: &SourceId) -> Option<MusicSource> {
    match id.provider {
        crate::source::ProviderKind::OpenSubsonic => {
            let server = config
                .servers
                .iter()
                .find(|server| server.id == id.profile_id && server.is_configured())?;
            MusicSource::opensubsonic(
                &server.id,
                &server.url,
                &server.username,
                &server.resolved_password(),
            )
            .ok()
        }
        crate::source::ProviderKind::Bilibili => None,
    }
}

impl NavidromeApp {
    pub(in crate::app) fn available_source(&self, id: &SourceId) -> Option<MusicSource> {
        if id.provider == crate::source::ProviderKind::OpenSubsonic {
            return configured_source(&self.config, id);
        }
        let crate::source::SourceProfile::Bilibili {
            account_id, proxy, ..
        } = self
            .config
            .sources
            .iter()
            .find(|profile| profile.id() == *id)?
        else {
            return None;
        };
        let crate::app::settings::bilibili_state::AccountStatus::Ready(session) =
            self.bilibili_accounts.get(id)?
        else {
            return None;
        };
        if *account_id != Some(session.account_id()) {
            return None;
        }
        crate::source::bilibili::BilibiliProvider::new(id.clone(), session.clone(), proxy.clone())
            .ok()
            .map(MusicSource::Bilibili)
    }

    pub(in crate::app) fn browsing_bilibili(&self) -> bool {
        self.config
            .active_source
            .as_ref()
            .is_some_and(|id| id.provider == crate::source::ProviderKind::Bilibili)
    }

    pub(in crate::app) fn switch_bilibili_source(&mut self, id: &SourceId) {
        if self.config.active_source.as_ref() == Some(id) {
            self.state.settings.close();
            self.state.view = View::Playlists;
            return;
        }
        if self.available_source(id).is_none() {
            return;
        }
        self.prepare_server_change();
        let mut next = self.config.clone();
        next.active_source = Some(id.clone());
        if config::save(&next).is_err() {
            self.notify_error(t!("Could not save source selection"));
            return;
        }
        self.config = next;
        self.reconnect_active_server();
        self.state.settings.close();
    }
    pub(in crate::app) fn browsing_owns_song(&self, song: &Song) -> bool {
        self.api
            .as_ref()
            .is_some_and(|api| song.belongs_to(api.id()))
    }

    pub(in crate::app) fn source_by_id(&self, id: &SourceId) -> Option<&MusicSource> {
        self.playback_source
            .as_ref()
            .filter(|api| api.id() == id)
            .or_else(|| self.api.as_ref().filter(|api| api.id() == id))
    }

    pub(in crate::app) fn source_for_song(&self, song: &Song) -> Option<&MusicSource> {
        match song.source.as_ref() {
            Some(id) => self.source_by_id(id),
            None => self.api.as_ref(),
        }
    }

    pub(in crate::app) fn song_cover_image(&self, song: &Song) -> Option<&Arc<GpuiImage>> {
        let source = song.source.as_ref().or_else(|| self.session.source())?;
        self.state.covers.image(source, song.cover_art.as_deref()?)
    }

    pub(in crate::app) fn browsing_cover_image(&self, id: &str) -> Option<&Arc<GpuiImage>> {
        self.state.covers.image(self.api.as_ref()?.id(), id)
    }

    pub(in crate::app) fn browsing_cover_palette(&self, id: &str) -> Option<(Hsla, Hsla)> {
        self.state.covers.palette(self.api.as_ref()?.id(), id)
    }

    pub(in crate::app) fn playback_source_label(&self) -> String {
        let Some(api) = self.playback_source.as_ref() else {
            return String::new();
        };
        match api.id().provider {
            crate::source::ProviderKind::OpenSubsonic => self
                .config
                .servers
                .iter()
                .find(|server| server.id == api.id().profile_id)
                .map(ServerProfile::label)
                .unwrap_or_else(|| "OpenSubsonic".to_string()),
            crate::source::ProviderKind::Bilibili => "Bilibili".to_string(),
        }
    }

    pub(in crate::app) fn playback_source_key(&self) -> String {
        self.playback_source
            .as_ref()
            .map(|api| api.id().cache_namespace())
            .unwrap_or_default()
    }

    pub(in crate::app) fn retire_playback_source(&mut self, source: &SourceId) {
        self.start_resolution.cancel_source(source);
        self.preload_resolution.cancel_source(source);
        if self
            .playback_source
            .as_ref()
            .is_none_or(|api| api.id() != source)
        {
            return;
        }
        self.save_session();
        self.persist_loudness();
        self.stop_playback();
        self.state.edit_queue().clear();
        self.state.listening = Default::default();
        self.state.loudness = Default::default();
        self.session.bind(None, None);
        self.playback_source = None;
        self.playback_server = None;
        self.config.playback_source = None;
    }
}
