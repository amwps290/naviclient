//! Capability-aware server lyrics requests and legacy fallback orchestration.

use anyhow::Result;

use super::OpenSubsonicProvider;

use crate::models::{CapabilitySupport, Lyrics, Song};

mod response;

use response::{parse_plain_lyrics, parse_structured_lyrics};

impl OpenSubsonicProvider {
    pub async fn lyrics(
        &self,
        song: &Song,
        structured_lyrics: CapabilitySupport,
    ) -> Result<Lyrics> {
        let structured = if structured_lyrics.allows_optimistic_request() {
            Some(
                self.get_json("getLyricsBySongId", &[("id", &song.id)])
                    .await,
            )
        } else {
            None
        };

        if let Some(Ok(body)) = &structured {
            if let Some(lyrics) = parse_structured_lyrics(body) {
                return Ok(lyrics);
            }
        }

        match self
            .get_json(
                "getLyrics",
                &[("artist", &song.artist), ("title", &song.title)],
            )
            .await
        {
            Ok(body) => Ok(parse_plain_lyrics(&body).unwrap_or_default()),
            Err(_) if structured.as_ref().is_some_and(Result::is_ok) => Ok(Lyrics::default()),
            Err(fallback_error) => match structured {
                Some(Err(structured_error)) => Err(structured_error.context(format!(
                    "legacy lyrics request also failed: {fallback_error:#}"
                ))),
                Some(Ok(_)) => unreachable!("successful structured lyrics handled above"),
                None => Err(fallback_error),
            },
        }
    }
}

#[cfg(test)]
mod http_tests;
