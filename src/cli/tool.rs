//! `peerclaw tool` commands - Tool management.

use clap::Subcommand;

use crate::tools::ToolRegistry;

#[derive(Subcommand)]
pub enum ToolCommand {
    /// List all available tools
    List {
        /// Show detailed information
        #[arg(short, long)]
        verbose: bool,
    },

    /// Show tool information
    Info {
        /// Tool name
        #[arg(value_name = "NAME")]
        name: String,
    },

    /// Execute a tool (for testing)
    Exec {
        /// Tool name
        name: String,
        /// JSON parameters
        #[arg(short, long, default_value = "{}")]
        params: String,
    },

    /// Show tool execution statistics
    Stats {
        /// Tool name (optional, shows all if not specified)
        name: Option<String>,
    },

    /// Build a WASM tool from description (planned)
    Build {
        /// Tool description
        #[arg(value_name = "DESC")]
        description: String,
    },

    /// Install a WASM tool from URL or registry (planned)
    Install {
        /// Tool URL or registry name
        #[arg(value_name = "SOURCE")]
        source: String,
    },

    /// Remove an installed tool (planned)
    Remove {
        /// Tool name
        #[arg(value_name = "NAME")]
        name: String,
    },
}

pub async fn run(cmd: ToolCommand) -> anyhow::Result<()> {
    // Create a tool registry for listing
    let registry = ToolRegistry::new("cli-user".to_string());

    match cmd {
        ToolCommand::List { verbose } => {
            let tools = registry.list_tools().await;

            println!("\n{:=<60}", "");
            println!(" Available Tools ({} total)", tools.len());
            println!("{:=<60}\n", "");

            if tools.is_empty() {
                println!("  No tools available.");
            } else {
                // Group by location
                let local_tools: Vec<_> = tools.iter()
                    .filter(|t| matches!(t.location, crate::tools::ToolLocation::Local))
                    .collect();
                let remote_tools: Vec<_> = tools.iter()
                    .filter(|t| matches!(t.location, crate::tools::ToolLocation::Remote))
                    .collect();

                if !local_tools.is_empty() {
                    println!(" Local Tools:");
                    println!(" {:-<58}", "");
                    for tool in &local_tools {
                        if verbose {
                            println!("  {} - {}", tool.name, tool.description);
                            println!("    Domain: {:?}, Price: {} micro-PCLAW", tool.domain, tool.price);
                        } else {
                            println!("  {:20} {}", tool.name, truncate(&tool.description, 35));
                        }
                    }
                }

                if !remote_tools.is_empty() {
                    println!("\n Remote Tools (from network):");
                    println!(" {:-<58}", "");
                    for tool in &remote_tools {
                        if verbose {
                            println!("  {} - {}", tool.name, tool.description);
                            println!("    Peer: {}, Price: {} micro-PCLAW",
                                tool.peer_id.as_ref().map(|s| &s[..12.min(s.len())]).unwrap_or("?"),
                                tool.price
                            );
                        } else {
                            println!("  {:20} {} ({} μPCLAW)",
                                tool.name,
                                truncate(&tool.description, 25),
                                tool.price
                            );
                        }
                    }
                }
            }
            println!();
        }

        ToolCommand::Info { name } => {
            if let Some(tool) = registry.get(&name) {
                println!("\n{:=<60}", "");
                println!(" Tool: {}", tool.name());
                println!("{:=<60}\n", "");
                println!("  Description: {}", tool.description());
                println!("  Domain:      {:?}", tool.domain());
                println!("  Approval:    {:?}", tool.approval_requirement());
                println!();
                println!("  Parameters:");
                let schema = tool.parameters_schema();
                println!("{}", serde_json::to_string_pretty(&schema)?);

                // Show stats if available
                if let Some(stats) = registry.get_stats(&name).await {
                    println!("\n  Statistics:");
                    println!("    Total calls:     {}", stats.total_calls);
                    println!("    Successful:      {}", stats.successful_calls);
                    println!("    Failed:          {}", stats.failed_calls);
                    println!("    Avg time:        {} ms",
                        if stats.total_calls > 0 {
                            stats.total_time_ms / stats.total_calls
                        } else { 0 }
                    );
                }
            } else {
                println!("Tool '{}' not found", name);
                println!("Use 'peerclaw tool list' to see available tools.");
            }
        }

        ToolCommand::Exec { name, params } => {
            let params: serde_json::Value = serde_json::from_str(&params)?;
            let ctx = crate::tools::ToolContext::local("cli-user".to_string());

            println!("Executing tool '{}'...", name);

            match registry.execute_local(&name, params, &ctx).await {
                Ok(result) => {
                    println!("\nResult ({}ms):", result.execution_time_ms);
                    if result.output.success {
                        println!("{}", serde_json::to_string_pretty(&result.output.data)?);
                    } else {
                        // Error is in the data field
                        if let Some(err) = result.output.data.get("error") {
                            println!("Error: {}", err);
                        } else if let Some(msg) = &result.output.message {
                            println!("Error: {}", msg);
                        }
                    }
                }
                Err(e) => {
                    println!("Execution failed: {}", e);
                }
            }
        }

        ToolCommand::Stats { name } => {
            println!("\n{:=<60}", "");
            println!(" Tool Execution Statistics");
            println!("{:=<60}\n", "");

            let stats = registry.all_stats().await;

            if let Some(name) = name {
                if let Some(s) = stats.get(&name) {
                    print_stats(&name, s);
                } else {
                    println!("  No statistics for tool '{}'", name);
                }
            } else if stats.is_empty() {
                println!("  No execution statistics recorded yet.");
            } else {
                for (name, s) in &stats {
                    print_stats(name, s);
                }
            }
        }

        ToolCommand::Build { description } => {
            println!("Building tool from description: {}", description);
            println!();
            println!("Dynamic tool building is planned for a future release.");
            println!("This feature will allow you to describe a tool in natural language");
            println!("and have the system generate a WASM module for it.");
        }

        ToolCommand::Install { source } => {
            println!("Installing tool from: {}", source);
            println!();
            println!("Tool installation from URLs is planned for a future release.");
            println!("Currently, tools must be built locally as WASM components.");
        }

        ToolCommand::Remove { name } => {
            println!("Removing tool: {}", name);
            println!();
            println!("Built-in tools cannot be removed.");
            println!("WASM tool removal is planned for a future release.");
        }
    }

    Ok(())
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

fn print_stats(name: &str, stats: &crate::tools::registry::ToolStats) {
    let success_rate = if stats.total_calls > 0 {
        (stats.successful_calls as f64 / stats.total_calls as f64) * 100.0
    } else {
        0.0
    };
    let avg_time = if stats.total_calls > 0 {
        stats.total_time_ms / stats.total_calls
    } else {
        0
    };

    println!("  {}", name);
    println!("    Calls: {} total, {} success, {} failed ({:.1}% success rate)",
        stats.total_calls, stats.successful_calls, stats.failed_calls, success_rate);
    println!("    Avg time: {} ms, Total time: {} ms", avg_time, stats.total_time_ms);
    if stats.tokens_earned > 0 || stats.tokens_spent > 0 {
        println!("    Tokens: {} earned, {} spent", stats.tokens_earned, stats.tokens_spent);
    }
    println!();
}
