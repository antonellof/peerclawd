//! `peerclaw agent` commands - Agent management.

use clap::Subcommand;
use std::path::PathBuf;

use crate::bootstrap;
use crate::db::Database;

#[derive(Subcommand)]
pub enum AgentCommand {
    /// Deploy and run an agent from spec
    Run {
        /// Path to agent spec file (TOML)
        #[arg(value_name = "SPEC")]
        spec: PathBuf,
    },

    /// List running agents
    List,

    /// Stream agent logs
    Logs {
        /// Agent ID
        #[arg(value_name = "ID")]
        id: String,

        /// Follow mode (like tail -f)
        #[arg(short, long)]
        follow: bool,
    },

    /// Stop an agent
    Stop {
        /// Agent ID
        #[arg(value_name = "ID")]
        id: String,
    },

    /// Show agent details
    Info {
        /// Agent ID
        #[arg(value_name = "ID")]
        id: String,
    },
}

pub async fn run(cmd: AgentCommand) -> anyhow::Result<()> {
    match cmd {
        AgentCommand::Run { spec } => {
            run_agent(spec).await?;
        }
        AgentCommand::List => {
            list_agents().await?;
        }
        AgentCommand::Logs { id, follow } => {
            stream_logs(&id, follow).await?;
        }
        AgentCommand::Stop { id } => {
            stop_agent(&id).await?;
        }
        AgentCommand::Info { id } => {
            show_agent_info(&id).await?;
        }
    }

    Ok(())
}

async fn run_agent(spec_path: PathBuf) -> anyhow::Result<()> {
    if !spec_path.exists() {
        anyhow::bail!("Agent spec file not found: {}", spec_path.display());
    }

    let spec_content = std::fs::read_to_string(&spec_path)?;

    // Parse the TOML spec
    let spec: toml::Value = toml::from_str(&spec_content)?;

    let agent_name = spec
        .get("agent")
        .and_then(|a| a.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("unnamed");

    let model = spec
        .get("model")
        .and_then(|m| m.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("llama-3.2-3b");

    println!("Loading agent: {}", agent_name);
    println!("Model: {}", model);
    println!("Spec: {}", spec_path.display());

    // Generate agent ID
    let agent_id = format!("agent_{}", uuid::Uuid::new_v4().to_string().replace('-', "")[..12].to_string());

    // Store agent state in database
    let db_path = bootstrap::database_path();
    if db_path.exists() {
        let db = Database::open(&db_path)?;
        let state = serde_json::json!({
            "id": agent_id,
            "name": agent_name,
            "model": model,
            "status": "running",
            "spec_path": spec_path.display().to_string(),
            "started_at": chrono::Utc::now().to_rfc3339(),
        });
        db.store_agent(&agent_id, &state)?;
    }

    println!("Agent {} deployed with ID: {}", agent_name, agent_id);
    println!("\nTo check status: peerclaw agent info {}", agent_id);
    println!("To stop:         peerclaw agent stop {}", agent_id);

    // In a full implementation this would start the agent loop.
    // For now, we register and exit - the node's event loop handles execution.
    println!("\nNote: Agent is registered. Start the node with 'peerclaw serve' to activate.");

    Ok(())
}

async fn list_agents() -> anyhow::Result<()> {
    let db_path = bootstrap::database_path();
    if !db_path.exists() {
        println!("No agents registered (database not initialized)");
        println!("Run 'peerclaw serve' first to initialize.");
        return Ok(());
    }

    let db = Database::open(&db_path)?;
    let agent_ids = db.list_agent_ids()?;

    if agent_ids.is_empty() {
        println!("No agents registered");
        println!("\nDeploy an agent: peerclaw agent run <spec.toml>");
        return Ok(());
    }

    println!("{:<16} {:<20} {:<15} {:<10}", "ID", "NAME", "MODEL", "STATUS");
    println!("{}", "-".repeat(65));

    for id in &agent_ids {
        if let Ok(Some(state)) = db.get_agent::<serde_json::Value>(id) {
            let name = state.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let model = state.get("model").and_then(|v| v.as_str()).unwrap_or("?");
            let status = state.get("status").and_then(|v| v.as_str()).unwrap_or("unknown");
            println!("{:<16} {:<20} {:<15} {:<10}", id, name, model, status);
        }
    }

    println!("\nTotal: {} agent(s)", agent_ids.len());

    Ok(())
}

async fn stream_logs(agent_id: &str, follow: bool) -> anyhow::Result<()> {
    let db_path = bootstrap::database_path();
    if !db_path.exists() {
        anyhow::bail!("Database not initialized. Run 'peerclaw serve' first.");
    }

    let db = Database::open(&db_path)?;
    let agent: Option<serde_json::Value> = db.get_agent(agent_id)?;

    if agent.is_none() {
        anyhow::bail!("Agent '{}' not found", agent_id);
    }

    let agent = agent.unwrap();
    let name = agent.get("name").and_then(|v| v.as_str()).unwrap_or("?");
    println!("Logs for agent: {} ({})", name, agent_id);
    println!("{}", "-".repeat(50));

    // Read logs from the agent's log directory
    let log_dir = bootstrap::data_dir().join("logs").join(agent_id);
    if !log_dir.exists() {
        println!("No log files found for this agent.");
        if follow {
            println!("Waiting for logs... (Ctrl+C to exit)");
            tokio::signal::ctrl_c().await?;
        }
        return Ok(());
    }

    // Read the latest log file
    let mut entries: Vec<_> = std::fs::read_dir(&log_dir)?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    if let Some(latest) = entries.last() {
        let content = std::fs::read_to_string(latest.path())?;
        print!("{}", content);
    }

    if follow {
        println!("\n--- Following logs (Ctrl+C to exit) ---");
        tokio::signal::ctrl_c().await?;
    }

    Ok(())
}

async fn stop_agent(agent_id: &str) -> anyhow::Result<()> {
    let db_path = bootstrap::database_path();
    if !db_path.exists() {
        anyhow::bail!("Database not initialized.");
    }

    let db = Database::open(&db_path)?;
    let agent: Option<serde_json::Value> = db.get_agent(agent_id)?;

    if agent.is_none() {
        anyhow::bail!("Agent '{}' not found", agent_id);
    }

    let mut state = agent.unwrap();
    let name = state.get("name").and_then(|v| v.as_str()).unwrap_or("?").to_string();

    // Update status to stopped
    if let Some(obj) = state.as_object_mut() {
        obj.insert("status".to_string(), serde_json::json!("stopped"));
        obj.insert("stopped_at".to_string(), serde_json::json!(chrono::Utc::now().to_rfc3339()));
    }
    db.store_agent(agent_id, &state)?;

    println!("Agent '{}' ({}) stopped.", name, agent_id);

    Ok(())
}

async fn show_agent_info(agent_id: &str) -> anyhow::Result<()> {
    let db_path = bootstrap::database_path();
    if !db_path.exists() {
        anyhow::bail!("Database not initialized.");
    }

    let db = Database::open(&db_path)?;
    let agent: Option<serde_json::Value> = db.get_agent(agent_id)?;

    if agent.is_none() {
        anyhow::bail!("Agent '{}' not found", agent_id);
    }

    let state = agent.unwrap();
    println!("Agent Details");
    println!("{}", "=".repeat(40));
    println!("ID:        {}", agent_id);

    if let Some(name) = state.get("name").and_then(|v| v.as_str()) {
        println!("Name:      {}", name);
    }
    if let Some(model) = state.get("model").and_then(|v| v.as_str()) {
        println!("Model:     {}", model);
    }
    if let Some(status) = state.get("status").and_then(|v| v.as_str()) {
        println!("Status:    {}", status);
    }
    if let Some(spec) = state.get("spec_path").and_then(|v| v.as_str()) {
        println!("Spec:      {}", spec);
    }
    if let Some(started) = state.get("started_at").and_then(|v| v.as_str()) {
        println!("Started:   {}", started);
    }
    if let Some(stopped) = state.get("stopped_at").and_then(|v| v.as_str()) {
        println!("Stopped:   {}", stopped);
    }

    Ok(())
}
