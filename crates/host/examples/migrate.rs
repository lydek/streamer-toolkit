//! Applies the merged migrations to `<folder>/toolkit.db`, backing it up first like a real start. Gives SQLx
//! offline data (`.sqlx/`) a database to read: `cargo run -p host --example migrate -- <folder>`.

#[tokio::main]
async fn main() {
    let dir: std::path::PathBuf = std::env::args()
        .nth(1)
        .expect("usage: migrate <folder>")
        .into();
    std::fs::create_dir_all(&dir).expect("create the folder");
    host::db::open(&dir).await.expect("migrate").close().await;
    println!("migrated {}", dir.join(host::db::DB_FILE).display());
}
