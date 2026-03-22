//! `peerclaw test` command - Test distributed execution and cluster operations.

use clap::{Args, Subcommand};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use crate::bootstrap;
use crate::config::Config;
use crate::db::Database;
use crate::executor::task::TaskData;
use crate::identity::NodeIdentity;
use crate::runtime::Runtime;
use crate::wallet::from_micro;

#[derive(Args)]
pub struct TestArgs {
    #[command(subcommand)]
    pub cmd: TestCommand,
}

#[derive(Subcommand)]
pub enum TestCommand {
    /// Run a local inference test
    Inference {
        /// Model to use
        #[arg(long, default_value = "llama-3.2-3b")]
        model: String,

        /// Prompt to send
        #[arg(long, default_value = "Hello, world! Please respond briefly.")]
        prompt: String,

        /// Maximum tokens
        #[arg(long, default_value = "100")]
        max_tokens: u32,
    },

    /// Run a web fetch test
    Fetch {
        /// URL to fetch
        #[arg(long, default_value = "https://httpbin.org/get")]
        url: String,
    },

    /// Run all tests in sequence
    All,

    /// Show runtime status
    Status,

    /// Run a multi-agent distributed test (spawns multiple nodes)
    Distributed {
        /// Number of agents to spawn
        #[arg(long, default_value = "3")]
        agents: u32,

        /// Duration to run in seconds
        #[arg(long, default_value = "30")]
        duration: u64,
    },

    /// Spawn a test cluster with multiple peer nodes
    Cluster {
        /// Number of nodes to spawn
        #[arg(long, default_value = "3")]
        nodes: u32,

        /// Base port for web UI (nodes get port, port+1, port+2, etc.)
        #[arg(long, default_value = "8080")]
        base_web_port: u16,

        /// Base port for P2P (nodes get p2p_port, p2p_port+1, etc.)
        #[arg(long, default_value = "9000")]
        base_p2p_port: u16,

        /// Run a test inference job after cluster is ready
        #[arg(long)]
        run_test_job: bool,

        /// Keep cluster running (wait for Ctrl+C)
        #[arg(long)]
        keep_alive: bool,
    },
}

pub async fn run(args: TestArgs) -> anyhow::Result<()> {
    match args.cmd {
        TestCommand::Inference { model, prompt, max_tokens } => {
            run_inference_test(&model, &prompt, max_tokens).await
        }
        TestCommand::Fetch { url } => {
            run_fetch_test(&url).await
        }
        TestCommand::All => {
            run_all_tests().await
        }
        TestCommand::Status => {
            show_status().await
        }
        TestCommand::Distributed { agents, duration } => {
            run_distributed_test(agents, duration).await
        }
        TestCommand::Cluster { nodes, base_web_port, base_p2p_port, run_test_job, keep_alive } => {
            run_cluster(nodes, base_web_port, base_p2p_port, run_test_job, keep_alive).await
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

async fn run_inference_test(model: &str, prompt: &str, max_tokens: u32) -> anyhow::Result<()> {
    println!("=== Inference Test ===");
    println!("Model: {}", model);
    println!("Prompt: {}", prompt);
    println!("Max tokens: {}", max_tokens);
    println!();

    let runtime = create_runtime().await?;

    let start = std::time::Instant::now();
    let result = runtime.inference(prompt, model, max_tokens).await?;
    let elapsed = start.elapsed();

    println!("Result:");
    match &result.data {
        TaskData::Inference(inference_result) => {
            println!("  Text: {}", inference_result.text);
            println!("  Tokens generated: {}", inference_result.tokens_generated);
            println!("  Tokens/sec: {:.2}", inference_result.tokens_per_second);
        }
        TaskData::Error(e) => {
            println!("  Error: {}", e);
        }
        _ => {
            println!("  Unexpected result type");
        }
    }
    println!("  Location: {:?}", result.location);
    println!("  Time: {:?}", elapsed);
    if let Some(cost) = result.cost {
        println!("  Cost: {:.6} PCLAW", from_micro(cost));
    }

    Ok(())
}

async fn run_fetch_test(url: &str) -> anyhow::Result<()> {
    println!("=== Web Fetch Test ===");
    println!("URL: {}", url);
    println!();

    let runtime = create_runtime().await?;

    let start = std::time::Instant::now();
    let result = runtime.web_fetch(url).await?;
    let elapsed = start.elapsed();

    println!("Result:");
    match &result.data {
        TaskData::WebFetch(fetch_result) => {
            println!("  Status: {}", fetch_result.status);
            println!("  Headers: {} entries", fetch_result.headers.len());
            println!("  Body size: {} bytes", fetch_result.body.len());
            if fetch_result.body.len() < 500 {
                if let Ok(body) = String::from_utf8(fetch_result.body.clone()) {
                    println!("  Body: {}", body);
                }
            }
        }
        TaskData::Error(e) => {
            println!("  Error: {}", e);
        }
        _ => {
            println!("  Unexpected result type");
        }
    }
    println!("  Location: {:?}", result.location);
    println!("  Time: {:?}", elapsed);

    Ok(())
}

async fn run_all_tests() -> anyhow::Result<()> {
    println!("=== Running All Tests ===\n");

    // Inference test
    run_inference_test("llama-3.2-3b", "Hello! Respond with one word.", 50).await?;
    println!();

    // Web fetch test
    run_fetch_test("https://httpbin.org/get").await?;
    println!();

    // Status
    show_status().await?;

    println!("\n=== All Tests Complete ===");
    Ok(())
}

async fn show_status() -> anyhow::Result<()> {
    println!("=== Runtime Status ===");

    let runtime = create_runtime().await?;
    let stats = runtime.stats().await;

    println!("Peer ID: {}", stats.peer_id);
    println!("Connected peers: {}", stats.connected_peers);
    println!("Balance: {:.6} PCLAW", stats.balance);
    println!("Active jobs: {}", stats.active_jobs);
    println!("Completed jobs: {}", stats.completed_jobs);
    println!();
    println!("Resource State:");
    println!("  CPU usage: {:.1}%", stats.resource_state.cpu_usage * 100.0);
    println!("  RAM: {}/{} MB available",
             stats.resource_state.ram_available_mb,
             stats.resource_state.ram_total_mb);
    println!("  Active inference tasks: {}", stats.resource_state.active_inference_tasks);
    println!("  Active web tasks: {}", stats.resource_state.active_web_tasks);
    println!("  Active WASM tasks: {}", stats.resource_state.active_wasm_tasks);
    println!("  Loaded models: {:?}", stats.resource_state.loaded_models);

    Ok(())
}

async fn run_distributed_test(agent_count: u32, duration_secs: u64) -> anyhow::Result<()> {
    println!("=== Distributed Execution Test ===");
    println!("Testing with {} simulated agents for {} seconds", agent_count, duration_secs);
    println!();

    // Create temporary directory for this test run
    let temp_base = std::env::temp_dir().join(format!("peerclaw_test_{}", std::process::id()));
    std::fs::create_dir_all(&temp_base)?;

    // Run agents sequentially to avoid thread-safety issues with libp2p
    // In production, each agent would be a separate process
    let mut results = vec![];

    for i in 0..agent_count {
        let agent_dir = temp_base.join(format!("agent_{}", i));
        std::fs::create_dir_all(&agent_dir)?;

        println!("Running agent {}...", i);

        match run_agent(i, agent_dir, duration_secs / agent_count as u64).await {
            Ok(stats) => {
                results.push((i as usize, stats));
            }
            Err(e) => {
                println!("Agent {} error: {}", i, e);
            }
        }
    }

    // Print summary
    println!("\n=== Results Summary ===");
    for (i, stats) in &results {
        println!("Agent {}:", i);
        println!("  Peer ID: {}...", &stats.peer_id[..16.min(stats.peer_id.len())]);
        println!("  Tasks completed: {}", stats.tasks_completed);
        println!("  Tasks received: {}", stats.tasks_received);
        println!("  Final balance: {:.6} PCLAW", stats.final_balance);
    }

    let total_tasks: usize = results.iter().map(|(_, s)| s.tasks_completed).sum();
    println!("\nTotal tasks completed across all agents: {}", total_tasks);

    // Cleanup
    let _ = std::fs::remove_dir_all(&temp_base);

    Ok(())
}

#[derive(Debug)]
struct AgentStats {
    peer_id: String,
    tasks_completed: usize,
    tasks_received: usize,
    final_balance: f64,
}

async fn run_agent(agent_num: u32, base_dir: std::path::PathBuf, duration_secs: u64) -> anyhow::Result<AgentStats> {
    // Create identity for this agent
    let identity = Arc::new(NodeIdentity::generate());
    let peer_id = identity.peer_id().to_string();

    // Create config with unique paths
    let mut config = Config::default();
    config.database.path = base_dir.join("data.redb");
    config.inference.models_dir = base_dir.join("models");
    std::fs::create_dir_all(&config.inference.models_dir)?;

    // Use different ports for each agent
    let port = 9000 + agent_num;
    config.p2p.listen_addresses = vec![format!("/ip4/127.0.0.1/tcp/{}", port)];

    // Connect agents to each other (agent 0 is bootstrap)
    if agent_num > 0 {
        config.p2p.bootstrap_peers = vec![format!("/ip4/127.0.0.1/tcp/{}", 9000)];
    }

    // Create runtime
    let db = Database::open(&config.database.path)?;
    let runtime = Runtime::new(identity, db, config).await?;

    // Subscribe to job topics
    runtime.subscribe_to_job_topics().await?;

    tracing::info!(
        agent = agent_num,
        peer_id = %peer_id,
        "Agent started"
    );

    let mut tasks_completed = 0;
    let mut tasks_received = 0;

    // Run for specified duration, periodically executing tasks
    let start = std::time::Instant::now();
    let duration = Duration::from_secs(duration_secs);

    while start.elapsed() < duration {
        // Each agent periodically executes a task
        if agent_num % 2 == 0 {
            // Even agents do inference tasks
            if runtime.inference("Test prompt", "test-model", 10).await.is_ok() {
                tasks_completed += 1;
            }
        } else {
            // Odd agents do web fetch tasks
            if runtime.web_fetch("https://httpbin.org/get").await.is_ok() {
                tasks_completed += 1;
            }
        }

        // Check for received tasks (job provider)
        let active = runtime.job_manager.read().await.active_jobs().await;
        tasks_received = active.len();

        // Small delay between tasks
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    let final_balance = from_micro(runtime.balance().await);

    Ok(AgentStats {
        peer_id,
        tasks_completed,
        tasks_received,
        final_balance,
    })
}

// ============================================================================
// Cluster Testing
// ============================================================================

/// Information about a running cluster node
#[allow(dead_code)]
struct ClusterNode {
    index: u32,
    child: Child,
    web_addr: String,
    p2p_addr: String,
    peer_id: Option<String>,
    base_dir: std::path::PathBuf,
}

/// Run a test cluster with multiple peer nodes
async fn run_cluster(
    nodes: u32,
    base_web_port: u16,
    base_p2p_port: u16,
    run_test_job: bool,
    keep_alive: bool,
) -> anyhow::Result<()> {
    println!("\x1b[1m=== PeerClaw Cluster Test ===\x1b[0m");
    println!("Spawning {} nodes...\n", nodes);

    // Get current executable path
    let exe_path = std::env::current_exe()?;

    // Create temp directory for cluster
    let cluster_dir = std::env::temp_dir().join(format!("peerclaw_cluster_{}", std::process::id()));
    std::fs::create_dir_all(&cluster_dir)?;

    let mut cluster_nodes: Vec<ClusterNode> = Vec::new();

    // First node address (bootstrap)
    let bootstrap_addr = format!("/ip4/127.0.0.1/tcp/{}", base_p2p_port);

    // Spawn nodes
    for i in 0..nodes {
        let web_port = base_web_port + i as u16;
        let p2p_port = base_p2p_port + i as u16;
        let web_addr = format!("127.0.0.1:{}", web_port);
        let p2p_addr = format!("/ip4/127.0.0.1/tcp/{}", p2p_port);

        let node_dir = cluster_dir.join(format!("node_{}", i));
        std::fs::create_dir_all(&node_dir)?;

        // Build command arguments
        let mut cmd = Command::new(&exe_path);
        cmd.arg("serve")
            .arg("--web")
            .arg(&web_addr)
            .arg("--listen")
            .arg(&p2p_addr)
            .arg("--provider")
            .env("PEERCLAWD_BASE_DIR", &node_dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        // Connect to bootstrap node (except for first node)
        if i > 0 {
            cmd.arg("--bootstrap").arg(&bootstrap_addr);
        }

        let child = cmd.spawn()?;

        println!("  Node {} starting: web={} p2p={}", i, web_addr, p2p_addr);

        cluster_nodes.push(ClusterNode {
            index: i,
            child,
            web_addr: format!("http://{}", web_addr),
            p2p_addr,
            peer_id: None,
            base_dir: node_dir,
        });

        // Small delay between spawns
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    // Wait for nodes to be ready
    println!("\nWaiting for nodes to be ready...");
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Poll status endpoints to get peer IDs
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;

    for node in &mut cluster_nodes {
        let url = format!("{}/api/status", node.web_addr);
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    if let Some(peer_id) = json.get("peer_id").and_then(|v| v.as_str()) {
                        node.peer_id = Some(peer_id.to_string());
                    }
                }
            }
            _ => {}
        }
    }

    // Display cluster status
    println!("\n\x1b[1m=== Cluster Status ===\x1b[0m\n");
    println!("{:<6} {:<20} {:<30} {:<10}",
             "Node", "Web URL", "Peer ID", "Status");
    println!("{}", "-".repeat(70));

    for node in &cluster_nodes {
        let peer_id_display = node.peer_id.as_ref()
            .map(|id| {
                if id.len() > 20 {
                    format!("{}...{}", &id[..8], &id[id.len()-8..])
                } else {
                    id.clone()
                }
            })
            .unwrap_or_else(|| "Unknown".to_string());

        let status = if node.peer_id.is_some() {
            "\x1b[32mReady\x1b[0m"
        } else {
            "\x1b[33mStarting\x1b[0m"
        };

        println!("{:<6} {:<20} {:<30} {}",
                 node.index, node.web_addr, peer_id_display, status);
    }
    println!();

    // Check connections
    println!("Checking peer connections...");
    tokio::time::sleep(Duration::from_secs(2)).await;

    for node in &cluster_nodes {
        let url = format!("{}/api/peers", node.web_addr);
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(peers) = resp.json::<Vec<serde_json::Value>>().await {
                    println!("  Node {}: {} connected peers", node.index, peers.len());
                }
            }
            _ => {
                println!("  Node {}: \x1b[33mUnable to check peers\x1b[0m", node.index);
            }
        }
    }
    println!();

    // Run test job if requested
    if run_test_job {
        println!("\x1b[1m=== Running Test Job ===\x1b[0m\n");

        // Submit a job to the first node
        if let Some(first_node) = cluster_nodes.first() {
            let url = format!("{}/api/chat", first_node.web_addr);

            println!("Submitting inference request to Node 0...");

            let payload = serde_json::json!({
                "message": "Hello from cluster test! Respond with a single word.",
                "model": "llama-3.2-3b",
                "max_tokens": 50
            });

            match client.post(&url).json(&payload).send().await {
                Ok(resp) => {
                    if let Ok(json) = resp.json::<serde_json::Value>().await {
                        println!("\nResponse received:");
                        if let Some(response) = json.get("response").and_then(|v| v.as_str()) {
                            println!("  Text: {}", response);
                        }
                        if let Some(tokens) = json.get("tokens").and_then(|v| v.as_u64()) {
                            println!("  Tokens: {}", tokens);
                        }
                        if let Some(provider) = json.get("provider_peer_id").and_then(|v| v.as_str()) {
                            let short = if provider.len() > 16 {
                                format!("{}...{}", &provider[..8], &provider[provider.len()-8..])
                            } else {
                                provider.to_string()
                            };
                            println!("  \x1b[36mExecuted by: {}\x1b[0m", short);
                        }
                    }
                }
                Err(e) => {
                    println!("\x1b[31mTest job failed: {}\x1b[0m", e);
                }
            }
            println!();
        }
    }

    // Keep alive or shutdown
    if keep_alive {
        println!("\x1b[33mCluster running. Press Ctrl+C to stop.\x1b[0m\n");
        println!("Web dashboards available at:");
        for node in &cluster_nodes {
            println!("  Node {}: {}", node.index, node.web_addr);
        }
        println!();

        // Wait for Ctrl+C
        tokio::signal::ctrl_c().await?;
        println!("\n\x1b[33mShutting down cluster...\x1b[0m");
    } else {
        println!("\x1b[32mCluster test complete.\x1b[0m\n");
    }

    // Cleanup: kill all child processes
    for mut node in cluster_nodes {
        let _ = node.child.kill();
        let _ = node.child.wait();
    }

    // Remove temp directory
    let _ = std::fs::remove_dir_all(&cluster_dir);

    println!("Cluster shutdown complete.");
    Ok(())
}
