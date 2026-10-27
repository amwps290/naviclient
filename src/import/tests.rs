//! 链接识别测试：分享文案抽取、网易云各路径形态、短链边界。

use super::*;

fn source(url: &str) -> Option<PlaylistSource> {
    PlaylistSource::parse_known_url(url)
}

#[test]
fn parses_netease_playlist_url_shapes() {
    let expected = Some(PlaylistSource::Netease {
        id: "3778678".to_string(),
    });
    // 网页版 hash 路由：?id= 在 URL fragment 里，标准解析器不认。
    assert_eq!(
        source("https://music.163.com/#/playlist?id=3778678"),
        expected
    );
    assert_eq!(
        source("https://music.163.com/playlist?id=3778678"),
        expected
    );
    assert_eq!(
        source("https://music.163.com/m/playlist?id=3778678"),
        expected
    );
    // 分享链接常带 userid，id 取第一个查询参数。
    assert_eq!(
        source("https://music.163.com/playlist?id=3778678&userid=123456"),
        expected
    );
    // hash 路由的 id 必须绑定到 hash 中的 playlist，不能误取外层 query。
    assert_eq!(
        source("https://music.163.com/song?id=1#/playlist?id=3778678"),
        expected
    );
}

#[test]
fn rejects_non_playlist_or_foreign_links() {
    // 单曲、专辑、电台和 QQ 域名都不是歌单源。
    assert!(source("https://music.163.com/#/song?id=1").is_none());
    assert!(source("https://music.163.com/#/album?id=1").is_none());
    assert!(source("https://y.qq.com/n/ryqq/playlist/7251112391").is_none());
    // 域名必须精确匹配，不能只是在路径或恶意后缀中出现。
    assert!(source("https://example.com/music.163.com/playlist?id=1").is_none());
    assert!(source("https://music.163.com.evil.example/playlist?id=1").is_none());
    assert!(!is_netease_short_url(
        "http://127.0.0.1:8080/?next=163cn.tv"
    ));
    assert!(is_netease_short_url("https://163cn.tv/a1B2c3"));
    // 纯文本。
    assert!(source("不是链接").is_none());
}

#[test]
fn extract_url_pulls_link_from_share_text() {
    assert_eq!(
        extract_url("看看这个歌单 https://163cn.tv/a1B2c3 来自@网易云音乐"),
        Some("https://163cn.tv/a1B2c3")
    );
    assert_eq!(
        extract_url("https://music.163.com/playlist?id=3778678"),
        Some("https://music.163.com/playlist?id=3778678")
    );
    // CJK 字符与全角括号都终止链接。
    assert_eq!(
        extract_url("（https://163cn.tv/xyz）"),
        Some("https://163cn.tv/xyz")
    );
    // 尾部标点被剥掉。
    assert_eq!(
        extract_url("链接：https://music.163.com/m/playlist?id=5."),
        Some("https://music.163.com/m/playlist?id=5")
    );
    assert_eq!(
        extract_url("先看 https://music.163.com/playlist?id=8 再看 http://example.com"),
        Some("https://music.163.com/playlist?id=8")
    );
    assert_eq!(extract_url("没有链接的文案"), None);
}

#[test]
fn query_id_ignores_other_keys_and_rejects_non_numeric_values() {
    assert_eq!(query_id("userid=99&id=7"), Some("7".to_string()));
    assert_eq!(query_id("id=7&userid=99"), Some("7".to_string()));
    assert_eq!(query_id("id="), None);
    assert_eq!(query_id("id=7x"), None);
    assert_eq!(query_id(""), None);
}
