//! Login orchestration: cancellable network tasks, non-aborted store commits.

use super::bilibili_state::{AccountStatus, LoginError, LoginStage, LoginTarget, LoginUpdate};
use crate::app::prelude::*;
use crate::secrets::source_credentials::{
    self, CredentialError, CredentialStore, StagedCredential, SystemStore,
};
use crate::source::bilibili::{
    auth::{Account, AuthSession, PollResult, POLL_INTERVAL, QR_LIFETIME},
    client::{BilibiliClient, BilibiliError},
};
use crate::source::{ProviderKind, SourceId, SourceProfile};

impl NavidromeApp {
    pub(in crate::app) fn start_bilibili_login(
        &mut self,
        source: Option<SourceId>,
        cx: &Context<Self>,
    ) {
        let selected_proxy = self.bilibili_proxy_input.read(cx).value().to_string();
        let selected_proxy =
            (!selected_proxy.trim().is_empty()).then(|| selected_proxy.trim().to_string());
        let (source, account_id, proxy) = match source {
            Some(source) => {
                let Some(SourceProfile::Bilibili { account_id, .. }) = self
                    .config
                    .sources
                    .iter()
                    .find(|profile| profile.id() == source)
                else {
                    return;
                };
                if matches!(
                    self.bilibili_accounts.get(&source),
                    Some(AccountStatus::Removing)
                ) {
                    return;
                }
                (source, *account_id, selected_proxy)
            }
            None => (
                SourceId::new(
                    ProviderKind::Bilibili,
                    format!("{:032x}", rand::random::<u128>()),
                ),
                None,
                selected_proxy,
            ),
        };
        let client = match BilibiliClient::new(proxy.as_deref()) {
            Ok(client) => client,
            Err(error) => {
                self.state.settings.bilibili_login.cancel();
                self.state.settings.bilibili_login.stage =
                    LoginStage::Failed(LoginError::Protocol(error));
                return;
            }
        };
        let request = self.state.settings.bilibili_login.begin(LoginTarget {
            source,
            account_id,
            proxy,
        });
        let tx = self.tx.clone();
        let task = self.runtime.spawn(async move {
            let result = async {
                SystemStore
                    .check(true)
                    .await
                    .map_err(LoginError::Credentials)?;
                let qr = client.generate_qr().await.map_err(LoginError::Protocol)?;
                if tx
                    .send(Msg::BilibiliLogin {
                        request,
                        update: LoginUpdate::Qr(qr.png.clone()),
                    })
                    .is_err()
                {
                    return Ok(());
                }
                let deadline = tokio::time::Instant::now() + QR_LIFETIME;
                let mut scanned = false;
                loop {
                    let polled = tokio::time::timeout_at(deadline, client.poll_qr(&qr)).await;
                    match polled {
                        Err(_) | Ok(Ok(PollResult::Expired)) => {
                            let _ = tx.send(Msg::BilibiliLogin {
                                request,
                                update: LoginUpdate::Expired,
                            });
                            return Ok(());
                        }
                        Ok(Err(error)) => return Err(LoginError::Protocol(error)),
                        Ok(Ok(PollResult::WaitingScan)) => {}
                        Ok(Ok(PollResult::WaitingConfirmation)) => {
                            if !scanned {
                                scanned = true;
                                if tx
                                    .send(Msg::BilibiliLogin {
                                        request,
                                        update: LoginUpdate::Scanned,
                                    })
                                    .is_err()
                                {
                                    return Ok(());
                                }
                            }
                        }
                        Ok(Ok(PollResult::Confirmed(session))) => {
                            if tx
                                .send(Msg::BilibiliLogin {
                                    request,
                                    update: LoginUpdate::Verifying,
                                })
                                .is_err()
                            {
                                return Ok(());
                            }
                            let account = client
                                .verify_account(&session)
                                .await
                                .map_err(LoginError::Protocol)?;
                            let _ = tx.send(Msg::BilibiliLogin {
                                request,
                                update: LoginUpdate::Verified { account, session },
                            });
                            return Ok(());
                        }
                    }
                    tokio::time::sleep_until(
                        (tokio::time::Instant::now() + POLL_INTERVAL).min(deadline),
                    )
                    .await;
                }
            }
            .await;
            if let Err(error) = result {
                let _ = tx.send(Msg::BilibiliLogin {
                    request,
                    update: LoginUpdate::Failed(error),
                });
            }
        });
        self.state
            .settings
            .bilibili_login
            .attach(request, task.abort_handle());
    }

    pub(in crate::app) fn finish_bilibili_login(&mut self, request: u128, update: LoginUpdate) {
        if !self.state.settings.open || !self.state.settings.bilibili_login.accepts(request) {
            return;
        }
        match update {
            LoginUpdate::Qr(png) => {
                let login = &mut self.state.settings.bilibili_login;
                login.image = Some(Arc::new(GpuiImage::from_bytes(GpuiImageFormat::Png, png)));
                login.stage = LoginStage::WaitingScan;
            }
            LoginUpdate::Scanned => {
                self.state.settings.bilibili_login.stage = LoginStage::WaitingConfirmation
            }
            LoginUpdate::Verifying => {
                self.state.settings.bilibili_login.image = None;
                self.state.settings.bilibili_login.stage = LoginStage::Verifying;
            }
            LoginUpdate::Verified { account, session } => {
                let Some(mut target) = self.state.settings.bilibili_login.target.clone() else {
                    return;
                };
                self.state.settings.bilibili_login.finish_polling();
                if target.account_id.is_some_and(|id| id != account.id) {
                    self.state.settings.bilibili_login.stage =
                        LoginStage::Failed(LoginError::Protocol(BilibiliError::AccountMismatch));
                    return;
                }
                // Adding an already configured account is reauthorization of
                // that stable source, rather than a second cache namespace.
                if target.account_id.is_none() {
                    if let Some(profile) = self.config.sources.iter().find(|profile| matches!(profile, SourceProfile::Bilibili { account_id: Some(id), .. } if *id == account.id)) {
                        target.source = profile.id();
                        target.account_id = Some(account.id);
                    }
                }
                if matches!(
                    self.bilibili_accounts.get(&target.source),
                    Some(AccountStatus::Removing)
                ) {
                    self.state.settings.bilibili_login.cancel();
                    return;
                }
                self.state.settings.bilibili_login.target = Some(target.clone());
                self.state.settings.bilibili_login.stage = LoginStage::Saving;
                let tx = self.tx.clone();
                // Do not attach this mutation task to the polling AbortHandle.
                // Cancellation invalidates ownership; its completion rolls back.
                self.runtime.spawn(async move {
                    let result = StagedCredential::create(
                        Arc::new(SystemStore),
                        &target.source,
                        account.id,
                        session.encode_secret(),
                    )
                    .await;
                    let result = match result {
                        Ok(credential) => {
                            let restored =
                                SystemStore.load(credential.key()).await.and_then(|secret| {
                                    AuthSession::decode_secret(secret, account.id)
                                        .map_err(|_| CredentialError::Invalid)
                                });
                            restored.map(|_| credential)
                        }
                        Err(error) => Err(error),
                    };
                    let update = match result {
                        Ok(credential) => LoginUpdate::Stored {
                            account,
                            session,
                            credential,
                        },
                        Err(error) => LoginUpdate::Failed(LoginError::Credentials(error)),
                    };
                    let _ = tx.send(Msg::BilibiliLogin { request, update });
                });
            }
            LoginUpdate::Stored {
                account,
                session,
                credential,
            } => {
                self.commit_bilibili_login(account, session, credential);
            }
            LoginUpdate::Failed(error) => {
                self.state.settings.bilibili_login.finish_polling();
                self.state.settings.bilibili_login.stage = LoginStage::Failed(error);
            }
            LoginUpdate::Expired => {
                self.state.settings.bilibili_login.finish_polling();
                self.state.settings.bilibili_login.stage = LoginStage::Expired;
            }
        }
    }

    fn commit_bilibili_login(
        &mut self,
        account: Account,
        session: AuthSession,
        credential: StagedCredential,
    ) {
        let Some(target) = self.state.settings.bilibili_login.target.clone() else {
            return;
        };
        if self.state.settings.bilibili_login.stage != LoginStage::Saving
            || source_credentials::validate_reference(&target.source, account.id, credential.key())
                .is_err()
        {
            return;
        }
        let (next, old_key) = match crate::source::bilibili::account::prepare_config(
            &self.config,
            &target.source,
            &account,
            &session,
            credential.key(),
            target.proxy,
        ) {
            Ok(result) => result,
            Err(error) => {
                self.state.settings.bilibili_login.stage =
                    LoginStage::Failed(LoginError::Credentials(error));
                return;
            }
        };
        if config::save(&next).is_err() {
            self.state.settings.bilibili_login.stage = LoginStage::Failed(LoginError::Config);
            return;
        }
        credential.commit();
        self.cancel_bilibili_authorization_work(&target.source);
        self.config = next;
        self.bilibili_accounts
            .insert(target.source.clone(), AccountStatus::Ready(session));
        self.activate_bilibili_account(&target.source);
        self.state.settings.bilibili_login.cancel();
        self.notify_success(t!("Bilibili account connected"));
        if let Some(old_key) = old_key {
            if source_credentials::validate_reference(&target.source, account.id, &old_key).is_ok()
            {
                self.spawn_future(async move {
                    if SystemStore.delete(&old_key).await.is_err() {
                        log::warn!("could not remove previous Bilibili credential");
                    }
                });
            }
        }
    }

    pub(in crate::app) fn restore_bilibili_accounts(&mut self) {
        let profiles = self.config.sources.clone();
        for profile in profiles {
            let source = profile.id();
            let SourceProfile::Bilibili {
                account_id,
                credential_key,
                proxy,
                ..
            } = profile
            else {
                continue;
            };
            self.bilibili_accounts
                .insert(source.clone(), AccountStatus::Checking);
            let tx = self.tx.clone();
            self.spawn_future(async move {
                let result = async {
                    let account_id =
                        account_id.ok_or(LoginError::Credentials(CredentialError::Missing))?;
                    let key =
                        credential_key.ok_or(LoginError::Credentials(CredentialError::Missing))?;
                    source_credentials::validate_reference(&source, account_id, &key)
                        .map_err(LoginError::Credentials)?;
                    let secret = SystemStore
                        .load(&key)
                        .await
                        .map_err(LoginError::Credentials)?;
                    let session = AuthSession::decode_secret(secret, account_id)
                        .map_err(LoginError::Protocol)?;
                    let client =
                        BilibiliClient::new(proxy.as_deref()).map_err(LoginError::Protocol)?;
                    client
                        .verify_account(&session)
                        .await
                        .map_err(LoginError::Protocol)?;
                    Ok(session)
                }
                .await;
                let _ = tx.send(Msg::BilibiliAccount { source, result });
            });
        }
    }

    pub(in crate::app) fn finish_bilibili_account(
        &mut self,
        source: SourceId,
        result: Result<AuthSession, LoginError>,
    ) {
        if !matches!(
            self.bilibili_accounts.get(&source),
            Some(AccountStatus::Checking)
        ) {
            return;
        }
        self.bilibili_accounts.insert(
            source.clone(),
            match result {
                Ok(session) => AccountStatus::Ready(session),
                Err(error) => AccountStatus::Failed(error),
            },
        );
        self.activate_bilibili_account(&source);
    }

    fn activate_bilibili_account(&mut self, source: &SourceId) {
        let Some(api) = self.available_source(source) else {
            if self.config.active_source.as_ref() == Some(source) {
                self.state.status = t!("Bilibili login expired. Sign in again.").to_string();
                self.state.settings.open = true;
                self.state.settings.section = SettingsSection::Servers;
            }
            return;
        };
        if self.config.playback_source.as_ref() == Some(source) && self.playback_source.is_none() {
            self.playback_source = Some(api.clone());
            self.load_active_server_cache();
        } else if self
            .playback_source
            .as_ref()
            .is_some_and(|play| play.id() == source)
        {
            self.playback_source = Some(api);
        }
        if self.config.active_source.as_ref() == Some(source) {
            self.reconnect_active_server();
        }
    }

    /// Retire work carrying old cookies while leaving already playing audio intact.
    fn cancel_bilibili_authorization_work(&mut self, source: &SourceId) {
        self.start_resolution.cancel_source(source);
        self.preload_resolution.cancel_source(source);
        if self
            .playback_source
            .as_ref()
            .is_some_and(|api| api.id() == source)
        {
            self.cancel_next_preload();
            self.reconnect.cancel();
        }
    }

    pub(in crate::app) fn expire_bilibili_account(&mut self, source: &SourceId) {
        self.cancel_bilibili_authorization_work(source);
        self.bilibili_accounts.insert(
            source.clone(),
            AccountStatus::Failed(LoginError::Protocol(BilibiliError::NotAuthenticated)),
        );
        if self.api.as_ref().is_some_and(|api| api.id() == source) {
            self.api = None;
            self.state.reset_browsing();
        }
        if self.config.active_source.as_ref() == Some(source) {
            self.state.status = t!("Bilibili login expired. Sign in again.").to_string();
            self.state.settings.open = true;
            self.state.settings.section = SettingsSection::Servers;
        }
    }

    pub(in crate::app) fn remove_bilibili_account(&mut self, source: SourceId) {
        if matches!(
            self.bilibili_accounts.get(&source),
            Some(AccountStatus::Removing)
        ) {
            return;
        }
        let Some(SourceProfile::Bilibili { .. }) = self
            .config
            .sources
            .iter()
            .find(|profile| profile.id() == source)
        else {
            return;
        };
        self.state.settings.bilibili_login.cancel();
        self.retire_playback_source(&source);
        if self.api.as_ref().is_some_and(|api| api.id() == &source) {
            self.api = None;
            self.state.reset_browsing();
        }
        self.bilibili_accounts
            .insert(source.clone(), AccountStatus::Removing);
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = async {
                let scope = source_credentials::source_scope(&source)?;
                SystemStore.check(true).await?;
                SystemStore.delete_scope(&scope).await
            }
            .await;
            let _ = tx.send(Msg::BilibiliRemoved { source, result });
        });
    }

    pub(in crate::app) fn finish_bilibili_removal(
        &mut self,
        source: SourceId,
        result: Result<(), CredentialError>,
    ) {
        if !matches!(
            self.bilibili_accounts.get(&source),
            Some(AccountStatus::Removing)
        ) {
            return;
        }
        if let Err(error) = result {
            self.bilibili_accounts.insert(
                source,
                AccountStatus::Failed(LoginError::Credentials(error)),
            );
            return;
        }
        let mut next = self.config.clone();
        next.sources.retain(|profile| profile.id() != source);
        next.sync_active_profile();
        if config::save(&next).is_err() {
            self.bilibili_accounts
                .insert(source, AccountStatus::Failed(LoginError::Config));
            return;
        }
        let removing_active = self.config.active_source.as_ref() == Some(&source);
        self.config = next;
        self.bilibili_accounts.remove(&source);
        if removing_active {
            self.reconnect_active_server();
        }
        self.server_cache_writer.discard_source(&source);
        config::remove_source_cache(&source);
        self.prune_source_covers();
        self.notify_success(t!("Bilibili account removed"));
    }
}
