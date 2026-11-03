use super::*;
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use windows::{
    Win32::{
        Foundation::{CloseHandle, HWND},
        System::Threading::{
            OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        },
        UI::{
            HiDpi::GetDpiForWindow,
            WindowsAndMessaging::{IsIconic, IsWindow},
        },
    },
    core::PWSTR,
};
use windows_capture::{
    capture::{Context, GraphicsCaptureApiHandler},
    frame::Frame,
    graphics_capture_api::InternalCaptureControl,
    settings::{
        ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
        MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
    },
    window::Window,
};

fn failure(error: impl std::fmt::Display) -> CaptureError {
    CaptureError::Windows(error.to_string())
}

// This deliberately avoids Window::process_name(), which requests PROCESS_VM_READ.
fn is_dota_process(window: &Window) -> bool {
    let Ok(pid) = window.process_id() else {
        return false;
    };
    // SAFETY: read-only process metadata; the handle is closed on every path.
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let mut buffer = vec![0u16; 32_768];
        let mut size = buffer.len() as u32;
        let result = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(handle);
        if result.is_err() {
            return false;
        }
        let executable = String::from_utf16_lossy(&buffer[..size as usize]);
        Path::new(&executable)
            .file_name()
            .is_some_and(|s| s.to_string_lossy().eq_ignore_ascii_case("dota2.exe"))
    }
}

pub fn discover_dota_windows() -> Result<Vec<WindowInfo>, CaptureError> {
    let mut windows = vec![];
    for window in Window::enumerate().map_err(failure)? {
        if !window.is_valid()
            || window.title().ok().as_deref() != Some("Dota 2")
            || !is_dota_process(&window)
        {
            continue;
        }
        let rect = window.rect().map_err(failure)?;
        let hwnd = HWND(window.as_raw_hwnd());
        windows.push(WindowInfo {
            id: (window.as_raw_hwnd() as usize).to_string(),
            title: "Dota 2".into(),
            width: rect.right.saturating_sub(rect.left) as u32,
            height: rect.bottom.saturating_sub(rect.top) as u32,
            dpi: unsafe { GetDpiForWindow(hwnd) },
            minimized: unsafe { IsIconic(hwnd).as_bool() },
        });
    }
    Ok(windows)
}

struct Flags {
    config: CaptureConfig,
    sender: SyncSender<CaptureEvent>,
    interval: Arc<AtomicU64>,
    hwnd_value: usize,
}

struct Handler {
    flags: Flags,
    last_sample: Option<Instant>,
}

impl GraphicsCaptureApiHandler for Handler {
    type Flags = Flags;
    type Error = CaptureError;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self {
            flags: ctx.flags,
            last_sample: None,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let hwnd = HWND(self.flags.hwnd_value as *mut _);
        if unsafe { IsIconic(hwnd).as_bool() } {
            let _ = self.flags.sender.try_send(CaptureEvent::Minimized);
            control.stop();
            return Ok(());
        }
        let period = Duration::from_millis(self.flags.interval.load(Ordering::Relaxed));
        if self.last_sample.is_some_and(|t| t.elapsed() < period) {
            return Ok(());
        }
        self.last_sample = Some(Instant::now());
        let (surface_width, surface_height) = (frame.width(), frame.height());
        let region = self.flags.config.region.unwrap_or(Rect {
            x: 0,
            y: 0,
            width: surface_width,
            height: surface_height,
        });
        if !region.fits(surface_width, surface_height) {
            let _ = self.flags.sender.try_send(CaptureEvent::Error(
                "窗口尺寸变化后区域越界，请重新校准".into(),
            ));
            control.stop();
            return Ok(());
        }
        // Copy only the configured ROI from GPU to CPU. No full-screen screenshot.
        let readback_started = Instant::now();
        let buffer = frame
            .buffer_crop(
                region.x,
                region.y,
                region.x + region.width,
                region.y + region.height,
            )
            .map_err(failure)?;
        let mut scratch = Vec::new();
        let rgba = buffer.as_nopadding_buffer(&mut scratch).to_vec();
        let captured = CaptureFrame {
            width: region.width,
            height: region.height,
            surface_width,
            surface_height,
            origin_x: region.x,
            origin_y: region.y,
            dpi: unsafe { GetDpiForWindow(hwnd) },
            timestamp_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            capture_ms: Some(readback_started.elapsed().as_secs_f64() * 1_000.0),
            rgba,
        };
        captured.validate()?;
        match self.flags.sender.try_send(CaptureEvent::Frame(captured)) {
            Ok(()) | Err(mpsc::TrySendError::Full(_)) => (),
            Err(mpsc::TrySendError::Disconnected(_)) => control.stop(),
        }
        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        let _ = self.flags.sender.try_send(CaptureEvent::WindowLost);
        Ok(())
    }
}

pub struct CaptureSession {
    receiver: Receiver<CaptureEvent>,
    interval: Arc<AtomicU64>,
    stop_requested: Arc<AtomicBool>,
    monitor: Option<JoinHandle<Result<(), CaptureError>>>,
}

impl CaptureSession {
    /// Call on a worker thread. Session creation may initialize D3D/WinRT.
    pub fn start(config: CaptureConfig) -> Result<Self, CaptureError> {
        config.validate()?;
        let window = discover_dota_windows()?
            .into_iter()
            .find(|w| w.id == config.window_id)
            .ok_or(CaptureError::WindowNotFound)?;
        if window.minimized {
            return Err(CaptureError::Minimized);
        }
        let hwnd_value = window.id.parse::<usize>().map_err(failure)?;
        let capture_window = Window::from_raw_hwnd(hwnd_value as *mut _);
        let (sender, receiver) = mpsc::sync_channel(2);
        let interval = Arc::new(AtomicU64::new(config.interval_ms));
        let flags = Flags {
            config,
            sender: sender.clone(),
            interval: interval.clone(),
            hwnd_value,
        };
        let settings = Settings::new(
            capture_window,
            CursorCaptureSettings::WithoutCursor,
            DrawBorderSettings::Default,
            SecondaryWindowSettings::Default,
            // Custom OS-side interval is unavailable on older Windows builds.
            // Software throttle runs before GPU readback on every supported build.
            MinimumUpdateIntervalSettings::Default,
            DirtyRegionSettings::Default,
            ColorFormat::Rgba8,
            flags,
        );
        let control = Handler::start_free_threaded(settings).map_err(failure)?;
        let stop_requested = Arc::new(AtomicBool::new(false));
        let stop_worker = stop_requested.clone();
        let monitor = thread::spawn(move || {
            while !control.is_finished() && !stop_worker.load(Ordering::Relaxed) {
                let hwnd = HWND(hwnd_value as *mut _);
                if !unsafe { IsWindow(Some(hwnd)).as_bool() } {
                    let _ = sender.try_send(CaptureEvent::WindowLost);
                    break;
                }
                if unsafe { IsIconic(hwnd).as_bool() } {
                    let _ = sender.try_send(CaptureEvent::Minimized);
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
            let result = control.stop().map_err(failure);
            if let Err(error) = &result {
                let _ = sender.try_send(CaptureEvent::Error(error.to_string()));
            }
            result
        });
        Ok(Self {
            receiver,
            interval,
            stop_requested,
            monitor: Some(monitor),
        })
    }

    pub fn try_recv(&self) -> Result<Option<CaptureEvent>, CaptureError> {
        match self.receiver.try_recv() {
            Ok(event) => Ok(Some(event)),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => Err(CaptureError::Stopped),
        }
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Result<Option<CaptureEvent>, CaptureError> {
        match self.receiver.recv_timeout(timeout) {
            Ok(event) => Ok(Some(event)),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(CaptureError::Stopped),
        }
    }

    pub fn set_interval(&self, interval_ms: u64) -> Result<(), CaptureError> {
        if !(250..=10_000).contains(&interval_ms) {
            return Err(CaptureError::InvalidConfig(
                "采样间隔须为 250..10000 ms".into(),
            ));
        }
        self.interval.store(interval_ms, Ordering::Relaxed);
        Ok(())
    }

    pub fn stop(mut self) -> Result<(), CaptureError> {
        self.stop_inner()
    }

    fn stop_inner(&mut self) -> Result<(), CaptureError> {
        self.stop_requested.store(true, Ordering::Relaxed);
        if let Some(worker) = self.monitor.take() {
            worker
                .join()
                .map_err(|_| CaptureError::Windows("捕获线程异常退出".into()))??;
        }
        Ok(())
    }
}

impl Drop for CaptureSession {
    fn drop(&mut self) {
        let _ = self.stop_inner();
    }
}
