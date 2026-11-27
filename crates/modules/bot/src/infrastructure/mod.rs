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
    settings::{self, BotCommand, BotTimer},
};
use utoipa::{OpenApi, ToSchema};

use crate::{
    application::{BotService, command_key},
    domain::{MESSAGE_MAX_CHARS, command_name},
};

const COMMANDS_MAX: usize = 100;
const COOLDOWN_MAX_SECONDS: u32 = 3600;
const TIMERS_MAX: usize = 20;
const TIMER_NAME_MAX_CHARS: usize = 50;
const EVERY_MINUTES: std::ops::RangeInclusive<u32> = 5..=120;
const MIN_CHAT_LINES_MAX: u32 = 100;

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

/// A timed message as the admin page shows it. `name` and `message` are the broadcaster's text, shown as text.
#[derive(Serialize, ToSchema)]
pub struct BotTimerView {
    id: String,
    name: String,
    message: String,
    every_minutes: u32,
    min_chat_lines: u32,
    enabled: bool,
    /// RFC 3339: when it is next due; `None` while not live (or when off).
    #[schema(required)]
    next_at: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct BotTimerBody {
    /// Up to 50 characters.
    name: String,
    /// Up to 500 characters; may use `{計數:名稱}` and `{直播時長}`.
    message: String,
    /// 5–120.
    every_minutes: u32,
    /// 0–100: chat lines needed since its last send; fewer skip this time.
    min_chat_lines: u32,
    enabled: bool,
}

fn timer_views(s: &BotService) -> Vec<BotTimerView> {
    let next = s.next_times();
    s.bot_timers
        .read()
        .unwrap()
        .iter()
        .map(|t| BotTimerView {
            id: t.id.clone(),
            name: t.name.clone(),
            message: t.message.clone(),
            every_minutes: t.every_minutes,
            min_chat_lines: t.min_chat_lines,
            enabled: t.enabled,
            next_at: next
                .get(&t.id)
                .map(|at| at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
        })
        .collect()
}

#[utoipa::path(get, path = "/api/admin/bot/timers", tag = "admin", responses((status = 200, body = Vec<BotTimerView>)))]
async fn list_timers(State(s): State<Arc<BotService>>) -> Json<Vec<BotTimerView>> {
    Json(timer_views(&s))
}

/// Saves timer `id` in `settings.json` and puts the timers on their next times.
fn save_timer(s: &BotService, id: &str, body: BotTimerBody, adding: bool) -> Response {
    let name = body.name.trim().to_owned();
    let message = body.message.trim().to_owned();
    let ok = !name.is_empty()
        && name.chars().count() <= TIMER_NAME_MAX_CHARS
        && !message.is_empty()
        && message.chars().count() <= MESSAGE_MAX_CHARS
        && EVERY_MINUTES.contains(&body.every_minutes)
        && body.min_chat_lines <= MIN_CHAT_LINES_MAX;
    if !ok {
        return error(StatusCode::BAD_REQUEST, "bot.invalid_timer");
    }
    let mut timers = s.bot_timers.write().unwrap();
    if adding && timers.len() >= TIMERS_MAX {
        return error(StatusCode::BAD_REQUEST, "bot.invalid_timer");
    }
    if !adding && !timers.iter().any(|t| t.id == id) {
        return error(StatusCode::NOT_FOUND, "bot.not_found");
    }
    let timer = BotTimer {
        id: id.to_owned(),
        name,
        message,
        every_minutes: body.every_minutes,
        min_chat_lines: body.min_chat_lines,
        enabled: body.enabled,
    };
    let mut next = timers.clone();
    match next.iter_mut().find(|t| t.id == id) {
        Some(t) => *t = timer,
        None => next.push(timer),
    }
    let saved = next.clone();
    if settings::update(&s.data_dir, move |set| set.bot.timers = saved).is_err() {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "settings.save_failed");
    }
    *timers = next;
    drop(timers);
    s.reschedule();
    Json(timer_views(s).into_iter().find(|t| t.id == id)).into_response()
}

#[utoipa::path(
    post,
    path = "/api/admin/bot/timers",
    tag = "admin",
    request_body = BotTimerBody,
    responses(
        (status = 200, body = BotTimerView),
        (status = 400, description = "`bot.invalid_timer`: an empty or over-long name or message, an interval outside 5–120 minutes, over 100 chat lines, or more than 20 timers", body = ErrorBody),
        (status = 500, description = "`settings.save_failed`", body = ErrorBody),
    )
)]
async fn create_timer(
    State(s): State<Arc<BotService>>,
    Json(body): Json<BotTimerBody>,
) -> Response {
    save_timer(&s, &uuid::Uuid::now_v7().to_string(), body, true)
}

#[utoipa::path(
    put,
    path = "/api/admin/bot/timers/{id}",
    tag = "admin",
    params(("id" = String, Path)),
    request_body = BotTimerBody,
    responses(
        (status = 200, body = BotTimerView),
        (status = 400, description = "`bot.invalid_timer`", body = ErrorBody),
        (status = 404, description = "`bot.not_found`", body = ErrorBody),
        (status = 500, description = "`settings.save_failed`", body = ErrorBody),
    )
)]
async fn update_timer(
    State(s): State<Arc<BotService>>,
    Path(id): Path<String>,
    Json(body): Json<BotTimerBody>,
) -> Response {
    save_timer(&s, &id, body, false)
}

#[utoipa::path(
    delete,
    path = "/api/admin/bot/timers/{id}",
    tag = "admin",
    params(("id" = String, Path)),
    responses(
        (status = 204),
        (status = 404, description = "`bot.not_found`", body = ErrorBody),
        (status = 500, description = "`settings.save_failed`", body = ErrorBody),
    )
)]
async fn delete_timer(State(s): State<Arc<BotService>>, Path(id): Path<String>) -> Response {
    let mut timers = s.bot_timers.write().unwrap();
    if !timers.iter().any(|t| t.id == id) {
        return error(StatusCode::NOT_FOUND, "bot.not_found");
    }
    let next: Vec<BotTimer> = timers.iter().filter(|t| t.id != id).cloned().collect();
    let saved = next.clone();
    if settings::update(&s.data_dir, move |set| set.bot.timers = saved).is_err() {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "settings.save_failed");
    }
    *timers = next;
    drop(timers);
    s.reschedule();
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
        .route("/api/admin/bot/timers", get(list_timers).post(create_timer))
        .route(
            "/api/admin/bot/timers/{id}",
            put(update_timer).delete(delete_timer),
        )
        .with_state(service)
}

#[derive(OpenApi)]
#[openapi(paths(
    list_commands,
    create_command,
    update_command,
    delete_command,
    status,
    list_timers,
    create_timer,
    update_timer,
    delete_timer
))]
struct BotApi;

pub fn openapi() -> utoipa::openapi::OpenApi {
    BotApi::openapi()
}

#[cfg(test)]
mod tests;
