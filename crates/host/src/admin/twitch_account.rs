use super::*;
use std::time::Duration;

#[derive(Serialize, ToSchema)]
pub(crate) struct Overview {
    client_id: String,
    status: AuthStatus,
    /// The EventSub session is connected and subscribed (chat, alerts, redemptions arrive).
    eventsub_connected: bool,
    /// Twitch's first-chat mark can be read (anonymous IRC joined); otherwise 第一次聊天 is the toolkit's first
    /// sighting of the viewer.
    irc_connected: bool,
    /// The scopes the current sign-in granted; empty while signed out. Features check theirs against it.
    scopes: Vec<String>,
    /// Present while a chat record or moderation update is waiting for storage to recover.
    #[serde(skip_serializing_if = "Option::is_none")]
    chat_write_delayed: Option<bool>,
    /// Storage operations refused since launch after the bounded chat queue filled.
    #[serde(skip_serializing_if = "Option::is_none")]
    chat_missed_writes: Option<u64>,
}

#[utoipa::path(get, path = "/api/admin/twitch", tag = "admin", responses((status = 200, body = Overview)))]
pub(super) async fn twitch_overview(State(s): State<Arc<AdminState>>) -> Json<Overview> {
    let twitch = s.twitch.read().await;
    Json(Overview {
        client_id: twitch.client_id().to_owned(),
        status: twitch.status(),
        eventsub_connected: s.eventsub_connected.lock().unwrap().1,
        irc_connected: s.irc_connected.lock().unwrap().1,
        scopes: twitch.token().map(|t| t.scopes).unwrap_or_default(),
        chat_write_delayed: s.chat.write_delayed().then_some(true),
        chat_missed_writes: Some(s.chat.missed_writes()).filter(|n| *n > 0),
    })
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct ClientIdBody {
    client_id: String,
}

/// Twitch Client IDs are short ASCII alphanumerics; anything else is rejected before it reaches settings.
fn valid_client_id(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

#[utoipa::path(
    put,
    path = "/api/admin/twitch/client-id",
    tag = "admin",
    request_body = ClientIdBody,
    responses(
        (status = 204),
        (status = 400, body = ErrorBody),
        (status = 500, body = ErrorBody),
    )
)]
pub(super) async fn set_client_id(
    State(s): State<Arc<AdminState>>,
    Json(body): Json<ClientIdBody>,
) -> Response {
    let client_id = body.client_id.trim().to_owned();
    if !valid_client_id(&client_id) {
        return error(StatusCode::BAD_REQUEST, "admin.invalid_client_id");
    }
    // Write lock first so two saves cannot interleave; an unchanged ID keeps the current sign-in.
    let mut twitch = s.twitch.write().await;
    if twitch.client_id() == client_id {
        return StatusCode::NO_CONTENT.into_response();
    }
    if settings::update(&s.data_dir, |set| set.twitch_client_id = client_id.clone()).is_err() {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "settings.save_failed");
    }
    // Tokens belong to the previous app registration, so a new Client ID starts signed out.
    let _ = twitch.logout();
    *twitch = TwitchAuth::new(
        client_id,
        &s.endpoints.oauth,
        s.http.clone(),
        s.secrets.clone(),
    );
    s.start_eventsub(twitch.clone());
    StatusCode::NO_CONTENT.into_response()
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct LoginBody {
    /// Scopes a feature needs, kept for every later sign-in too; each must be in `FEATURE_SCOPES`.
    #[serde(default)]
    add_scopes: Vec<String>,
}

#[utoipa::path(
    post,
    path = "/api/admin/twitch/login",
    tag = "admin",
    request_body(content = Option<LoginBody>, description = "Absent: sign in with the base and the saved feature scopes."),
    responses(
        (status = 200, body = DeviceLogin),
        (status = 400, description = "`twitch.unknown_scope`, or a sign-in error", body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 500, description = "`settings.save_failed`", body = ErrorBody),
        (status = 502, body = ErrorBody),
    )
)]
pub(super) async fn login(
    State(s): State<Arc<AdminState>>,
    body: Option<Json<LoginBody>>,
) -> Response {
    let add = body.map(|Json(b)| b.add_scopes).unwrap_or_default();
    if add.iter().any(|a| !FEATURE_SCOPES.contains(&a.as_str())) {
        return error(StatusCode::BAD_REQUEST, "twitch.unknown_scope");
    }
    let saved = settings::load(&s.data_dir).twitch_scopes;
    // Saved scopes outside the list (an edited settings.json) are left out rather than sent to Twitch.
    let mut scopes: Vec<&str> = MVP_SCOPES.to_vec();
    for x in saved.iter().chain(&add).map(String::as_str) {
        if FEATURE_SCOPES.contains(&x) && !scopes.contains(&x) {
            scopes.push(x);
        }
    }
    let twitch = s.twitch.read().await.clone();
    // Subscribed before the flow starts, so a sign-in that finishes at once is still seen.
    let account = twitch.account();
    match twitch.start_login(&scopes).await {
        Ok(device) => {
            let wait = Duration::from_secs(device.expires_in_secs);
            tokio::spawn(keep_granted_scopes(
                s.data_dir.clone(),
                twitch,
                account,
                wait,
            ));
            Json(device).into_response()
        }
        Err(e) => auth_error(e),
    }
}

/// Once the sign-in finishes, keeps the feature scopes Twitch granted for every later sign-in; a sign-in that is
/// declined or expires keeps nothing, so a feature never counts as granted before it is.
async fn keep_granted_scopes(
    data_dir: PathBuf,
    twitch: TwitchAuth,
    mut account: tokio::sync::watch::Receiver<Option<String>>,
    wait: Duration,
) {
    if !matches!(
        tokio::time::timeout(wait, account.changed()).await,
        Ok(Ok(()))
    ) {
        return;
    }
    let granted: Vec<String> = twitch
        .token()
        .map(|t| t.scopes)
        .unwrap_or_default()
        .into_iter()
        .filter(|x| FEATURE_SCOPES.contains(&x.as_str()))
        .collect();
    if granted
        .iter()
        .all(|g| settings::load(&data_dir).twitch_scopes.contains(g))
    {
        return;
    }
    let saved = settings::update(&data_dir, |set| {
        for g in granted {
            if !set.twitch_scopes.contains(&g) {
                set.twitch_scopes.push(g);
            }
        }
    });
    if let Err(e) = saved {
        tracing::warn!(error = %e, "could not keep the granted Twitch scopes");
    }
}

#[utoipa::path(
    post,
    path = "/api/admin/twitch/logout",
    tag = "admin",
    responses(
        (status = 204),
        (status = 500, description = "`twitch.storage_failed`: signed out, but the stored token could not be deleted, so the next start would sign in again", body = ErrorBody),
    )
)]
pub(super) async fn logout(State(s): State<Arc<AdminState>>) -> Response {
    match s.twitch.read().await.logout() {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => auth_error(e),
    }
}

/// Profile images for the stamp card overlay, from Helix Get Users with the broadcaster's sign-in.
pub(crate) struct TwitchProfiles {
    pub(crate) helix: String,
    pub(crate) http: HttpsClient,
    pub(crate) twitch: Arc<RwLock<TwitchAuth>>,
}

impl checkin::ProfileImages for TwitchProfiles {
    fn profile_image<'a>(
        &'a self,
        twitch_user_id: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<String>> + Send + 'a>> {
        Box::pin(async move {
            if twitch_user_id.is_empty() || !twitch_user_id.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let auth = self.twitch.read().await.clone();
            let body = Helix::new(&self.helix, auth, self.http.clone())
                .get_json(&format!("/users?id={twitch_user_id}"))
                .await
                .ok()?;
            body["data"][0]["profile_image_url"]
                .as_str()
                .map(str::to_owned)
        })
    }

    fn profile_images<'a>(
        &'a self,
        ids: &'a [String],
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = toolkit_core::twitch::ProfileBatch> + Send + 'a>,
    > {
        Box::pin(async move {
            use std::time::Duration;

            let query = ids
                .iter()
                .filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
                .map(|id| format!("id={id}"))
                .collect::<Vec<_>>()
                .join("&");
            if query.is_empty() {
                return Ok(Default::default());
            }
            let auth = self.twitch.read().await.clone();
            let body = Helix::new(&self.helix, auth, self.http.clone())
                .get_json(&format!("/users?{query}"))
                .await
                .map_err(|err| match err {
                    HelixError::RateLimited(Some(wait)) => wait,
                    _ => Duration::from_secs(30),
                })?;
            Ok(body["data"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|user| {
                    Some((
                        user["id"].as_str()?.to_owned(),
                        user["profile_image_url"].as_str()?.to_owned(),
                    ))
                })
                .collect())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_id_validation() {
        assert!(valid_client_id("abc123XYZ"));
        assert!(!valid_client_id(""));
        assert!(!valid_client_id("has space"));
        assert!(!valid_client_id("<script>"));
        assert!(!valid_client_id(&"a".repeat(65)));
    }
}
