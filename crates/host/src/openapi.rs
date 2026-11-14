//! The Host's OpenAPI document (code-first contract). `pnpm api:gen` writes it and the TypeScript types
//! generated from it to `apps/web/src/api/`; CI fails when they are stale.

use utoipa::OpenApi;

use crate::{admin, overlay};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Streamer Toolkit Host",
        description = "Local Host API on 127.0.0.1. `/ws/overlay` is a read-only WebSocket: the first message \
                       is a `Snapshot`, then events such as `ChatMessageEvent` follow in `seq` order."
    ),
    paths(
        admin::twitch_overview,
        admin::set_client_id,
        admin::login,
        admin::logout,
        admin::list_modules,
        admin::set_module,
        admin::simulate_chat,
    ),
    components(schemas(overlay::Snapshot, chat::ChatMessageEvent)),
    tags((name = "admin", description = "Requires `Authorization: Bearer <per-launch admin token>`; \
                                         otherwise 401."))
)]
struct ApiDoc;

pub fn openapi() -> utoipa::openapi::OpenApi {
    ApiDoc::openapi()
}
