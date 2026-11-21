use super::*;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};
use tokio::time::sleep;

const WAV: &[u8] = b"RIFF\x24\0\0\0WAVEfmt \x10\0\0\0";

struct Rig {
    dir: PathBuf,
    service: Arc<BgmService>,
    modules: Arc<Modules>,
    player: Arc<BgmPlayer>,
    statuses: Arc<Mutex<Vec<PlayerStatus>>>,
    on_air: Arc<AtomicBool>,
    overlays: watch::Sender<()>,
    playlist: String,
    tracks: Vec<String>,
}

impl Rig {
    /// A playlist with one track per length; `None` leaves that track unmeasured.
    fn new(name: &str, lengths: &[Option<u32>], on_air: bool) -> Self {
        let dir = std::env::temp_dir().join(format!("st-bgm-player-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let service = Arc::new(BgmService::new(dir.clone()));
        let modules = Arc::new(Modules::load(dir.clone()));
        let playlist = service.create_playlist("Chat").unwrap().id;
        let tracks: Vec<String> = lengths
            .iter()
            .enumerate()
            .map(|(i, length)| {
                let id = service.import(&format!("t{i}.wav"), WAV).unwrap().id;
                if let Some(ms) = length {
                    service.set_duration(&id, *ms).unwrap();
                }
                service
                    .set_playlists(&id, std::slice::from_ref(&playlist))
                    .unwrap();
                id
            })
            .collect();
        let statuses = Arc::new(Mutex::new(Vec::new()));
        let on_air = Arc::new(AtomicBool::new(on_air));
        let (overlays, changes) = watch::channel(());
        let (seen, open) = (statuses.clone(), on_air.clone());
        let player = BgmPlayer::start(
            service.clone(),
            modules.clone(),
            move || open.load(Ordering::Relaxed),
            changes,
            move |s| seen.lock().unwrap().push(s.clone()),
        );
        Self {
            dir,
            service,
            modules,
            player,
            statuses,
            on_air,
            overlays,
            playlist,
            tracks,
        }
    }

    fn last(&self) -> PlayerStatus {
        self.statuses.lock().unwrap().last().unwrap().clone()
    }

    /// The ID of the track playing now.
    fn playing(&self) -> Option<String> {
        self.last().track.map(|t| t.id)
    }

    fn play(&self) -> Result<(), BgmError> {
        self.player.command(Command::Play {
            playlist: Some(self.playlist.clone()),
        })
    }

    /// Opens or closes the music overlay in OBS.
    async fn set_on_air(&self, on_air: bool) {
        self.on_air.store(on_air, Ordering::Relaxed);
        self.overlays.send_replace(());
        tokio::task::yield_now().await;
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

#[tokio::test(start_paused = true)]
async fn moves_to_the_next_track_at_its_end_and_starts_the_playlist_over() {
    let rig = Rig::new("order", &[Some(1000), Some(2000), Some(500)], true);
    assert_eq!(rig.last().state, PlayState::Stopped, "published at start");
    rig.play().unwrap();
    let t = &rig.tracks;
    assert_eq!(rig.playing().as_ref(), Some(&t[0]));
    assert_eq!(rig.last().position_ms, 0);

    sleep(ms(999)).await;
    assert_eq!(rig.playing().as_ref(), Some(&t[0]));
    sleep(ms(2)).await;
    assert_eq!(rig.playing().as_ref(), Some(&t[1]));
    assert_eq!(rig.last().position_ms, 0);
    sleep(ms(2000)).await;
    assert_eq!(rig.playing().as_ref(), Some(&t[2]));
    sleep(ms(500)).await;
    assert_eq!(rig.playing().as_ref(), Some(&t[0]), "after the last one");
    assert_eq!(rig.last().state, PlayState::Playing);
}

#[tokio::test(start_paused = true)]
async fn shuffled_plays_every_track_once_per_round() {
    let rig = Rig::new("shuffle", &[Some(100); 6], true);
    rig.player.set_options(50, true).unwrap();
    rig.play().unwrap();
    // Sampled mid-track, never on a track end.
    sleep(ms(50)).await;
    let mut round = vec![rig.playing().unwrap()];
    for _ in 1..6 {
        sleep(ms(100)).await;
        round.push(rig.playing().unwrap());
    }
    round.sort();
    let mut all = rig.tracks.clone();
    all.sort();
    assert_eq!(round, all);
    assert!(rig.last().shuffle);
    assert_eq!(rig.last().volume, 50);
}

#[tokio::test(start_paused = true)]
async fn skips_missing_files_and_unmeasured_tracks() {
    let rig = Rig::new("skip", &[Some(1000), None, Some(1000)], true);
    let gone = rig.tracks[0].clone();
    let entry = rig
        .service
        .library()
        .library
        .into_iter()
        .find(|e| e.id == gone)
        .unwrap();
    let (path, _) = rig.service.file_of(&entry).unwrap();
    std::fs::remove_file(path).unwrap();

    rig.play().unwrap();
    assert_eq!(rig.playing().as_ref(), Some(&rig.tracks[2]));
    sleep(ms(1001)).await;
    assert_eq!(
        rig.playing().as_ref(),
        Some(&rig.tracks[2]),
        "the others still skipped"
    );

    std::fs::remove_file(rig.service.file(&rig.tracks[2]).unwrap().0).unwrap();
    sleep(ms(1001)).await;
    assert_eq!(rig.last().state, PlayState::Stopped, "nothing left to play");
    assert!(matches!(rig.play(), Err(BgmError::NothingToPlay)));
}

#[tokio::test(start_paused = true)]
async fn without_an_overlay_it_reports_off_air_and_holds_the_clock() {
    let rig = Rig::new("off-air", &[Some(1000), Some(1000)], false);
    rig.play().unwrap();
    let status = rig.last();
    assert_eq!((status.state, status.on_air), (PlayState::Playing, false));
    sleep(ms(5000)).await;
    assert_eq!(rig.player.status().position_ms, 0, "not pretending to play");
    assert_eq!(rig.playing().as_ref(), Some(&rig.tracks[0]));

    rig.set_on_air(true).await;
    assert!(rig.last().on_air);
    sleep(ms(400)).await;
    // OBS reloads the source: the clock stops while it is gone, and goes on from there.
    rig.set_on_air(false).await;
    assert_eq!((rig.last().on_air, rig.last().position_ms), (false, 400));
    sleep(ms(10_000)).await;
    rig.set_on_air(true).await;
    assert_eq!((rig.last().on_air, rig.last().position_ms), (true, 400));
    sleep(ms(601)).await;
    assert_eq!(rig.playing().as_ref(), Some(&rig.tracks[1]));
}

#[tokio::test(start_paused = true)]
async fn pause_seek_previous_next_and_stop() {
    let rig = Rig::new("controls", &[Some(10_000), Some(10_000)], true);
    let t = &rig.tracks;
    rig.play().unwrap();
    sleep(ms(4000)).await;
    rig.player.command(Command::Pause).unwrap();
    sleep(ms(60_000)).await;
    assert_eq!(
        (rig.last().state, rig.last().position_ms),
        (PlayState::Paused, 4000)
    );
    rig.play().unwrap();
    assert_eq!(
        (rig.last().state, rig.last().position_ms),
        (PlayState::Playing, 4000)
    );

    rig.player
        .command(Command::Seek { position_ms: 8000 })
        .unwrap();
    sleep(ms(1000)).await;
    assert_eq!(rig.player.status().position_ms, 9000);
    // Past three seconds "previous" starts the track over; within them it goes back one.
    rig.player.command(Command::Previous).unwrap();
    assert_eq!(
        (rig.playing().as_ref(), rig.last().position_ms),
        (Some(&t[0]), 0)
    );
    rig.player.command(Command::Previous).unwrap();
    assert_eq!(rig.playing().as_ref(), Some(&t[1]), "wraps to the last");
    rig.player.command(Command::Next).unwrap();
    assert_eq!(rig.playing().as_ref(), Some(&t[0]));
    // Seeking to the end ends the track.
    rig.player
        .command(Command::Seek {
            position_ms: 99_999,
        })
        .unwrap();
    tokio::task::yield_now().await;
    assert_eq!(rig.playing().as_ref(), Some(&t[1]));

    rig.player.command(Command::Stop).unwrap();
    let status = rig.last();
    assert_eq!((status.state, status.track), (PlayState::Stopped, None));
    assert_eq!(
        status.playlist.as_ref(),
        Some(&rig.playlist),
        "kept for the next play"
    );
    rig.player
        .command(Command::Play { playlist: None })
        .unwrap();
    assert_eq!(rig.playing().as_ref(), Some(&t[0]));
}

#[tokio::test(start_paused = true)]
async fn play_needs_a_known_playlist_and_the_feature_on() {
    let rig = Rig::new("refuse", &[Some(1000)], true);
    assert!(matches!(
        rig.player.command(Command::Play { playlist: None }),
        Err(BgmError::Invalid)
    ));
    assert!(matches!(
        rig.player.command(Command::Play {
            playlist: Some("nope".into())
        }),
        Err(BgmError::NotFound)
    ));
    let empty = rig.service.create_playlist("Empty").unwrap().id;
    assert!(matches!(
        rig.player.command(Command::Play {
            playlist: Some(empty)
        }),
        Err(BgmError::NothingToPlay)
    ));

    rig.modules.set_enabled(modules::BGM, false).unwrap();
    assert!(matches!(rig.play(), Err(BgmError::Off)));
    rig.modules.set_enabled(modules::BGM, true).unwrap();
    rig.play().unwrap();
    rig.modules.set_enabled(modules::BGM, false).unwrap();
    sleep(ms(1001)).await;
    assert_eq!(
        rig.last().state,
        PlayState::Stopped,
        "stops at the end of the track"
    );
}

/// Saves `rules`, each `(name, scenes, playlist, autoplay, stop_on_leave)`, shuffle off at 70%.
fn set_rules(rig: &Rig, rules: &[(&str, &[&str], &str, bool, bool)]) {
    let rules = rules
        .iter()
        .map(|(name, scenes, playlist, autoplay, stop_on_leave)| {
            toolkit_core::settings::SceneRule {
                id: (*name).into(),
                name: (*name).into(),
                scenes: scenes.iter().map(|s| (*s).into()).collect(),
                playlist: Some((*playlist).into()),
                shuffle: false,
                volume: 70,
                autoplay: *autoplay,
                stop_on_leave: *stop_on_leave,
                enabled: true,
            }
        })
        .collect();
    settings::update(&rig.dir, |s| s.bgm.scene_rules = rules).unwrap();
}

fn scene(rig: &Rig, name: Option<&str>) {
    rig.player.scene_changed(name.map(Into::into));
}

#[tokio::test(start_paused = true)]
async fn obs_connecting_applies_the_scene_on_air_and_the_same_scene_does_nothing() {
    let rig = Rig::new("scene-start", &[Some(10_000), Some(10_000)], true);
    let chat = rig.playlist.clone();
    set_rules(&rig, &[("Chat", &["Chat"], &chat, true, true)]);
    scene(&rig, Some("Chat"));
    let status = rig.last();
    assert_eq!(status.state, PlayState::Playing);
    assert_eq!(status.playlist.as_ref(), Some(&chat));
    assert_eq!(rig.playing().as_ref(), Some(&rig.tracks[0]));
    assert_eq!(
        (status.volume, status.shuffle),
        (70, false),
        "the rule's options"
    );

    rig.player.command(Command::Pause).unwrap();
    scene(&rig, Some("CHAT"));
    assert_eq!(
        rig.last().state,
        PlayState::Paused,
        "the same scene, by any case"
    );
}

#[tokio::test(start_paused = true)]
async fn a_track_in_both_playlists_plays_on_uninterrupted() {
    let rig = Rig::new(
        "scene-keep",
        &[Some(10_000), Some(10_000), Some(10_000)],
        true,
    );
    let (chat, t) = (rig.playlist.clone(), rig.tracks.clone());
    let game = rig.service.create_playlist("Game").unwrap().id;
    rig.service
        .set_playlists(&t[1], &[chat.clone(), game.clone()])
        .unwrap();
    rig.service
        .set_playlists(&t[2], std::slice::from_ref(&game))
        .unwrap();
    set_rules(
        &rig,
        &[
            ("Chat", &["Chat"], &chat, true, true),
            ("Game", &["Game"], &game, true, true),
        ],
    );
    scene(&rig, Some("Chat"));
    rig.player.command(Command::Next).unwrap();
    sleep(ms(4000)).await;
    assert_eq!(rig.playing().as_ref(), Some(&t[1]));

    scene(&rig, Some("Game"));
    let status = rig.last();
    assert_eq!(status.playlist.as_ref(), Some(&game));
    assert_eq!(
        (rig.playing().as_ref(), status.position_ms),
        (Some(&t[1]), 4000)
    );
    sleep(ms(6001)).await;
    assert_eq!(
        rig.playing().as_ref(),
        Some(&t[2]),
        "goes on in the new playlist"
    );

    // A track the new playlist does not have: it starts from its first track.
    scene(&rig, Some("Chat"));
    assert_eq!(
        (rig.playing().as_ref(), rig.last().position_ms),
        (Some(&t[0]), 0)
    );
}

#[tokio::test(start_paused = true)]
async fn unmapped_scenes_stop_and_autoplay_off_follows_the_scene_left() {
    let rig = Rig::new("scene-rules", &[Some(60_000)], true);
    let chat = rig.playlist.clone();
    set_rules(
        &rig,
        &[
            ("Chat", &["Chat"], &chat, true, false),
            ("Talk", &["Talk"], &chat, true, true),
            ("Quiet", &["Game"], &chat, false, false),
        ],
    );
    scene(&rig, Some("Chat"));
    scene(&rig, Some("Game"));
    assert_eq!(
        rig.last().state,
        PlayState::Playing,
        "Chat does not stop on leaving"
    );
    scene(&rig, Some("Talk"));
    scene(&rig, Some("Game"));
    assert_eq!(
        rig.last().state,
        PlayState::Stopped,
        "Talk stops on leaving"
    );
    scene(&rig, Some("Chat"));
    scene(&rig, Some("Singing"));
    assert_eq!(rig.last().state, PlayState::Stopped, "no rule: stop");
    scene(&rig, Some("Chat"));
    scene(&rig, None);
    assert_eq!(rig.last().state, PlayState::Stopped, "OBS went away");
}

#[tokio::test(start_paused = true)]
async fn without_rules_or_with_the_feature_off_scenes_leave_the_player_alone() {
    let rig = Rig::new("scene-none", &[Some(60_000)], true);
    rig.play().unwrap();
    scene(&rig, Some("Chat"));
    scene(&rig, None);
    assert_eq!(
        rig.last().state,
        PlayState::Playing,
        "no rules: used by hand"
    );

    set_rules(
        &rig,
        &[("Chat", &["Chat"], &rig.playlist.clone(), true, true)],
    );
    rig.player.command(Command::Stop).unwrap();
    rig.modules.set_enabled(modules::BGM, false).unwrap();
    scene(&rig, Some("Chat"));
    assert_eq!(rig.last().state, PlayState::Stopped);
}

#[tokio::test(start_paused = true)]
async fn a_test_picks_the_first_playable_track_of_the_last_playlist_and_changes_nothing() {
    let rig = Rig::new("test", &[None, Some(1000), Some(2000)], true);
    let test = || {
        let mut sent = None;
        rig.player.test(|t| sent = Some(t)).map(|()| sent.unwrap())
    };
    assert!(
        matches!(test(), Err(BgmError::NothingToPlay)),
        "no playlist chosen yet"
    );
    rig.play().unwrap();
    assert!(matches!(test(), Err(BgmError::Playing)));
    rig.player.command(Command::Pause).unwrap();
    let before = rig.statuses.lock().unwrap().len();
    let sent = test().unwrap();
    // The unmeasured first track cannot play.
    assert_eq!(sent.track.id, rig.tracks[1]);
    assert_eq!(sent.volume, rig.last().volume);
    assert_eq!(
        rig.statuses.lock().unwrap().len(),
        before,
        "nothing published"
    );
    assert_eq!(rig.last().state, PlayState::Paused);

    rig.player.command(Command::Stop).unwrap();
    assert_eq!(
        test().unwrap().track.id,
        rig.tracks[1],
        "the playlist is kept after a stop"
    );
    rig.modules.set_enabled(modules::BGM, false).unwrap();
    assert!(matches!(test(), Err(BgmError::Off)));
}

#[tokio::test(start_paused = true)]
async fn a_play_command_waits_until_the_test_is_sent() {
    let rig = Rig::new("test-lock", &[Some(1000)], true);
    rig.play().unwrap();
    rig.player.command(Command::Stop).unwrap();
    let played = Arc::new(AtomicBool::new(false));
    let mut play = None;
    rig.player
        .test(|_| {
            let (player, done, playlist) =
                (rig.player.clone(), played.clone(), rig.playlist.clone());
            play = Some(std::thread::spawn(move || {
                player
                    .command(Command::Play {
                        playlist: Some(playlist),
                    })
                    .unwrap();
                done.store(true, Ordering::SeqCst);
            }));
            std::thread::sleep(std::time::Duration::from_millis(50));
            assert!(
                !played.load(Ordering::SeqCst),
                "play got in before the test was sent"
            );
        })
        .unwrap();
    play.unwrap().join().unwrap();
    assert!(played.load(Ordering::SeqCst));
}
