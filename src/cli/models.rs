//! `peerclaw models` command - Manage AI models.

use clap::{Args, Subcommand};
use std::io::{self, Write};

use crate::bootstrap;

#[derive(Args)]
pub struct ModelsArgs {
    #[command(subcommand)]
    pub cmd: Option<ModelsCommand>,
}

#[derive(Subcommand)]
pub enum ModelsCommand {
    /// List available models
    List,

    /// Download a model from Hugging Face
    Download {
        /// Model name (e.g., llama-3.2-1b, llama-3.2-3b, phi-3-mini)
        model: String,

        /// Quantization level (q4_k_m, q5_k_m, q6_k, q8_0)
        #[arg(long, default_value = "q4_k_m")]
        quant: String,
    },

    /// Remove a downloaded model
    Remove {
        /// Model name to remove
        model: String,
    },

    /// Show model information
    Info {
        /// Model name
        model: String,
    },
}

/// Known models with their Hugging Face URLs
const KNOWN_MODELS: &[(&str, &str, &str)] = &[
    ("llama-3.2-1b", "bartowski/Llama-3.2-1B-Instruct-GGUF", "Llama-3.2-1B-Instruct"),
    ("llama-3.2-3b", "bartowski/Llama-3.2-3B-Instruct-GGUF", "Llama-3.2-3B-Instruct"),
    ("phi-3-mini", "microsoft/Phi-3-mini-4k-instruct-gguf", "Phi-3-mini-4k-instruct"),
    ("qwen2.5-0.5b", "Qwen/Qwen2.5-0.5B-Instruct-GGUF", "qwen2.5-0.5b-instruct"),
    ("qwen2.5-1.5b", "Qwen/Qwen2.5-1.5B-Instruct-GGUF", "qwen2.5-1.5b-instruct"),
    ("qwen2.5-3b", "Qwen/Qwen2.5-3B-Instruct-GGUF", "qwen2.5-3b-instruct"),
    ("gemma-2-2b", "bartowski/gemma-2-2b-it-GGUF", "gemma-2-2b-it"),
    ("tinyllama-1.1b", "TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF", "tinyllama-1.1b-chat-v1.0"),
];

pub async fn run(args: ModelsArgs) -> anyhow::Result<()> {
    match args.cmd {
        None | Some(ModelsCommand::List) => list_models().await,
        Some(ModelsCommand::Download { model, quant }) => download_model(&model, &quant).await,
        Some(ModelsCommand::Remove { model }) => remove_model(&model).await,
        Some(ModelsCommand::Info { model }) => show_info(&model).await,
    }
}

async fn list_models() -> anyhow::Result<()> {
    let models_dir = bootstrap::base_dir().join("models");
    std::fs::create_dir_all(&models_dir)?;

    println!();
    println!("\x1b[1m═══ Downloaded Models ═══\x1b[0m");
    println!("  Directory: \x1b[90m{}\x1b[0m", models_dir.display());
    println!();

    let mut found = false;
    if let Ok(entries) = std::fs::read_dir(&models_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "gguf") {
                found = true;
                let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
                let size = std::fs::metadata(&path)
                    .map(|m| {
                        let bytes = m.len();
                        if bytes >= 1_073_741_824 {
                            format!("{:.2} GB", bytes as f64 / 1_073_741_824.0)
                        } else {
                            format!("{:.0} MB", bytes as f64 / 1_048_576.0)
                        }
                    })
                    .unwrap_or_else(|_| "? bytes".to_string());
                println!("  \x1b[32m✓\x1b[0m \x1b[36m{}\x1b[0m \x1b[90m({})\x1b[0m", name, size);
            }
        }
    }

    if !found {
        println!("  \x1b[33mNo models downloaded yet.\x1b[0m");
    }

    println!();
    println!("\x1b[1m═══ Available for Download ═══\x1b[0m");
    println!();

    for (name, _repo, _filename) in KNOWN_MODELS {
        println!("  • \x1b[36m{}\x1b[0m", name);
    }

    println!();
    println!("  To download: \x1b[36mpeerclaw models download <name>\x1b[0m");
    println!("  Example:     \x1b[36mpeerclaw models download llama-3.2-1b\x1b[0m");
    println!();

    Ok(())
}

async fn download_model(model: &str, quant: &str) -> anyhow::Result<()> {
    let models_dir = bootstrap::base_dir().join("models");
    std::fs::create_dir_all(&models_dir)?;

    // Find model in known list
    let model_info = KNOWN_MODELS.iter().find(|(name, _, _)| *name == model);

    let (repo, filename) = match model_info {
        Some((_, repo, filename)) => (*repo, *filename),
        None => {
            println!("\x1b[33mModel '{}' not in known list.\x1b[0m", model);
            println!();
            println!("Available models:");
            for (name, _, _) in KNOWN_MODELS {
                println!("  • {}", name);
            }
            return Ok(());
        }
    };

    // Construct download URL
    let quant_upper = quant.to_uppercase().replace("_", "-");
    let filename_gguf = format!("{}-{}.gguf", filename, quant_upper);
    let url = format!(
        "https://huggingface.co/{}/resolve/main/{}",
        repo, filename_gguf
    );

    let output_path = models_dir.join(format!("{}-{}.gguf", model, quant));

    if output_path.exists() {
        println!("\x1b[33mModel already exists:\x1b[0m {}", output_path.display());
        return Ok(());
    }

    println!();
    println!("\x1b[1m═══ Downloading Model ═══\x1b[0m");
    println!("  Model:  \x1b[36m{}\x1b[0m", model);
    println!("  Quant:  \x1b[36m{}\x1b[0m", quant);
    println!("  From:   \x1b[90m{}\x1b[0m", url);
    println!("  To:     \x1b[90m{}\x1b[0m", output_path.display());
    println!();

    // Download with progress
    println!("\x1b[33mDownloading...\x1b[0m (this may take a while)");
    println!();

    let client = reqwest::Client::new();
    let response = client.get(&url).send().await?;

    if !response.status().is_success() {
        println!("\x1b[31mDownload failed:\x1b[0m HTTP {}", response.status());
        println!();
        println!("The model file may not exist at the expected URL.");
        println!("Try downloading manually from: \x1b[36mhttps://huggingface.co/{}\x1b[0m", repo);
        return Ok(());
    }

    let total_size = response.content_length().unwrap_or(0);
    let mut file = std::fs::File::create(&output_path)?;

    // Download the full content
    let bytes = response.bytes().await?;
    let downloaded = bytes.len() as u64;
    std::io::copy(&mut bytes.as_ref(), &mut file)?;

    // Show progress
    if total_size > 0 {
        let downloaded_mb = downloaded as f64 / 1_048_576.0;
        let total_mb = total_size as f64 / 1_048_576.0;
        println!("  [100%] {:.0}/{:.0} MB", downloaded_mb, total_mb);
    } else {
        let downloaded_mb = downloaded as f64 / 1_048_576.0;
        println!("  Downloaded: {:.0} MB", downloaded_mb);
    }

    println!();
    println!();
    println!("\x1b[32m✓ Download complete!\x1b[0m");
    println!("  Model saved to: \x1b[36m{}\x1b[0m", output_path.display());
    println!();
    println!("  To use in chat: \x1b[36mpeerclaw chat --model {}-{}\x1b[0m", model, quant);
    println!();

    Ok(())
}

async fn remove_model(model: &str) -> anyhow::Result<()> {
    let models_dir = bootstrap::base_dir().join("models");

    // Find matching model file
    let mut found = None;
    if let Ok(entries) = std::fs::read_dir(&models_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "gguf") {
                let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                if name.to_lowercase().contains(&model.to_lowercase()) {
                    found = Some(path);
                    break;
                }
            }
        }
    }

    match found {
        Some(path) => {
            let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
            print!("Remove model '{}'? [y/N] ", name);
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;

            if input.trim().eq_ignore_ascii_case("y") {
                std::fs::remove_file(&path)?;
                println!("\x1b[32m✓\x1b[0m Model removed.");
            } else {
                println!("Cancelled.");
            }
        }
        None => {
            println!("\x1b[33mModel '{}' not found.\x1b[0m", model);
            println!("Run \x1b[36mpeerclaw models list\x1b[0m to see downloaded models.");
        }
    }

    Ok(())
}

async fn show_info(model: &str) -> anyhow::Result<()> {
    let models_dir = bootstrap::base_dir().join("models");

    // Find matching model file
    let mut found = None;
    if let Ok(entries) = std::fs::read_dir(&models_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "gguf") {
                let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                if name.to_lowercase().contains(&model.to_lowercase()) {
                    found = Some(path);
                    break;
                }
            }
        }
    }

    match found {
        Some(path) => {
            let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
            let meta = std::fs::metadata(&path)?;
            let size_gb = meta.len() as f64 / 1_073_741_824.0;

            println!();
            println!("\x1b[1m═══ Model Info ═══\x1b[0m");
            println!("  Name:     \x1b[36m{}\x1b[0m", name);
            println!("  Path:     \x1b[90m{}\x1b[0m", path.display());
            println!("  Size:     {:.2} GB", size_gb);
            println!("  Format:   GGUF");
            println!();
        }
        None => {
            println!("\x1b[33mModel '{}' not found.\x1b[0m", model);
        }
    }

    Ok(())
}
