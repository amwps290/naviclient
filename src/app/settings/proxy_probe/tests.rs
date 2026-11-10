use super::*;
use std::time::Duration;

#[test]
fn a_result_is_shown_only_for_the_latest_test_of_that_proxy() {
    let mut probes = ProxyProbes::default();
    let first = probes.begin("clash");
    assert_eq!(probes.get("clash"), Some(&ProbeState::Testing));
    let second = probes.begin("clash");
    assert!(!probes.complete("clash", first, Ok(Duration::from_millis(10))));
    assert_eq!(probes.get("clash"), Some(&ProbeState::Testing));
    assert!(probes.complete("clash", second, Err(RouteProbeError::Timeout)));
    assert_eq!(
        probes.get("clash"),
        Some(&ProbeState::Failed(RouteProbeError::Timeout))
    );
}

#[test]
fn editing_or_removing_a_proxy_discards_its_pending_and_finished_results() {
    let mut probes = ProxyProbes::default();
    let request = probes.begin("clash");
    probes.forget("clash");
    assert!(!probes.complete("clash", request, Ok(Duration::from_millis(10))));
    assert_eq!(probes.get("clash"), None);
    let request = probes.begin("clash");
    assert!(probes.complete("clash", request, Ok(Duration::from_millis(42))));
    assert_eq!(
        probes.get("clash"),
        Some(&ProbeState::Reachable(Duration::from_millis(42)))
    );
    probes.forget("clash");
    assert_eq!(probes.get("clash"), None);
}

#[test]
fn proxies_are_tested_independently() {
    let mut probes = ProxyProbes::default();
    let a = probes.begin("a");
    let b = probes.begin("b");
    assert!(probes.complete("b", b, Ok(Duration::from_millis(5))));
    assert_eq!(probes.get("a"), Some(&ProbeState::Testing));
    assert!(probes.complete("a", a, Err(RouteProbeError::Unreachable)));
}
