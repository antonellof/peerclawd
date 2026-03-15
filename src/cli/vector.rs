//! Vector database CLI commands.

use clap::{Args, Subcommand};

use crate::vector::{
    get_embedder, init_embedder, EmbeddingConfig, EmbeddingProvider, VectorStore, VectorStoreConfig,
};

#[derive(Args)]
pub struct VectorArgs {
    #[command(subcommand)]
    pub cmd: VectorCommand,
}

#[derive(Subcommand)]
pub enum VectorCommand {
    /// Create a new vector collection
    Create {
        /// Collection name
        name: String,
        /// Vector dimension (default: 384)
        #[arg(short, long, default_value = "384")]
        dim: usize,
    },
    /// List all collections
    List,
    /// Show collection info
    Info {
        /// Collection name
        name: String,
    },
    /// Delete a collection
    Delete {
        /// Collection name
        name: String,
        /// Skip confirmation
        #[arg(short, long)]
        force: bool,
    },
    /// Add a vector to a collection
    Add {
        /// Collection name
        collection: String,
        /// Point ID
        id: String,
        /// Text content (will be embedded)
        text: String,
        /// Optional metadata as JSON
        #[arg(short, long)]
        metadata: Option<String>,
    },
    /// Search for similar vectors
    Search {
        /// Collection name
        collection: String,
        /// Search query text
        query: String,
        /// Number of results (default: 10)
        #[arg(short, long, default_value = "10")]
        limit: usize,
        /// Use hybrid search (vector + text)
        #[arg(long)]
        hybrid: bool,
    },
    /// Get a specific point by ID
    Get {
        /// Collection name
        collection: String,
        /// Point ID
        id: String,
    },
    /// Delete a point
    Remove {
        /// Collection name
        collection: String,
        /// Point ID
        id: String,
    },
    /// Generate embedding for text
    Embed {
        /// Text to embed
        text: String,
        /// Show full vector (default: summary only)
        #[arg(long)]
        full: bool,
    },
    /// Test embedding similarity
    Similarity {
        /// First text
        text1: String,
        /// Second text
        text2: String,
    },
}

pub async fn run(args: VectorArgs) -> anyhow::Result<()> {
    match args.cmd {
        VectorCommand::Create { name, dim } => {
            let store = VectorStore::new(VectorStoreConfig {
                embedding_dim: dim,
                ..Default::default()
            });

            match store.create_collection_with_dim(&name, dim) {
                Ok(()) => {
                    println!("\x1b[32m✓\x1b[0m Created collection '{}' (dim={})", name, dim);
                }
                Err(e) => {
                    println!("\x1b[31m✗\x1b[0m Failed to create collection: {}", e);
                }
            }
        }

        VectorCommand::List => {
            let store = VectorStore::new(VectorStoreConfig::default());
            let collections = store.list_collections();

            if collections.is_empty() {
                println!("No collections found.");
            } else {
                println!("\n\x1b[1mVector Collections\x1b[0m");
                println!("{}", "─".repeat(50));
                println!(
                    "{:<20} {:>10} {:>10}",
                    "NAME", "VECTORS", "DIMENSION"
                );
                println!("{}", "─".repeat(50));
                for col in collections {
                    println!(
                        "{:<20} {:>10} {:>10}",
                        col.name, col.count, col.dimension
                    );
                }
                println!("{}", "─".repeat(50));
            }
        }

        VectorCommand::Info { name } => {
            let store = VectorStore::new(VectorStoreConfig::default());
            let collections = store.list_collections();

            if let Some(col) = collections.iter().find(|c| c.name == name) {
                println!("\n\x1b[1mCollection: {}\x1b[0m", col.name);
                println!("{}", "─".repeat(30));
                println!("Vectors:    {}", col.count);
                println!("Dimension:  {}", col.dimension);
            } else {
                println!("\x1b[33m!\x1b[0m Collection '{}' not found", name);
            }
        }

        VectorCommand::Delete { name, force } => {
            if !force {
                print!(
                    "\x1b[33m!\x1b[0m Delete collection '{}'? [y/N] ",
                    name
                );
                use std::io::{self, Write};
                io::stdout().flush()?;

                let mut input = String::new();
                io::stdin().read_line(&mut input)?;

                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("Cancelled.");
                    return Ok(());
                }
            }

            let store = VectorStore::new(VectorStoreConfig::default());
            match store.delete_collection(&name) {
                Ok(true) => {
                    println!("\x1b[32m✓\x1b[0m Deleted collection '{}'", name);
                }
                Ok(false) => {
                    println!("\x1b[33m!\x1b[0m Collection '{}' not found", name);
                }
                Err(e) => {
                    println!("\x1b[31m✗\x1b[0m Failed to delete: {}", e);
                }
            }
        }

        VectorCommand::Add {
            collection,
            id,
            text,
            metadata,
        } => {
            let store = VectorStore::new(VectorStoreConfig::default());

            // Ensure collection exists
            if let Err(e) = store.get_or_create_collection(&collection) {
                println!("\x1b[31m✗\x1b[0m Failed to access collection: {}", e);
                return Ok(());
            }

            // Generate embedding
            let embedder = get_embedder();
            let embedding = embedder.embed(&text).await?;

            // Parse metadata
            let payload = if let Some(meta) = metadata {
                let mut payload: serde_json::Value = serde_json::from_str(&meta)?;
                if let Some(obj) = payload.as_object_mut() {
                    obj.insert("text".to_string(), serde_json::json!(text));
                }
                Some(payload)
            } else {
                Some(serde_json::json!({ "text": text }))
            };

            store.upsert(&collection, &id, embedding, payload)?;
            println!("\x1b[32m✓\x1b[0m Added '{}' to '{}'", id, collection);
        }

        VectorCommand::Search {
            collection,
            query,
            limit,
            hybrid,
        } => {
            let store = VectorStore::new(VectorStoreConfig::default());
            let embedder = get_embedder();

            // Generate query embedding
            let query_embedding = embedder.embed(&query).await?;

            let results = if hybrid {
                store.hybrid_search(&collection, query_embedding, &query, limit, 0.7)?
            } else {
                store.search(&collection, query_embedding, limit)?
            };

            if results.is_empty() {
                println!("No results found.");
            } else {
                println!(
                    "\n\x1b[1mSearch Results\x1b[0m (query: \"{}\", {})",
                    truncate(&query, 30),
                    if hybrid { "hybrid" } else { "vector" }
                );
                println!("{}", "─".repeat(70));

                for (i, result) in results.iter().enumerate() {
                    let score_color = if result.score > 0.7 {
                        "\x1b[32m"
                    } else if result.score > 0.4 {
                        "\x1b[33m"
                    } else {
                        "\x1b[31m"
                    };

                    println!(
                        "{}. [{}{:.3}\x1b[0m] {} - {}",
                        i + 1,
                        score_color,
                        result.score,
                        result.id,
                        truncate(&result.text.clone().unwrap_or_default(), 50)
                    );
                }
                println!("{}", "─".repeat(70));
            }
        }

        VectorCommand::Get { collection, id } => {
            let store = VectorStore::new(VectorStoreConfig::default());

            match store.get(&collection, &id)? {
                Some(result) => {
                    println!("\n\x1b[1mPoint: {}\x1b[0m", result.id);
                    println!("{}", "─".repeat(40));
                    if let Some(text) = &result.text {
                        println!("Text: {}", text);
                    }
                    if let Some(payload) = &result.payload {
                        println!("Payload: {}", serde_json::to_string_pretty(payload)?);
                    }
                }
                None => {
                    println!("\x1b[33m!\x1b[0m Point '{}' not found in '{}'", id, collection);
                }
            }
        }

        VectorCommand::Remove { collection, id } => {
            let store = VectorStore::new(VectorStoreConfig::default());

            match store.delete(&collection, &id) {
                Ok(true) => {
                    println!("\x1b[32m✓\x1b[0m Removed '{}' from '{}'", id, collection);
                }
                Ok(false) => {
                    println!("\x1b[33m!\x1b[0m Point '{}' not found", id);
                }
                Err(e) => {
                    println!("\x1b[31m✗\x1b[0m Failed to remove: {}", e);
                }
            }
        }

        VectorCommand::Embed { text, full } => {
            let embedder = get_embedder();
            let embedding = embedder.embed(&text).await?;

            println!("\n\x1b[1mEmbedding\x1b[0m (dim={})", embedding.len());
            println!("{}", "─".repeat(40));

            if full {
                // Print full vector
                println!("[");
                for (i, chunk) in embedding.chunks(8).enumerate() {
                    print!("  ");
                    for (j, val) in chunk.iter().enumerate() {
                        if i > 0 || j > 0 {
                            print!(", ");
                        }
                        print!("{:.6}", val);
                    }
                    println!();
                }
                println!("]");
            } else {
                // Print summary
                let min = embedding.iter().cloned().fold(f32::INFINITY, f32::min);
                let max = embedding.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
                let mean: f32 = embedding.iter().sum::<f32>() / embedding.len() as f32;
                let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();

                println!("Min:    {:.6}", min);
                println!("Max:    {:.6}", max);
                println!("Mean:   {:.6}", mean);
                println!("Norm:   {:.6}", norm);
                println!();
                println!(
                    "First 8: [{:.4}, {:.4}, {:.4}, {:.4}, {:.4}, {:.4}, {:.4}, {:.4}]",
                    embedding[0],
                    embedding[1],
                    embedding[2],
                    embedding[3],
                    embedding[4],
                    embedding[5],
                    embedding[6],
                    embedding[7]
                );
            }
        }

        VectorCommand::Similarity { text1, text2 } => {
            let embedder = get_embedder();

            let e1 = embedder.embed(&text1).await?;
            let e2 = embedder.embed(&text2).await?;
            let similarity = embedder.similarity(&e1, &e2);

            let color = if similarity > 0.7 {
                "\x1b[32m"
            } else if similarity > 0.4 {
                "\x1b[33m"
            } else {
                "\x1b[31m"
            };

            println!("\n\x1b[1mSimilarity\x1b[0m");
            println!("{}", "─".repeat(40));
            println!("Text 1: {}", truncate(&text1, 50));
            println!("Text 2: {}", truncate(&text2, 50));
            println!();
            println!("Score:  {}{:.4}\x1b[0m", color, similarity);

            let interpretation = if similarity > 0.8 {
                "Very similar"
            } else if similarity > 0.6 {
                "Similar"
            } else if similarity > 0.4 {
                "Somewhat related"
            } else if similarity > 0.2 {
                "Loosely related"
            } else {
                "Different"
            };
            println!("        ({})", interpretation);
        }
    }

    Ok(())
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}
