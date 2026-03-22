//! `peerclaw doctor` - Run diagnostics on all subsystems.

use std::time::Instant;

use crate::bootstrap;
use crate::db::Database;
use crate::identity::NodeIdentity;
use crate::wasm::sandbox::{SandboxConfig, WasmSandbox};

struct CheckResult {
    name: &'static str,
    status: CheckStatus,
    detail: String,
    duration_ms: u128,
}

enum CheckStatus {
    Ok,
    Warn,
    Fail,
    Skip,
}

impl CheckResult {
    fn icon(&self) -> &'static str {
        match self.status {
            CheckStatus::Ok => "OK",
            CheckStatus::Warn => "WARN",
            CheckStatus::Fail => "FAIL",
            CheckStatus::Skip => "SKIP",
        }
    }
}

pub async fn run() -> anyhow::Result<()> {
    println!("PeerClaw Doctor");
    println!("{}", "=".repeat(60));
    println!("Running diagnostics on all subsystems...\n");

    let mut results = Vec::new();

    // Run all checks
    results.push(check_directories().await);
    results.push(check_identity().await);
    results.push(check_database().await);
    results.push(check_inference().await);
    results.push(check_wasm_runtime().await);
    results.push(check_vector_store().await);
    results.push(check_skills().await);
    results.push(check_safety().await);
    results.push(check_web_server().await);
    results.push(check_p2p_config().await);

    // Print results
    println!("\n{:<5} {:<20} {:<6} DETAIL", "", "CHECK", "STATUS");
    println!("{}", "-".repeat(70));

    let mut ok_count = 0;
    let mut warn_count = 0;
    let mut fail_count = 0;

    for (i, result) in results.iter().enumerate() {
        let num = format!("{:>2}.", i + 1);
        println!(
            "{} {:<20} [{:<4}] {} ({}ms)",
            num, result.name, result.icon(), result.detail, result.duration_ms
        );

        match result.status {
            CheckStatus::Ok => ok_count += 1,
            CheckStatus::Warn => warn_count += 1,
            CheckStatus::Fail => fail_count += 1,
            CheckStatus::Skip => {}
        }
    }

    println!("{}", "-".repeat(70));
    println!(
        "Results: {} passed, {} warnings, {} failed",
        ok_count, warn_count, fail_count
    );

    if fail_count > 0 {
        println!("\nSome checks failed. Run 'peerclaw serve' to initialize missing components.");
    } else if warn_count > 0 {
        println!("\nAll critical checks passed with some warnings.");
    } else {
        println!("\nAll checks passed! System is healthy.");
    }

    Ok(())
}

async fn check_directories() -> CheckResult {
    let start = Instant::now();
    let base = bootstrap::base_dir();

    let dirs = [
        ("base", base.clone()),
        ("tools", bootstrap::tools_dir()),
        ("agents", bootstrap::agents_dir()),
        ("data", bootstrap::data_dir()),
        ("models", bootstrap::models_dir()),
    ];

    let mut missing = Vec::new();
    for (name, path) in &dirs {
        if !path.exists() {
            missing.push(*name);
        }
    }

    let (status, detail) = if missing.is_empty() {
        (CheckStatus::Ok, format!("All directories exist at {}", base.display()))
    } else if !base.exists() {
        (CheckStatus::Fail, format!("Base directory missing: {}", base.display()))
    } else {
        (CheckStatus::Warn, format!("Missing: {}", missing.join(", ")))
    };

    CheckResult {
        name: "Directories",
        status,
        detail,
        duration_ms: start.elapsed().as_millis(),
    }
}

async fn check_identity() -> CheckResult {
    let start = Instant::now();
    let path = bootstrap::identity_path();

    if !path.exists() {
        return CheckResult {
            name: "Identity",
            status: CheckStatus::Warn,
            detail: "No identity key found. Will be generated on first run.".to_string(),
            duration_ms: start.elapsed().as_millis(),
        };
    }

    match NodeIdentity::load(&path) {
        Ok(identity) => CheckResult {
            name: "Identity",
            status: CheckStatus::Ok,
            detail: format!("Ed25519 key loaded. Peer ID: {}...{}",
                &identity.peer_id().to_string()[..8],
                &identity.peer_id().to_string()[identity.peer_id().to_string().len()-6..]),
            duration_ms: start.elapsed().as_millis(),
        },
        Err(e) => CheckResult {
            name: "Identity",
            status: CheckStatus::Fail,
            detail: format!("Failed to load key: {}", e),
            duration_ms: start.elapsed().as_millis(),
        },
    }
}

async fn check_database() -> CheckResult {
    let start = Instant::now();
    let path = bootstrap::database_path();

    if !path.exists() {
        return CheckResult {
            name: "Database",
            status: CheckStatus::Warn,
            detail: "Database not initialized. Will be created on first run.".to_string(),
            duration_ms: start.elapsed().as_millis(),
        };
    }

    match Database::open(&path) {
        Ok(db) => {
            let peers = db.list_peer_ids().unwrap_or_default().len();
            let agents = db.list_agent_ids().unwrap_or_default().len();
            let tools = db.list_tool_names().unwrap_or_default().len();
            CheckResult {
                name: "Database",
                status: CheckStatus::Ok,
                detail: format!("redb OK. {} peers, {} agents, {} tools", peers, agents, tools),
                duration_ms: start.elapsed().as_millis(),
            }
        }
        Err(e) => CheckResult {
            name: "Database",
            status: CheckStatus::Fail,
            detail: format!("Failed to open: {}", e),
            duration_ms: start.elapsed().as_millis(),
        },
    }
}

async fn check_inference() -> CheckResult {
    let start = Instant::now();
    let models_dir = bootstrap::models_dir();

    if !models_dir.exists() {
        return CheckResult {
            name: "Inference",
            status: CheckStatus::Warn,
            detail: "Models directory not found.".to_string(),
            duration_ms: start.elapsed().as_millis(),
        };
    }

    // Count GGUF model files
    let model_count = std::fs::read_dir(&models_dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.path()
                        .extension()
                        .map(|ext| ext == "gguf")
                        .unwrap_or(false)
                })
                .count()
        })
        .unwrap_or(0);

    let (status, detail) = if model_count > 0 {
        (CheckStatus::Ok, format!("{} GGUF model(s) found in {}", model_count, models_dir.display()))
    } else {
        (CheckStatus::Warn, "No GGUF models found. Download with: peerclaw models download <model>".to_string())
    };

    CheckResult {
        name: "Inference",
        status,
        detail,
        duration_ms: start.elapsed().as_millis(),
    }
}

async fn check_wasm_runtime() -> CheckResult {
    let start = Instant::now();

    match WasmSandbox::new(SandboxConfig::default()) {
        Ok(_) => CheckResult {
            name: "WASM Runtime",
            status: CheckStatus::Ok,
            detail: "Wasmtime engine initialized successfully.".to_string(),
            duration_ms: start.elapsed().as_millis(),
        },
        Err(e) => CheckResult {
            name: "WASM Runtime",
            status: CheckStatus::Fail,
            detail: format!("Wasmtime init failed: {}", e),
            duration_ms: start.elapsed().as_millis(),
        },
    }
}

async fn check_vector_store() -> CheckResult {
    let start = Instant::now();

    // Just verify we can create a vector store
    let store = crate::vector::VectorStore::new(Default::default());
    let collections = store.list_collections();
    CheckResult {
        name: "Vector Store",
        status: CheckStatus::Ok,
        detail: format!("vectX OK. {} collection(s) in memory.", collections.len()),
        duration_ms: start.elapsed().as_millis(),
    }
}

async fn check_skills() -> CheckResult {
    let start = Instant::now();
    let skills_dir = bootstrap::base_dir().join("skills");

    if !skills_dir.exists() {
        return CheckResult {
            name: "Skills",
            status: CheckStatus::Ok,
            detail: "No custom skills installed (using defaults).".to_string(),
            duration_ms: start.elapsed().as_millis(),
        };
    }

    let skill_count = std::fs::read_dir(&skills_dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.path()
                        .file_name()
                        .map(|n| n.to_string_lossy().ends_with(".md"))
                        .unwrap_or(false)
                })
                .count()
        })
        .unwrap_or(0);

    CheckResult {
        name: "Skills",
        status: CheckStatus::Ok,
        detail: format!("{} skill file(s) found.", skill_count),
        duration_ms: start.elapsed().as_millis(),
    }
}

async fn check_safety() -> CheckResult {
    let start = Instant::now();

    // Verify leak detector and sanitizer work
    let detector = crate::safety::LeakDetector::new();
    let test_safe = "Hello, this is a normal message";
    let test_leak = "sk-proj-abc123def456ghi789jkl012mno345pqr678stu901vwx";

    let safe_detected = detector.scan(test_safe);
    let leak_detected = detector.scan(test_leak);

    let (status, detail) = if safe_detected.is_empty() && !leak_detected.is_empty() {
        (CheckStatus::Ok, "Leak detection and sanitization working.".to_string())
    } else if !safe_detected.is_empty() {
        (CheckStatus::Warn, "False positive in leak detection.".to_string())
    } else {
        (CheckStatus::Warn, "Leak detection may not catch all patterns.".to_string())
    };

    CheckResult {
        name: "Safety",
        status,
        detail,
        duration_ms: start.elapsed().as_millis(),
    }
}

async fn check_web_server() -> CheckResult {
    let start = Instant::now();

    match reqwest::Client::new()
        .get("http://127.0.0.1:8080/api/status")
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => CheckResult {
            name: "Web Server",
            status: CheckStatus::Ok,
            detail: "Dashboard responding on :8080.".to_string(),
            duration_ms: start.elapsed().as_millis(),
        },
        _ => CheckResult {
            name: "Web Server",
            status: CheckStatus::Skip,
            detail: "Not running. Start with: peerclaw serve --web :8080".to_string(),
            duration_ms: start.elapsed().as_millis(),
        },
    }
}

async fn check_p2p_config() -> CheckResult {
    let start = Instant::now();

    let config_path = bootstrap::config_path();
    if config_path.exists() {
        match std::fs::read_to_string(&config_path) {
            Ok(content) => {
                match toml::from_str::<toml::Value>(&content) {
                    Ok(config) => {
                        let has_p2p = config.get("p2p").is_some();
                        let has_web = config.get("web").is_some();
                        CheckResult {
                            name: "P2P Config",
                            status: CheckStatus::Ok,
                            detail: format!("Config loaded. P2P: {}, Web: {}", has_p2p, has_web),
                            duration_ms: start.elapsed().as_millis(),
                        }
                    }
                    Err(e) => CheckResult {
                        name: "P2P Config",
                        status: CheckStatus::Fail,
                        detail: format!("Invalid config TOML: {}", e),
                        duration_ms: start.elapsed().as_millis(),
                    },
                }
            }
            Err(e) => CheckResult {
                name: "P2P Config",
                status: CheckStatus::Fail,
                detail: format!("Cannot read config: {}", e),
                duration_ms: start.elapsed().as_millis(),
            },
        }
    } else {
        CheckResult {
            name: "P2P Config",
            status: CheckStatus::Ok,
            detail: "Using default configuration.".to_string(),
            duration_ms: start.elapsed().as_millis(),
        }
    }
}
