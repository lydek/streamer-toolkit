//! Admin API for the chat bot's commands; the Host puts these routes behind the admin token.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, put},
};
use serde::{Deserialize, Serialize};
use toolkit_core::{
    commands::{Allowed, CommandEntry, CommandError, CommandTakenBody},
    modules,
    settings::{self, BotCommand},
};
use utoipa::{OpenApi, ToSchema};

use crate::{
    application::{BotService, command_key},
    domain::{MESSAGE_MAX_CHARS, command_name},
};

const COMMANDS_MAX: usize = 100;
const COOLDOWN_MAX_SECONDS: u32 = 3600;

#[derive(Serialize, ToSchema)]
pub struct ErrorBody {
    code: String,
}

fn error(status: StatusCode, code: &str) -> Response {
    (status, Json(ErrorBody { code: code.into() })).into_response()
}

/// A bot command as the admin page shows it. `reply` is the broadcaster's text, shown as text.
#[derive(Serialize, ToSchema)]
pub struct BotCommandView {
    id: String,
    /// Without its `!`.
    name: String,
    allowed: Allowed,
    reply: String,
    /// 0: no cooldown.
    cooldown_seconds: u32,
    enabled: bool,
}

#[derive(Deserialize, ToSchema)]
pub struct BotCommandBody {
    /// One word, with or without its `!`, up to 25 characters; unique among every feature's commands.
    name: String,
    allowed: Allowed,
    /// Up to 500 characters; may use `{觀眾}`, `{計數:名稱}` and `{直播時長}`.
    reply: String,
    /// 0–3600; 0 is no cooldown.
    cooldown_seconds: u32,
    enabled: bool,
}

/// How many messages the bot dropped since launch (the queue was full, or they waited too long).
#[derive(Serialize, ToSchema)]
pub struct BotStatus {
    dropped: u64,
}

fn views(s: &BotService) -> Vec<BotCommandView> {
    s.bot_commands
        .read()
        .unwrap()
        .iter()
        .map(|c| {
            let entry = s.commands.get(&command_key(&c.id));
            BotCommandView {
                id: c.id.clone(),
                name: entry.name,
                allowed: entry.allowed,
                reply: c.reply.clone(),
                cooldown_seconds: c.cooldown_seconds,
                enabled: c.enabled,
            }
        })
        .collect()
}

#[utoipa::path(get, path = "/api/admin/bot/commands", tag = "admin", responses((status = 200, body = Vec<BotCommandView>)))]
async fn list_commands(State(s): State<Arc<BotService>>) -> Json<Vec<BotCommandView>> {
    Json(views(&s))
}

/// Saves command `id` (its name in the command table, the rest in the bot's settings) in one write.
fn save(s: &BotService, id: &str, body: BotCommandBody, adding: bool) -> Response {
    let reply = body.reply.trim().to_owned();
    let name = command_name(&body.name);
    let (Some(name), true, true) = (
        name,
        !reply.is_empty() && reply.chars().count() <= MESSAGE_MAX_CHARS,
        body.cooldown_seconds <= COOLDOWN_MAX_SECONDS,
    ) else {
        return error(StatusCode::BAD_REQUEST, "bot.invalid");
    };
    let mut commands = s.bot_commands.write().unwrap();
    if adding && commands.len() >= COMMANDS_MAX {
        return error(StatusCode::BAD_REQUEST, "bot.invalid");
    }
    if !adding && !commands.iter().any(|c| c.id == id) {
        return error(StatusCode::NOT_FOUND, "bot.not_found");
    }
    let command = BotCommand {
        id: id.to_owned(),
        reply,
        cooldown_seconds: body.cooldown_seconds,
        enabled: body.enabled,
    };
    let mut next = commands.clone();
    match next.iter_mut().find(|c| c.id == id) {
        Some(c) => *c = command,
        None => next.push(command),
    }
    let entry = CommandEntry {
        name,
        allowed: body.allowed,
        module: modules::BOT.into(),
        label: None,
    };
    let saved = next.clone();
    match s
        .commands
        .set(&command_key(id), entry, move |set| set.bot.commands = saved)
    {
        Ok(()) => {
            *commands = next;
            drop(commands);
            let view = views(s).into_iter().find(|v| v.id == id);
            Json(view).into_response()
        }
        Err(CommandError::Taken { module, label }) => (
            StatusCode::CONFLICT,
            Json(CommandTakenBody::new(module, label)),
        )
            .into_response(),
        Err(e @ CommandError::SaveFailed(_)) => {
            error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
        }
    }
}

#[utoipa::path(
    post,
    path = "/api/admin/bot/commands",
    tag = "admin",
    request_body = BotCommandBody,
    responses(
        (status = 200, body = BotCommandView),
        (status = 400, description = "`bot.invalid`: a name that is not one word, an empty reply or one over 500 characters, a cooldown over 3600 s, or more than 100 commands", body = ErrorBody),
        (status = 409, description = "`commands.taken`: another feature's command has the name", body = CommandTakenBody),
        (status = 500, description = "`settings.save_failed`", body = ErrorBody),
    )
)]
async fn create_command(
    State(s): State<Arc<BotService>>,
    Json(body): Json<BotCommandBody>,
) -> Response {
    save(&s, &uuid::Uuid::now_v7().to_string(), body, true)
}

#[utoipa::path(
    put,
    path = "/api/admin/bot/commands/{id}",
    tag = "admin",
    params(("id" = String, Path)),
    request_body = BotCommandBody,
    responses(
        (status = 200, body = BotCommandView),
        (status = 400, description = "`bot.invalid`", body = ErrorBody),
        (status = 404, description = "`bot.not_found`", body = ErrorBody),
        (status = 409, description = "`commands.taken`", body = CommandTakenBody),
        (status = 500, description = "`settings.save_failed`", body = ErrorBody),
    )
)]
async fn update_command(
    State(s): State<Arc<BotService>>,
    Path(id): Path<String>,
    Json(body): Json<BotCommandBody>,
) -> Response {
    save(&s, &id, body, false)
}

#[utoipa::path(
    delete,
    path = "/api/admin/bot/commands/{id}",
    tag = "admin",
    params(("id" = String, Path)),
    responses(
        (status = 204),
        (status = 404, description = "`bot.not_found`", body = ErrorBody),
        (status = 500, description = "`settings.save_failed`", body = ErrorBody),
    )
)]
async fn delete_command(State(s): State<Arc<BotService>>, Path(id): Path<String>) -> Response {
    let mut commands = s.bot_commands.write().unwrap();
    if !commands.iter().any(|c| c.id == id) {
        return error(StatusCode::NOT_FOUND, "bot.not_found");
    }
    // The name goes first, so a failure never leaves a name nothing answers to.
    if let Err(e) = s.commands.remove(&command_key(&id)) {
        return error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string());
    }
    let next: Vec<BotCommand> = commands.iter().filter(|c| c.id != id).cloned().collect();
    let saved = next.clone();
    if settings::update(&s.data_dir, move |set| set.bot.commands = saved).is_err() {
        // The name is free already; the left-over reply answers to nothing and is gone after the next save.
        tracing::warn!(
            code = "settings.save_failed",
            "deleted bot command's reply not removed"
        );
    }
    *commands = next;
    StatusCode::NO_CONTENT.into_response()
}

#[utoipa::path(get, path = "/api/admin/bot/status", tag = "admin", responses((status = 200, body = BotStatus)))]
async fn status(State(s): State<Arc<BotService>>) -> Json<BotStatus> {
    Json(BotStatus {
        dropped: s.dropped(),
    })
}

pub fn routes(service: Arc<BotService>) -> Router {
    Router::new()
        .route(
            "/api/admin/bot/commands",
            get(list_commands).post(create_command),
        )
        .route(
            "/api/admin/bot/commands/{id}",
            put(update_command).delete(delete_command),
        )
        .route("/api/admin/bot/status", get(status))
        .with_state(service)
}

#[derive(OpenApi)]
#[openapi(paths(list_commands, create_command, update_command, delete_command, status))]
struct BotApi;

pub fn openapi() -> utoipa::openapi::OpenApi {
    BotApi::openapi()
}

#[cfg(test)]
mod tests;
