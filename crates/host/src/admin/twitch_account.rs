use super::*;

#[derive(Serialize, ToSchema)]
pub(crate) struct Overview {
    client_id: String,
    status: AuthStatus,
    /// The EventSub session is connected and subscribed (chat, alerts, redemptions arrive).
    eventsub_connected: bool,
    /// Twitch's first-chat mark can be read (anonymous IRC joined); otherwise 第一次聊天 is the toolkit's first
    /// sighting of the viewer.
    irc_connected: bool,
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

#[utoipa::path(
    post,
    path = "/api/admin/twitch/login",
    tag = "admin",
    responses(
        (status = 200, body = DeviceLogin),
        (status = 400, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 502, body = ErrorBody),
    )
)]
pub(super) async fn login(State(s): State<Arc<AdminState>>) -> Response {
    let twitch = s.twitch.read().await.clone();
    match twitch.start_login(MVP_SCOPES).await {
        Ok(device) => Json(device).into_response(),
        Err(e) => auth_error(e),
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
