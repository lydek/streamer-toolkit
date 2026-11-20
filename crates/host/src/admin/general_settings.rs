use super::*;

/// Settings → General. The desktop app reads the same `settings.json` flag on every ✕ of the admin window.
#[derive(Serialize, Deserialize, ToSchema)]
pub(crate) struct GeneralSettings {
    /// Show the close hint on ✕; the hint's "Don't show again" turns it off.
    ask_before_close: bool,
    /// Read only (ignored on PUT): `settings.json` exists but cannot be read or parsed. Defaults are in use and
    /// no setting saves (`settings.save_failed`) until the file is fixed or removed.
    #[serde(default)]
    settings_unreadable: bool,
}

#[utoipa::path(get, path = "/api/admin/general", tag = "admin", responses((status = 200, body = GeneralSettings)))]
pub(super) async fn general(State(s): State<Arc<AdminState>>) -> Json<GeneralSettings> {
    let read = settings::read(&s.data_dir);
    Json(GeneralSettings {
        settings_unreadable: read.is_err(),
        ask_before_close: !read.unwrap_or_default().skip_close_hint,
    })
}

#[utoipa::path(
    put,
    path = "/api/admin/general",
    tag = "admin",
    request_body = GeneralSettings,
    responses((status = 204), (status = 500, body = ErrorBody))
)]
pub(super) async fn set_general(
    State(s): State<Arc<AdminState>>,
    Json(body): Json<GeneralSettings>,
) -> Response {
    match settings::update(&s.data_dir, |set| {
        set.skip_close_hint = !body.ask_before_close
    }) {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "settings.save_failed"),
    }
}

#[utoipa::path(get, path = "/api/admin/modules", tag = "admin", responses((status = 200, body = Vec<ModuleState>)))]
pub(super) async fn list_modules(State(s): State<Arc<AdminState>>) -> Response {
    Json(s.modules.list()).into_response()
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct ModuleBody {
    enabled: bool,
    /// With `enabled: false`: skip the module's outbox events instead of keeping them for later ("close"; the
    /// default pauses). Enabling a closed module never delivers what was published while it was closed.
    #[serde(default)]
    close: bool,
}

#[utoipa::path(
    put,
    path = "/api/admin/modules/{id}",
    tag = "admin",
    params(("id" = String, Path)),
    request_body = ModuleBody,
    responses(
        (status = 204),
        (status = 404, body = ErrorBody),
        (status = 500, body = ErrorBody),
    )
)]
pub(super) async fn set_module(
    State(s): State<Arc<AdminState>>,
    Path(id): Path<String>,
    Json(body): Json<ModuleBody>,
) -> Response {
    if body.enabled
        && s.modules.is_closed(&id)
        && let Err(e) = s.outbox.skip_pending(&id).await
    {
        tracing::error!(code = "outbox.storage_failed", error = %e, "closed-period events not skipped");
        return error(StatusCode::INTERNAL_SERVER_ERROR, "outbox.storage_failed");
    }
    let result = match (body.enabled, body.close) {
        (false, true) => s.modules.close(&id),
        (enabled, _) => s.modules.set_enabled(&id, enabled),
    };
    s.outbox.wake();
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e @ ModuleError::Unknown) => error(StatusCode::NOT_FOUND, &e.to_string()),
        Err(e @ ModuleError::SaveFailed(_)) => {
            error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
        }
    }
}
