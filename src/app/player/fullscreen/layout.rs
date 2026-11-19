//! Viewport-derived geometry for the Vinyl Salon presentation.

#[derive(Clone, Copy, Debug)]
pub(super) struct FullscreenLayout {
    pub(super) padding_x: f32,
    pub(super) padding_top: f32,
    pub(super) padding_bottom: f32,
    pub(super) header_height: f32,
    pub(super) footer_height: f32,
    pub(super) gap: f32,
    pub(super) column_gap: f32,
    pub(super) art_width: f32,
    pub(super) stage_width: f32,
    pub(super) cover_size: f32,
    pub(super) metadata_width: f32,
    pub(super) lyric_size: f32,
    pub(super) active_lyric_size: f32,
    pub(super) stacked: bool,
    pub(super) compact: bool,
    pub(super) body_height: f32,
}

impl FullscreenLayout {
    pub(super) fn new(width: f32, height: f32) -> Self {
        let width = width.max(1.0);
        let height = height.max(1.0);
        let compact = height < 780.0 || width < 1100.0;
        let stacked = width < 840.0;
        let padding_x = (width * 0.044).clamp(20.0, 88.0);
        let padding_top = (height * 0.038).clamp(16.0, 44.0);
        let padding_bottom = if compact { 16.0 } else { 25.0 };
        let header_height = 40.0;
        let footer_height = if stacked {
            138.0
        } else if compact {
            110.0
        } else {
            130.0
        };
        let gap = (height * 0.025).clamp(12.0, 28.0);
        let body_height =
            (height - padding_top - padding_bottom - header_height - footer_height - gap * 2.0)
                .max(0.0);
        let column_gap = if stacked {
            24.0
        } else {
            (width * 0.074).clamp(40.0, 145.0)
        };
        let available_width =
            (width - padding_x * 2.0)
                .max(0.0)
                .min(if width < 2000.0 { 1510.0 } else { 1840.0 });
        let art_width = if stacked {
            available_width
        } else {
            ((available_width - column_gap) * 0.432).max(0.0)
        };
        let cover_height = if stacked {
            (body_height * 0.43 - 110.0).max(48.0)
        } else {
            (body_height - if compact { 220.0 } else { 240.0 }).max(48.0)
        };
        let cover_size = (width * 0.30)
            .min(height * 0.44)
            .min(cover_height)
            .min(art_width * 0.90)
            .clamp(1.0, 550.0);
        let active_lyric_size = (width * 0.0313).clamp(28.0, 66.0);
        let lyric_size = (width * 0.026).clamp(24.0, 50.0);
        Self {
            padding_x,
            padding_top,
            padding_bottom,
            header_height,
            footer_height,
            gap,
            column_gap,
            art_width,
            cover_size,
            metadata_width: cover_size.max(240.0).min(art_width * 0.90),
            lyric_size,
            active_lyric_size,
            stage_width: available_width,
            stacked,
            compact,
            body_height,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artwork_and_controls_fit_short_desktops_and_presentation_displays() {
        for (width, height) in [
            (1024.0, 600.0),
            (1280.0, 720.0),
            (1440.0, 900.0),
            (1920.0, 1080.0),
            (3840.0, 2160.0),
        ] {
            let layout = FullscreenLayout::new(width, height);
            assert!(layout.cover_size <= layout.art_width);
            assert!(layout.art_width + layout.column_gap < width - layout.padding_x * 2.0);
            assert!(layout.body_height > layout.cover_size);
            assert!(!layout.stacked);
        }
    }

    #[test]
    fn narrow_portrait_windows_stack_the_cover_above_lyrics() {
        let layout = FullscreenLayout::new(720.0, 1280.0);
        assert!(layout.stacked);
        assert!(layout.cover_size < layout.art_width);
        assert!(layout.cover_size < layout.body_height * 0.43);
    }
}
