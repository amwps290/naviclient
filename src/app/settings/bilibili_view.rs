//! Bilibili login and removal dialogs, and account error messages.

use super::bilibili_state::{LoginError, LoginStage};
use super::state::SettingsDialog;
use crate::app::prelude::*;
use crate::models::network::SourceConnection;
use crate::secrets::source_credentials::CredentialError;
use crate::source::bilibili::client::BilibiliError;
use crate::source::{SourceId, SourceProfile};
use gpui_kit::component::dialog::Dialog;

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
                t!("Enter a valid proxy address.")
            }
            Self::Protocol(BilibiliError::NotAuthenticated | BilibiliError::Api(-101)) => {
                t!("Bilibili login expired. Sign in again.")
            }
            Self::Protocol(
                BilibiliError::Network | BilibiliError::Timeout | BilibiliError::Http(_),
            ) => {
                t!("Cannot reach Bilibili. Check the connection or proxy and retry.")
            }
            Self::Protocol(_) => t!("Bilibili returned an unexpected response. Retry signing in."),
        }
        .into()
    }
}

impl NavidromeApp {
    /// Sign in a saved Bilibili source again, or add a new account when
    /// `source` is `None`. A new account is saved only after login succeeds.
    pub(in crate::app) fn open_bilibili_login(
        &mut self,
        source: Option<SourceId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.start_bilibili_login(source, SourceConnection::System);
        if self.state.settings.bilibili_login.stage == LoginStage::Idle {
            return;
        }
        self.state
            .settings
            .open_dialog(SettingsDialog::BilibiliLogin);
        self.show_settings_dialog(window, cx);
    }

    pub(in crate::app) fn confirm_remove_bilibili(
        &mut self,
        source: SourceId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state
            .settings
            .open_dialog(SettingsDialog::RemoveBilibili(source));
        self.show_settings_dialog(window, cx);
    }

    pub(in crate::app) fn render_bilibili_login_dialog(
        &self,
        dialog: Dialog,
        cx: &Context<Self>,
    ) -> Dialog {
        let login = &self.state.settings.bilibili_login;
        let Some(target) = login.target.as_ref() else {
            // The login could not start, for example with an invalid proxy.
            let message = match login.stage {
                LoginStage::Failed(error) => error.message(),
                _ => t!("Sign in to Bilibili").into(),
            };
            return dialog
                .title(t!("Sign in to Bilibili"))
                .w(px(400.0))
                .child(div().text_sm().text_color(cx.theme().danger).child(message))
                .footer(
                    h_flex().justify_end().child(
                        Button::new("bilibili-close-login")
                            .label(t!("Cancel"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.cancel_settings_dialog(window, cx);
                            })),
                    ),
                );
        };
        let saved = self
            .config
            .sources
            .iter()
            .any(|profile| profile.id() == target.source);
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
            LoginStage::Idle => "".into(),
        };
        let failed = matches!(login.stage, LoginStage::Failed(_) | LoginStage::Expired);
        let working = matches!(
            login.stage,
            LoginStage::Generating | LoginStage::Verifying | LoginStage::Saving
        );
        let placeholder = if failed {
            KitIcon::TriangleAlert
        } else {
            KitIcon::LoaderCircle
        };
        let qr = div()
            .size(px(220.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(tokens::radius::COVER))
            .map(|frame| match login.image.clone() {
                // QR codes need a white quiet zone in dark mode as well.
                Some(image) => frame
                    .bg(gpui_kit::white())
                    .child(img(image).size(px(196.0)).object_fit(ObjectFit::Contain)),
                None => frame.bg(palette(cx).sunken).child(
                    Icon::new(placeholder)
                        .with_size(px(28.0))
                        .text_color(palette(cx).text_secondary),
                ),
            });
        let body = v_flex()
            .w_full()
            .gap_4()
            .items_center()
            .child(qr)
            .child(
                div()
                    .text_sm()
                    .text_center()
                    .when(failed, |text| text.text_color(cx.theme().danger))
                    .child(status),
            )
            .child(
                div()
                    .text_xs()
                    .text_center()
                    .text_color(palette(cx).text_secondary)
                    .child(t!("Credentials are kept in the system credential store.")),
            )
            // A new account chooses its connection here; changing it starts
            // a new QR code through that connection.
            .when(!saved, |body| {
                body.child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .text_xs()
                                .text_color(palette(cx).text_secondary)
                                .child(t!("Connection")),
                        )
                        .child(self.connection_picker(
                            "bilibili-login-connection",
                            &target.connection,
                            working,
                            |this, choice, _| this.start_bilibili_login(None, choice),
                            cx,
                        )),
                )
            });
        let footer = h_flex()
            .justify_end()
            .gap_2()
            .child(
                Button::new("bilibili-cancel-login")
                    .label(t!("Cancel"))
                    .ghost()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.cancel_settings_dialog(window, cx);
                    })),
            )
            .when(!working, |footer| {
                footer.child(
                    Button::new("bilibili-refresh-qr")
                        .label(t!("New QR code"))
                        .outline()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.restart_bilibili_login();
                            cx.notify();
                        })),
                )
            });
        dialog
            .title(if saved {
                t!("Sign in to Bilibili again")
            } else {
                t!("Add Bilibili account")
            })
            .w(px(400.0))
            .child(body)
            .footer(footer)
    }

    pub(in crate::app) fn render_remove_bilibili_dialog(
        &self,
        dialog: Dialog,
        source: &SourceId,
        cx: &Context<Self>,
    ) -> Dialog {
        let label = self
            .config
            .sources
            .iter()
            .find(|profile| profile.id() == *source)
            .and_then(|profile| match profile {
                SourceProfile::Bilibili { name, .. } if !name.trim().is_empty() => {
                    Some(name.trim().to_string())
                }
                _ => None,
            })
            .unwrap_or_else(|| "Bilibili".into());
        let remove = source.clone();
        let footer = h_flex()
            .justify_end()
            .gap_2()
            .child(
                Button::new("cancel-remove-bilibili")
                    .label(t!("Cancel"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.cancel_settings_dialog(window, cx);
                    })),
            )
            .child(
                Button::new("confirm-remove-bilibili")
                    .label(t!("Remove"))
                    .danger()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.remove_bilibili_account(remove.clone());
                        this.cancel_settings_dialog(window, cx);
                    })),
            );
        dialog
            .title(t!("Remove Bilibili account?"))
            .w(px(440.0))
            .child(
                v_flex()
                    .gap_2()
                    .child(div().text_sm().child(t!(
                        "Remove “%{label}” from this app?",
                        label = label
                    )))
                    .child(
                        div()
                            .text_xs()
                            .text_color(palette(cx).text_secondary)
                            .child(t!("Its saved login and local caches are deleted. Adding it again requires scanning a new QR code.")),
                    ),
            )
            .footer(footer)
    }
}
