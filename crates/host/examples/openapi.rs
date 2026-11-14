//! Writes the Host's OpenAPI document to the given path; run through `pnpm api:gen`.

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: openapi <output.json>");
    let json = host::openapi()
        .to_pretty_json()
        .expect("OpenAPI serializes");
    std::fs::write(path, json + "\n").expect("write OpenAPI");
}
