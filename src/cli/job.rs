//! `peerclawd job` command - Submit and manage distributed jobs.

use clap::{Args, Subcommand};
use std::sync::Arc;
use std::time::Duration;

use crate::bootstrap;
use crate::config::Config;
use crate::db::Database;
use crate::executor::task::{ExecutionTask, InferenceTask, TaskData, WebFetchTask};
use crate::identity::NodeIdentity;
use crate::job::{JobRequest, ResourceType};
use crate::runtime::Runtime;
use crate::wallet::{from_micro, to_micro};

#[derive(Args)]
pub struct JobArgs {
    #[command(subcommand)]
    pub cmd: JobCommand,
}

#[derive(Subcommand)]
pub enum JobCommand {
    /// Submit an inference job to the network
    Inference {
        /// Model to use
        #[arg(long)]
        model: String,

        /// Prompt to send
        #[arg(long)]
        prompt: String,

        /// Maximum tokens to generate
        #[arg(long, default_value = "100")]
        max_tokens: u32,

        /// Maximum budget in PCLAW
        #[arg(long, default_value = "10.0")]
        budget: f64,

        /// Timeout in seconds
        #[arg(long, default_value = "300")]
        timeout: u64,
    },

    /// Submit a web fetch job to the network
    Fetch {
        /// URL to fetch
        #[arg(long)]
        url: String,

        /// Maximum budget in PCLAW
        #[arg(long, default_value = "1.0")]
        budget: f64,

        /// Timeout in seconds
        #[arg(long, default_value = "60")]
        timeout: u64,
    },

    /// List active jobs
    List,

    /// Show job history
    History {
        /// Number of jobs to show
        #[arg(long, default_value = "10")]
        limit: usize,
    },

    /// Cancel a pending job
    Cancel {
        /// Job ID to cancel
        job_id: String,
    },
}

pub async fn run(args: JobArgs) -> anyhow::Result<()> {
    match args.cmd {
        JobCommand::Inference {
            model,
            prompt,
            max_tokens,
            budget,
            timeout,
        } => {
            run_inference_job(&model, &prompt, max_tokens, budget, timeout).await
        }
        JobCommand::Fetch { url, budget, timeout } => {
            run_fetch_job(&url, budget, timeout).await
        }
        JobCommand::List => {
            list_jobs().await
        }
        JobCommand::History { limit } => {
            show_history(limit).await
        }
        JobCommand::Cancel { job_id } => {
            cancel_job(&job_id).await
        }
    }
}

async fn create_runtime() -> anyhow::Result<Runtime> {
    bootstrap::ensure_dirs()?;

    let identity_path = bootstrap::identity_path();
    let identity = if identity_path.exists() {
        Arc::new(NodeIdentity::load(&identity_path)?)
    } else {
        let id = NodeIdentity::generate();
        id.save(&identity_path)?;
        Arc::new(id)
    };

    let config = Config::load()?;
    let db = Database::open(&config.database.path)?;

    Runtime::new(identity, db, config).await
}

async fn run_inference_job(
    model: &str,
    prompt: &str,
    max_tokens: u32,
    budget: f64,
    timeout: u64,
) -> anyhow::Result<()> {
    println!("=== Submit Inference Job ===");
    println!("Model: {}", model);
    println!("Prompt: {}...", &prompt[..prompt.len().min(50)]);
    println!("Max tokens: {}", max_tokens);
    println!("Budget: {:.6} PCLAW", budget);
    println!("Timeout: {}s", timeout);
    println!();

    let runtime = create_runtime().await?;

    // Subscribe to job topics
    runtime.subscribe_to_job_topics().await?;

    // Start network
    {
        let mut network = runtime.network.write().await;
        network.start().await?;
    }

    // Wait for peer connections
    println!("Waiting for peer connections...");
    tokio::time::sleep(Duration::from_secs(3)).await;

    let peers = runtime.connected_peers_count().await;
    println!("Connected to {} peers", peers);

    if peers == 0 {
        println!("\nNo peers connected. Running locally instead.");
    }

    // Create and broadcast job request
    let request = JobRequest::new(
        ResourceType::Inference {
            model: model.to_string(),
            tokens: max_tokens,
        },
        to_micro(budget),
        timeout,
    );

    let job_id = request.id.clone();
    println!("\nJob ID: {}", job_id);

    // Store the request
    runtime.job_manager.write().await.create_request(request.clone()).await?;

    // Broadcast to network
    let msg = crate::job::network::JobMessage::Request(
        crate::job::network::JobRequestMessage::new(request, runtime.identity.peer_id())
    );
    let data = crate::job::network::serialize_message(&msg)?;

    {
        let mut network = runtime.network.write().await;
        network.publish(crate::job::network::topics::JOB_REQUESTS, data)?;
    }

    println!("Job broadcast to network. Waiting for bids...");

    // Wait for bids (with timeout)
    let bid_timeout = Duration::from_secs(10);
    tokio::time::sleep(bid_timeout).await;

    // Check for bids
    let bids = runtime.job_manager.read().await.get_bids(&job_id).await;
    println!("\nReceived {} bids", bids.len());

    if bids.is_empty() {
        println!("No bids received. Executing locally...");

        // Execute locally
        let task = InferenceTask::new(model, prompt).with_max_tokens(max_tokens);
        let result = runtime.execute_task(ExecutionTask::Inference(task)).await?;

        println!("\nResult:");
        match &result.data {
            TaskData::Inference(r) => {
                println!("  Text: {}", r.text);
                println!("  Tokens: {}", r.tokens_generated);
            }
            TaskData::Error(e) => println!("  Error: {}", e),
            _ => println!("  Unexpected result"),
        }
    } else {
        // Display bids
        println!("\nBids received:");
        for (i, bid) in bids.iter().enumerate() {
            println!(
                "  {}. Peer: {}... | Price: {:.6} PCLAW | Latency: {}ms",
                i + 1,
                &bid.bidder_id[..16.min(bid.bidder_id.len())],
                from_micro(bid.price),
                bid.estimated_latency_ms
            );
        }

        // Select best bid
        let best_bid = bids.iter()
            .min_by_key(|b| b.price)
            .unwrap();

        println!("\nAccepting best bid from {}...", &best_bid.bidder_id[..16.min(best_bid.bidder_id.len())]);

        // Accept the bid
        match runtime.job_manager.write().await.accept_bid(&job_id, &best_bid.id).await {
            Ok(job) => {
                println!("Job accepted! Escrow: {}", job.escrow_id);
                println!("Waiting for result...");

                // In a real implementation, we'd wait for the result via gossip
                tokio::time::sleep(Duration::from_secs(5)).await;

                if let Some(job) = runtime.job_manager.read().await.get_job(&job_id).await {
                    if let Some(result) = &job.result {
                        println!("\nResult received!");
                        println!("  Data: {} bytes", result.data.len());
                        if let Ok(text) = String::from_utf8(result.data.clone()) {
                            println!("  Text: {}", text);
                        }
                    } else {
                        println!("  No result yet (job still in progress)");
                    }
                }
            }
            Err(e) => {
                println!("Failed to accept bid: {}", e);
            }
        }
    }

    Ok(())
}

async fn run_fetch_job(url: &str, budget: f64, timeout: u64) -> anyhow::Result<()> {
    println!("=== Submit Web Fetch Job ===");
    println!("URL: {}", url);
    println!("Budget: {:.6} PCLAW", budget);
    println!("Timeout: {}s", timeout);
    println!();

    let runtime = create_runtime().await?;

    // For simplicity, just execute locally
    println!("Executing web fetch...");

    let task = WebFetchTask::get(url);
    let result = runtime.execute_task(ExecutionTask::WebFetch(task)).await?;

    println!("\nResult:");
    match &result.data {
        TaskData::WebFetch(r) => {
            println!("  Status: {}", r.status);
            println!("  Headers: {} entries", r.headers.len());
            println!("  Body: {} bytes", r.body.len());
            if r.body.len() < 500 {
                if let Ok(body) = String::from_utf8(r.body.clone()) {
                    println!("  Content: {}", body);
                }
            }
        }
        TaskData::Error(e) => println!("  Error: {}", e),
        _ => println!("  Unexpected result"),
    }

    Ok(())
}

async fn list_jobs() -> anyhow::Result<()> {
    println!("=== Active Jobs ===\n");

    let runtime = create_runtime().await?;
    let jobs = runtime.job_manager.read().await.active_jobs().await;

    if jobs.is_empty() {
        println!("No active jobs.");
    } else {
        for job in &jobs {
            println!(
                "Job: {} | Status: {} | Provider: {}... | Price: {:.6} PCLAW",
                job.id,
                job.status,
                &job.bid.bidder_id[..16.min(job.bid.bidder_id.len())],
                from_micro(job.bid.price)
            );
        }
    }

    Ok(())
}

async fn show_history(limit: usize) -> anyhow::Result<()> {
    println!("=== Job History (last {}) ===\n", limit);

    let runtime = create_runtime().await?;
    let jobs = runtime.job_manager.read().await.completed_jobs(limit).await;

    if jobs.is_empty() {
        println!("No completed jobs.");
    } else {
        for job in &jobs {
            let duration = job.execution_duration()
                .map(|d| format!("{}s", d.num_seconds()))
                .unwrap_or_else(|| "N/A".to_string());

            println!(
                "Job: {} | Status: {} | Duration: {} | Price: {:.6} PCLAW",
                job.id,
                job.status,
                duration,
                from_micro(job.bid.price)
            );
        }
    }

    Ok(())
}

async fn cancel_job(job_id: &str) -> anyhow::Result<()> {
    println!("Cancelling job: {}", job_id);

    let runtime = create_runtime().await?;
    let job_id = crate::job::JobId(job_id.to_string());

    // Try to settle with failure (refund)
    match runtime.job_manager.write().await.settle_job(&job_id, false).await {
        Ok(()) => {
            println!("Job cancelled and refunded.");
        }
        Err(e) => {
            println!("Failed to cancel job: {}", e);
        }
    }

    Ok(())
}
