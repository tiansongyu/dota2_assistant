use clap::{Parser, Subcommand};
use dpda_debug::{date_boundary, Result};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

#[derive(Parser)]
#[command(
    name = "dpda-debug",
    version,
    about = "DPDA headless diagnostics: real public history, local statistics and chronological backtests"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check headless runtime and SQLite without making network requests.
    Doctor {
        #[arg(long)]
        db: Option<PathBuf>,
    },
    /// Probe official APIs and report verified response structure, never fabricated data.
    ApiProbe {
        /// Public Steam32 or exact Steam64 string; lookup is not authentication.
        #[arg(long)]
        account: Option<String>,
        #[arg(long)]
        match_id: Option<String>,
        /// Allow explicit optional STRATZ/Steam credentials from environment variables.
        #[arg(long)]
        probe_optional: bool,
    },
    /// Import public OpenDota history. Ctrl-C persists a paused checkpoint.
    Sync {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        account: String,
        /// Inclusive UTC start date, YYYY-MM-DD.
        #[arg(long)]
        from: String,
        /// Inclusive UTC end date; defaults to the current UTC date.
        #[arg(long)]
        to: Option<String>,
        #[arg(long, default_value_t=100, value_parser=clap::value_parser!(u32).range(1..=500))]
        page_size: u32,
        /// Explicit per-run budget. Reaching it pauses; it does not mean full import.
        #[arg(long, value_parser=clap::value_parser!(u32).range(1..))]
        max_pages: Option<u32>,
    },
    /// Inspect an existing database, coverage, checkpoints and recent records.
    Inspect {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        account: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Compute observed personal hero statistics from the local database.
    Stats {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        account: String,
        #[arg(long, default_value_t = 180.0)]
        half_life_days: f64,
        /// UTC date cutoff. If absent, uses current time.
        #[arg(long)]
        as_of: Option<String>,
        #[arg(long)]
        patch: Option<u32>,
    },
    /// Evaluate historical hero-choice coverage; not counterfactual win-rate improvement.
    Backtest {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        account: String,
        /// Explicit PoolConfig JSON; defaults to the desktop's saved per-account pool.
        #[arg(long)]
        pool: Option<PathBuf>,
        #[arg(long, value_parser=clap::value_parser!(u8).range(1..=5))]
        role: u8,
        #[arg(long, default_value_t = 20)]
        min_training: usize,
        #[arg(long, default_value_t = 180.0)]
        half_life_days: f64,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    match run(cli.command).await {
        Ok((report, success)) => {
            match serde_json::to_string_pretty(&report) {
                Ok(text) => println!("{text}"),
                Err(error) => {
                    eprintln!("JSON serialization failed: {error}");
                    std::process::exit(1);
                }
            }
            if !success {
                std::process::exit(1);
            }
        }
        Err(error) => {
            eprintln!("{}", json!({"status":"error","message":error.to_string()}));
            std::process::exit(1);
        }
    }
}

async fn run(command: Command) -> Result<(Value, bool)> {
    Ok(match command {
        Command::Doctor { db } => {
            let report = dpda_debug::doctor(db.as_deref());
            let success = report.pointer("/sqlite/status").and_then(Value::as_str) == Some("ok")
                && report.pointer("/database/status").and_then(Value::as_str) != Some("error");
            (report, success)
        }
        Command::ApiProbe {
            account,
            match_id,
            probe_optional,
        } => {
            let report =
                dpda_debug::api_probe(account.as_deref(), match_id.as_deref(), probe_optional)
                    .await?;
            let ok = report["ok"].as_bool().unwrap_or(false);
            (report, ok)
        }
        Command::Sync {
            db,
            account,
            from,
            to,
            page_size,
            max_pages,
        } => {
            let account_id = dpda_domain::normalize_account_id(&account)?;
            let from_time = date_boundary(&from, false)?;
            let to = to.unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d").to_string());
            let to_time = date_boundary(&to, true)?;
            if from_time > to_time {
                return Err("起始日期不能晚于结束日期".into());
            }
            if let Some(parent) = db.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)?;
            }
            let mut database = dpda_persistence::Database::open(&db)?;
            let client = dpda_data_provider::OpenDotaClient::new()?;
            let cancel = Arc::new(AtomicBool::new(false));
            let signal_cancel = cancel.clone();
            let signal = tokio::spawn(async move {
                if tokio::signal::ctrl_c().await.is_ok() {
                    signal_cancel.store(true, Ordering::Relaxed);
                    eprintln!(
                        "{}",
                        json!({"event":"pause_requested","message":"正在保存同步检查点"})
                    );
                }
            });
            let progress = dpda_history::sync_history(
                &client,
                &mut database,
                dpda_history::SyncOptions {
                    account_id,
                    from_time,
                    to_time,
                    page_size,
                    max_pages,
                },
                &cancel,
                |p| eprintln!("{}", json!({"event":"sync_progress","progress":p})),
            )
            .await;
            signal.abort();
            match progress {
                Ok(progress) => (
                    json!({"database":db,"progress":progress,"stored_matches":database.count(account_id)?,"note":"只有 completed 表示已扫描数据源当前可访问范围；paused 需要继续运行，不能证明已导入全部历史。"}),
                    true,
                ),
                Err(error) => (
                    json!({"status":"error","message":error.to_string(),"database":db,"checkpoint":database.checkpoint(account_id)?}),
                    false,
                ),
            }
        }
        Command::Inspect { db, account, limit } => {
            if limit > 5000 {
                return Err("--limit 不得大于5000".into());
            }
            (
                dpda_debug::inspect_database(&db, account.as_deref(), limit)?,
                true,
            )
        }
        Command::Stats {
            db,
            account,
            half_life_days,
            as_of,
            patch,
        } => {
            let now = as_of
                .as_deref()
                .map(|date| date_boundary(date, true))
                .transpose()?
                .unwrap_or_else(|| chrono::Utc::now().timestamp());
            (
                dpda_debug::statistics(&db, &account, half_life_days, now, patch)?,
                true,
            )
        }
        Command::Backtest {
            db,
            account,
            pool,
            role,
            min_training,
            half_life_days,
        } => (
            dpda_debug::run_backtest(
                &db,
                &account,
                pool.as_deref(),
                role,
                min_training,
                half_life_days,
            )?,
            true,
        ),
    })
}
