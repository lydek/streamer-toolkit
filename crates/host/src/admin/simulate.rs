use super::*;

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SimulatedChat {
    #[serde(default)]
    user_name: String,
    text: String,
}

const SIMULATED_TEXT_MAX: usize = 500;

/// Sends a chat message tagged `source=simulation` through the same path as Twitch chat.
#[utoipa::path(
    post,
    path = "/api/admin/simulate/chat",
    tag = "admin",
    request_body = SimulatedChat,
    responses((status = 204), (status = 400, body = ErrorBody))
)]
pub(super) async fn simulate_chat(
    State(s): State<Arc<AdminState>>,
    Json(body): Json<SimulatedChat>,
) -> Response {
    match send_chat(&s, &body.user_name, &body.text) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(code) => error(StatusCode::BAD_REQUEST, code),
    }
}

/// A chat message tagged `source=simulation`, through the same path as Twitch chat; it reaches the chat display only.
pub(crate) fn send_chat(s: &AdminState, user_name: &str, text: &str) -> Result<(), &'static str> {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > SIMULATED_TEXT_MAX {
        return Err("simulate.invalid_text");
    }
    let user_name = match user_name.trim() {
        "" => "Simulator",
        name => name,
    };
    s.chat.receive(
        ChatMessage {
            id: uuid::Uuid::now_v7().to_string(),
            broadcaster_id: String::new(),
            user_id: "simulator".into(),
            user_login: "simulator".into(),
            user_name: user_name.chars().take(25).collect(),
            color: None,
            text: text.into(),
            fragments: vec![Fragment::Text(text.into())],
            badges: Vec::new(),
            highlighted: false,
            bits: 0,
            reply: None,
            twitch_badges: Vec::new(),
            shared_from: None,
            sent_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        },
        Source::Simulation,
    );
    Ok(())
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SimulatedAlert {
    kind: AlertKindName,
    /// Up to 25 characters; empty means "Simulator".
    #[serde(default)]
    user_name: String,
    /// Months (`resubscribe`), subs (`gift`, 1 to 100), Bits (`cheer`) or raiders (`raid`), 1 to 100000;
    /// ignored for the other kinds.
    #[serde(default)]
    amount: Option<u32>,
    /// Up to 500 characters (`resubscribe`, `cheer`); ignored for the other kinds.
    #[serde(default)]
    message: String,
}

const SIMULATED_NAME_MAX: usize = 25;

const SIMULATED_GIFT_MAX: u32 = 100;

const SIMULATED_AMOUNT_MAX: u32 = 100_000;

/// The alerts Twitch would send. A gift also brings one gifted sub per recipient, so the simulator exercises
/// the same grouping as real gifts.
fn simulated_alerts(body: SimulatedAlert) -> Result<Vec<Alert>, &'static str> {
    let name = body.user_name.trim();
    if name.chars().count() > SIMULATED_NAME_MAX {
        return Err("simulate.invalid_name");
    }
    let message = body.message.trim().to_owned();
    if message.chars().count() > SIMULATED_TEXT_MAX {
        return Err("simulate.invalid_text");
    }
    let max = match body.kind {
        AlertKindName::Gift => SIMULATED_GIFT_MAX,
        _ => SIMULATED_AMOUNT_MAX,
    };
    let amount = || {
        body.amount
            .filter(|n| (1..=max).contains(n))
            .ok_or("simulate.invalid_amount")
    };
    let kind = match body.kind {
        AlertKindName::Follow => AlertKind::Follow,
        AlertKindName::Subscribe => AlertKind::Subscribe {
            tier: 1,
            gifted: false,
        },
        AlertKindName::Resubscribe => AlertKind::Resubscribe {
            tier: 1,
            months: amount()?,
            message,
        },
        AlertKindName::Gift => AlertKind::Gift {
            tier: 1,
            count: amount()?,
        },
        AlertKindName::Cheer => AlertKind::Cheer {
            bits: amount()?,
            message,
        },
        AlertKindName::Raid => AlertKind::Raid { viewers: amount()? },
    };
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    // Like the simulated chat, every simulated viewer is the "simulator" account.
    let alert = |user_name: Option<String>, kind| Alert {
        id: uuid::Uuid::now_v7().to_string(),
        user_id: Some("simulator".into()),
        user_login: Some("simulator".into()),
        user_name,
        kind,
        occurred_at: now.clone(),
    };
    let recipients = match kind {
        AlertKind::Gift { count, .. } => count,
        _ => 0,
    };
    let mut alerts = vec![alert(
        Some(if name.is_empty() { "Simulator" } else { name }.into()),
        kind,
    )];
    alerts.extend((1..=recipients).map(|n| {
        alert(
            Some(format!("Recipient{n}")),
            AlertKind::Subscribe {
                tier: 1,
                gifted: true,
            },
        )
    }));
    Ok(alerts)
}

/// Sends a follow, sub, resub, gift, cheer or raid tagged `source=simulation` through the same path as Twitch.
#[utoipa::path(
    post,
    path = "/api/admin/simulate/alert",
    tag = "admin",
    request_body = SimulatedAlert,
    responses((status = 204), (status = 400, body = ErrorBody))
)]
pub(super) async fn simulate_alert(
    State(s): State<Arc<AdminState>>,
    body: Result<Json<SimulatedAlert>, JsonRejection>,
) -> Response {
    // An unknown kind or a malformed body gets an error code instead of axum's plain-text rejection.
    let Ok(Json(body)) = body else {
        return error(StatusCode::BAD_REQUEST, "simulate.invalid_alert");
    };
    match send_alert(&s, body) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(code) => error(StatusCode::BAD_REQUEST, code),
    }
}

/// An alert tagged `source=simulation`, through the same path as Twitch; it reaches the alerts overlay only.
pub(crate) fn send_alert(s: &AdminState, body: SimulatedAlert) -> Result<(), &'static str> {
    for alert in simulated_alerts(body)? {
        s.alerts.receive(alert, Source::Simulation);
    }
    Ok(())
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SimulatedCheckin {
    #[serde(default)]
    user_name: String,
    /// Stamps on the card, 1 to its slots; all of them shows the full-card effect.
    stamps: u32,
}

/// Shows a stamp card on `/overlay/checkin/` tagged `source=simulation`; nothing is recorded.
#[utoipa::path(
    post,
    path = "/api/admin/simulate/checkin",
    tag = "admin",
    request_body = SimulatedCheckin,
    responses((status = 204), (status = 400, description = "`simulate.invalid_checkin` or `simulate.invalid_stamps`", body = ErrorBody))
)]
pub(super) async fn simulate_checkin(
    State(s): State<Arc<AdminState>>,
    body: Result<Json<SimulatedCheckin>, JsonRejection>,
) -> Response {
    let Ok(Json(body)) = body else {
        return error(StatusCode::BAD_REQUEST, "simulate.invalid_checkin");
    };
    match send_checkin(&s, &body.user_name, body.stamps) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(code) => error(StatusCode::BAD_REQUEST, code),
    }
}

/// A stamp card tagged `source=simulation`; nothing is recorded.
pub(crate) fn send_checkin(
    s: &AdminState,
    user_name: &str,
    stamps: u32,
) -> Result<(), &'static str> {
    let name: String = match user_name.trim() {
        "" => "Simulator".into(),
        name => name.chars().take(25).collect(),
    };
    if s.checkin.simulate_card(&name, stamps) {
        Ok(())
    } else {
        Err("simulate.invalid_stamps")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simulate(json: serde_json::Value) -> Result<Vec<Alert>, &'static str> {
        simulated_alerts(serde_json::from_value(json).unwrap())
    }

    #[test]
    fn simulated_alerts_cover_every_kind_and_validate_input() {
        let kinds =
            |json| simulate(json).map(|a| a.into_iter().map(|a| a.kind).collect::<Vec<_>>());
        assert_eq!(
            kinds(serde_json::json!({"kind":"follow"})),
            Ok(vec![AlertKind::Follow])
        );
        assert_eq!(
            kinds(serde_json::json!({"kind":"subscribe"})),
            Ok(vec![AlertKind::Subscribe {
                tier: 1,
                gifted: false
            }])
        );
        assert_eq!(
            kinds(serde_json::json!({"kind":"resubscribe","amount":12,"message":" hi "})),
            Ok(vec![AlertKind::Resubscribe {
                tier: 1,
                months: 12,
                message: "hi".into()
            }])
        );
        assert_eq!(
            kinds(serde_json::json!({"kind":"cheer","amount":100})),
            Ok(vec![AlertKind::Cheer {
                bits: 100,
                message: String::new()
            }])
        );
        assert_eq!(
            kinds(serde_json::json!({"kind":"raid","amount":42})),
            Ok(vec![AlertKind::Raid { viewers: 42 }])
        );

        let gift =
            simulate(serde_json::json!({"kind":"gift","amount":3,"userName":"Viewer"})).unwrap();
        assert_eq!(gift[0].kind, AlertKind::Gift { tier: 1, count: 3 });
        assert_eq!(gift[0].user_name.as_deref(), Some("Viewer"));
        assert_eq!(gift.len(), 4, "the gift and one gifted sub per recipient");
        assert!(gift[1..].iter().all(|a| a.kind
            == AlertKind::Subscribe {
                tier: 1,
                gifted: true
            }));
        let follow = simulate(serde_json::json!({"kind":"follow","userName":"  "})).unwrap();
        assert_eq!(follow[0].user_name.as_deref(), Some("Simulator"));

        for (json, code) in [
            (
                serde_json::json!({"kind":"follow","userName":"x".repeat(26)}),
                "simulate.invalid_name",
            ),
            (
                serde_json::json!({"kind":"cheer","amount":1,"message":"x".repeat(501)}),
                "simulate.invalid_text",
            ),
            (
                serde_json::json!({"kind":"gift","amount":101}),
                "simulate.invalid_amount",
            ),
            (
                serde_json::json!({"kind":"raid","amount":0}),
                "simulate.invalid_amount",
            ),
            (
                serde_json::json!({"kind":"cheer","amount":100_001}),
                "simulate.invalid_amount",
            ),
            (
                serde_json::json!({"kind":"resubscribe"}),
                "simulate.invalid_amount",
            ),
        ] {
            assert_eq!(simulate(json).unwrap_err(), code);
        }
    }
}
