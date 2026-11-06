//! Song and artist radio generation, cancellation, and guarded queue updates.

use super::playback_modes::PlaybackModeEdit;
use super::*;
use crate::recommendation::{generate_radio, RadioGenerationContext, RadioSeed};

impl NavidromeApp {
    /// Start a song radio immediately with its seed while unified recommendation
    /// candidates are generated in the background.
    pub(super) fn start_song_radio(&mut self, seed: Song) {
        self.start_radio(RadioSeed::Song(Box::new(seed.clone())), seed);
    }

    /// Artist radio has its own recommendation context. A loaded popular song
    /// starts immediately, but candidate scoring is based on the artist rather
    /// than treating that song as a song-radio shortcut.
    pub(super) fn start_artist_radio_from_seed(&mut self, artist: Artist, initial_song: Song) {
        self.start_radio(RadioSeed::Artist(artist), initial_song);
    }

    fn start_radio(&mut self, seed: RadioSeed, initial_song: Song) {
        let Some(api) = self.api.clone() else {
            self.notify_error(t!("Configure a server before playing"));
            return;
        };
        let Some(server_id) = self.config.active_server_id().map(str::to_string) else {
            self.notify_error(t!("Configure a server before playing"));
            return;
        };
        let origin = if matches!(seed, RadioSeed::Artist(_)) {
            QueueOrigin::ArtistRadio
        } else {
            QueueOrigin::SongRadio
        };
        if !self.replace_queue_and_play(vec![initial_song.clone()], 0, origin) {
            return;
        }
        let request =
            self.state
                .radio
                .begin(&server_id, &seed, &initial_song, &mut self.library_requests);
        debug_assert_eq!(request.origin(), origin);

        // Explicitly starting a radio means "continue after this track".
        self.apply_playback_mode_edit(PlaybackModeEdit::AllowContinuation);
        self.save_session();

        let capabilities = self
            .state
            .connection
            .server()
            .map(|server| server.capabilities)
            .unwrap_or_default();
        let common = self.recommendation_generation_context(
            server_id.clone(),
            capabilities.sonic_similarity,
            50,
        );
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = generate_radio(
                api,
                RadioGenerationContext {
                    common,
                    seed,
                    top_songs_by_artist_id: capabilities.top_songs_by_artist_id,
                },
            )
            .await
            .map_err(error_message);
            let _ = tx.send(Msg::Radio { request, result });
        });
    }

    pub(super) fn song_radio_loading(&self, song_id: &str) -> bool {
        self.state.radio.song_loading(song_id)
    }

    pub(super) fn artist_radio_loading(&self, artist_id: &str) -> bool {
        self.state.radio.artist_loading(artist_id)
    }

    pub(super) fn cancel_recommendation_tasks(&mut self) {
        self.state.radio.cancel();
        self.state.cancel_deferred_playback();
    }

    pub(super) fn finish_radio(
        &mut self,
        request: RadioRequest,
        result: Result<Vec<crate::recommendation::RecommendedTrack>, String>,
    ) {
        let Some(result) = self.state.radio.complete(
            &request,
            self.config.active_server_id(),
            self.state.queue_index,
            &self.state.queue,
            self.state.now_playing.as_ref(),
            result,
        ) else {
            return;
        };

        match result {
            Ok(tracks) if !tracks.is_empty() => {
                let count = tracks.len();
                self.state.edit_queue().append_entries(
                    tracks
                        .into_iter()
                        .map(|track| QueueEntry::new(track.song, track.origin)),
                );
                self.save_session();
                if request.is_artist() {
                    self.notify_success(t!("Artist radio added %{count} tracks", count = count));
                } else {
                    self.notify_success(t!("Song radio added %{count} tracks", count = count));
                }
            }
            Ok(_) if request.is_artist() => {
                self.notify_warning(t!("No tracks were found for this artist radio"))
            }
            Ok(_) => self.notify_warning(t!("No related tracks were found for this song")),
            Err(error) if request.is_artist() => {
                self.notify_error(t!("Could not build artist radio: %{error}", error = error))
            }
            Err(error) => {
                self.notify_error(t!("Could not build song radio: %{error}", error = error))
            }
        }
    }
}
