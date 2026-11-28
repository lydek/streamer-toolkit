//! `fake-services [--obs-port <port>] [--twitch-port <port>] [--password <password>] [--host-port <port>]`: runs the
//! fake OBS and the fake Twitch on `127.0.0.1` until stopped. `--host-port` is the app's Host port, which the fake
//! OBS's overlay browser sources point at (default 47811). Point the app's OBS settings at the OBS port, and a dev build at the
//! Twitch port with `TOOLKIT_FAKE_SERVICES=127.0.0.1:<port>`. Without `--password` the fake OBS asks for none.
//! `fake-services [--port <twitch port>] send|obs|eventsub|poll|prediction|scenario …` controls a running one (see `USAGE`).
//! `--ui` also opens the control panel (built with `--features ui`); closing it stops the fakes.

use fakes::{
    control::{self, SCENARIOS},
    events::KINDS,
    metrics::{self, Recorder},
    obs::{DEFAULT_HOST_PORT, FakeObs},
    twitch::FakeTwitch,
};
use tokio::net::TcpListener;

/// Not OBS's own 4455, so a real OBS on this machine keeps its port.
const DEFAULT_OBS_PORT: u16 = 4466;
const DEFAULT_TWITCH_PORT: u16 = 4467;
const USAGE: &str = "usage:
  fake-services [--obs-port <port>] [--twitch-port <port>] [--password <password>] [--host-port <port>] [--ui]
  fake-services [--port <twitch port>] send <kind> [--user <login>] [--count <n>] [--message <text>]
                [--reward <title>] [--bits <n>] [--tier 1000|2000|3000] [--months <n>] [--id <id>]
                [--status fulfilled|canceled] [--first]
  fake-services [--port <twitch port>] obs drop | obs stats [--cpu <%>] [--dropped <%>] [--kbps <n>]
                | obs stream on|off | obs record on|off
                | obs meters [--inputs <extra inputs>] [--interval <ms>] | obs burst scenes|items [--count <n>]
                | obs sources <0-1000>
  fake-services [--port <twitch port>] eventsub drop
  fake-services [--port <twitch port>] poll vote --choice <n> [--count <n>] | poll end
  fake-services [--port <twitch port>] prediction bet --outcome <n> [--points <n>] [--users <n>] | prediction lock
  fake-services [--port <twitch port>] scenario <name>
  fake-services [--port <twitch port>] stats [--reset] [--json]
  fake-services [--port <twitch port>] fault <call key> [--status <code>] [--delay <time>] [--for <time>] [--close]
                | fault --clear     (keys: 'GET /helix/users', an OBS request type, 'ws-connect eventsub|irc|obs')";

/// Sends one control command to a running fake-services and exits.
fn control_command(mut args: Vec<String>) -> ! {
    let mut port = DEFAULT_TWITCH_PORT;
    if let Some(i) = args.iter().position(|a| a == "--port") {
        let value = args.get(i + 1).and_then(|p| p.parse().ok());
        let Some(value) = value else {
            eprintln!("--port needs a port number\n{USAGE}");
            std::process::exit(2)
        };
        port = value;
        args.drain(i..=i + 1);
    }
    let (path, body) = control::command(&args).unwrap_or_else(|e| {
        let kinds: Vec<&str> = KINDS.iter().map(|(k, _)| *k).collect();
        let scenarios: Vec<String> = SCENARIOS
            .iter()
            .map(|(n, what)| format!("  {n}: {what}"))
            .collect();
        eprintln!(
            "{e}\n{USAGE}\nkinds: {}\nscenarios:\n{}",
            kinds.join(", "),
            scenarios.join("\n")
        );
        std::process::exit(2)
    });
    match control::call(port, path, &body) {
        Ok(answer) => {
            let report = (path == "/_control/stats" && body["json"] != true)
                .then(|| serde_json::from_str(&answer).ok())
                .flatten();
            match report {
                Some(report) => print!("{}", metrics::text(&report)),
                None => println!("{answer}"),
            }
            std::process::exit(0)
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1)
        }
    }
}

struct Args {
    obs_port: u16,
    twitch_port: u16,
    password: Option<String>,
    host_port: u16,
    ui: bool,
}

fn parse(mut args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut parsed = Args {
        obs_port: DEFAULT_OBS_PORT,
        twitch_port: DEFAULT_TWITCH_PORT,
        password: None,
        host_port: DEFAULT_HOST_PORT,
        ui: false,
    };
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} needs a value"));
        let port = |v: String| v.parse().map_err(|_| format!("{arg} needs a port number"));
        match arg.as_str() {
            "--obs-port" => parsed.obs_port = port(value()?)?,
            "--twitch-port" => parsed.twitch_port = port(value()?)?,
            "--password" => parsed.password = Some(value()?),
            "--host-port" => parsed.host_port = port(value()?)?,
            "--ui" => parsed.ui = true,
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

type Stopped = (&'static str, std::io::Result<()>);

fn stopped((which, result): Stopped) -> ! {
    if let Err(e) = result {
        eprintln!("fake {which} stopped: {e}");
    }
    std::process::exit(1)
}

/// The fakes on their runtime's threads, the panel on this one (windows need the main thread).
#[cfg(feature = "ui")]
fn open_panel(
    runtime: tokio::runtime::Runtime,
    serve: impl Future<Output = Stopped> + Send + 'static,
    obs: FakeObs,
    twitch: FakeTwitch,
    ports: (u16, u16),
) -> ! {
    runtime.spawn(async move { stopped(serve.await) });
    if let Err(e) = fakes::panel::run(obs, twitch, ports) {
        eprintln!("the panel failed: {e}");
        std::process::exit(1)
    }
    // Closing the panel stops the fakes too: nothing is left running in the background.
    std::process::exit(0)
}

#[cfg(not(feature = "ui"))]
fn open_panel(
    _: tokio::runtime::Runtime,
    _: impl Future<Output = Stopped>,
    _: FakeObs,
    _: FakeTwitch,
    _: (u16, u16),
) -> ! {
    eprintln!("--ui needs the panel built in: cargo run -p fakes --features ui -- --ui");
    std::process::exit(2)
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let is_command = raw
        .iter()
        .find(|a| !a.starts_with("--") && a.parse::<u16>().is_err())
        .is_some_and(|a| {
            [
                "send",
                "obs",
                "eventsub",
                "scenario",
                "stats",
                "fault",
                "poll",
                "prediction",
            ]
            .contains(&a.as_str())
        });
    if is_command {
        control_command(raw);
    }
    let args = parse(raw.into_iter()).unwrap_or_else(|e| {
        eprintln!("{e}\n{USAGE}");
        std::process::exit(2)
    });
    let runtime = tokio::runtime::Runtime::new().expect("a tokio runtime");
    let (obs, twitch) =
        runtime.block_on(async { (listen(args.obs_port).await, listen(args.twitch_port).await) });
    println!("fake OBS on ws://127.0.0.1:{}", args.obs_port);
    println!(
        "fake Twitch on http://127.0.0.1:{0} (TOOLKIT_FAKE_SERVICES=127.0.0.1:{0})",
        args.twitch_port
    );
    // One timeline for both fakes, so `stats` sees OBS and Twitch calls together.
    let recorder = Recorder::default();
    let fake_obs = FakeObs::for_host(args.password, recorder.clone(), args.host_port);
    let fake_twitch = FakeTwitch::with_recorder(std::time::Duration::from_secs(10), recorder);
    let app = fake_twitch
        .router()
        .merge(control::router(fake_obs.clone(), fake_twitch.clone()));
    let serve = {
        let fake_obs = fake_obs.clone();
        async move {
            tokio::select! {
                r = fake_obs.serve(obs) => ("OBS", r),
                r = axum::serve(twitch, app).into_future() => ("Twitch", r),
            }
        }
    };
    if args.ui {
        open_panel(
            runtime,
            serve,
            fake_obs,
            fake_twitch,
            (args.obs_port, args.twitch_port),
        );
    }
    stopped(runtime.block_on(serve))
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
            (a.obs_port, a.twitch_port, a.password, a.host_port, a.ui),
            (
                DEFAULT_OBS_PORT,
                DEFAULT_TWITCH_PORT,
                None,
                DEFAULT_HOST_PORT,
                false
            )
        );
        assert_eq!(args(&["--host-port", "47812"]).unwrap().host_port, 47812);
        assert!(args(&["--host-port", "x"]).is_err());
        assert!(args(&["--ui"]).unwrap().ui);
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
