//! Bilibili account controls in the existing connection settings.

use super::bilibili_state::{AccountStatus, LoginError, LoginStage};
use crate::app::prelude::*;
use crate::secrets::source_credentials::CredentialError;
use crate::source::bilibili::client::BilibiliError;
use crate::source::SourceProfile;

impl LoginError {
    pub(in crate::app) fn message(self) -> SharedString {
        match self {
            Self::Credentials(CredentialError::Missing) => {
                t!("Bilibili credentials are missing. Sign in again.")
            }
            Self::Credentials(CredentialError::Locked) => {
                t!("Unlock the system credential store and retry.")
            }
            Self::Credentials(CredentialError::Unavailable) => {
                t!("System credential store unavailable. Bilibili credentials cannot be saved.")
            }
            Self::Credentials(CredentialError::Invalid) => {
                t!("Bilibili credentials are invalid. Sign in again.")
            }
            Self::Config => t!("Could not save Bilibili account settings. Retry signing in."),
            Self::Protocol(BilibiliError::AccountMismatch) => {
                t!("Scan with the same Bilibili account, or add a new account.")
            }
            Self::Protocol(BilibiliError::InvalidProxy) => {
                t!("Enter a valid HTTP proxy address, or leave it empty.")
            }
            Self::Protocol(BilibiliError::NotAuthenticated | BilibiliError::Api(-101)) => {
                t!("Bilibili login expired. Sign in again.")
            }
            Self::Protocol(BilibiliError::Network | BilibiliError::Http(_)) => {
                t!("Cannot reach Bilibili. Check the connection or proxy and retry.")
            }
            Self::Protocol(_) => t!("Bilibili returned an unexpected response. Retry signing in."),
        }
        .into()
    }
}

impl NavidromeApp {
    pub(in crate::app) fn render_bilibili_settings(
        &self,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let login = &self.state.settings.bilibili_login;
        let mut card = v_flex().w_full().gap_3()
            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(t!("Bilibili accounts")))
            .child(div().text_xs().text_color(palette(cx).text_secondary)
                .child(t!("Sign in with the Bilibili App. Credentials are kept in the system credential store.")));
        if login.stage == LoginStage::Idle {
            card = card.child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(palette(cx).text_secondary)
                            .child(t!("Bilibili proxy (optional)")),
                    )
                    .child(Input::new(&self.bilibili_proxy_input).w_full()),
            );
        }
        for profile in &self.config.sources {
            let SourceProfile::Bilibili {
                name, account_id, ..
            } = profile
            else {
                continue;
            };
            let source = profile.id();
            let status = match self.bilibili_accounts.get(&source) {
                Some(AccountStatus::Ready(session))
                    if Some(session.account_id()) == *account_id =>
                {
                    t!("Signed in").into()
                }
                Some(AccountStatus::Checking) => t!("Verifying Bilibili account…").into(),
                Some(AccountStatus::Removing) => t!("Removing Bilibili account…").into(),
                Some(AccountStatus::Failed(error)) => error.message(),
                _ => t!("Sign in to Bilibili").into(),
            };
            let removing = matches!(
                self.bilibili_accounts.get(&source),
                Some(AccountStatus::Removing)
            );
            let reauth = source.clone();
            let remove = source.clone();
            let browse = source.clone();
            let ready = matches!(
                self.bilibili_accounts.get(&source),
                Some(AccountStatus::Ready(_))
            );
            card = card.child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(div().text_sm().truncate().child(if name.is_empty() {
                                "Bilibili".to_string()
                            } else {
                                name.clone()
                            }))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(palette(cx).text_secondary)
                                    .child(status),
                            ),
                    )
                    .child(
                        Button::new(SharedString::from(format!(
                            "bilibili-browse-{}",
                            source.cache_namespace()
                        )))
                        .label(t!("Browse favorites"))
                        .outline()
                        .small()
                        .disabled(!ready || removing)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.switch_bilibili_source(&browse);
                            cx.notify();
                        })),
                    )
                    .child(
                        Button::new(SharedString::from(format!(
                            "bilibili-reauth-{}",
                            source.cache_namespace()
                        )))
                        .label(t!("Sign in again"))
                        .ghost()
                        .small()
                        .disabled(removing)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.start_bilibili_login(Some(reauth.clone()), cx);
                            cx.notify();
                        })),
                    )
                    .child(
                        Button::new(SharedString::from(format!(
                            "bilibili-remove-{}",
                            source.cache_namespace()
                        )))
                        .label(t!("Remove account"))
                        .ghost()
                        .small()
                        .disabled(removing)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.remove_bilibili_account(remove.clone());
                            cx.notify();
                        })),
                    ),
            );
        }
        if login.stage == LoginStage::Idle {
            card = card.child(
                v_flex().gap_2().child(
                    Button::new("bilibili-add-account")
                        .label(t!("Add Bilibili account"))
                        .ghost()
                        .small()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.start_bilibili_login(None, cx);
                            cx.notify();
                        })),
                ),
            );
        } else {
            let status: SharedString = match login.stage {
                LoginStage::Generating => t!("Preparing Bilibili QR login…").into(),
                LoginStage::WaitingScan => t!("Scan with the Bilibili App to sign in.").into(),
                LoginStage::WaitingConfirmation => {
                    t!("Scanned. Confirm login in the Bilibili App.").into()
                }
                LoginStage::Verifying => t!("Verifying Bilibili account…").into(),
                LoginStage::Saving => t!("Saving Bilibili credentials…").into(),
                LoginStage::Expired => t!("QR code expired. Generate a new one.").into(),
                LoginStage::Failed(error) => error.message(),
                LoginStage::Idle => unreachable!(),
            };
            card = card.child(
                v_flex()
                    .gap_3()
                    .items_start()
                    .when_some(login.image.clone(), |this, image| {
                        this.child(img(image).size(px(220.0)).object_fit(ObjectFit::Contain))
                    })
                    .child(div().text_sm().child(status))
                    .child(
                        h_flex()
                            .gap_2()
                            .when(
                                !matches!(
                                    login.stage,
                                    LoginStage::Generating
                                        | LoginStage::Verifying
                                        | LoginStage::Saving
                                ),
                                |this| {
                                    this.child(
                                        Button::new("bilibili-refresh-qr")
                                            .label(t!("New QR code"))
                                            .ghost()
                                            .small()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                let source = this
                                                    .state
                                                    .settings
                                                    .bilibili_login
                                                    .target
                                                    .as_ref()
                                                    .filter(|target| target.account_id.is_some())
                                                    .map(|target| target.source.clone());
                                                this.start_bilibili_login(source, cx);
                                                cx.notify();
                                            })),
                                    )
                                },
                            )
                            .child(
                                Button::new("bilibili-cancel-login")
                                    .label(t!("Cancel"))
                                    .ghost()
                                    .small()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.state.settings.bilibili_login.cancel();
                                        cx.notify();
                                    })),
                            ),
                    ),
            );
        }
        card.into_any_element()
    }
}
