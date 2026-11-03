//! Synthetic workload diagnostic only; never uses an API or an existing user database.
use clap::Parser;
use dpda_domain::MatchRecord;
use dpda_hero_pool::{aggregate, PoolConfig, PoolMode};
use dpda_persistence::Database;
use dpda_recommender::{recommend, DraftContext, ModelConfig};
use serde::Serialize;
use serde_json::json;
use std::{collections::BTreeMap, hint::black_box, time::Instant};

const SYNTHETIC_ACCOUNT: u32 = 1;
const FIXED_NOW: i64 = 1_793_664_000; // 2026-11-03 00:00:00 UTC, independent of wall clock.
const SEED: u64 = 0x4450_4441_5359_4e54;
// Verified against the bundled real ID catalog. Assignments below are benchmark
// inputs, not assertions that these heroes should be played in those positions.
const CANDIDATES: [u32; 25] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 25, 26,
];
const ALL_HEROES: [u32; 30] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 25, 26, 29,
    30, 31, 35, 37,
];

#[derive(Parser)]
#[command(about = "SYNTHETIC ONLY: temporary SQLite load diagnostic; no real player results")]
struct Arguments {
    #[arg(long, default_value_t = 10_000, value_parser = clap::value_parser!(u32).range(100..=50_000))]
    matches: u32,
    #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u32).range(3..=10))]
    rounds: u32,
}

#[derive(Serialize)]
struct Measurements {
    samples_ms: Vec<f64>,
    first_ms: f64,
    subsequent_min_ms: f64,
    subsequent_median_ms: f64,
    subsequent_max_ms: f64,
}

impl Measurements {
    fn new(samples_ms: Vec<f64>) -> Self {
        let first_ms = samples_ms[0];
        let mut subsequent = samples_ms[1..].to_vec();
        subsequent.sort_by(f64::total_cmp);
        let middle = subsequent.len() / 2;
        let median = if subsequent.len().is_multiple_of(2) {
            (subsequent[middle - 1] + subsequent[middle]) / 2.0
        } else {
            subsequent[middle]
        };
        Self {
            first_ms,
            subsequent_min_ms: subsequent[0],
            subsequent_median_ms: median,
            subsequent_max_ms: *subsequent.last().expect("at least two subsequent rounds"),
            samples_ms,
        }
    }
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
    *seed
}

fn generate(count: u32) -> Vec<MatchRecord> {
    let mut seed = SEED;
    (0..count)
        .map(|index| {
            let hero_id = CANDIDATES[index as usize % CANDIDATES.len()];
            let offset = (next(&mut seed) as usize) % ALL_HEROES.len();
            let others: Vec<_> = (0..ALL_HEROES.len())
                .map(|i| ALL_HEROES[(offset + i) % ALL_HEROES.len()])
                .filter(|id| *id != hero_id)
                .take(9)
                .collect();
            let start_time = FIXED_NOW - 86_400 - i64::from(count - index) * 3_000;
            MatchRecord {
                match_id: (10_000_000_000_u64 + u64::from(index)).to_string(),
                account_id: SYNTHETIC_ACCOUNT,
                hero_id,
                start_time,
                duration: Some(2400),
                game_mode: Some(22),
                patch: Some(100 + index / 2_000),
                player_slot: Some(0),
                radiant_win: Some(next(&mut seed) >> 63 != 0),
                kills: Some((next(&mut seed) % 20) as u32),
                deaths: Some((next(&mut seed) % 15) as u32),
                assists: Some((next(&mut seed) % 25) as u32),
                last_hits: Some((next(&mut seed) % 400) as u32),
                denies: Some((next(&mut seed) % 30) as u32),
                gold_per_min: Some(300 + (next(&mut seed) % 400) as u32),
                xp_per_min: Some(400 + (next(&mut seed) % 600) as u32),
                lane_role: None,
                team_heroes: others[..4].to_vec(),
                enemy_heroes: others[4..].to_vec(),
                lineup_complete: true,
                source: "synthetic-performance-v1; NOT REAL MATCH DATA".into(),
                fetched_at: start_time + 2401,
            }
        })
        .collect()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Arguments::parse();
    let started = Instant::now();
    let generated = generate(args.matches);
    let generation_ms = elapsed_ms(started);
    // tempfile is already a dev dependency; there is intentionally no --db option.
    let folder = tempfile::tempdir()?;
    let path = folder.path().join("SYNTHETIC_ONLY.db");
    let started = Instant::now();
    let mut database = Database::open(&path)?;
    let create_and_migrate_ms = elapsed_ms(started);
    let started = Instant::now();
    let inserted = database.upsert_records(&generated)?;
    let insert_ms = elapsed_ms(started);
    database.set_setting(
        "synthetic_benchmark",
        &json!({"source":"synthetic","capture_hz":2}),
    )?;
    assert_eq!(inserted.inserted, u64::from(args.matches));
    drop(generated);
    drop(database);

    let mut measurements = BTreeMap::new();
    let mut times = vec![];
    for _ in 0..args.rounds {
        let started = Instant::now();
        let db = Database::open(&path)?;
        times.push(elapsed_ms(started));
        black_box(&db);
        drop(db);
    }
    measurements.insert(
        "database_open_with_quick_check".to_string(),
        Measurements::new(times),
    );
    let database = Database::open(&path)?;

    let mut times = vec![];
    for _ in 0..args.rounds {
        let started = Instant::now();
        let value = database.setting::<serde_json::Value>("synthetic_benchmark")?;
        times.push(elapsed_ms(started));
        assert!(value.is_some());
        black_box(value);
    }
    measurements.insert(
        "setting_on_reused_connection".into(),
        Measurements::new(times),
    );

    let mut records = vec![];
    let mut times = vec![];
    for _ in 0..args.rounds {
        drop(records);
        let started = Instant::now();
        records = database.records(SYNTHETIC_ACCOUNT)?;
        times.push(elapsed_ms(started));
        assert_eq!(records.len(), args.matches as usize);
        assert!(records
            .iter()
            .all(|m| m.team_heroes.len() == 4 && m.enemy_heroes.len() == 5));
        black_box(&records);
    }
    measurements.insert(
        "database_records_and_lineups".into(),
        Measurements::new(times),
    );

    let mut times = vec![];
    let mut stats = vec![];
    for _ in 0..args.rounds {
        let started = Instant::now();
        stats = aggregate(&records, FIXED_NOW, 180.0, None);
        times.push(elapsed_ms(started));
        assert_eq!(stats.len(), CANDIDATES.len());
        black_box(&stats);
    }
    measurements.insert("hero_pool_aggregate".into(), Measurements::new(times));

    let pool = PoolConfig {
        mode: PoolMode::Manual,
        included: CANDIDATES.to_vec(),
        excluded: vec![],
        roles: CANDIDATES
            .iter()
            .enumerate()
            .map(|(index, hero)| (hero.to_string(), vec![(index / 5 + 1) as u8]))
            .collect(),
        priorities: BTreeMap::new(),
    };
    let config = ModelConfig::default();
    let mut output_counts = BTreeMap::new();
    for role in 1..=5 {
        let context = DraftContext {
            allies: vec![30, 35],
            enemies: vec![29, 31, 37],
            banned: vec![],
            role,
            uncertain_slots: 4,
        };
        let mut times = vec![];
        for _ in 0..args.rounds {
            let started = Instant::now();
            let recommendations = recommend(&records, &stats, &pool, &context, &config, FIXED_NOW);
            times.push(elapsed_ms(started));
            assert_eq!(recommendations.len(), 5);
            output_counts.insert(role.to_string(), recommendations.len());
            black_box(recommendations);
        }
        measurements.insert(format!("recommend_role_{role}"), Measurements::new(times));
    }
    let database_bytes = std::fs::metadata(&path)?.len();
    let report = json!({
        "provenance":"synthetic",
        "generator":"synthetic-performance-v1",
        "seed_hex":format!("{SEED:016x}"),
        "fixed_now":FIXED_NOW,
        "matches":args.matches,
        "lineup_rows":u64::from(args.matches)*9,
        "rounds":args.rounds,
        "candidate_heroes":CANDIDATES,
        "role_assignments":"benchmark-only: five candidates per role; not gameplay advice",
        "recommendation_counts":output_counts,
        "database_bytes":database_bytes,
        "setup_ms":{"generate":generation_ms,"database_create_and_migrate":create_and_migrate_ms,"insert_transaction":insert_ms},
        "measurements":measurements,
        "notes":[
            "SYNTHETIC ONLY. No API calls and no real player data. The temporary SQLite database is deleted on exit.",
            "All records have distinct match IDs, one account, valid mode 22, duration 2400 and finished before fixed_now.",
            "Database was populated in this process; OS caches are not cleared. First sample is not a cold-disk benchmark.",
            "recommend includes its own eligibility filtering and aggregation. Per-role timings are separate scenarios, not summed frame cost.",
            "No historical backtest, screen capture, vision, GUI or game FPS measurement is performed.",
            "Use /usr/bin/time -v around this executable for whole-process peak RSS, including synthetic generation and SQLite insertion."
        ]
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
