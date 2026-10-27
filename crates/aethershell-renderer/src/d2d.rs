//! Direct2D 1.1 and DirectWrite hardware-accelerated canvas renderer.

use crate::canvas::{CanvasRenderer, RenderCommand, SceneGraph};
use crate::theme_resolver::ColorRgba;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use windows::core::w;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_POINT_2F, D2D_RECT_F,
    D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Factory, ID2D1HwndRenderTarget, ID2D1RenderTarget,
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_MULTI_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
    D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_MEASURING_MODE_NATURAL,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;

/// Direct2D device recreation required HRESULT (0x8899000C)
pub const D2DERR_RECREATE_TARGET: i32 = 0x8899000Cu32 as i32;

/// DXGI device removed HRESULT (0x887A0005)
pub const DXGI_ERROR_DEVICE_REMOVED: i32 = 0x887A0005u32 as i32;

/// DXGI device reset HRESULT (0x887A0007)
pub const DXGI_ERROR_DEVICE_RESET: i32 = 0x887A0007u32 as i32;

/// DXGI device hung HRESULT (0x887A0006)
pub const DXGI_ERROR_DEVICE_HUNG: i32 = 0x887A0006u32 as i32;

/// DXGI driver internal error HRESULT (0x887A0020)
pub const DXGI_ERROR_DRIVER_INTERNAL_ERROR: i32 = 0x887A0020u32 as i32;

/// Evaluates if an HRESULT code indicates GPU device loss or driver reset requiring target recreation.
pub fn is_device_lost_hresult(hr: i32) -> bool {
    hr == D2DERR_RECREATE_TARGET
        || hr == DXGI_ERROR_DEVICE_REMOVED
        || hr == DXGI_ERROR_DEVICE_RESET
        || hr == DXGI_ERROR_DEVICE_HUNG
        || hr == DXGI_ERROR_DRIVER_INTERNAL_ERROR
}

/// Real Direct2D 1.1 / DirectWrite canvas renderer implementing `CanvasRenderer`.
pub struct D2D1CanvasRenderer {
    factory: ID2D1Factory,
    dwrite_factory: IDWriteFactory,
    render_target: Option<ID2D1HwndRenderTarget>,
    hwnd: Option<HWND>,
    width: u32,
    height: u32,
    clear_color: D2D1_COLOR_F,
    text_formats: HashMap<u32, IDWriteTextFormat>,

    // GPU Device Loss & Bounded Recovery State (PRD Section 6 & 18.2)
    is_device_lost: bool,
    consecutive_recovery_failures: u32,
    max_recovery_retries: u32,
    base_backoff_ms: u64,
    max_backoff_ms: u64,
    last_recovery_attempt: Option<Instant>,
    total_device_loss_count: u64,
    total_successful_recoveries: u64,
}

unsafe impl Send for D2D1CanvasRenderer {}
unsafe impl Sync for D2D1CanvasRenderer {}

impl D2D1CanvasRenderer {
    /// Creates a new unattached Direct2D renderer with factories initialized.
    pub fn new() -> Result<Self, String> {
        unsafe {
            let factory: ID2D1Factory =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_MULTI_THREADED, None)
                    .map_err(|e| format!("Failed to create D2D1Factory: {}", e))?;

            let dwrite_factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)
                .map_err(|e| format!("Failed to create DWriteFactory: {}", e))?;

            Ok(Self {
                factory,
                dwrite_factory,
                render_target: None,
                hwnd: None,
                width: 0,
                height: 0,
                clear_color: D2D1_COLOR_F {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.0,
                },
                text_formats: HashMap::new(),
                is_device_lost: false,
                consecutive_recovery_failures: 0,
                max_recovery_retries: 5,
                base_backoff_ms: 50,
                max_backoff_ms: 1000,
                last_recovery_attempt: None,
                total_device_loss_count: 0,
                total_successful_recoveries: 0,
            })
        }
    }

    /// Creates and binds a Direct2D renderer targeting a Win32 HWND.
    pub fn new_for_hwnd(hwnd: HWND, width: u32, height: u32) -> Result<Self, String> {
        let mut renderer = Self::new()?;
        renderer.attach_hwnd(hwnd, width, height)?;
        Ok(renderer)
    }

    /// Attaches or re-creates the HWND render target.
    pub fn attach_hwnd(&mut self, hwnd: HWND, width: u32, height: u32) -> Result<(), String> {
        self.hwnd = Some(hwnd);
        self.width = width;
        self.height = height;

        let rt_props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 0.0,
            dpiY: 0.0,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };

        let hwnd_props = D2D1_HWND_RENDER_TARGET_PROPERTIES {
            hwnd,
            pixelSize: D2D_SIZE_U { width, height },
            presentOptions: D2D1_PRESENT_OPTIONS_NONE,
        };

        unsafe {
            let rt = self
                .factory
                .CreateHwndRenderTarget(&rt_props, &hwnd_props)
                .map_err(|e| format!("Failed to create D2D1 HWND render target: {}", e))?;
            self.render_target = Some(rt);
            self.is_device_lost = false;
        }

        Ok(())
    }

    /// Checks if the underlying GPU device or render target is lost.
    pub fn is_device_lost(&self) -> bool {
        self.is_device_lost || self.render_target.is_none()
    }

    /// Returns the total number of device loss events recorded during the lifetime of this renderer.
    pub fn total_device_loss_count(&self) -> u64 {
        self.total_device_loss_count
    }

    /// Returns the total number of successful target recreations.
    pub fn total_successful_recoveries(&self) -> u64 {
        self.total_successful_recoveries
    }

    /// Returns the number of consecutive recovery failures.
    pub fn consecutive_recovery_failures(&self) -> u32 {
        self.consecutive_recovery_failures
    }

    /// Returns the maximum recovery retries before entering extended cooldown.
    pub fn max_recovery_retries(&self) -> u32 {
        self.max_recovery_retries
    }

    /// Calculates current exponential backoff duration based on consecutive failure count.
    pub fn current_backoff_duration(&self) -> Duration {
        let shift = self.consecutive_recovery_failures.min(6);
        let factor = 1u64 << shift;
        let ms = (self.base_backoff_ms * factor).min(self.max_backoff_ms);
        Duration::from_millis(ms)
    }

    /// Simulates a GPU device loss event for testing.
    pub fn simulate_device_loss_for_test(&mut self) {
        self.render_target = None;
        self.is_device_lost = true;
        self.total_device_loss_count += 1;
    }

    /// Recreates the render target following device loss, enforcing bounded retries and backoff.
    pub fn recover_device(&mut self) -> Result<(), String> {
        self.recover_device_impl()
    }

    fn recover_device_impl(&mut self) -> Result<(), String> {
        let hwnd = match self.hwnd {
            Some(h) if !h.0.is_null() => h,
            _ => {
                return Err("Cannot recover Direct2D device: no valid HWND attached".into());
            }
        };

        // Enforce maximum retry limit and extended backoff cooldown
        if self.consecutive_recovery_failures >= self.max_recovery_retries {
            if let Some(last_attempt) = self.last_recovery_attempt {
                let extended_cooldown = Duration::from_millis(self.max_backoff_ms);
                if last_attempt.elapsed() < extended_cooldown {
                    return Err(format!(
                        "Direct2D device recovery reached maximum retry limit ({}); in extended cooldown ({:?} remaining)",
                        self.max_recovery_retries,
                        extended_cooldown.saturating_sub(last_attempt.elapsed())
                    ));
                }
            }
        }

        // Enforce exponential backoff if prior attempts failed
        if self.consecutive_recovery_failures > 0 {
            if let Some(last_attempt) = self.last_recovery_attempt {
                let required_backoff = self.current_backoff_duration();
                if last_attempt.elapsed() < required_backoff {
                    return Err(format!(
                        "Direct2D device recovery in backoff cooldown ({:?} remaining)",
                        required_backoff.saturating_sub(last_attempt.elapsed())
                    ));
                }
            }
        }

        self.last_recovery_attempt = Some(Instant::now());

        // Drop any stale target before recreating
        self.render_target = None;

        match self.attach_hwnd(hwnd, self.width, self.height) {
            Ok(()) => {
                self.is_device_lost = false;
                self.consecutive_recovery_failures = 0;
                self.total_successful_recoveries += 1;
                tracing::info!(
                    "Direct2D render target recovered successfully from GPU device loss (total recoveries: {})",
                    self.total_successful_recoveries
                );
                Ok(())
            }
            Err(e) => {
                self.consecutive_recovery_failures =
                    self.consecutive_recovery_failures.saturating_add(1);
                self.is_device_lost = true;
                tracing::warn!(
                    "Direct2D device recovery attempt failed (consecutive failures: {}): {}",
                    self.consecutive_recovery_failures,
                    e
                );
                Err(e)
            }
        }
    }

    /// Resizes the underlying Direct2D render target on window resize or DPI change.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        self.width = width;
        self.height = height;
        if self.is_device_lost() {
            return self.recover_device();
        }
        if let Some(ref rt) = self.render_target {
            unsafe {
                if let Err(e) = rt.Resize(&D2D_SIZE_U { width, height }) {
                    let hr = e.code().0;
                    if is_device_lost_hresult(hr) {
                        tracing::warn!(
                            "Direct2D Resize encountered GPU device loss (hr=0x{:08X}): {}",
                            hr as u32,
                            e
                        );
                        self.render_target = None;
                        self.is_device_lost = true;
                        self.total_device_loss_count += 1;
                        return self.recover_device();
                    }
                    return Err(format!("D2D1 Resize failed: {}", e));
                }
            }
        } else {
            return self.recover_device();
        }
        Ok(())
    }

    /// Sets the canvas clear color (transparent or semi-transparent background).
    pub fn set_clear_color(&mut self, color: ColorRgba) {
        self.clear_color = to_d2d_color(&color);
    }

    /// Returns the active clear color.
    pub fn clear_color(&self) -> D2D1_COLOR_F {
        self.clear_color
    }

    /// Checks if a render target is currently active.
    pub fn is_attached(&self) -> bool {
        self.render_target.is_some()
    }

    fn get_or_create_text_format(
        &mut self,
        font_size_dip: f32,
    ) -> Result<IDWriteTextFormat, String> {
        let key = (font_size_dip.max(1.0) * 10.0) as u32;
        if let Some(format) = self.text_formats.get(&key) {
            return Ok(format.clone());
        }

        unsafe {
            let format = self
                .dwrite_factory
                .CreateTextFormat(
                    w!("Segoe UI"),
                    None,
                    DWRITE_FONT_WEIGHT_NORMAL,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    font_size_dip.max(1.0),
                    w!("en-us"),
                )
                .map_err(|e| format!("CreateTextFormat failed: {}", e))?;

            self.text_formats.insert(key, format.clone());
            Ok(format)
        }
    }
}

fn to_d2d_color(c: &ColorRgba) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: c.r as f32 / 255.0,
        g: c.g as f32 / 255.0,
        b: c.b as f32 / 255.0,
        a: c.a as f32 / 255.0,
    }
}

impl CanvasRenderer for D2D1CanvasRenderer {
    fn is_device_lost(&self) -> bool {
        self.is_device_lost()
    }

    fn recover_device(&mut self) -> Result<(), String> {
        self.recover_device_impl()
    }

    fn render_scene(&mut self, scene: &SceneGraph) -> Result<usize, String> {
        // If device was previously marked lost, attempt recovery before drawing
        if self.is_device_lost() {
            self.recover_device()?;
        }

        match self.render_scene_internal(scene) {
            Ok(count) => Ok(count),
            Err(e) => {
                if self.is_device_lost {
                    // Exactly 1 bounded recovery attempt after drawing failure, avoiding unbounded recursion
                    tracing::info!("Direct2D attempting bounded recovery after render failure");
                    if self.recover_device().is_ok() {
                        return self.render_scene_internal(scene);
                    }
                }
                Err(e)
            }
        }
    }
}

impl D2D1CanvasRenderer {
    fn render_scene_internal(&mut self, scene: &SceneGraph) -> Result<usize, String> {
        let hwnd_rt = self
            .render_target
            .clone()
            .ok_or_else(|| "No active render target attached".to_string())?;

        unsafe {
            use std::ops::Deref;
            let rt: &ID2D1RenderTarget = hwnd_rt.deref();
            rt.BeginDraw();
            rt.Clear(Some(&self.clear_color as *const _));

            let mut drawn_count = 0;

            for cmd in &scene.commands {
                match cmd {
                    RenderCommand::DrawRect {
                        x,
                        y,
                        width,
                        height,
                        color,
                        radius,
                        border_color,
                    } => {
                        let rect = D2D_RECT_F {
                            left: *x,
                            top: *y,
                            right: *x + *width,
                            bottom: *y + *height,
                        };
                        let fill_brush = rt
                            .CreateSolidColorBrush(&to_d2d_color(color) as *const _, None)
                            .map_err(|e| format!("CreateSolidColorBrush error: {}", e))?;

                        if *radius > 0.0 {
                            let rounded = D2D1_ROUNDED_RECT {
                                rect,
                                radiusX: *radius,
                                radiusY: *radius,
                            };
                            rt.FillRoundedRectangle(&rounded, &fill_brush);
                            if let Some(bc) = border_color {
                                let border_brush = rt
                                    .CreateSolidColorBrush(&to_d2d_color(bc) as *const _, None)
                                    .map_err(|e| format!("Border brush error: {}", e))?;
                                rt.DrawRoundedRectangle(&rounded, &border_brush, 1.0, None);
                            }
                        } else {
                            rt.FillRectangle(&rect, &fill_brush);
                            if let Some(bc) = border_color {
                                let border_brush = rt
                                    .CreateSolidColorBrush(&to_d2d_color(bc) as *const _, None)
                                    .map_err(|e| format!("Border brush error: {}", e))?;
                                rt.DrawRectangle(&rect, &border_brush, 1.0, None);
                            }
                        }
                        drawn_count += 1;
                    }
                    RenderCommand::DrawText {
                        text,
                        x,
                        y,
                        font_size_dip,
                        color,
                    } => {
                        let text_format = self.get_or_create_text_format(*font_size_dip)?;
                        let text_brush = rt
                            .CreateSolidColorBrush(&to_d2d_color(color) as *const _, None)
                            .map_err(|e| format!("Text brush error: {}", e))?;

                        let rect = D2D_RECT_F {
                            left: *x,
                            top: *y,
                            right: *x + 2000.0,
                            bottom: *y + font_size_dip * 3.0,
                        };

                        let wide_text: Vec<u16> = text.encode_utf16().collect();
                        rt.DrawText(
                            &wide_text,
                            &text_format,
                            &rect,
                            &text_brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                        drawn_count += 1;
                    }
                    RenderCommand::DrawProgressBar {
                        x,
                        y,
                        width,
                        height,
                        progress,
                        fill,
                        track,
                        radius,
                    } => {
                        let track_rect = D2D_RECT_F {
                            left: *x,
                            top: *y,
                            right: *x + *width,
                            bottom: *y + *height,
                        };
                        let track_brush = rt
                            .CreateSolidColorBrush(&to_d2d_color(track) as *const _, None)
                            .map_err(|e| format!("Track brush error: {}", e))?;

                        if *radius > 0.0 {
                            rt.FillRoundedRectangle(
                                &D2D1_ROUNDED_RECT {
                                    rect: track_rect,
                                    radiusX: *radius,
                                    radiusY: *radius,
                                },
                                &track_brush,
                            );
                        } else {
                            rt.FillRectangle(&track_rect, &track_brush);
                        }

                        let fill_w = (*width * progress.clamp(0.0, 1.0)).max(0.0);
                        if fill_w > 0.0 {
                            let fill_rect = D2D_RECT_F {
                                left: *x,
                                top: *y,
                                right: *x + fill_w,
                                bottom: *y + *height,
                            };
                            let fill_brush = rt
                                .CreateSolidColorBrush(&to_d2d_color(fill) as *const _, None)
                                .map_err(|e| format!("Fill brush error: {}", e))?;

                            if *radius > 0.0 {
                                rt.FillRoundedRectangle(
                                    &D2D1_ROUNDED_RECT {
                                        rect: fill_rect,
                                        radiusX: *radius,
                                        radiusY: *radius,
                                    },
                                    &fill_brush,
                                );
                            } else {
                                rt.FillRectangle(&fill_rect, &fill_brush);
                            }
                        }
                        drawn_count += 1;
                    }
                    RenderCommand::DrawSpectrum {
                        x,
                        y,
                        width,
                        height,
                        bands,
                        color,
                    } => {
                        if !bands.is_empty() {
                            let spec_brush = rt
                                .CreateSolidColorBrush(&to_d2d_color(color) as *const _, None)
                                .map_err(|e| format!("Spectrum brush error: {}", e))?;
                            let gap = 2.0f32;
                            let total_gaps = gap * (bands.len() - 1) as f32;
                            let band_w = ((*width - total_gaps) / bands.len() as f32).max(1.0);

                            for (i, &band) in bands.iter().enumerate() {
                                let bar_h = (*height * band.clamp(0.0, 1.0)).max(2.0);
                                let bx = *x + (i as f32) * (band_w + gap);
                                let by = *y + *height - bar_h;
                                let bar_rect = D2D_RECT_F {
                                    left: bx,
                                    top: by,
                                    right: bx + band_w,
                                    bottom: *y + *height,
                                };
                                let rounded = D2D1_ROUNDED_RECT {
                                    rect: bar_rect,
                                    radiusX: 1.0,
                                    radiusY: 1.0,
                                };
                                rt.FillRoundedRectangle(&rounded, &spec_brush);
                            }
                            drawn_count += 1;
                        }
                    }
                    RenderCommand::DrawLine {
                        x1,
                        y1,
                        x2,
                        y2,
                        color,
                        stroke_width,
                    } => {
                        let stroke_brush = rt
                            .CreateSolidColorBrush(&to_d2d_color(color) as *const _, None)
                            .map_err(|e| format!("Stroke brush error: {}", e))?;
                        rt.DrawLine(
                            D2D_POINT_2F { x: *x1, y: *y1 },
                            D2D_POINT_2F { x: *x2, y: *y2 },
                            &stroke_brush,
                            *stroke_width,
                            None,
                        );
                        drawn_count += 1;
                    }
                }
            }

            let end_draw_res = rt.EndDraw(None, None);
            if let Err(e) = end_draw_res {
                let hr = e.code().0;
                if is_device_lost_hresult(hr) {
                    tracing::warn!(
                        "Direct2D EndDraw encountered GPU device loss (hr=0x{:08X}): {}",
                        hr as u32,
                        e
                    );
                    drop(hwnd_rt);
                    self.render_target = None;
                    self.is_device_lost = true;
                    self.total_device_loss_count += 1;
                    return Err(format!("Direct2D device lost (hr=0x{:08X})", hr as u32));
                }
                return Err(format!("EndDraw error: {}", e));
            }
            Ok(drawn_count)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_d2d_factory_initialization() {
        let renderer = D2D1CanvasRenderer::new();
        assert!(
            renderer.is_ok(),
            "D2D1Factory should initialize on Windows: {:?}",
            renderer.err()
        );
        let r = renderer.unwrap();
        assert!(!r.is_attached());
    }

    #[test]
    fn test_d2d_clear_color() {
        let mut r = D2D1CanvasRenderer::new().unwrap();
        r.set_clear_color(ColorRgba::new(10, 20, 30, 40));
        let c = r.clear_color();
        assert!((c.r - 10.0 / 255.0).abs() < 0.001);
        assert!((c.g - 20.0 / 255.0).abs() < 0.001);
        assert!((c.b - 30.0 / 255.0).abs() < 0.001);
        assert!((c.a - 40.0 / 255.0).abs() < 0.001);
    }

    #[test]
    fn test_d2d_rendering_to_test_window() {
        use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassExW, CS_HREDRAW,
            CS_VREDRAW, WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_POPUP,
        };

        unsafe extern "system" fn test_wnd_proc(
            hwnd: HWND,
            msg: u32,
            wparam: WPARAM,
            lparam: LPARAM,
        ) -> LRESULT {
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }

        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let class_name = w!("AetherShellD2DTestClass");
            let wnd_class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(test_wnd_proc),
                hInstance: hinstance.into(),
                lpszClassName: class_name,
                ..Default::default()
            };
            let _ = RegisterClassExW(&wnd_class);

            let hwnd_res = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                class_name,
                w!("D2D Test"),
                WS_POPUP,
                0,
                0,
                200,
                100,
                None,
                None,
                hinstance,
                None,
            );

            if let Ok(hwnd) = hwnd_res {
                if !hwnd.0.is_null() {
                    let mut renderer = D2D1CanvasRenderer::new_for_hwnd(hwnd, 200, 100)
                        .expect("Failed to create D2D1 for HWND");
                    assert!(renderer.is_attached());

                    let scene = SceneGraph {
                        commands: vec![
                            RenderCommand::DrawRect {
                                x: 10.0,
                                y: 10.0,
                                width: 80.0,
                                height: 40.0,
                                color: ColorRgba::new(255, 0, 0, 255),
                                radius: 4.0,
                                border_color: Some(ColorRgba::new(255, 255, 255, 255)),
                            },
                            RenderCommand::DrawText {
                                text: "D2D Test".into(),
                                x: 12.0,
                                y: 15.0,
                                font_size_dip: 12.0,
                                color: ColorRgba::new(255, 255, 255, 255),
                            },
                            RenderCommand::DrawProgressBar {
                                x: 10.0,
                                y: 60.0,
                                width: 100.0,
                                height: 8.0,
                                progress: 0.5,
                                fill: ColorRgba::new(0, 255, 0, 255),
                                track: ColorRgba::new(50, 50, 50, 255),
                                radius: 2.0,
                            },
                            RenderCommand::DrawSpectrum {
                                x: 10.0,
                                y: 75.0,
                                width: 100.0,
                                height: 20.0,
                                bands: vec![0.3, 0.6, 0.9, 0.4],
                                color: ColorRgba::new(0, 150, 255, 255),
                            },
                            RenderCommand::DrawLine {
                                x1: 5.0,
                                y1: 5.0,
                                x2: 195.0,
                                y2: 5.0,
                                color: ColorRgba::new(200, 200, 200, 255),
                                stroke_width: 2.0,
                            },
                        ],
                    };

                    let drawn = renderer.render_scene(&scene).expect("Render scene failed");
                    assert_eq!(drawn, 5);

                    renderer.resize(250, 150).expect("Resize should succeed");

                    // Exercise GPU device loss simulation & recovery round-trip
                    renderer.simulate_device_loss_for_test();
                    assert!(renderer.is_device_lost());
                    assert_eq!(renderer.total_device_loss_count(), 1);
                    assert_eq!(renderer.total_successful_recoveries(), 0);

                    // Automatic bounded recovery inside render_scene
                    let drawn2 = renderer
                        .render_scene(&scene)
                        .expect("Render scene should automatically recover from device loss");
                    assert_eq!(drawn2, 5);
                    assert!(!renderer.is_device_lost());
                    assert_eq!(renderer.total_successful_recoveries(), 1);

                    let _ = DestroyWindow(hwnd);
                }
            }
        }
    }

    #[test]
    fn test_device_lost_hresult_classification() {
        assert!(is_device_lost_hresult(D2DERR_RECREATE_TARGET));
        assert!(is_device_lost_hresult(DXGI_ERROR_DEVICE_REMOVED));
        assert!(is_device_lost_hresult(DXGI_ERROR_DEVICE_RESET));
        assert!(is_device_lost_hresult(DXGI_ERROR_DEVICE_HUNG));
        assert!(is_device_lost_hresult(DXGI_ERROR_DRIVER_INTERNAL_ERROR));

        // Normal success or other errors must NOT be classified as device loss
        assert!(!is_device_lost_hresult(0)); // S_OK
        assert!(!is_device_lost_hresult(0x80004005u32 as i32)); // E_FAIL
        assert!(!is_device_lost_hresult(0x80070057u32 as i32)); // E_INVALIDARG
    }

    #[test]
    fn test_device_recovery_exponential_backoff_calculation() {
        let mut r = D2D1CanvasRenderer::new().unwrap();
        assert_eq!(r.current_backoff_duration(), Duration::from_millis(50));

        r.consecutive_recovery_failures = 1;
        assert_eq!(r.current_backoff_duration(), Duration::from_millis(100));

        r.consecutive_recovery_failures = 2;
        assert_eq!(r.current_backoff_duration(), Duration::from_millis(200));

        r.consecutive_recovery_failures = 3;
        assert_eq!(r.current_backoff_duration(), Duration::from_millis(400));

        r.consecutive_recovery_failures = 4;
        assert_eq!(r.current_backoff_duration(), Duration::from_millis(800));

        // Capped at max_backoff_ms (1000ms)
        r.consecutive_recovery_failures = 5;
        assert_eq!(r.current_backoff_duration(), Duration::from_millis(1000));

        r.consecutive_recovery_failures = 10;
        assert_eq!(r.current_backoff_duration(), Duration::from_millis(1000));
    }

    #[test]
    fn test_device_recovery_fails_gracefully_without_hwnd() {
        let mut r = D2D1CanvasRenderer::new().unwrap();
        r.simulate_device_loss_for_test();
        assert!(r.is_device_lost());

        let res = r.recover_device();
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("no valid HWND attached"));
        assert!(r.is_device_lost());
    }
}
