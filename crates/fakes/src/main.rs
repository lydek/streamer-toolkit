//! `fake-services [--obs-port <port>] [--twitch-port <port>] [--password <password>]`: runs the fake OBS and the
//! fake Twitch on `127.0.0.1` until stopped. Point the app's OBS settings at the OBS port, and a dev build at the
//! Twitch port with `TOOLKIT_FAKE_SERVICES=127.0.0.1:<port>`. Without `--password` the fake OBS asks for none.

use fakes::{obs::FakeObs, twitch::FakeTwitch};
use tokio::net::TcpListener;

/// Not OBS's own 4455, so a real OBS on this machine keeps its port.
const DEFAULT_OBS_PORT: u16 = 4466;
const DEFAULT_TWITCH_PORT: u16 = 4467;
const USAGE: &str =
    "usage: fake-services [--obs-port <port>] [--twitch-port <port>] [--password <password>]";

struct Args {
    obs_port: u16,
    twitch_port: u16,
    password: Option<String>,
}

fn parse(mut args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut parsed = Args {
        obs_port: DEFAULT_OBS_PORT,
        twitch_port: DEFAULT_TWITCH_PORT,
        password: None,
    };
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} needs a value"));
        let port = |v: String| v.parse().map_err(|_| format!("{arg} needs a port number"));
        match arg.as_str() {
            "--obs-port" => parsed.obs_port = port(value()?)?,
            "--twitch-port" => parsed.twitch_port = port(value()?)?,
            "--password" => parsed.password = Some(value()?),
            _ => return Err(format!("unknown argument {arg}")),
        }
    }
    Ok(parsed)
}

async fn listen(port: u16) -> TcpListener {
    TcpListener::bind(("127.0.0.1", port))
        .await
        .unwrap_or_else(|e| {
            eprintln!("cannot listen on 127.0.0.1:{port}: {e}");
            std::process::exit(1)
        })
}

#[tokio::main]
async fn main() {
    let args = parse(std::env::args().skip(1)).unwrap_or_else(|e| {
        eprintln!("{e}\n{USAGE}");
        std::process::exit(2)
    });
    let obs = listen(args.obs_port).await;
    let twitch = listen(args.twitch_port).await;
    println!("fake OBS on ws://127.0.0.1:{}", args.obs_port);
    println!(
        "fake Twitch on http://127.0.0.1:{0} (TOOLKIT_FAKE_SERVICES=127.0.0.1:{0})",
        args.twitch_port
    );
    let stopped = tokio::select! {
        r = FakeObs::new(args.password).serve(obs) => ("OBS", r),
        r = FakeTwitch::default().serve(twitch) => ("Twitch", r),
    };
    if let (which, Err(e)) = stopped {
        eprintln!("fake {which} stopped: {e}");
    }
    std::process::exit(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Result<Args, String> {
        parse(list.iter().map(|s| s.to_string()))
    }

    #[test]
    fn arguments_have_defaults_and_reject_unknown_ones() {
        let a = args(&[]).unwrap();
        assert_eq!(
            (a.obs_port, a.twitch_port, a.password),
            (DEFAULT_OBS_PORT, DEFAULT_TWITCH_PORT, None)
        );
        let a = args(&[
            "--obs-port",
            "5000",
            "--twitch-port",
            "5001",
            "--password",
            "pw",
        ])
        .unwrap();
        assert_eq!(
            (a.obs_port, a.twitch_port, a.password.as_deref()),
            (5000, 5001, Some("pw"))
        );
        assert!(args(&["--obs-port"]).is_err());
        assert!(args(&["--twitch-port", "x"]).is_err());
        assert!(args(&["--nope"]).is_err());
    }
}
