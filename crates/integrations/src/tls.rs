//! The one TLS setup outgoing connections use: Twitch's HTTPS API and, later, EventSub's `wss://`.

use std::sync::Arc;

use bytes::Bytes;
use http_body_util::Full;
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::{
    client::legacy::{Client, connect::HttpConnector},
    rt::TokioExecutor,
};
pub use rustls::ClientConfig;
use rustls_platform_verifier::BuilderVerifierExt;

pub type HttpsClient = Client<HttpsConnector<HttpConnector>, Full<Bytes>>;

/// Certificates are checked against the Windows store, so a proxy or antivirus the user trusts there is trusted
/// here too; `ring` keeps the build free of the C toolchain `aws-lc-rs` would need.
pub fn client_config() -> Result<Arc<ClientConfig>, rustls::Error> {
    let config =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()?
            .with_platform_verifier()?
            .with_no_client_auth();
    Ok(Arc::new(config))
}

/// `http://` stays allowed only so tests can point an adapter at a fake service on 127.0.0.1.
pub fn https_client(config: Arc<ClientConfig>) -> HttpsClient {
    let connector = HttpsConnectorBuilder::new()
        .with_tls_config((*config).clone())
        .https_or_http()
        .enable_http1()
        .build();
    Client::builder(TokioExecutor::new()).build(connector)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{io::AsyncWriteExt, net::TcpListener};

    #[tokio::test]
    async fn https_refuses_a_server_without_a_trusted_handshake() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let _ = socket.write_all(b"HTTP/1.1 200 OK\r\n\r\nnot tls").await;
            }
        });
        let client = https_client(client_config().unwrap());
        let result = client
            .get(format!("https://{address}/").parse().unwrap())
            .await;
        assert!(result.is_err());
    }
}
