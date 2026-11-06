//! Library sorting and contiguous album pagination.

use crate::models::{Artist, ArtistSort, Song};

pub(super) const ALBUM_PAGE_SIZE: usize = 100;

/// 专辑内歌曲按（碟号， 曲目号）排序；缺失字段排到最后，
/// 同一曲内缺失曲目号的按标题保持稳定顺序。
pub(super) fn sort_album_songs(songs: &mut [Song]) {
    songs.sort_by(|a, b| {
        let disc = |song: &Song| song.disc.unwrap_or(i32::MAX);
        let track = |song: &Song| song.track.unwrap_or(i32::MAX);
        disc(a)
            .cmp(&disc(b))
            .then_with(|| track(a).cmp(&track(b)))
            .then_with(|| a.title.cmp(&b.title))
    });
}

/// 按所选方式对艺术家列表原地排序。
/// 名称比较不区分大小写，与 Navidrome 索引分组的顺序一致；
/// 专辑数相同时回退到名称 A→Z，保证多次排序结果稳定。
pub(super) fn sort_artists(artists: &mut [Artist], sort: ArtistSort) {
    match sort {
        ArtistSort::Name => artists.sort_by_key(artist_name_key),
        ArtistSort::NameDesc => {
            artists.sort_by_key(|artist| std::cmp::Reverse(artist_name_key(artist)))
        }
        ArtistSort::AlbumCount => artists.sort_by(|a, b| {
            b.album_count
                .unwrap_or(0)
                .cmp(&a.album_count.unwrap_or(0))
                .then_with(|| artist_name_key(a).cmp(&artist_name_key(b)))
        }),
    }
}

fn artist_name_key(artist: &Artist) -> String {
    artist.name.trim().to_lowercase()
}

/// 计算专辑分页的 offset；相邻页之间连续、不重叠。
pub(super) fn album_page_offset(page: usize, page_size: usize) -> u32 {
    (page as u32) * (page_size as u32)
}

#[cfg(test)]
mod tests;
