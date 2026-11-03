mod models;
use dpda_domain::{normalize_account_id, steam64, Hero};
use dpda_draft_state::{DraftTracker, RecognitionState};
use dpda_hero_pool::{aggregate, automatic_pool, PoolConfig};
use dpda_persistence::Database;
use dpda_recommender::{recommend, ModelConfig};
use models::*;
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

#[derive(Default)]
struct Runtime {
    draft: Draft,
    tracker: DraftTracker,
    capture: CaptureStatus,
    analysis: Option<dpda_vision::FrameAnalysis>,
}
struct AppState {
    path: PathBuf,
    runtime: Arc<Mutex<Runtime>>,
    sync_cancel: Arc<AtomicBool>,
    sync_running: Arc<AtomicBool>,
    capture_cancel: Arc<AtomicBool>,
    capture_running: Arc<AtomicBool>,
}
fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn db(state: &AppState) -> Result<Database, String> {
    Database::open(&state.path).map_err(error)
}
fn heroes() -> Result<Vec<Hero>, String> {
    serde_json::from_str(include_str!("../../../../assets/heroes.json")).map_err(error)
}
fn hero_ids() -> Result<BTreeSet<u32>, String> {
    Ok(heroes()?.into_iter().map(|h| h.id).collect())
}
fn emit(app: &tauri::AppHandle) {
    let _ = app.emit("dpda://state-updated", ());
}
fn profile(database: &Database) -> Result<Option<Profile>, String> {
    database.setting("active_profile").map_err(error)
}
fn settings(database: &Database) -> Result<AppSettings, String> {
    Ok(database
        .setting("app_settings")
        .map_err(error)?
        .unwrap_or_default())
}
fn pool_key(id: u32) -> String {
    format!("pool:{id}")
}

#[tauri::command]
async fn get_snapshot(state: State<'_, AppState>) -> Result<Value, String> {
    let path = state.path.clone();
    let runtime = state.runtime.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let database=Database::open(path).map_err(error)?;
        let profile=profile(&database)?;let settings=settings(&database)?;
        let records=if let Some(p)=&profile {database.records(p.account_id).map_err(error)?}else{vec![]};
        let pool:PoolConfig=if let Some(p)=&profile {database.setting(&pool_key(p.account_id)).map_err(error)?.unwrap_or_default()}else{PoolConfig::default()};
        let now=chrono::Utc::now().timestamp();
        let stats=aggregate(&records,now,settings.half_life_days,None);
        let (draft,mut capture)={let rt=runtime.lock().map_err(error)?;(rt.draft.clone(),rt.capture.clone())};
        let recommendation_records = dpda_recommender::eligible_records(&records, now);
        let recommendation_stats = aggregate(&recommendation_records, now, settings.half_life_days, None);
        let automatic_pool = automatic_pool(&recommendation_stats);
        let context = draft.context(settings.selected_role);
        let pool_candidates: Vec<_> = pool.candidates(&recommendation_stats, settings.selected_role).into_iter()
            .filter(|id| !context.allies.contains(id) && !context.enemies.contains(id) && !context.banned.contains(id)).collect();
        let started=Instant::now();
        let recommendations=recommend(&records,&stats,&pool,&draft.context(settings.selected_role),&ModelConfig{weights:settings.weights,half_life_days:settings.half_life_days,top_k:settings.top_k,current_patch:None},now);
        capture.recommendation_ms=Some(started.elapsed().as_secs_f64()*1000.0);
        let sync=if let Some(p)=&profile {database.checkpoint(p.account_id).map_err(error)?}else{None};
        let sync=match sync {Some(p)=>serde_json::to_value(p).map_err(error)?,None=>json!({"account_id":profile.as_ref().map(|p|p.account_id),"status":"idle","offset":0,"pages_fetched":0,"imported":0,"updated":0,"failed":0,"missing_lineups":0,"from_time":null,"to_time":null,"last_synced_at":null,"error":null})};
        Ok(json!({"schema_version":1,"automatic_pool":automatic_pool,"pool_candidates":pool_candidates,"profile":profile,"heroes":heroes()?,"hero_stats":stats,"matches":records,"pool":pool,"draft":draft,
            "recommendations":recommendations,"sync":sync,"capture":capture,"settings":settings,"data_updated_at":records.iter().map(|m|m.fetched_at).max()}))
    }).await.map_err(error)?
}

#[tauri::command]
fn save_account(
    input: String,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    if state.sync_running.load(Ordering::SeqCst) {
        return Err("请先暂停同步，再切换账号".into());
    }
    let account_id = normalize_account_id(&input).map_err(error)?;
    let database = db(&state)?;
    database.ensure_profile(account_id).map_err(error)?;
    database
        .set_setting(
            "active_profile",
            &Profile {
                account_id,
                steam64: steam64(account_id),
            },
        )
        .map_err(error)?;
    emit(&app);
    Ok(())
}
#[tauri::command]
async fn start_sync(
    from_time: i64,
    to_time: i64,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let database = db(&state)?;
    let profile = profile(&database)?.ok_or("请先设置公开账号 ID")?;
    if from_time < 0 || to_time < from_time || to_time > chrono::Utc::now().timestamp() + 86400 {
        return Err("同步日期范围无效".into());
    }
    let client = dpda_data_provider::OpenDotaClient::new().map_err(error)?;
    let mut worker_db = Database::open(&state.path).map_err(error)?;
    state
        .sync_running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .map_err(|_| "同步已经运行".to_string())?;
    state.sync_cancel.store(false, Ordering::SeqCst);
    let cancel = state.sync_cancel.clone();
    let running = state.sync_running.clone();
    tauri::async_runtime::spawn(async move {
        let result = dpda_history::sync_history(
            &client,
            &mut worker_db,
            dpda_history::SyncOptions {
                account_id: profile.account_id,
                from_time,
                to_time,
                page_size: 100,
                max_pages: None,
            },
            &cancel,
            |_| emit(&app),
        )
        .await;
        if let Err(e) = result {
            tracing::error!(error=%e,"history sync failed");
            let _ = app.emit("dpda://error", e.to_string());
        }
        running.store(false, Ordering::SeqCst);
        emit(&app);
    });
    Ok(())
}
#[tauri::command]
fn pause_sync(state: State<'_, AppState>) {
    state.sync_cancel.store(true, Ordering::SeqCst);
}
#[tauri::command]
fn save_pool(
    pool: PoolConfig,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    pool.validate()?;
    let ids = hero_ids()?;
    if pool
        .included
        .iter()
        .chain(&pool.excluded)
        .any(|id| !ids.contains(id))
        || pool
            .roles
            .keys()
            .chain(pool.priorities.keys())
            .any(|id| id.parse::<u32>().map_or(true, |id| !ids.contains(&id)))
    {
        return Err("英雄池含未知英雄 ID".into());
    }
    let database = db(&state)?;
    let p = profile(&database)?.ok_or("请先设置账号")?;
    database
        .set_setting(&pool_key(p.account_id), &pool)
        .map_err(error)?;
    emit(&app);
    Ok(())
}
#[tauri::command]
fn update_draft(
    draft: Draft,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    draft.validate(&hero_ids()?)?;
    let mut rt = state.runtime.lock().map_err(error)?;
    let mut tracker = rt.tracker.clone();
    let timestamp = chrono::Utc::now().timestamp_millis().max(0) as u64;
    let incoming: Vec<_> = draft.radiant.iter().chain(&draft.dire).copied().collect();
    let previous: Vec<_> = rt
        .draft
        .radiant
        .iter()
        .chain(&rt.draft.dire)
        .copied()
        .collect();
    if incoming.iter().all(Option::is_none) && draft.banned.is_empty() {
        tracker.reset();
    } else {
        tracker.set_bans(Vec::new()).map_err(error)?;
        // Clear changed slots before assigning heroes, so swaps are validated atomically.
        for (slot, (old, next)) in previous.iter().zip(&incoming).enumerate() {
            if old != next {
                tracker
                    .manual_set(slot as u8, None, timestamp)
                    .map_err(error)?;
            }
        }
        for (slot, (old, next)) in previous.iter().zip(&incoming).enumerate() {
            if old != next && next.is_some() {
                tracker
                    .manual_set(slot as u8, *next, timestamp)
                    .map_err(error)?;
            }
        }
        tracker.set_bans(draft.banned.clone()).map_err(error)?;
    }
    tracker.set_team(Some(if draft.own_team == "radiant" {
        dpda_draft_state::Team::Radiant
    } else {
        dpda_draft_state::Team::Dire
    }));
    rt.tracker = tracker;
    rt.draft = draft;
    drop(rt);
    emit(&app);
    Ok(())
}
#[tauri::command]
fn save_settings(
    settings: AppSettings,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    settings.validate()?;
    if settings.overlay_click_through
        && !app
            .global_shortcut()
            .is_registered("CommandOrControl+Shift+F9")
    {
        return Err("恢复快捷键未注册，不能启用点击穿透".into());
    }
    let database = db(&state)?;
    let previous = crate::settings(&database)?;
    if let Some(window) = app.get_webview_window("overlay") {
        window
            .set_ignore_cursor_events(settings.overlay_click_through)
            .map_err(error)?;
        if previous.overlay_compact != settings.overlay_compact {
            let size = window
                .inner_size()
                .map_err(error)?
                .to_logical::<f64>(window.scale_factor().map_err(error)?);
            window
                .set_size(tauri::LogicalSize::new(
                    size.width,
                    overlay_height(settings.overlay_compact),
                ))
                .map_err(error)?;
        }
    }
    database
        .set_setting("app_settings", &settings)
        .map_err(error)?;
    emit(&app);
    Ok(())
}
fn overlay_height(compact: bool) -> f64 {
    if compact {
        420.0
    } else {
        620.0
    }
}
#[tauri::command]
fn open_overlay(state: State<'_, AppState>, app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("overlay") {
        window.show().map_err(error)?;
        return Ok(());
    }
    let settings = settings(&db(&state)?)?;
    let window = tauri::WebviewWindowBuilder::new(
        &app,
        "overlay",
        tauri::WebviewUrl::App("index.html?overlay=1".into()),
    )
    .title("DPDA · BP 推荐")
    .inner_size(420.0, overlay_height(settings.overlay_compact))
    .min_inner_size(320.0, 240.0)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .build()
    .map_err(error)?;
    // Apply the selected mode after the window-state plugin has restored geometry.
    let width = window
        .inner_size()
        .map_err(error)?
        .to_logical::<f64>(window.scale_factor().map_err(error)?)
        .width;
    window
        .set_size(tauri::LogicalSize::new(
            width,
            overlay_height(settings.overlay_compact),
        ))
        .map_err(error)?;
    if settings.overlay_click_through
        && app
            .global_shortcut()
            .is_registered("CommandOrControl+Shift+F9")
    {
        window.set_ignore_cursor_events(true).map_err(error)?;
    }
    Ok(())
}
#[tauri::command]
fn get_capture_diagnostics(state: State<'_, AppState>) -> Result<Value, String> {
    let rt = state.runtime.lock().map_err(error)?;
    Ok(json!({"draft":rt.tracker.snapshot(),"analysis":rt.analysis,"status":rt.capture}))
}
#[tauri::command]
fn start_capture(state: State<'_, AppState>, app: tauri::AppHandle) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("Windows Graphics Capture 需要 Windows 真机；当前可使用手动阵容".into());
    }
    let settings = settings(&db(&state)?)?;
    let files: CalibrationFiles = serde_json::from_str(&settings.calibration_json)
        .map_err(|_| "请先指定并保存校准文件和头像模板清单".to_string())?;
    state
        .capture_running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .map_err(|_| "识别已经运行".to_string())?;
    state.capture_cancel.store(false, Ordering::SeqCst);
    let runtime = state.runtime.clone();
    let cancel = state.capture_cancel.clone();
    let running = state.capture_running.clone();
    let path = state.path.clone();
    std::thread::spawn(move || {
        let result = (|| -> Result<(), String> {
            let mut recognizer =
                dpda_vision::Recognizer::from_files(&files.profile_path, &files.templates_path)
                    .map_err(error)?;
            let window = dpda_capture::discover_dota_windows()
                .map_err(error)?
                .into_iter()
                .find(|w| !w.minimized)
                .ok_or("找不到可捕获的 DOTA2 窗口")?;

            let session = dpda_capture::CaptureSession::start(dpda_capture::CaptureConfig {
                window_id: window.id,
                region: Some(recognizer.profile().capture_region()),
                interval_ms: (1000.0 / settings.capture_hz) as u64,
            })
            .map_err(error)?;
            {
                let mut rt = runtime.lock().map_err(error)?;
                rt.capture.status = "running".into();
                rt.capture.message = "WGC 识别运行中；只采纳有锁定证据的连续匹配".into();
                rt.capture.window_title = Some(window.title);
            }
            emit(&app);
            let settings_database = Database::open(&path).map_err(error)?;
            let mut last_interval = Instant::now();
            while !cancel.load(Ordering::Relaxed) {
                if last_interval.elapsed() > Duration::from_secs(2) {
                    if let Ok(s) = crate::settings(&settings_database) {
                        session
                            .set_interval((1000.0 / s.capture_hz) as u64)
                            .map_err(error)?;
                    }
                    last_interval = Instant::now();
                }
                match session
                    .recv_timeout(Duration::from_millis(250))
                    .map_err(error)?
                {
                    Some(dpda_capture::CaptureEvent::Frame(frame)) => {
                        let analysis = recognizer.analyze_frame(&frame).map_err(error)?;
                        let mut rt = runtime.lock().map_err(error)?;
                        rt.tracker
                            .apply_frame(
                                analysis.timestamp_ms,
                                analysis.draft_screen_detected,
                                &analysis.observations,
                            )
                            .map_err(error)?;
                        let snapshot = rt.tracker.snapshot();
                        let selected = |slots: Vec<dpda_draft_state::SlotState>| {
                            slots
                                .into_iter()
                                .map(|s| {
                                    if s.recognition_state == RecognitionState::Confirmed {
                                        s.hero_id
                                    } else {
                                        None
                                    }
                                })
                                .collect()
                        };
                        let next = Draft {
                            radiant: selected(snapshot.radiant),
                            dire: selected(snapshot.dire),
                            own_team: rt.draft.own_team.clone(),
                            banned: rt.draft.banned.clone(),
                        };
                        let changed = next != rt.draft;
                        rt.draft = next;
                        rt.capture.capture_ms = frame.capture_ms;
                        rt.capture.recognition_ms = Some(analysis.elapsed_ms);
                        rt.analysis = Some(analysis);
                        drop(rt);
                        if changed {
                            emit(&app);
                        }
                    }
                    Some(dpda_capture::CaptureEvent::WindowLost) => {
                        return Err("游戏窗口已退出，识别已停止".into())
                    }
                    Some(dpda_capture::CaptureEvent::Minimized) => {
                        return Err("游戏已最小化，恢复窗口后重新启动识别".into())
                    }
                    Some(dpda_capture::CaptureEvent::Error(e)) => return Err(e),
                    None => {}
                }
            }
            session.stop().map_err(error)
        })();
        if let Ok(mut rt) = runtime.lock() {
            rt.tracker.window_lost();
            rt.draft = Draft {
                own_team: rt.draft.own_team.clone(),
                ..Default::default()
            };
            rt.capture.status = if result.is_ok() { "idle" } else { "error" }.into();
            rt.capture.message = match result {
                Ok(()) => "识别已停止".into(),
                Err(e) => {
                    tracing::warn!(error=%e,"capture stopped");
                    e
                }
            };
        }
        running.store(false, Ordering::SeqCst);
        emit(&app);
    });
    Ok(())
}
#[tauri::command]
fn stop_capture(state: State<'_, AppState>) {
    state.capture_cancel.store(true, Ordering::SeqCst);
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    let recover: tauri_plugin_global_shortcut::Shortcut =
                        "CommandOrControl+Shift+F9"
                            .parse()
                            .expect("static shortcut");
                    if let Some(window) = app.get_webview_window("overlay") {
                        if shortcut == &recover {
                            let _ = window.set_ignore_cursor_events(false);
                            let _ = window.show();
                            let state = app.state::<AppState>();
                            if let Ok(database) = db(&state) {
                                if let Ok(mut s) = settings(&database) {
                                    s.overlay_click_through = false;
                                    let _ = database.set_setting("app_settings", &s);
                                }
                            }
                            emit(app);
                        } else if window.is_visible().unwrap_or(false) {
                            let _ = window.hide();
                        } else {
                            let _ = window.show();
                        }
                    }
                })
                .build(),
        )
        .setup(|app| {
            let directory = app.path().app_data_dir()?;
            std::fs::create_dir_all(&directory)?;
            let file = tracing_appender::rolling::daily(directory.join("logs"), "dpda.log");
            let _ = tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "dpda=info,warn".into()),
                )
                .with_writer(file)
                .try_init();
            let path = directory.join("dpda.sqlite");
            match Database::open(&path) {
                Ok(database) => {
                    for account in database.profiles()? {
                        if let Some(mut p) = database.checkpoint(account)? {
                            if p.status == "running" {
                                p.status = "paused".into();
                                p.error = Some("上次进程中断，可继续同步".into());
                                database.save_checkpoint(&p)?;
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!(error=%e,"database startup failed; original file preserved")
                }
            }
            app.manage(AppState {
                path,
                runtime: Arc::new(Mutex::new(Runtime::default())),
                sync_cancel: Arc::new(AtomicBool::new(false)),
                sync_running: Arc::new(AtomicBool::new(false)),
                capture_cancel: Arc::new(AtomicBool::new(false)),
                capture_running: Arc::new(AtomicBool::new(false)),
            });
            for shortcut in ["CommandOrControl+Shift+F8", "CommandOrControl+Shift+F9"] {
                if let Err(e) = app.global_shortcut().register(shortcut) {
                    tracing::warn!(error=%e,shortcut,"shortcut unavailable");
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            save_account,
            start_sync,
            pause_sync,
            save_pool,
            update_draft,
            save_settings,
            open_overlay,
            start_capture,
            stop_capture,
            get_capture_diagnostics
        ])
        .build(tauri::generate_context!())
        .expect("DPDA desktop initialization failed")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
                let state = app.state::<AppState>();
                state.sync_cancel.store(true, Ordering::SeqCst);
                state.capture_cancel.store(true, Ordering::SeqCst);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundled_hero_ids_are_unique_and_not_array_indices() {
        let list = heroes().unwrap();
        assert_eq!(list.len(), hero_ids().unwrap().len());
        assert!(list.len() > 120);
        assert!(list.iter().all(|h| h.id > 0));
    }
    #[test]
    fn settings_reject_invalid_numeric_values() {
        let mut s = AppSettings::default();
        assert!(s.validate().is_ok());
        s.capture_hz = f64::NAN;
        assert!(s.validate().is_err());
    }
    #[test]
    fn draft_rejects_duplicates_and_unknown_ids() {
        let mut d = Draft::default();
        d.radiant[0] = Some(1);
        d.dire[0] = Some(1);
        assert!(d.validate(&hero_ids().unwrap()).is_err());
    }
}
