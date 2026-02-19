mod app;
mod config;
mod constants;
mod conversion;
mod errors;
mod handlers;
mod models;
mod state;
mod upstream;
mod upstream_parse;
mod utils;

#[tokio::main]
async fn main() {
    app::run().await;
}
