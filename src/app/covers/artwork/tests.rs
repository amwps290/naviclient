use super::*;

#[test]
fn extracts_palette_from_default_cover() {
    let palette = extract_cover_palette(include_bytes!("../../../../assets/default-cover.png"));
    assert!(palette.is_some());
}

#[test]
fn cover_request_needed_never_burns_an_id_against_a_palette_revisit() {
    // 网格/队列预取：已有图像、不需要调色板 → 不请求。
    assert!(!cover_request_needed(true, false, false));
    assert!(!cover_request_needed(true, true, false));
    // 预取过的封面在播放时必须能补提调色板（历史 bug：只增不减的
    // 请求集合把 id 永久“烧掉”，播放页/侧边栏动态配色拿不到数据）。
    assert!(cover_request_needed(true, false, true));
    // 调色板已在 → 不重复请求。
    assert!(!cover_request_needed(true, true, true));
    // 图像还没有 → 总要请求。
    assert!(cover_request_needed(false, false, false));
    assert!(cover_request_needed(false, false, true));
    assert!(cover_request_needed(false, true, false));
}
