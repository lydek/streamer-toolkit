//! What a music file may be, and what switching OBS scenes does to the music (thin module: simple rules, no
//! aggregates).

pub(crate) use toolkit_core::audio::{Format, clean_label};
use toolkit_core::settings::SceneRule;

/// Largest music file accepted.
pub const MAX_BYTES: usize = 50 * 1024 * 1024;

/// The longest length the admin window may report: 24 hours, more than any file within the import limit can hold
/// even at a low bitrate, so every importable track can have its length.
pub(crate) const DURATION_MAX_MS: u32 = 24 * 60 * 60 * 1000;

/// The most scene rules kept.
pub(crate) const SCENE_RULES_MAX: usize = 100;

/// The longest scene name a rule accepts, in characters.
pub(crate) const SCENE_NAME_MAX: usize = 256;

/// What switching OBS scenes does to the music.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SceneAction<'a> {
    /// Leave the player as it is.
    Keep,
    Stop,
    /// Play this rule's playlist.
    Play(&'a SceneRule),
}

/// The enabled rule with a playlist that plays on `scene`, compared without regard to case.
fn rule_for<'a>(rules: &'a [SceneRule], scene: &str) -> Option<&'a SceneRule> {
    let scene = scene.to_lowercase();
    rules.iter().find(|r| {
        r.enabled && r.playlist.is_some() && r.scenes.iter().any(|s| s.to_lowercase() == scene)
    })
}

/// Switching from `from` to `to` (`None`: OBS not connected). The caller skips switches to the same scene.
pub(crate) fn scene_action<'a>(
    rules: &'a [SceneRule],
    from: Option<&str>,
    to: Option<&str>,
) -> SceneAction<'a> {
    let Some(rule) = to.and_then(|to| rule_for(rules, to)) else {
        // No rule for the new scene, or OBS went away.
        return SceneAction::Stop;
    };
    if rule.autoplay {
        return SceneAction::Play(rule);
    }
    match from.and_then(|from| rule_for(rules, from)) {
        Some(left) if left.stop_on_leave => SceneAction::Stop,
        _ => SceneAction::Keep,
    }
}

/// Whether `a` and `b` name the same scene.
pub(crate) fn same_scene(a: Option<&str>, b: Option<&str>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.to_lowercase() == b.to_lowercase(),
        (a, b) => a.is_none() && b.is_none(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(name: &str, scenes: &[&str], autoplay: bool, stop_on_leave: bool) -> SceneRule {
        SceneRule {
            id: name.into(),
            name: name.into(),
            scenes: scenes.iter().map(|s| (*s).into()).collect(),
            playlist: Some(format!("{name}-list")),
            shuffle: false,
            volume: 50,
            autoplay,
            stop_on_leave,
            enabled: true,
        }
    }

    #[test]
    fn switching_scenes_follows_the_rules() {
        let rules = [
            rule("chat", &["Chat", "Break"], true, true),
            rule("quiet", &["Game"], false, false),
            rule("talk", &["Talk"], false, true),
        ];
        let play = |to| match scene_action(&rules, Some("Talk"), Some(to)) {
            SceneAction::Play(r) => r.name.clone(),
            other => panic!("{other:?}"),
        };
        // Autoplay plays, whatever came before; names match without regard to case.
        assert_eq!(play("chat"), "chat");
        assert_eq!(play("BREAK"), "chat");
        // No rule for the new scene: always stop.
        assert_eq!(
            scene_action(&rules, Some("Chat"), Some("Singing")),
            SceneAction::Stop
        );
        assert_eq!(
            scene_action(&rules, Some("Game"), Some("Singing")),
            SceneAction::Stop
        );
        // Autoplay off: the scene left decides.
        assert_eq!(
            scene_action(&rules, Some("chat"), Some("Game")),
            SceneAction::Stop
        );
        assert_eq!(
            scene_action(&rules, Some("Game"), Some("Talk")),
            SceneAction::Keep
        );
        assert_eq!(
            scene_action(&rules, Some("Singing"), Some("Game")),
            SceneAction::Keep
        );
        assert_eq!(scene_action(&rules, None, Some("Game")), SceneAction::Keep);
        // OBS went away.
        assert_eq!(scene_action(&rules, Some("Chat"), None), SceneAction::Stop);
    }

    #[test]
    fn disabled_rules_and_rules_without_a_playlist_count_as_none() {
        let mut off = rule("off", &["Chat"], true, true);
        off.enabled = false;
        let mut gone = rule("gone", &["Game"], true, true);
        gone.playlist = None;
        let rules = [off, gone];
        assert_eq!(scene_action(&rules, None, Some("Chat")), SceneAction::Stop);
        assert_eq!(scene_action(&rules, None, Some("Game")), SceneAction::Stop);
    }

    #[test]
    fn scene_names_compare_without_regard_to_case() {
        assert!(same_scene(Some("Chat"), Some("CHAT")));
        assert!(same_scene(None, None));
        assert!(!same_scene(Some("Chat"), None));
        assert!(!same_scene(Some("Chat"), Some("Game")));
    }
}
