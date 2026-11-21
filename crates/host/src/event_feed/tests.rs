use super::*;

fn at(min: i64) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-11-21T10:00:00Z")
        .unwrap()
        .to_utc()
        + chrono::TimeDelta::minutes(min)
}

/// Stored the way the modules store times: microseconds.
fn stored(min: i64) -> String {
    at(min).to_rfc3339_opts(SecondsFormat::Micros, true)
}

fn activity(id: &str, kind: &str, min: i64, gifted: bool) -> ActivityEntry {
    ActivityEntry {
        alert_id: id.into(),
        kind: kind.into(),
        user_name: Some("阿凱".into()),
        tier: Some(1),
        months: None,
        count: (kind == "gift").then_some(3),
        bits: None,
        viewers: None,
        gifted,
        message: None,
        occurred_at: stored(min),
    }
}

fn redemption(id: &str, min: i64) -> RedeemedEntry {
    RedeemedEntry {
        twitch_redemption_id: id.into(),
        user_name: "橘子".into(),
        reward_title: "喝水".into(),
        reward_cost: 100,
        user_input: String::new(),
        redeemed_at: stored(min),
    }
}

fn checkin(id: &str, min: i64) -> CheckedInEntry {
    CheckedInEntry {
        record_id: id.into(),
        user_name: "小魚".into(),
        trigger: "chat".into(),
        occurred_at: stored(min),
    }
}

fn records() -> Records {
    Records {
        activity: vec![
            activity("a1", "follow", 1, false),
            activity("a2", "gift", 4, false),
            // Each gifted sub is stored too; the gift stands for them.
            activity("a3", "subscribe", 4, true),
        ],
        redemptions: vec![redemption("r1", 2)],
        checkins: vec![checkin("c1", 3)],
    }
}

fn remembered_item(
    key: Option<&str>,
    subject: Subject,
    source: Source,
    outcome: Option<Outcome>,
    min: i64,
) -> RecentItem {
    RecentItem {
        id: 1,
        key: key.map(Into::into),
        subject,
        source,
        queued_at: at(min).to_rfc3339_opts(SecondsFormat::Millis, true),
        outcome,
    }
}

fn alert(id: &str, kind: AlertKind, min: i64) -> Alert {
    Alert {
        id: id.into(),
        user_id: Some("7".into()),
        user_login: Some("kai".into()),
        user_name: Some("阿凱".into()),
        kind,
        occurred_at: at(min).to_rfc3339(),
    }
}

fn this_launch() -> Remembered {
    Remembered {
        alerts: vec![
            RememberedAlert {
                alert: alert("a1", AlertKind::Follow, 1),
                source: Source::Twitch,
                outcome: Some(Outcome::OffAir),
            },
            RememberedAlert {
                alert: alert("a2", AlertKind::Gift { tier: 1, count: 3 }, 4),
                source: Source::Twitch,
                outcome: None,
            },
            RememberedAlert {
                alert: alert("s1", AlertKind::Raid { viewers: 12 }, 5),
                source: Source::Simulation,
                outcome: Some(Outcome::Played),
            },
        ],
        sounds: vec![remembered_item(
            Some("r1"),
            Subject::Sound {
                sound: "Bell".into(),
                viewer: "橘子".into(),
            },
            Source::Twitch,
            Some(Outcome::SoundMissing),
            2,
        )],
        cards: vec![
            remembered_item(
                Some("c1"),
                Subject::Card {
                    viewer: "小魚".into(),
                    stamps: 3,
                    slots: 10,
                },
                Source::Twitch,
                Some(Outcome::Skipped),
                3,
            ),
            remembered_item(
                None,
                Subject::Card {
                    viewer: "Kai".into(),
                    stamps: 2,
                    slots: 10,
                },
                Source::Simulation,
                Some(Outcome::Stopped),
                6,
            ),
        ],
    }
}

fn empty() -> Remembered {
    Remembered {
        alerts: Vec::new(),
        sounds: Vec::new(),
        cards: Vec::new(),
    }
}

#[test]
fn it_merges_every_module_newest_first_with_what_became_of_each() {
    let feed = compose(at(0), records(), this_launch());
    let summary: Vec<_> = feed
        .iter()
        .map(|i| {
            (
                i.event.clone(),
                i.result.as_ref().map(|r| (r.triggered, r.outcome)),
                i.simulated,
            )
        })
        .collect();
    let checkin = |trigger: &str| FeedEvent::Checkin {
        trigger: trigger.into(),
    };
    assert_eq!(
        summary,
        [
            (
                checkin("simulation"),
                Some((Triggered::Card, FeedOutcome::Stopped)),
                true
            ),
            (
                FeedEvent::Alert {
                    alert: "raid".into(),
                    amount: Some(12)
                },
                Some((Triggered::Alert, FeedOutcome::Played)),
                true
            ),
            (
                FeedEvent::Alert {
                    alert: "gift".into(),
                    amount: Some(3)
                },
                Some((Triggered::Alert, FeedOutcome::Queued)),
                false
            ),
            (
                checkin("chat"),
                Some((Triggered::Card, FeedOutcome::Skipped)),
                false
            ),
            (
                FeedEvent::Redemption {
                    reward: "喝水".into()
                },
                Some((Triggered::Sound, FeedOutcome::SoundMissing)),
                false
            ),
            (
                FeedEvent::Alert {
                    alert: "follow".into(),
                    amount: None
                },
                Some((Triggered::Alert, FeedOutcome::OffAir)),
                false
            ),
        ]
    );
    assert_eq!(
        feed[4].result.as_ref().unwrap().sound.as_deref(),
        Some("Bell")
    );
    assert_eq!(feed[5].at, "2026-11-21T10:01:00.000Z", "one time format");
    assert_eq!(feed[2].viewer.as_deref(), Some("阿凱"));
}

#[test]
fn after_a_restart_the_events_stay_without_their_outcomes() {
    let feed = compose(at(0), records(), empty());
    assert_eq!(feed.len(), 4);
    assert!(feed.iter().all(|i| i.result.is_none() && !i.simulated));
}

#[test]
fn only_what_happened_since_the_start_and_at_most_fifty() {
    let feed = compose(at(3), records(), this_launch());
    assert!(feed.iter().all(|i| parse(&i.at).unwrap() >= at(3)));
    assert_eq!(feed.len(), 4);

    let many = Records {
        activity: (0..80)
            .map(|n| activity(&format!("a{n}"), "follow", n, false))
            .collect(),
        redemptions: Vec::new(),
        checkins: Vec::new(),
    };
    let feed = compose(at(0), many, empty());
    assert_eq!(feed.len(), ITEMS);
    assert_eq!(feed[0].at, "2026-11-21T11:19:00.000Z", "the latest kept");
}
