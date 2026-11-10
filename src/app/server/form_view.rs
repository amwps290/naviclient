//! Server dialog and the shared label/control form row.

use crate::app::prelude::*;
use crate::app::settings::state::{ServerDialog, SettingsDialog};
use crate::source::{ProviderKind, SourceId};
use gpui_kit::component::dialog::Dialog;

impl NavidromeApp {
    /// The add/edit server dialog. Saving the browsing server's address,
    /// account or connection reconnects; other edits are a plain save.
    pub(in crate::app) fn render_server_dialog(
        &self,
        dialog: Dialog,
        state: &ServerDialog,
        cx: &Context<Self>,
    ) -> Dialog {
        let target = self.state.settings.server_form_target(&self.config);
        if state.target.is_some() && target.is_none() {
            // The server was removed while its dialog was open.
            return dialog.title(t!("Server unavailable"));
        }
        let reconnect = match target {
            None => true,
            Some(target) => {
                let source = SourceId::new(ProviderKind::OpenSubsonic, &target.id);
                self.server_form(cx).requires_reconnect(
                    &self.config,
                    target,
                    &target.resolved_password(),
                ) || (self.config.active_source.as_ref() == Some(&source)
                    && self.config.source_connection(&source) != state.connection)
            }
        };
        let title = match target {
            Some(target) => t!("Edit “%{editing}”", editing = target.label()),
            None => t!("Add Navidrome server"),
        };
        let form = v_flex()
            .w_full()
            .gap_3()
            .child(self.render_server_field(t!("Display name"), &self.name_input, cx))
            .child(self.render_server_field(t!("Server URL"), &self.server_input, cx))
            .child(self.render_server_field(t!("Username"), &self.username_input, cx))
            .child(self.render_server_field(t!("Password"), &self.password_input, cx))
            .child(self.render_form_row(
                t!("Connection"),
                self.connection_picker(
                    "server-dialog-connection",
                    &state.connection,
                    false,
                    |this, choice, _| {
                        if let Some(SettingsDialog::Server(server)) =
                            this.state.settings.dialog.as_mut()
                        {
                            server.connection = choice;
                        }
                    },
                    cx,
                ),
                cx,
            ));
        let footer = h_flex()
            .justify_end()
            .gap_2()
            .child(
                Button::new("cancel-server-edit")
                    .label(t!("Cancel"))
                    .ghost()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.cancel_settings_dialog(window, cx);
                    })),
            )
            .child(
                Button::new("save-settings")
                    .label(if reconnect {
                        t!("Save and connect")
                    } else {
                        t!("Save")
                    })
                    .primary()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.save_settings(cx);
                        this.sync_settings_dialog(window, cx);
                        cx.notify();
                    })),
            );
        dialog.title(title).w(px(520.0)).child(form).footer(footer)
    }

    fn render_server_field(
        &self,
        label: impl Into<SharedString>,
        input: &Entity<InputState>,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        self.render_form_row(
            label,
            Input::new(input)
                .w_full()
                .bg(palette(cx).sunken)
                .into_any_element(),
            cx,
        )
    }

    /// A label column of fixed width, so every control starts at the same x.
    pub(in crate::app) fn render_form_row(
        &self,
        label: impl Into<SharedString>,
        control: gpui_kit::AnyElement,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let label: SharedString = label.into();
        h_flex()
            .w_full()
            .gap_3()
            .items_center()
            .child(
                div()
                    .w(px(110.0))
                    .flex_none()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(label),
            )
            .child(div().flex_1().min_w_0().flex().child(control))
            .into_any_element()
    }
}
