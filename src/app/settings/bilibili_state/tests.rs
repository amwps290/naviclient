use super::*;
use crate::source::ProviderKind;

fn target() -> LoginTarget {
    LoginTarget {
        source: SourceId::new(ProviderKind::Bilibili, "account"),
        account_id: Some(123),
        proxy: None,
    }
}

#[tokio::test]
async fn refreshing_or_closing_settings_aborts_polling_and_rejects_late_results() {
    let mut settings = crate::app::settings::state::SettingsUi::default();
    settings.open = true;
    let first = settings.bilibili_login.begin(target());
    let task = tokio::spawn(std::future::pending::<()>());
    settings.bilibili_login.attach(first, task.abort_handle());
    let second = settings.bilibili_login.begin(target());
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(!settings.bilibili_login.accepts(first));
    assert!(settings.bilibili_login.accepts(second));
    let task = tokio::spawn(std::future::pending::<()>());
    settings.bilibili_login.attach(second, task.abort_handle());
    settings.close();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(!settings.bilibili_login.accepts(second));
    assert!(settings.bilibili_login.target.is_none());
    assert_eq!(settings.bilibili_login.stage, LoginStage::Idle);
}

#[tokio::test]
async fn navigation_and_escape_cancel_login_without_changing_the_queue() {
    let mut state = crate::app::shell::state::AppState::default();
    state.settings.open = true;
    let request = state.settings.bilibili_login.begin(target());
    state.navigate_to(crate::app::shell::view_state::View::Home);
    assert!(!state.settings.bilibili_login.accepts(request));
    state.settings.open = true;
    let request = state.settings.bilibili_login.begin(target());
    state.close_top_overlay();
    assert!(!state.settings.bilibili_login.accepts(request));
    assert!(state.queue.is_empty());
}
