use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::sync::Notify;
use toolkit_core::commands::{Allowed, CommandEntry};
use toolkit_core::events::ChatBadge;

/// Posts into a list; `hold` keeps each send waiting until released, as a slow Twitch answer would.
#[derive(Default)]
struct FakeSender {
    posted: Mutex<Vec<String>>,
    next: AtomicUsize,
    signed_in: AtomicBool,
    hold: AtomicBool,
    release: Notify,
}

impl ChatSender for Arc<FakeSender> {
    fn can_send(&self) -> bool {
        self.signed_in.load(Ordering::SeqCst)
    }
    fn send(&self, text: String) -> BoxFuture<Result<String, String>> {
        let me = self.clone();
        Box::pin(async move {
            if me.hold.load(Ordering::SeqCst) {
                me.release.notified().await;
            }
            me.posted.lock().unwrap().push(text);
            Ok(format!("sent-{}", me.next.fetch_add(1, Ordering::SeqCst)))
        })
    }
}

struct Lookups;
impl BotLookups for Lookups {
    fn counter_value(&self, name: String) -> BoxFuture<Option<i64>> {
        Box::pin(async move { (name == "死亡次數").then_some(3) })
    }
}

fn message(id: &str, user: &str, text: &str, badges: Vec<ChatBadge>) -> ChatMessage {
    ChatMessage {
        id: id.into(),
        broadcaster_id: "42".into(),
        user_id: user.into(),
        user_login: format!("u{user}"),
        user_name: if user == "42" {
            "實況主".into()
        } else {
            "小魚".into()
        },
        color: None,
        text: text.into(),
        fragments: vec![],
        badges,
        highlighted: false,
        bits: 0,
        reply: None,
        twitch_badges: vec![],
        shared_from: None,
        sent_at: "2026-11-28T10:00:00Z".into(),
    }
}

struct Bot {
    service: Arc<BotService>,
    sender: Arc<FakeSender>,
    dir: PathBuf,
}

impl Drop for Bot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A bot with these commands (name, who may use it, reply, cooldown seconds, enabled), signed in and on.
fn bot(name: &str, commands: &[(&str, Allowed, &str, u32, bool)]) -> Bot {
    let dir = std::env::temp_dir().join(format!("st-bot-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let table = Arc::new(CommandTable::load(dir.clone(), vec![]));
    let mut saved = Vec::new();
    for (i, (command, allowed, reply, cooldown, enabled)) in commands.iter().enumerate() {
        let id = format!("c{i}");
        saved.push(BotCommand {
            id: id.clone(),
            reply: (*reply).into(),
            cooldown_seconds: *cooldown,
            enabled: *enabled,
        });
        let all = saved.clone();
        table
            .set(
                &command_key(&id),
                CommandEntry {
                    name: (*command).into(),
                    allowed: *allowed,
                    module: "bot".into(),
                    label: None,
                },
                move |s| s.bot.commands = all,
            )
            .unwrap();
    }
    let modules = Arc::new(Modules::load(dir.clone()));
    modules.set_enabled(modules::BOT, true).unwrap();
    let sender = Arc::new(FakeSender::default());
    sender.signed_in.store(true, Ordering::SeqCst);
    let service = BotService::start(
        dir.clone(),
        table,
        modules,
        Arc::new(sender.clone()),
        Arc::new(Lookups),
    );
    Bot {
        service,
        sender,
        dir,
    }
}

/// Lets the worker run (time is paused in these tests, so nothing waits for real).
async fn settle() {
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
}

fn posted(b: &Bot) -> Vec<String> {
    b.sender.posted.lock().unwrap().clone()
}

#[tokio::test(start_paused = true)]
async fn a_command_is_answered_with_its_variables_and_only_for_who_may_use_it() {
    let b = bot(
        "answer",
        &[
            (
                "死亡",
                Allowed::Everyone,
                "{觀眾}：死了 {計數:死亡次數} 次，開台 {直播時長}",
                0,
                true,
            ),
            ("mod", Allowed::Moderator, "管理員好", 0, true),
            ("off", Allowed::Everyone, "停用了", 0, false),
        ],
    );
    b.service
        .receive_chat(&message("1", "7", "!死亡 多少", vec![]), Source::Twitch);
    b.service
        .receive_chat(&message("2", "7", "!MOD", vec![]), Source::Twitch);
    b.service
        .receive_chat(&message("3", "7", "!off", vec![]), Source::Twitch);
    // The streamer may use every command; a moderator may use moderator ones.
    b.service
        .receive_chat(&message("4", "42", "!mod", vec![]), Source::Twitch);
    b.service.receive_chat(
        &message("5", "8", "!mod", vec![ChatBadge::Moderator]),
        Source::Twitch,
    );
    settle().await;
    assert_eq!(
        posted(&b),
        ["小魚：死了 3 次，開台 未開播", "管理員好", "管理員好"]
    );
}

#[tokio::test(start_paused = true)]
async fn simulated_chat_never_makes_it_post_and_neither_does_a_bot_that_is_off_or_signed_out() {
    let b = bot("quiet", &[("hi", Allowed::Everyone, "嗨", 0, true)]);
    b.service
        .receive_chat(&message("1", "7", "!hi", vec![]), Source::Simulation);
    b.sender.signed_in.store(false, Ordering::SeqCst);
    b.service
        .receive_chat(&message("2", "7", "!hi", vec![]), Source::Twitch);
    b.sender.signed_in.store(true, Ordering::SeqCst);
    b.service.modules.set_enabled(modules::BOT, false).unwrap();
    b.service
        .receive_chat(&message("3", "7", "!hi", vec![]), Source::Twitch);
    settle().await;
    assert!(posted(&b).is_empty());
    assert_eq!(b.service.dropped(), 0);
}

#[tokio::test(start_paused = true)]
async fn a_command_waits_out_its_cooldown_for_everyone() {
    let b = bot(
        "cooldown",
        &[("dc", Allowed::Everyone, "discord", 30, true)],
    );
    b.service
        .receive_chat(&message("1", "7", "!dc", vec![]), Source::Twitch);
    b.service
        .receive_chat(&message("2", "42", "!dc", vec![]), Source::Twitch);
    settle().await;
    tokio::time::advance(Duration::from_secs(30)).await;
    b.service
        .receive_chat(&message("3", "7", "!dc", vec![]), Source::Twitch);
    settle().await;
    assert_eq!(posted(&b), ["discord", "discord"]);
}

#[tokio::test(start_paused = true)]
async fn the_bot_never_answers_its_own_messages_even_before_twitch_answers() {
    // A reply with its own command in it.
    let b = bot(
        "own",
        &[("loop", Allowed::Everyone, "打 !loop 試試", 0, true)],
    );
    b.sender.hold.store(true, Ordering::SeqCst);
    b.service
        .receive_chat(&message("1", "7", "!loop", vec![]), Source::Twitch);
    settle().await;
    // Chat brings the reply back while the send still waits for Twitch.
    b.service.receive_chat(
        &message("echo-1", "42", "打 !loop 試試", vec![]),
        Source::Twitch,
    );
    b.sender.release.notify_one();
    settle().await;
    // And by its ID once Twitch answered.
    b.sender.hold.store(false, Ordering::SeqCst);
    b.service.receive_chat(
        &message("sent-0", "42", "打 !loop 試試", vec![]),
        Source::Twitch,
    );
    b.service.receive_chat(
        &message("echo-1", "42", "打 !loop 試試", vec![]),
        Source::Twitch,
    );
    settle().await;
    assert_eq!(posted(&b), ["打 !loop 試試"]);
    // The streamer's own command still counts.
    b.service
        .receive_chat(&message("9", "42", "!loop", vec![]), Source::Twitch);
    settle().await;
    assert_eq!(posted(&b).len(), 2);
}

#[tokio::test(start_paused = true)]
async fn at_most_twenty_go_out_in_thirty_seconds_counting_the_streamer_and_the_rest_wait_or_drop() {
    let b = bot("rate", &[("hi", Allowed::Everyone, "嗨", 0, true)]);
    // Five sent by hand use up part of the 30 seconds.
    for _ in 0..5 {
        b.service.note_manual_send();
    }
    // The queue holds 20, all asked before the worker takes any.
    for i in 0..20 {
        b.service
            .receive_chat(&message(&i.to_string(), "7", "!hi", vec![]), Source::Twitch);
    }
    settle().await;
    // 15 fit in what is left of the 30 seconds; the other 5 wait.
    assert_eq!(posted(&b).len(), 15);
    assert_eq!(b.service.dropped(), 0);
    tokio::time::advance(Duration::from_secs(30)).await;
    settle().await;
    assert_eq!(posted(&b).len(), 20);

    // More than the queue holds: the rest is dropped and counted.
    for i in 0..45 {
        b.service.receive_chat(
            &message(&format!("x{i}"), "7", "!hi", vec![]),
            Source::Twitch,
        );
    }
    settle().await;
    assert!(b.service.dropped() > 0);
}

#[tokio::test(start_paused = true)]
async fn a_message_waiting_over_two_minutes_is_dropped() {
    let b = bot("expiry", &[("hi", Allowed::Everyone, "嗨", 0, true)]);
    b.sender.hold.store(true, Ordering::SeqCst);
    b.service
        .receive_chat(&message("1", "7", "!hi", vec![]), Source::Twitch);
    b.service
        .receive_chat(&message("2", "7", "!hi", vec![]), Source::Twitch);
    settle().await;
    // The first send hangs for over two minutes; the second one waited too long by then.
    tokio::time::advance(Duration::from_secs(121)).await;
    b.sender.hold.store(false, Ordering::SeqCst);
    b.sender.release.notify_one();
    settle().await;
    assert_eq!(posted(&b), ["嗨"]);
    assert_eq!(b.service.dropped(), 1);
}
