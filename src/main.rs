mod api;
mod fetch;
mod errors;
use api::{fetch_post, fetch_unblock};
use axum::{routing::post, Router};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    //let input = "Hello" ;
    //let output = "Livy";
    //mockattestation(input, output).await
    let fetcher =  Arc::new(fetch::Fetcher::new());
    let app = Router::new()
        .route("/fetch", post(fetch_post))
        .route("/fetchunblock",post(fetch_unblock)).with_state(fetcher)
        ;
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3001").await.unwrap();
    axum::serve(listener,app).await.unwrap();
}


