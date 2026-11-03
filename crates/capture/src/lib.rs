//! Windows Graphics Capture adapter. Other platforms return an explicit error.
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub fn fits(self, width: u32, height: u32) -> bool {
        self.width > 0
            && self.height > 0
            && self.x.checked_add(self.width).is_some_and(|r| r <= width)
            && self.y.checked_add(self.height).is_some_and(|b| b <= height)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WindowInfo {
    /// String avoids pointer/integer precision loss in JavaScript.
    pub id: String,
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub dpi: u32,
    pub minimized: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CaptureConfig {
    pub window_id: String,
    /// Pixel coordinates inside the captured WGC surface, including window borders.
    pub region: Option<Rect>,
    pub interval_ms: u64,
}

impl CaptureConfig {
    pub fn validate(&self) -> Result<(), CaptureError> {
        if !(250..=10_000).contains(&self.interval_ms) {
            return Err(CaptureError::InvalidConfig(
                "采样间隔须为 250..10000 ms".into(),
            ));
        }
        if self.region.is_some_and(|r| r.width == 0 || r.height == 0) {
            return Err(CaptureError::InvalidConfig("捕获区域不能为空".into()));
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct CaptureFrame {
    pub width: u32,
    pub height: u32,
    pub surface_width: u32,
    pub surface_height: u32,
    pub origin_x: u32,
    pub origin_y: u32,
    pub dpi: u32,
    pub timestamp_ms: u64,
    /// Measured GPU ROI readback + tight-buffer copy; not end-to-end capture latency.
    pub capture_ms: Option<f64>,
    /// Tightly packed RGBA8, not D3D row-pitched memory.
    pub rgba: Vec<u8>,
}

impl CaptureFrame {
    pub fn validate(&self) -> Result<(), CaptureError> {
        let size = (self.width as usize)
            .checked_mul(self.height as usize)
            .and_then(|n| n.checked_mul(4));
        if self.width == 0 || self.height == 0 || size != Some(self.rgba.len()) {
            return Err(CaptureError::InvalidFrame);
        }
        if !(Rect {
            x: self.origin_x,
            y: self.origin_y,
            width: self.width,
            height: self.height,
        })
        .fits(self.surface_width, self.surface_height)
        {
            return Err(CaptureError::InvalidFrame);
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub enum CaptureEvent {
    Frame(CaptureFrame),
    WindowLost,
    Minimized,
    Error(String),
}

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("Windows Graphics Capture 仅在 Windows 上可用")]
    UnsupportedPlatform,
    #[error("未找到可见的 Dota 2 游戏窗口")]
    WindowNotFound,
    #[error("游戏窗口已最小化；恢复后重新启动捕获")]
    Minimized,
    #[error("捕获配置无效：{0}")]
    InvalidConfig(String),
    #[error("捕获像素尺寸或缓冲区长度无效")]
    InvalidFrame,
    #[error("Windows 捕获失败：{0}")]
    Windows(String),
    #[error("捕获会话已结束")]
    Stopped,
}

#[cfg(windows)]
mod windows_impl;
#[cfg(windows)]
pub use windows_impl::{CaptureSession, discover_dota_windows};

#[cfg(not(windows))]
pub fn discover_dota_windows() -> Result<Vec<WindowInfo>, CaptureError> {
    Err(CaptureError::UnsupportedPlatform)
}

#[cfg(not(windows))]
pub struct CaptureSession;

#[cfg(not(windows))]
impl CaptureSession {
    pub fn start(config: CaptureConfig) -> Result<Self, CaptureError> {
        config.validate()?;
        Err(CaptureError::UnsupportedPlatform)
    }
    pub fn try_recv(&self) -> Result<Option<CaptureEvent>, CaptureError> {
        Err(CaptureError::UnsupportedPlatform)
    }
    pub fn recv_timeout(&self, _timeout: Duration) -> Result<Option<CaptureEvent>, CaptureError> {
        Err(CaptureError::UnsupportedPlatform)
    }
    pub fn set_interval(&self, _interval_ms: u64) -> Result<(), CaptureError> {
        Err(CaptureError::UnsupportedPlatform)
    }
    pub fn stop(self) -> Result<(), CaptureError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_rect_and_overflow_are_rejected() {
        assert!(
            !Rect {
                x: u32::MAX,
                y: 0,
                width: 20,
                height: 10
            }
            .fits(1920, 1080)
        );
        assert!(
            !Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 10
            }
            .fits(1920, 1080)
        );
        assert!(
            Rect {
                x: 20,
                y: 30,
                width: 40,
                height: 50
            }
            .fits(1920, 1080)
        );
    }

    #[test]
    fn malformed_frame_is_rejected() {
        let frame = CaptureFrame {
            width: 4,
            height: 4,
            surface_width: 4,
            surface_height: 4,
            origin_x: 0,
            origin_y: 0,
            dpi: 96,
            timestamp_ms: 1,
            capture_ms: None,
            rgba: vec![0; 63],
        };
        assert!(frame.validate().is_err());
    }
}
