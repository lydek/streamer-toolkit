//! A scene item's source settings (the scene page's 來源設定 tab): only the fields listed per kind are read or
//! written, the rest of OBS's settings stay as they are. A browser source's address is shown and written only when
//! it opens one of this Host's overlays: other addresses may hold another service's token, so they never reach the
//! admin UI, a log or the public WebSocket (docs/architecture.md, OBS sources). Admin routes; the caller adds the
//! token check.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Query, State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use toolkit_core::streaming::StreamingSoftware;
use utoipa::{IntoParams, ToSchema};

use crate::{
    admin::{ErrorBody, error},
    obs::{ObsLink, command_response, own_overlay},
};

/// The kinds of source with a form; any other (a plugin's included) is shown as "change it in OBS".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SourceFamily {
    Browser,
    Text,
    Image,
    VideoCapture,
    AppAudio,
    Game,
    Other,
}

fn family(kind: &str) -> SourceFamily {
    let base = kind
        .trim_end_matches(|c: char| c.is_ascii_digit())
        .trim_end_matches("_v");
    match base {
        "browser_source" => SourceFamily::Browser,
        "text_gdiplus" | "text_ft2_source" => SourceFamily::Text,
        "image_source" => SourceFamily::Image,
        "dshow_input" => SourceFamily::VideoCapture,
        "wasapi_process_output_capture" => SourceFamily::AppAudio,
        "game_capture" => SourceFamily::Game,
        _ => SourceFamily::Other,
    }
}

/// The settings each form reads and writes.
fn fields(family: SourceFamily) -> &'static [&'static str] {
    match family {
        SourceFamily::Browser => &["url", "width", "height", "css", "shutdown"],
        SourceFamily::Text => &["text", "font", "color", "color1"],
        SourceFamily::Image => &["file"],
        SourceFamily::VideoCapture => &["video_device_id", "resolution", "res_type"],
        SourceFamily::AppAudio => &["window"],
        SourceFamily::Game => &["capture_mode", "window"],
        SourceFamily::Other => &[],
    }
}

pub(crate) fn routes(link: Arc<ObsLink>) -> Router {
    Router::new()
        .route("/api/admin/obs/source", get(source).put(set_source))
        .with_state(link)
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct SourceQuery {
    /// The source's name (a scene item's `source` in `obs.state`).
    input: String,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct SourceSettings {
    family: SourceFamily,
    /// Only the fields of this kind's form; a browser source's `url` only when it opens this Host's overlay.
    #[schema(value_type = Object)]
    fields: Map<String, Value>,
    /// A browser source opening another address: it is not shown, and can be changed in OBS only.
    url_hidden: bool,
}

/// The listed settings of one source, for its form.
#[utoipa::path(
    get,
    path = "/api/admin/obs/source",
    tag = "admin",
    params(SourceQuery),
    responses(
        (status = 200, body = SourceSettings),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 502, body = ErrorBody),
    )
)]
pub(crate) async fn source(
    State(link): State<Arc<ObsLink>>,
    Query(q): Query<SourceQuery>,
) -> Response {
    match link.input_settings(&q.input).await {
        Ok((kind, settings)) => Json(shown(&kind, settings, link.port())).into_response(),
        Err(e) => command_response(Err(e)),
    }
}

fn shown(kind: &str, settings: Map<String, Value>, port: u16) -> SourceSettings {
    let family = family(kind);
    let mut fields: Map<String, Value> = settings
        .into_iter()
        .filter(|(k, _)| fields(family).contains(&k.as_str()))
        .collect();
    let mut url_hidden = false;
    if family == SourceFamily::Browser
        && let Some(url) = fields.get("url").and_then(Value::as_str)
        && own_overlay(url, port).is_none()
    {
        fields.remove("url");
        url_hidden = true;
    }
    SourceSettings {
        family,
        fields,
        url_hidden,
    }
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct SourceBody {
    input: String,
    /// Fields of the source's form to change; the rest of its settings stay.
    #[schema(value_type = Object)]
    fields: Map<String, Value>,
}

/// Changes listed settings of one source; refused for a kind without a form, a field not on its form, a value of
/// the wrong shape, or an address that is not one of this Host's overlays.
#[utoipa::path(
    put,
    path = "/api/admin/obs/source",
    tag = "admin",
    request_body = SourceBody,
    responses(
        (status = 204),
        (status = 400, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 502, body = ErrorBody),
    )
)]
pub(crate) async fn set_source(
    State(link): State<Arc<ObsLink>>,
    body: Result<Json<SourceBody>, JsonRejection>,
) -> Response {
    let Ok(Json(body)) = body else {
        return error(StatusCode::BAD_REQUEST, "obs.invalid_source");
    };
    let kind = match link.input_settings(&body.input).await {
        Ok((kind, _)) => kind,
        Err(e) => return command_response(Err(e)),
    };
    if let Err(code) = check(family(&kind), &body.fields, link.port()) {
        return error(StatusCode::BAD_REQUEST, code);
    }
    command_response(link.set_input_settings(&body.input, body.fields).await)
}

/// Whether `fields` may be written to a source of `family`.
fn check(
    family: SourceFamily,
    fields_in: &Map<String, Value>,
    port: u16,
) -> Result<(), &'static str> {
    if family == SourceFamily::Other {
        return Err("obs.source_not_editable");
    }
    for (key, value) in fields_in {
        if !fields(family).contains(&key.as_str()) {
            return Err("obs.invalid_source");
        }
        let text = |max: usize| value.as_str().is_some_and(|s| s.chars().count() <= max);
        let ok = match key.as_str() {
            "url" => value
                .as_str()
                .is_some_and(|u| own_overlay(u, port).is_some()),
            "width" | "height" => value.as_u64().is_some_and(|n| (1..=16_384).contains(&n)),
            "css" => text(20_000),
            "shutdown" => value.is_boolean(),
            "text" => text(10_000),
            // OBS's font object: face, size, style and flags only.
            "font" => value.as_object().is_some_and(|font| {
                font.iter().all(|(k, v)| match k.as_str() {
                    "face" => v.as_str().is_some_and(|f| f.chars().count() <= 200),
                    "size" => v.as_u64().is_some_and(|n| (1..=1_000).contains(&n)),
                    "style" => v.as_str().is_some_and(|f| f.chars().count() <= 100),
                    "flags" => v.as_u64().is_some_and(|n| n <= 0xFFFF),
                    _ => false,
                })
            }),
            "color" | "color1" => value.as_u64().is_some_and(|n| n <= u64::from(u32::MAX)),
            "res_type" => value.as_u64().is_some_and(|n| n <= 1),
            _ => text(1_000),
        };
        if !ok {
            return Err("obs.invalid_source");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const PORT: u16 = 47810;
    fn map(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn only_listed_fields_show_and_a_foreign_address_never_does() {
        let own = shown(
            "browser_source",
            map(json!({"url": "http://127.0.0.1:47810/overlay/chat/", "width": 400, "fps": 30})),
            PORT,
        );
        assert_eq!(own.family, SourceFamily::Browser);
        assert_eq!(
            Value::Object(own.fields),
            json!({"url": "http://127.0.0.1:47810/overlay/chat/", "width": 400})
        );
        assert!(!own.url_hidden);
        let foreign = shown(
            "browser_source",
            map(json!({"url": "https://example.test/alerts?token=secret", "width": 400})),
            PORT,
        );
        assert_eq!(Value::Object(foreign.fields), json!({"width": 400}));
        assert!(foreign.url_hidden);
        let plugin = shown(
            "spout_capture",
            map(json!({"spout_sender": "VSeeFace"})),
            PORT,
        );
        assert_eq!(
            (plugin.family, plugin.fields.len()),
            (SourceFamily::Other, 0)
        );
        assert_eq!(family("text_gdiplus_v3"), SourceFamily::Text);
        assert_eq!(family("text_ft2_source_v2"), SourceFamily::Text);
    }

    #[test]
    fn writes_are_checked_field_by_field() {
        assert_eq!(
            check(
                SourceFamily::Browser,
                &map(json!({"width": 800, "css": "body{}"})),
                PORT
            ),
            Ok(())
        );
        assert_eq!(
            check(
                SourceFamily::Browser,
                &map(json!({"url": "http://127.0.0.1:47810/overlay/alerts/"})),
                PORT
            ),
            Ok(())
        );
        for bad in [
            json!({"url": "https://example.test/"}),
            json!({"width": 0}),
            json!({"fps": 60}),
            json!({"shutdown": "yes"}),
        ] {
            assert_eq!(
                check(SourceFamily::Browser, &map(bad.clone()), PORT),
                Err("obs.invalid_source"),
                "{bad}"
            );
        }
        assert_eq!(
            check(SourceFamily::Other, &map(json!({})), PORT),
            Err("obs.source_not_editable")
        );
        assert_eq!(
            check(
                SourceFamily::Text,
                &map(json!({"font": {"face": "Noto Sans TC", "size": 36}})),
                PORT
            ),
            Ok(())
        );
        for bad in [
            json!({"font": 5}),
            json!({"font": "Arial"}),
            json!({"font": {"face": "Arial", "script": "x"}}),
        ] {
            assert_eq!(
                check(SourceFamily::Text, &map(bad.clone()), PORT),
                Err("obs.invalid_source"),
                "{bad}"
            );
        }
    }
}
