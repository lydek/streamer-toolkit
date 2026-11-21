use super::*;
use axum::body::Body;
use std::io::Write;
use tower::ServiceExt;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const HTML: &str = "<link rel=\"stylesheet\" href=\"style.css\">\n<div data-card-stage></div>\n<template data-card><div class=\"card\"><b data-field=\"title\"></b></div></template>\n";
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n rest";

fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, bytes) in files {
        out.start_file(*name, opts).unwrap();
        out.write_all(bytes).unwrap();
    }
    out.finish().unwrap().into_inner()
}

fn good() -> Vec<(&'static str, &'static [u8])> {
    vec![
        ("pack.json", r#"{"name": "星星"}"#.as_bytes()),
        ("index.html", HTML.as_bytes()),
        ("style.css", b".card { color: gold; }"),
        ("img/star.png", PNG),
    ]
}

fn refused_with(files: &[(&str, &[u8])]) -> PackRefused {
    read_pack(&zip_of(files)).unwrap_err()
}

#[test]
fn a_good_pack_is_read_with_its_name_and_files() {
    let (name, files) = read_pack(&zip_of(&good())).unwrap();
    assert_eq!(name, "星星");
    assert_eq!(files.len(), 4);
}

#[test]
fn scripts_are_refused_naming_the_file_or_the_place() {
    let mut files = good();
    files.push(("fx/run.js", b"alert(1)"));
    assert_eq!(
        refused_with(&files),
        refused("card_pack.script", "fx/run.js")
    );
    let mut files = good();
    files.push(("a.MJS", b"export {}"));
    assert_eq!(refused_with(&files).detail, "a.MJS");

    for (html, at) in [
        ("<div>\n  <SCRIPT>alert(1)</SCRIPT>", "index.html:2:3"),
        ("<img src=x onerror = alert(1)>", "index.html:1:12"),
        ("<a href=\"javascript:alert(1)\">x</a>", "index.html:1:10"),
    ] {
        let mut files = good();
        files[1] = ("index.html", html.as_bytes());
        assert_eq!(
            refused_with(&files),
            refused("card_pack.script", at),
            "{html}"
        );
    }
}

#[test]
fn types_and_magic_bytes_are_checked_and_svg_is_refused() {
    for (name, bytes) in [
        ("x.svg", b"<svg/>".as_slice()),
        ("x.png", b"GIF89a".as_slice()),
        ("x.exe", b"MZ".as_slice()),
    ] {
        let mut files = good();
        files.push((name, bytes));
        let r = refused_with(&files);
        assert_eq!(r.code, "card_pack.invalid", "{name}");
        assert!(r.detail.starts_with(name), "{name}: {}", r.detail);
    }
    let mut files = good();
    files.retain(|(n, _)| *n != "pack.json");
    assert_eq!(refused_with(&files).detail, "pack.json is missing");
    let mut files = good();
    files[0] = ("pack.json", br#"{"title": "x"}"#);
    assert_eq!(refused_with(&files).code, "card_pack.invalid");
}

#[test]
fn paths_never_leave_the_pack() {
    for name in [
        "../evil.css",
        "/abs.css",
        "a/../../b.css",
        "c:/x.css",
        "a\\b.css",
    ] {
        let mut files = good();
        files.push((name, b"x"));
        let r = refused_with(&files);
        assert_eq!(r.code, "card_pack.invalid", "{name}");
    }
}

#[test]
fn a_zip_bomb_stops_at_the_unpacked_limit() {
    let zeros = vec![0u8; (MAX_UNPACKED_BYTES + 1) as usize];
    let mut files = good();
    files.push(("big.css", &zeros));
    let zip = zip_of(&files);
    assert!(zip.len() < MAX_ZIP_BYTES, "deflate makes it small");
    assert_eq!(read_pack(&zip).unwrap_err().code, "card_pack.too_large");
}

#[tokio::test]
async fn installed_packs_are_listed_served_and_removed() {
    let dir = std::env::temp_dir().join(format!("st-card-packs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let app = routes(dir.clone()).merge(file_routes(dir.clone()));
    let send = |req: axum::http::Request<Body>| {
        let app = app.clone();
        async move {
            let res = app.oneshot(req).await.unwrap();
            let status = res.status();
            let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap();
            (status, bytes)
        }
    };
    let upload = |zip: Vec<u8>| {
        axum::http::Request::post("/api/admin/checkin/packs")
            .body(Body::from(zip))
            .unwrap()
    };

    let (status, body) = send(upload(zip_of(&good()))).await;
    assert_eq!(status, StatusCode::OK);
    let pack: CardPack = serde_json::from_slice(&body).unwrap();
    assert_eq!(pack.name, "星星");
    assert_eq!(
        list(&dir),
        vec![CardPack {
            id: pack.id.clone(),
            name: "星星".into()
        }]
    );

    let get = |p: String| axum::http::Request::get(p).body(Body::empty()).unwrap();
    let (status, body) = send(get(format!("/templates/checkin/{}/img/star.png", pack.id))).await;
    assert_eq!((status, body.as_ref()), (StatusCode::OK, PNG));
    let (status, _) = send(get(format!(
        "/templates/checkin/{}/../../settings.json",
        pack.id
    )))
    .await;
    assert_ne!(status, StatusCode::OK);

    let mut bad = good();
    bad.push(("x.js", b"1"));
    let (status, body) = send(upload(zip_of(&bad))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()["detail"],
        "x.js"
    );
    assert_eq!(list(&dir).len(), 1, "a refused pack writes nothing");

    let del = |id: &str| {
        axum::http::Request::delete(format!("/api/admin/checkin/packs/{id}"))
            .body(Body::empty())
            .unwrap()
    };
    assert_eq!(send(del(&pack.id)).await.0, StatusCode::NO_CONTENT);
    assert!(list(&dir).is_empty());
    assert_eq!(send(del(&pack.id)).await.0, StatusCode::NOT_FOUND);
    let _ = std::fs::remove_dir_all(dir);
}
