use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod commands;
use commands::{check, dev, new, run};

#[derive(Parser)]
#[command(name = "verdict")]
#[command(about = "CLI for Verdict agent framework", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new Verdict project
    New {
        /// Project name
        name: String,
    },

    /// Run development agent
    Dev {
        /// Project directory
        #[arg(short, long)]
        path: Option<PathBuf>,
    },

    /// Check compilation (runs cargo check)
    Check {
        /// Project directory
        #[arg(short, long)]
        path: Option<PathBuf>,
    },

    /// Run a named agent
    Run {
        /// Agent name
        agent: String,

        /// Project directory
        #[arg(short, long)]
        path: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::New { name } => {
            new::handle(&name)?;
        }
        Commands::Dev { path } => {
            dev::handle(path).await?;
        }
        Commands::Check { path } => {
            check::handle(path)?;
        }
        Commands::Run { agent, path } => {
            run::handle(&agent, path)?;
        }
    }

    Ok(())
}
