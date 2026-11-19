//! Twitch chat over IRC, anonymous and read-only, used for one tag EventSub does not carry: `first-msg`, Twitch's
//! mark on a viewer's first message in the channel. No sign-in and no scope: it joins the signed-in broadcaster's
//! channel as a `justinfan` guest and never sends chat.

use std::{convert::Infallible, sync::Arc, time::Duration};

use futures_util::{SinkExt, StreamExt};
use rustls::ClientConfig;
use tokio_tungstenite::tungstenite;

use super::{
    auth::TwitchAuth,
    eventsub::{Socket, connect},
};

/// Twitch chat over WebSocket.
pub const IRC_URL: &str = "wss://irc-ws.chat.twitch.tv:443";

/// Twitch sends tags only once both are granted; it asks for `commands` alongside `tags`.
const CAPABILITIES: &str = "twitch.tv/tags twitch.tv/commands";

/// Twitch pings about every five minutes; a connection silent for longer is gone.
const SILENT: Duration = Duration::from_secs(6 * 60);

#[derive(Debug, thiserror::Error)]
pub enum IrcError {
    #[error("irc.connect_failed")]
    Connect,
    #[error("irc.closed")]
    Closed,
    #[error("irc.silent_too_long")]
    Silent,
    #[error("irc.capabilities_refused")]
    CapabilitiesRefused,
}

pub struct FirstChats {
    /// `wss://irc-ws.chat.twitch.tv:443`.
    pub url: String,
    pub auth: TwitchAuth,
    pub tls: Arc<ClientConfig>,
    /// The ID of a message Twitch marks as the sender's first in the channel.
    pub deliver: Arc<dyn Fn(String) + Send + Sync>,
    /// `true` once joined with the tags granted, `false` when the connection ends or the account signs out.
    pub connected: Arc<dyn Fn(bool) + Send + Sync>,
}

impl FirstChats {
    /// Runs until the task is dropped: joins while signed in, leaves on logout, retries with backoff.
    pub async fn run(self) {
        let mut account = self.auth.account();
        let mut failures = 0u32;
        loop {
            let signed_in = account.borrow_and_update().is_some();
            let login = self
                .auth
                .token()
                .map(|t| t.login)
                .filter(|l| valid_login(l));
            let (true, Some(login)) = (signed_in, login) else {
                (self.connected)(false);
                if account.changed().await.is_err() {
                    return;
                }
                continue;
            };
            let error = tokio::select! {
                result = self.session(&login, &mut failures) => match result {
                    Err(e) => e,
                },
                changed = account.changed() => {
                    (self.connected)(false);
                    if changed.is_err() {
                        return;
                    }
                    continue;
                }
            };
            (self.connected)(false);
            failures += 1;
            let wait = crate::backoff(failures);
            tracing::warn!(error = %error, retry_in = ?wait, "Twitch IRC connection ended");
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                changed = account.changed() => if changed.is_err() { return },
            }
        }
    }

    async fn session(&self, login: &str, failures: &mut u32) -> Result<Infallible, IrcError> {
        let mut socket = connect(&self.url, &self.tls)
            .await
            .map_err(|_| IrcError::Connect)?;
        // A guest nick needs no password; the number only keeps two toolkits apart.
        let nick = format!("justinfan{}", 10_000 + std::process::id() % 80_000);
        for line in [
            format!("CAP REQ :{CAPABILITIES}"),
            format!("NICK {nick}"),
            format!("JOIN #{login}"),
        ] {
            send(&mut socket, line).await?;
        }
        // Without the tags no first-chat mark arrives, so the join alone does not count as connected.
        let (mut granted, mut joined) = (false, false);
        loop {
            let text = tokio::time::timeout(SILENT, socket.next())
                .await
                .map_err(|_| IrcError::Silent)?;
            let text = match text {
                Some(Ok(tungstenite::Message::Text(t))) => t,
                Some(Ok(tungstenite::Message::Close(_))) | None | Some(Err(_)) => {
                    return Err(IrcError::Closed);
                }
                Some(Ok(_)) => continue,
            };
            for line in text.as_str().split("\r\n").filter(|l| !l.is_empty()) {
                if let Some(rest) = line.strip_prefix("PING") {
                    send(&mut socket, format!("PONG{rest}")).await?;
                } else if let Some(ack) = capability_reply(line) {
                    if !ack {
                        return Err(IrcError::CapabilitiesRefused);
                    }
                    granted = true;
                    self.up(granted, joined, failures);
                } else if is_joined(line, &nick, login) {
                    joined = true;
                    self.up(granted, joined, failures);
                } else if let Some(id) = first_message_id(line) {
                    (self.deliver)(id);
                }
            }
        }
    }

    /// Connected once both the tags and the join are confirmed (each confirmation arrives once per connection).
    fn up(&self, granted: bool, joined: bool, failures: &mut u32) {
        if granted && joined {
            *failures = 0;
            (self.connected)(true);
        }
    }
}

async fn send(socket: &mut Socket, line: String) -> Result<(), IrcError> {
    socket
        .send(tungstenite::Message::Text(line.into()))
        .await
        .map_err(|_| IrcError::Closed)
}

/// A Twitch login: lowercase letters, digits and `_`, up to 25; anything else never reaches a command line.
fn valid_login(login: &str) -> bool {
    (1..=25).contains(&login.len())
        && login
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Twitch confirms the join with `:<nick>!<nick>@<nick>.tmi.twitch.tv JOIN #<login>`.
fn is_joined(line: &str, nick: &str, login: &str) -> bool {
    line.starts_with(&format!(":{nick}!")) && line.ends_with(&format!(" JOIN #{login}"))
}

/// Twitch's answer to `CAP REQ`: `:tmi.twitch.tv CAP * ACK :<caps>` (`true`) or `... NAK :<caps>` (`false`).
/// An acknowledgement must include both requested capabilities.
fn capability_reply(line: &str) -> Option<bool> {
    let mut words = line.split(' ');
    if words.nth(1) != Some("CAP") {
        return None;
    }
    match words.nth(1) {
        Some("ACK") => {
            let granted = words.map(|w| w.trim_start_matches(':')).collect::<Vec<_>>();
            Some(
                CAPABILITIES
                    .split_whitespace()
                    .all(|cap| granted.contains(&cap)),
            )
        }
        Some("NAK") => Some(false),
        _ => None,
    }
}

/// The `id` of a `PRIVMSG` tagged `first-msg=1`.
fn first_message_id(line: &str) -> Option<String> {
    let (tags, rest) = line.strip_prefix('@')?.split_once(' ')?;
    if rest.split(' ').nth(1) != Some("PRIVMSG") {
        return None;
    }
    let mut first = false;
    let mut id = None;
    for tag in tags.split(';') {
        match tag.split_once('=') {
            Some(("first-msg", "1")) => first = true,
            Some(("id", v)) if !v.is_empty() => id = Some(v.to_owned()),
            _ => {}
        }
    }
    id.filter(|_| first)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{tls, twitch::eventsub::tests::signed_in_auth};
    use axum::{
        Router,
        extract::{State, WebSocketUpgrade, ws::Message as WsMessage},
        response::Response,
        routing::any,
    };
    use tokio::sync::mpsc;

    #[test]
    fn only_a_first_message_gives_its_id() {
        let line =
            |tags: &str| format!("@{tags} :fan!fan@fan.tmi.twitch.tv PRIVMSG #streamer :hi there");
        assert_eq!(
            first_message_id(&line("badge-info=;first-msg=1;id=abc-1;user-id=7")),
            Some("abc-1".into())
        );
        assert_eq!(first_message_id(&line("first-msg=0;id=abc-2")), None);
        assert_eq!(first_message_id(&line("first-msg=1")), None, "no id");
        assert_eq!(
            first_message_id("@first-msg=1;id=x :tmi.twitch.tv USERNOTICE #streamer"),
            None,
            "not a chat message"
        );
        assert_eq!(
            first_message_id(":tmi.twitch.tv 001 justinfan1 :Welcome"),
            None
        );
    }

    #[test]
    fn logins_are_checked_before_they_reach_a_command() {
        assert!(valid_login("streamer_01"));
        for bad in [
            "",
            "Streamer",
            "a b",
            "a\r\nPRIVMSG",
            "x".repeat(26).as_str(),
        ] {
            assert!(!valid_login(bad), "{bad:?}");
        }
    }

    /// A fake Twitch IRC: reports what the client sent, answers `CAP REQ` with `cap` (`ACK` or `NAK`), confirms
    /// the join, then sends `lines`.
    async fn fake_irc(
        cap: &'static str,
        lines: Vec<&'static str>,
    ) -> (String, mpsc::UnboundedReceiver<String>) {
        let (tx, rx) = mpsc::unbounded_channel();
        type Fake = (
            mpsc::UnboundedSender<String>,
            &'static str,
            Vec<&'static str>,
        );
        async fn ws(ws: WebSocketUpgrade, State((tx, cap, lines)): State<Fake>) -> Response {
            ws.on_upgrade(move |mut socket| async move {
                let mut nick = String::new();
                while let Some(Ok(WsMessage::Text(t))) = socket.recv().await {
                    let t = t.to_string();
                    if let Some(n) = t.strip_prefix("NICK ") {
                        nick = n.to_owned();
                    }
                    let joined = t.starts_with("JOIN ");
                    tx.send(t.clone()).unwrap();
                    if let Some(caps) = t.strip_prefix("CAP REQ ") {
                        let reply = format!(":tmi.twitch.tv CAP * {cap} {caps}\r\n");
                        socket.send(WsMessage::Text(reply.into())).await.unwrap();
                    }
                    if joined {
                        let channel = &t[5..];
                        let mut out =
                            format!(":{nick}!{nick}@{nick}.tmi.twitch.tv JOIN {channel}\r\n");
                        for l in &lines {
                            out.push_str(l);
                            out.push_str("\r\n");
                        }
                        socket.send(WsMessage::Text(out.into())).await.unwrap();
                    }
                }
                std::future::pending::<()>().await;
            })
        }
        let app = Router::new()
            .route("/", any(ws))
            .with_state((tx, cap, lines));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (url, rx)
    }

    async fn next(rx: &mut mpsc::UnboundedReceiver<String>) -> String {
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn joins_the_broadcasters_channel_as_a_guest_and_delivers_first_messages() {
        let (url, mut sent) = fake_irc(
            "ACK",
            vec![
                "PING :tmi.twitch.tv",
                "@first-msg=0;id=m1 :a!a@a.tmi.twitch.tv PRIVMSG #streamer :hello",
                "@first-msg=1;id=m2 :b!b@b.tmi.twitch.tv PRIVMSG #streamer :first!",
            ],
        )
        .await;
        let config = tls::client_config().unwrap();
        let http = tls::https_client(config.clone());
        let (ids_tx, mut ids) = mpsc::unbounded_channel();
        let (up_tx, mut up) = mpsc::unbounded_channel();
        let task = tokio::spawn(
            FirstChats {
                url,
                auth: signed_in_auth(http),
                tls: config,
                deliver: Arc::new(move |id| ids_tx.send(id).unwrap()),
                connected: Arc::new(move |c| up_tx.send(c).unwrap()),
            }
            .run(),
        );
        assert_eq!(
            next(&mut sent).await,
            "CAP REQ :twitch.tv/tags twitch.tv/commands"
        );
        assert!(next(&mut sent).await.starts_with("NICK justinfan"));
        assert_eq!(next(&mut sent).await, "JOIN #streamer");
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), up.recv())
                .await
                .unwrap(),
            Some(true)
        );
        assert_eq!(next(&mut sent).await, "PONG :tmi.twitch.tv");
        assert_eq!(next(&mut ids).await, "m2");
        task.abort();
    }

    #[tokio::test]
    async fn a_join_without_the_tags_granted_never_counts_as_connected() {
        let (url, mut sent) = fake_irc(
            "NAK",
            vec!["@first-msg=1;id=m1 :b!b@b.tmi.twitch.tv PRIVMSG #streamer :first!"],
        )
        .await;
        let config = tls::client_config().unwrap();
        let (up_tx, mut up) = mpsc::unbounded_channel();
        let task = tokio::spawn(
            FirstChats {
                url,
                auth: signed_in_auth(tls::https_client(config.clone())),
                tls: config,
                deliver: Arc::new(|_| {}),
                connected: Arc::new(move |c| up_tx.send(c).unwrap()),
            }
            .run(),
        );
        assert!(next(&mut sent).await.starts_with("CAP REQ "));
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), up.recv())
                .await
                .unwrap(),
            Some(false),
            "refused: the connection ends and the fallback stays on"
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(200), up.recv())
                .await
                .is_err(),
            "never reported connected"
        );
        task.abort();
    }

    #[test]
    fn capability_replies_are_read() {
        assert_eq!(
            capability_reply(":tmi.twitch.tv CAP * ACK :twitch.tv/tags twitch.tv/commands"),
            Some(true)
        );
        assert_eq!(
            capability_reply(":tmi.twitch.tv CAP * NAK :twitch.tv/tags"),
            Some(false)
        );
        assert_eq!(
            capability_reply(":a!a@a.tmi.twitch.tv PRIVMSG #s :CAP * ACK"),
            None
        );
    }
}
