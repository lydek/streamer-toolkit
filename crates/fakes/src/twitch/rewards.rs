//! Channel point rewards and their redemptions: `/helix/channel_points/custom_rewards` (list, create, change,
//! delete) and `…/redemptions` (mark fulfilled or refund), as Twitch answers them. Twitch lets only the app that made
//! a reward change it or its redemptions, so each reward knows whether the app made it.

use super::*;

/// A reward as the panel shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct RewardView {
    pub id: String,
    pub title: String,
    pub cost: u64,
    /// Made by the app through Helix (the preloaded ones were made elsewhere).
    pub app_created: bool,
}

pub(super) struct Reward {
    pub app_created: bool,
    /// The reward as Helix returns it.
    pub helix: Value,
}

impl Reward {
    pub(super) fn view(&self) -> RewardView {
        RewardView {
            id: self.helix["id"].as_str().unwrap_or_default().to_owned(),
            title: self.helix["title"].as_str().unwrap_or_default().to_owned(),
            cost: self.helix["cost"].as_u64().unwrap_or_default(),
            app_created: self.app_created,
        }
    }
}

/// A redemption a viewer made, as the EventSub `…redemption.add` event carried it.
pub(super) struct Redemption {
    pub event: Value,
}

/// The preloaded rewards, made in the Twitch dashboard rather than by the app.
pub(super) fn preloaded() -> Vec<Reward> {
    REWARDS
        .iter()
        .map(|(id, title, cost)| Reward {
            app_created: false,
            helix: new_reward(id, &json!({"title": title, "cost": cost})),
        })
        .collect()
}

/// A new reward with Twitch's defaults, then `body`'s fields.
fn new_reward(id: &str, body: &Value) -> Value {
    let mut reward = json!({
        "broadcaster_id": BROADCASTER_ID, "broadcaster_login": BROADCASTER_LOGIN,
        "broadcaster_name": BROADCASTER_NAME, "id": id, "title": "", "prompt": "", "cost": 1,
        "image": null, "default_image": {"url_1x": "", "url_2x": "", "url_4x": ""},
        "background_color": "#9147FF", "is_enabled": true, "is_user_input_required": false,
        "max_per_stream_setting": {"is_enabled": false, "max_per_stream": 0},
        "max_per_user_per_stream_setting": {"is_enabled": false, "max_per_user_per_stream": 0},
        "global_cooldown_setting": {"is_enabled": false, "global_cooldown_seconds": 0},
        "is_paused": false, "is_in_stock": true, "should_redemptions_skip_request_queue": false,
        "redemptions_redeemed_current_stream": null, "cooldown_expires_at": null,
    });
    apply(&mut reward, body);
    reward
}

/// Copies the fields Create and Update Custom Reward take into a reward.
fn apply(reward: &mut Value, body: &Value) {
    for key in [
        "title",
        "prompt",
        "cost",
        "background_color",
        "is_enabled",
        "is_paused",
        "is_user_input_required",
        "should_redemptions_skip_request_queue",
    ] {
        if !body[key].is_null() {
            reward[key] = body[key].clone();
        }
    }
    for (setting, enabled, value) in [
        (
            "max_per_stream_setting",
            "is_max_per_stream_enabled",
            "max_per_stream",
        ),
        (
            "max_per_user_per_stream_setting",
            "is_max_per_user_per_stream_enabled",
            "max_per_user_per_stream",
        ),
        (
            "global_cooldown_setting",
            "is_global_cooldown_enabled",
            "global_cooldown_seconds",
        ),
    ] {
        if !body[enabled].is_null() {
            reward[setting]["is_enabled"] = body[enabled].clone();
        }
        if !body[value].is_null() {
            reward[setting][value] = body[value].clone();
        }
    }
}

fn data(item: Value) -> Response {
    Json(json!({ "data": [item] })).into_response()
}

/// Whether `body` has what Twitch needs: a title of 1–45 characters and a cost of at least 1.
fn invalid(body: &Value, creating: bool) -> Option<Response> {
    let title = body["title"].as_str();
    if (creating && title.is_none())
        || title.is_some_and(|t| t.trim().is_empty() || t.chars().count() > 45)
    {
        return Some(rejected(
            StatusCode::BAD_REQUEST,
            "The title must be 1 to 45 characters.",
        ));
    }
    let cost = &body["cost"];
    if (creating && cost.is_null()) || (!cost.is_null() && cost.as_u64().is_none_or(|c| c < 1)) {
        return Some(rejected(
            StatusCode::BAD_REQUEST,
            "The cost must be at least 1.",
        ));
    }
    None
}

impl FakeTwitch {
    /// The channel's rewards, preloaded ones first.
    pub fn rewards(&self) -> Vec<RewardView> {
        self.0
            .world
            .lock()
            .unwrap()
            .rewards
            .iter()
            .map(Reward::view)
            .collect()
    }

    /// Fills a redemption event's reward from the channel's reward with that title (keeping the fake one when there
    /// is none), and keeps the redemption so the app can mark it later.
    pub(crate) fn redeemed(&self, event: &mut Value) {
        let mut w = self.0.world.lock().unwrap();
        let title = event["reward"]["title"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        if let Some(r) = w
            .rewards
            .iter()
            .find(|r| r.helix["title"] == title.as_str())
        {
            event["reward"] = json!({"id": r.helix["id"], "title": r.helix["title"], "cost": r.helix["cost"],
                                     "prompt": r.helix["prompt"]});
        }
        w.redemptions.push(Redemption {
            event: event.clone(),
        });
    }

    /// A redemption's status changed on Twitch's side (an `…redemption.update` sent from the panel or a command).
    pub(crate) fn redemption_changed(&self, event: &Value) {
        let mut w = self.0.world.lock().unwrap();
        if let Some(r) = w
            .redemptions
            .iter_mut()
            .find(|r| r.event["id"] == event["id"])
        {
            r.event["status"] = event["status"].clone();
        }
    }
}

/// `?broadcaster_id=…[&id=…][&only_manageable_rewards=true]`.
pub(super) async fn list(State(fake): State<FakeTwitch>, RawQuery(query): RawQuery) -> Response {
    let only_app = param(&query, "only_manageable_rewards") == Some("true");
    let id = param(&query, "id");
    let w = fake.0.world.lock().unwrap();
    let data: Vec<Value> = w
        .rewards
        .iter()
        .filter(|r| (!only_app || r.app_created) && id.is_none_or(|id| r.helix["id"] == id))
        .map(|r| r.helix.clone())
        .collect();
    Json(json!({ "data": data })).into_response()
}

pub(super) async fn create(State(fake): State<FakeTwitch>, Json(body): Json<Value>) -> Response {
    if let Some(refusal) = invalid(&body, true) {
        return refusal;
    }
    let reward = new_reward(&fake.id("fake-reward"), &body);
    fake.0.world.lock().unwrap().rewards.push(Reward {
        app_created: true,
        helix: reward.clone(),
    });
    data(reward)
}

/// The index of the reward `?id=` names, or Twitch's refusal: 404 unknown, 403 made by something else.
fn own_reward(w: &World, query: &Option<String>) -> Result<usize, (StatusCode, &'static str)> {
    let id = param(query, "id").unwrap_or_default();
    let at = w
        .rewards
        .iter()
        .position(|r| r.helix["id"] == id)
        .ok_or((StatusCode::NOT_FOUND, "The reward was not found."))?;
    if !w.rewards[at].app_created {
        return Err((StatusCode::FORBIDDEN, "The app did not create this reward."));
    }
    Ok(at)
}

pub(super) async fn update(
    State(fake): State<FakeTwitch>,
    RawQuery(query): RawQuery,
    Json(body): Json<Value>,
) -> Response {
    if let Some(refusal) = invalid(&body, false) {
        return refusal;
    }
    let mut w = fake.0.world.lock().unwrap();
    match own_reward(&w, &query) {
        Ok(at) => {
            apply(&mut w.rewards[at].helix, &body);
            data(w.rewards[at].helix.clone())
        }
        Err((status, message)) => rejected(status, message),
    }
}

pub(super) async fn delete(State(fake): State<FakeTwitch>, RawQuery(query): RawQuery) -> Response {
    let mut w = fake.0.world.lock().unwrap();
    match own_reward(&w, &query) {
        Ok(at) => {
            w.rewards.remove(at);
            StatusCode::NO_CONTENT.into_response()
        }
        Err((status, message)) => rejected(status, message),
    }
}

/// `?id=…&broadcaster_id=…&reward_id=…` with `{"status": "FULFILLED" | "CANCELED"}`: only an unfulfilled redemption
/// of a reward the app made. Sends the EventSub update, as Twitch does.
pub(super) async fn mark(
    State(fake): State<FakeTwitch>,
    RawQuery(query): RawQuery,
    Json(body): Json<Value>,
) -> Response {
    let status = match body["status"].as_str() {
        Some(s @ ("FULFILLED" | "CANCELED")) => s,
        _ => {
            return rejected(
                StatusCode::BAD_REQUEST,
                "The status must be FULFILLED or CANCELED.",
            );
        }
    };
    let (id, reward_id) = (
        param(&query, "id").unwrap_or_default(),
        param(&query, "reward_id").unwrap_or_default(),
    );
    let event = {
        let mut w = fake.0.world.lock().unwrap();
        match w.rewards.iter().find(|r| r.helix["id"] == reward_id) {
            Some(r) if !r.app_created => {
                return rejected(StatusCode::FORBIDDEN, "The app did not create this reward.");
            }
            _ => {}
        }
        let Some(r) = w.redemptions.iter_mut().find(|r| {
            r.event["id"] == id
                && r.event["reward"]["id"] == reward_id
                && r.event["status"] == "unfulfilled"
        }) else {
            return rejected(
                StatusCode::NOT_FOUND,
                "No unfulfilled redemption has this ID.",
            );
        };
        r.event["status"] = json!(status.to_ascii_lowercase());
        r.event.clone()
    };
    fake.notify(
        "channel.channel_points_custom_reward_redemption.update",
        event.clone(),
        Some(id),
    );
    let mut helix = event;
    helix["status"] = json!(status);
    data(helix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_pairs_land_in_their_objects() {
        let mut r = new_reward("fake-reward-1", &json!({"title": "Hug", "cost": 5}));
        apply(
            &mut r,
            &json!({"is_global_cooldown_enabled": true, "global_cooldown_seconds": 60, "is_paused": true}),
        );
        assert_eq!(
            r["global_cooldown_setting"],
            json!({"is_enabled": true, "global_cooldown_seconds": 60})
        );
        assert_eq!(
            (r["title"].as_str(), r["is_paused"].as_bool()),
            (Some("Hug"), Some(true))
        );
        assert_eq!(r["max_per_stream_setting"]["is_enabled"], false);
    }

    #[test]
    fn bad_titles_and_costs_are_refused() {
        assert!(invalid(&json!({"title": "Hug", "cost": 1}), true).is_none());
        assert!(invalid(&json!({"cost": 1}), true).is_some());
        assert!(invalid(&json!({"title": "Hug", "cost": 0}), true).is_some());
        assert!(invalid(&json!({"title": " "}), false).is_some());
        assert!(invalid(&json!({"is_paused": true}), false).is_none());
    }
}
