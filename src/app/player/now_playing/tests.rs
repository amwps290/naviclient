use super::*;

#[test]
fn opacity_falls_with_distance_and_never_below_the_floor() {
    let values = [1, 2, 3, 4, 10, usize::MAX].map(lyric_opacity);
    assert!(values.windows(2).all(|pair| pair[0] >= pair[1]));
    assert_eq!(lyric_opacity(usize::MAX), 0.55);
    assert!(values.iter().all(|value| *value >= 0.55));
}
