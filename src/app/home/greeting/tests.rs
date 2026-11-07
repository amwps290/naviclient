use super::*;

#[test]
fn hour_boundaries_match_the_spec() {
    assert_eq!(greeting_for_hour(4), Greeting::Evening);
    assert_eq!(greeting_for_hour(5), Greeting::Morning);
    assert_eq!(greeting_for_hour(11), Greeting::Morning);
    assert_eq!(greeting_for_hour(12), Greeting::Afternoon);
    assert_eq!(greeting_for_hour(17), Greeting::Afternoon);
    assert_eq!(greeting_for_hour(18), Greeting::Evening);
    assert_eq!(greeting_for_hour(23), Greeting::Evening);
    assert_eq!(greeting_for_hour(0), Greeting::Evening);
}
