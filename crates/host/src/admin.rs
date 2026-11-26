//! Admin API under `/api/admin`, reachable only with the per-launch admin token.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use activity::ActivityService;
use alerts::{AlertKindName, AlertsService};
use axum::{
    Json, Router,
    extract::{Path, Request, State, rejection::JsonRejection},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, patch, post, put},
};
use chat::ChatService;
use checkin::CheckinService;
use integrations::{
    secrets::SecretStore,
    tls::{ClientConfig, HttpsClient},
    twitch::{
        FEATURE_SCOPES, MVP_SCOPES,
        auth::{AuthError, AuthStatus, DeviceLogin, TwitchAuth},
        eventsub::EventSub,
        helix::{Helix, HelixError},
        irc::FirstChats,
        streams::StreamCheck,
    },
};
use redemption::RedemptionService;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use sound::SoundService;
use stream::StreamService;
use tokio::{sync::RwLock, task::AbortHandle};
use toolkit_core::{
    events::{Alert, AlertKind, ChatMessage, Fragment, Source, StreamChange},
    modules::{self, ModuleError, ModuleState, Modules},
    outbox::Outbox,
    settings::{self, ExternalService},
};
use viewers::ViewerService;

use utoipa::ToSchema;

use crate::TwitchEndpoints;

pub(super) mod chat_send;
pub(super) mod checkin_admin;
pub(super) mod general_settings;
pub(super) mod rewards;
pub(super) mod simulate;
pub(super) mod stream_info;
pub(super) mod twitch_account;

use chat_send::chat_send;
use checkin_admin::{checkin_outbox, checkin_rewards, retry_dead_letter, skip_dead_letter};
use general_settings::{first_run, general, list_modules, set_first_run, set_general, set_module};
use rewards::{create_reward, delete_reward, list_rewards, update_reward};
use simulate::{simulate_alert, simulate_chat, simulate_checkin};
use stream_info::{search_categories, set_stream_info, stream_info};
use twitch_account::{login, logout, set_client_id, twitch_overview};

pub(crate) use twitch_account::TwitchProfiles;

pub(crate) struct AdminState {
    token: SecretString,
    data_dir: PathBuf,
    secrets: Arc<dyn SecretStore>,
    endpoints: TwitchEndpoints,
    http: HttpsClient,
    tls: Arc<ClientConfig>,
    chat: Arc<ChatService>,
    alerts: Arc<AlertsService>,
    redemption: Arc<RedemptionService>,
    checkin: Arc<CheckinService>,
    counters: Arc<counter::Counters>,
    sounds: Arc<SoundService>,
    screen_tools: Arc<overlays::Overlays>,
    stream: Arc<StreamService>,
    activity: Arc<ActivityService>,
    viewers: Arc<ViewerService>,
    outbox: Arc<Outbox>,
    modules: Arc<Modules>,
    /// Shared with [`TwitchProfiles`], which looks up profile images with the same sign-in.
    twitch: Arc<RwLock<TwitchAuth>>,
    /// The EventSub task belongs to the current TwitchAuth and is replaced with it.
    eventsub: Mutex<Option<AbortHandle>>,
    /// Which EventSub task may report (bumped when one is replaced), and whether its session is subscribed.
    eventsub_connected: Arc<Mutex<(u64, bool)>>,
    /// The same for the anonymous IRC connection that reads Twitch's first-chat mark.
    irc_connected: Arc<Mutex<(u64, bool)>>,
}

impl AdminState {
    // One argument per dependency the composition root hands over; a wrapper struct would only rename them.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        token: SecretString,
        data_dir: PathBuf,
        secrets: Arc<dyn SecretStore>,
        endpoints: TwitchEndpoints,
        tls: Arc<ClientConfig>,
        http: HttpsClient,
        twitch: Arc<RwLock<TwitchAuth>>,
        chat: Arc<ChatService>,
        alerts: Arc<AlertsService>,
        redemption: Arc<RedemptionService>,
        checkin: Arc<CheckinService>,
        counters: Arc<counter::Counters>,
        sounds: Arc<SoundService>,
        screen_tools: Arc<overlays::Overlays>,
        stream: Arc<StreamService>,
        activity: Arc<ActivityService>,
        viewers: Arc<ViewerService>,
        outbox: Arc<Outbox>,
        modules: Arc<Modules>,
    ) -> Self {
        let current = twitch.try_read().expect("not locked yet").clone();
        let state = Self {
            token,
            data_dir,
            secrets,
            endpoints,
            http,
            tls,
            chat,
            alerts,
            redemption,
            checkin,
            counters,
            sounds,
            screen_tools,
            stream,
            activity,
            viewers,
            outbox,
            modules,
            twitch,
            eventsub: Mutex::new(None),
            eventsub_connected: Arc::new(Mutex::new((0, false))),
            irc_connected: Arc::new(Mutex::new((0, false))),
        };
        state.start_eventsub(current);
        state
    }

    /// Starts or stops the Twitch connections for the saved choice of services, with the current sign-in.
    pub(crate) async fn apply_services(&self) {
        let auth = self.twitch.read().await.clone();
        self.start_eventsub(auth);
    }

    /// Whether the Twitch event connection is up right now.
    pub(crate) fn eventsub_up(&self) -> bool {
        self.eventsub_connected.lock().unwrap().1
    }

    /// Connects chat (and its check-in and counter commands), alerts, redemptions (and their reward sounds) and the stream's
    /// start and end for `auth`, and checks once a minute whether the stream is live (both wait until signed in);
    /// stops the tasks of the previous auth.
    pub(crate) fn start_eventsub(&self, auth: TwitchAuth) {
        // Not using Twitch: the old connections stop and none start (no events, no stream checks).
        if settings::load(&self.data_dir).unused(ExternalService::Twitch) {
            if let Some(old) = self.eventsub.lock().unwrap().take() {
                old.abort();
            }
            for state in [&self.eventsub_connected, &self.irc_connected] {
                let mut c = state.lock().unwrap();
                *c = (c.0 + 1, false);
            }
            // Nothing tells whether the stream is live any more: not "still live" from before.
            self.stream.receive(StreamChange::SignedOut);
            return;
        }
        let chat = self.chat.clone();
        let removals = self.chat.clone();
        let alerts = self.alerts.clone();
        let activity = self.activity.clone();
        let redemption = self.redemption.clone();
        let checkin = self.checkin.clone();
        let counters = self.counters.clone();
        let marathon = self.counters.clone();
        let sounds = self.sounds.clone();
        let media = self.screen_tools.clone();
        // One handle per delivery: chat, alerts and redemptions each record their viewer.
        let (chat_viewers, alert_viewers, redemption_viewers) = (
            self.viewers.clone(),
            self.viewers.clone(),
            self.viewers.clone(),
        );
        let (stream, checked) = (self.stream.clone(), self.stream.clone());
        // A replaced task may still report before it is aborted; only the newest one counts.
        let reporter = |state: &Arc<Mutex<(u64, bool)>>| -> Arc<dyn Fn(bool) + Send + Sync> {
            let state = state.clone();
            let generation = {
                let mut c = state.lock().unwrap();
                *c = (c.0 + 1, false);
                c.0
            };
            Arc::new(move |up| {
                let mut c = state.lock().unwrap();
                if c.0 == generation {
                    c.1 = up;
                }
            })
        };
        let eventsub_connected = reporter(&self.eventsub_connected);
        let irc_connected = reporter(&self.irc_connected);
        // While Twitch's first-chat mark cannot be read, a viewer the toolkit never saw before counts instead.
        let irc_up = self.irc_connected.clone();
        let (first_chats, fallback_chat) = (self.chat.clone(), self.chat.clone());
        let check = StreamCheck {
            helix: Helix::new(&self.endpoints.helix, auth.clone(), self.http.clone()),
            deliver: Arc::new(move |c| checked.receive(c)),
        };
        let eventsub = EventSub {
            url: self.endpoints.eventsub.clone(),
            helix: Helix::new(&self.endpoints.helix, auth.clone(), self.http.clone()),
            tls: self.tls.clone(),
            deliver: Arc::new(move |m| {
                let new = chat_viewers.receive_chat_first_time(&m, Source::Twitch);
                if !irc_up.lock().unwrap().1 {
                    let (chat, id) = (fallback_chat.clone(), m.id.clone());
                    tokio::spawn(async move {
                        if new.await {
                            chat.first_chat(id);
                        }
                    });
                }
                checkin.receive_chat(&m, Source::Twitch);
                counters.receive_chat(&m, Source::Twitch);
                chat.receive(m, Source::Twitch)
            }),
            deliver_alert: Arc::new(move |a| {
                alert_viewers.receive_alert(&a, Source::Twitch);
                marathon.receive_alert(&a, Source::Twitch);
                activity.receive(a.clone(), Source::Twitch);
                alerts.receive(a, Source::Twitch)
            }),
            // Reward sounds take the in-memory path, not the outbox: a sound played late is worse than none.
            deliver_redemption: Arc::new(move |r| {
                redemption_viewers.receive_redemption(&r, Source::Twitch);
                sounds.receive_redemption(&r);
                media.receive_redemption(&r);
                redemption.receive(r, Source::Twitch)
            }),
            deliver_stream: Arc::new(move |c| stream.receive(c)),
            deliver_removal: Arc::new(move |channel, r| {
                let chat = removals.clone();
                // Let a removal already received finish even if this EventSub session gets replaced.
                let write = tokio::spawn(async move { chat.remove(channel, r).await });
                Box::pin(async move {
                    if let Err(e) = write.await {
                        tracing::error!(error = %e, "chat removal task failed");
                    }
                })
            }),
            connected: eventsub_connected,
        };
        let irc = FirstChats {
            url: self.endpoints.irc.clone(),
            auth: auth.clone(),
            tls: self.tls.clone(),
            deliver: Arc::new(move |id| first_chats.first_chat(id)),
            connected: irc_connected,
        };
        let task = tokio::spawn(async move {
            tokio::join!(eventsub.run(), check.run(), irc.run());
        });
        if let Some(old) = self.eventsub.lock().unwrap().replace(task.abort_handle()) {
            old.abort();
        }
    }
}

pub(crate) fn routes(state: Arc<AdminState>) -> Router {
    Router::new()
        .route("/api/admin/twitch", get(twitch_overview))
        .route("/api/admin/twitch/client-id", put(set_client_id))
        .route("/api/admin/twitch/login", post(login))
        .route("/api/admin/twitch/logout", post(logout))
        .route("/api/admin/general", get(general).put(set_general))
        .route("/api/admin/first-run", get(first_run).put(set_first_run))
        .route("/api/admin/modules", get(list_modules))
        .route("/api/admin/modules/{id}", put(set_module))
        .route("/api/admin/checkin/rewards", get(checkin_rewards))
        .route(
            "/api/admin/stream-info",
            get(stream_info).put(set_stream_info),
        )
        .route("/api/admin/stream-info/categories", get(search_categories))
        .route("/api/admin/chat/send", post(chat_send))
        .route("/api/admin/rewards", get(list_rewards).post(create_reward))
        .route(
            "/api/admin/rewards/{id}",
            patch(update_reward).delete(delete_reward),
        )
        .route("/api/admin/checkin/outbox", get(checkin_outbox))
        .route(
            "/api/admin/checkin/outbox/{event_id}/retry",
            post(retry_dead_letter),
        )
        .route(
            "/api/admin/checkin/outbox/{event_id}/skip",
            post(skip_dead_letter),
        )
        .route("/api/admin/simulate/chat", post(simulate_chat))
        .route("/api/admin/simulate/alert", post(simulate_alert))
        .route("/api/admin/simulate/checkin", post(simulate_checkin))
        .layer(middleware::from_fn_with_state(state.clone(), require_token))
        .with_state(state)
}

pub(crate) async fn require_token(
    State(s): State<Arc<AdminState>>,
    req: Request,
    next: Next,
) -> Response {
    let ok = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|given| {
            constant_time_eq(given.as_bytes(), s.token.expose_secret().as_bytes())
        });
    if !ok {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    next.run(req).await
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn auth_error(e: AuthError) -> Response {
    let status = match e {
        AuthError::ClientIdRequired | AuthError::SignInRequired => StatusCode::CONFLICT,
        AuthError::Unreachable => StatusCode::BAD_GATEWAY,
        AuthError::StorageFailed => StatusCode::INTERNAL_SERVER_ERROR,
        _ => StatusCode::BAD_REQUEST,
    };
    error(status, &e.to_string())
}

/// Body of the errors admin handlers return: a stable `<area>.<reason>` code the admin UI translates.
/// The missing-token 401 has an empty body, and malformed JSON gets axum's plain-text rejection.
#[derive(Serialize, ToSchema)]
pub(crate) struct ErrorBody {
    code: String,
}

pub(crate) fn error(status: StatusCode, code: &str) -> Response {
    (status, Json(ErrorBody { code: code.into() })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_eq_matches_only_equal_input() {
        assert!(constant_time_eq(b"token", b"token"));
        assert!(!constant_time_eq(b"token", b"tokem"));
        assert!(!constant_time_eq(b"token", b"tok"));
    }
}
