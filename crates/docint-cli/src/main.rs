use anyhow::Result;
use docint_core::{db, embeddings::Embedder, store::VectorStore};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<()> {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://docint:docint_local@localhost:5432/docint".into());

    let pool = db::create_pool(&database_url).await?;
    let store = VectorStore::new(pool);
    let embedder = Embedder::new().await;

    let doc_a: Uuid = "aaaaaaaa-0000-0000-0000-000000000001".parse()?;
    let doc_b: Uuid = "bbbbbbbb-0000-0000-0000-000000000002".parse()?;

    let rust_chunks = [
        "Rust uses ownership and borrowing to manage memory safely at compile time.",
        "Rust achieves concurrency safety through the Send and Sync traits.",
        "Error handling in Rust uses Result and Option types for explicit control flow.",
    ];
    let go_chunks = [
        "Go uses garbage collection for automatic memory management.",
        "Go achieves concurrency through goroutines and channels for message passing.",
        "Error handling in Go uses explicit error return values checked with if err != nil.",
    ];

    println!("--- Embedding Rust chunks ---");
    for (i, text) in rust_chunks.iter().enumerate() {
        let emb = embedder.embed(text).await?;
        store.insert_chunk(doc_a, text, i as i32, &emb).await?;
        println!("  Chunk {}: {}", i, text);
    }

    println!("--- Embedding Go chunks ---");
    for (i, text) in go_chunks.iter().enumerate() {
        let emb = embedder.embed(text).await?;
        store.insert_chunk(doc_b, text, i as i32, &emb).await?;
        println!("  Chunk {}: {}", i, text);
    }

    println!("\nDone. Doc A = {doc_a}, Doc B = {doc_b}");
    Ok(())
}
