//! `fake-services [--obs-port <port>] [--password <password>]`: runs the fake OBS on `127.0.0.1` until stopped.
//! Point the app's OBS settings at it; without `--password` it asks for no authentication.

use fakes::obs::FakeObs;
use tokio::net::TcpListener;

/// Not OBS's own 4455, so a real OBS on this machine keeps its port.
const DEFAULT_OBS_PORT: u16 = 4466;
const USAGE: &str = "usage: fake-services [--obs-port <port>] [--password <password>]";

struct Args {
    obs_port: u16,
    password: Option<String>,
}

fn parse(mut args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut parsed = Args {
        obs_port: DEFAULT_OBS_PORT,
        password: None,
    };
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--obs-port" => {
                parsed.obs_port = value()?
                    .parse()
                    .map_err(|_| "--obs-port needs a port number")?
            }
            "--password" => parsed.password = Some(value()?),
            _ => return Err(format!("unknown argument {arg}")),
        }
    }
    Ok(parsed)
}

#[tokio::main]
async fn main() {
    let args = parse(std::env::args().skip(1)).unwrap_or_else(|e| {
        eprintln!("{e}\n{USAGE}");
        std::process::exit(2)
    });
    let obs = TcpListener::bind(("127.0.0.1", args.obs_port))
        .await
        .unwrap_or_else(|e| {
            eprintln!("cannot listen on 127.0.0.1:{}: {e}", args.obs_port);
            std::process::exit(1)
        });
    println!("fake OBS on ws://127.0.0.1:{}", args.obs_port);
    if let Err(e) = FakeObs::new(args.password).serve(obs).await {
        eprintln!("fake OBS stopped: {e}");
        std::process::exit(1)
    }
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
        assert_eq!((a.obs_port, a.password), (DEFAULT_OBS_PORT, None));
        let a = args(&["--obs-port", "5000", "--password", "pw"]).unwrap();
        assert_eq!((a.obs_port, a.password.as_deref()), (5000, Some("pw")));
        assert!(args(&["--obs-port"]).is_err());
        assert!(args(&["--obs-port", "x"]).is_err());
        assert!(args(&["--nope"]).is_err());
    }
}
