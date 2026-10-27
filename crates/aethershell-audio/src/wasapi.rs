use crate::notification::AudioEndpointNotificationClient;
use crate::spectrum::AudioSpectrumProcessor;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing::{info, warn};
use windows::core::GUID;
use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator,
    IMMNotificationClient, MMDeviceEnumerator, AUDCLNT_BUFFERFLAGS_SILENT,
    AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, WAVEFORMATEXTENSIBLE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_MULTITHREADED,
};

const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
const KSDATAFORMAT_SUBTYPE_IEEE_FLOAT: GUID =
    GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);

/// Adaptive presentation frame rate modes per PRD Section 7.3 and Q36:A.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum FpsMode {
    Performance = 0,  // 60 FPS
    AcBalanced = 1,   // 30 FPS (default on AC power)
    BatterySaver = 2, // 15 FPS (automatic on battery power)
}

impl FpsMode {
    pub fn from_u8(val: u8) -> Self {
        match val {
            0 => FpsMode::Performance,
            2 => FpsMode::BatterySaver,
            _ => FpsMode::AcBalanced,
        }
    }

    pub fn target_fps(&self) -> u32 {
        match self {
            FpsMode::Performance => 60,
            FpsMode::AcBalanced => 30,
            FpsMode::BatterySaver => 15,
        }
    }

    pub fn frame_interval(&self) -> Duration {
        Duration::from_millis((1000.0 / self.target_fps() as f64).round() as u64)
    }

    /// Active polling interval when audio packets are actively flowing.
    pub fn active_poll_interval(&self) -> Duration {
        match self {
            FpsMode::Performance => Duration::from_millis(10),
            FpsMode::AcBalanced => Duration::from_millis(15),
            FpsMode::BatterySaver => Duration::from_millis(33),
        }
    }

    /// Relaxed polling interval during light silence (< 2 seconds).
    pub fn idle_poll_interval(&self) -> Duration {
        match self {
            FpsMode::Performance => Duration::from_millis(33),
            FpsMode::AcBalanced => Duration::from_millis(50),
            FpsMode::BatterySaver => Duration::from_millis(100),
        }
    }

    /// Non-waking deep silence polling interval (> 2 seconds of silence) to minimize battery drain.
    pub fn deep_silence_poll_interval(&self) -> Duration {
        match self {
            FpsMode::Performance => Duration::from_millis(50),
            FpsMode::AcBalanced => Duration::from_millis(100),
            FpsMode::BatterySaver => Duration::from_millis(200),
        }
    }
}

/// WASAPI shared loopback audio capture controller with dynamic `IMMNotificationClient` device migration.
pub struct WasapiLoopbackCapture {
    fps_mode: Arc<AtomicU8>,
    is_paused: Arc<AtomicBool>,
    is_running: Arc<AtomicBool>,
    migration_requested: Arc<AtomicBool>,
    migration_counter: Arc<AtomicUsize>,
    processor: Arc<Mutex<AudioSpectrumProcessor>>,
    capture_thread: Option<std::thread::JoinHandle<()>>,
}

impl WasapiLoopbackCapture {
    pub fn new(band_count: usize, fps_mode: FpsMode) -> Self {
        Self {
            fps_mode: Arc::new(AtomicU8::new(fps_mode as u8)),
            is_paused: Arc::new(AtomicBool::new(false)),
            is_running: Arc::new(AtomicBool::new(false)),
            migration_requested: Arc::new(AtomicBool::new(false)),
            migration_counter: Arc::new(AtomicUsize::new(0)),
            processor: Arc::new(Mutex::new(AudioSpectrumProcessor::new(band_count))),
            capture_thread: None,
        }
    }

    pub fn fps_mode(&self) -> FpsMode {
        FpsMode::from_u8(self.fps_mode.load(Ordering::Relaxed))
    }

    pub fn set_fps_mode(&self, mode: FpsMode) {
        self.fps_mode.store(mode as u8, Ordering::SeqCst);
    }

    pub fn set_paused(&self, paused: bool) {
        self.is_paused.store(paused, Ordering::SeqCst);
    }

    pub fn is_paused(&self) -> bool {
        self.is_paused.load(Ordering::Relaxed)
    }

    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::Relaxed)
    }

    pub fn migration_counter(&self) -> usize {
        self.migration_counter.load(Ordering::Relaxed)
    }

    pub fn is_migration_requested(&self) -> bool {
        self.migration_requested.load(Ordering::Relaxed)
    }

    /// Programmatically signals that the default audio endpoint has changed or been invalidated.
    pub fn request_migration(&self) {
        self.migration_requested.store(true, Ordering::SeqCst);
        self.migration_counter.fetch_add(1, Ordering::SeqCst);
    }

    /// Spawns the genuine Windows WASAPI shared-loopback audio capture background thread.
    ///
    /// If no active audio endpoint exists (e.g. headless CI / VM), gracefully enters fallback mode without panicking.
    pub fn start(&mut self) -> Result<(), String> {
        if self.is_running.load(Ordering::Relaxed) {
            return Ok(());
        }

        self.is_running.store(true, Ordering::SeqCst);
        let is_running = Arc::clone(&self.is_running);
        let is_paused = Arc::clone(&self.is_paused);
        let migration_requested = Arc::clone(&self.migration_requested);
        let migration_counter = Arc::clone(&self.migration_counter);
        let fps_mode = Arc::clone(&self.fps_mode);
        let processor = Arc::clone(&self.processor);

        let handle = std::thread::spawn(move || {
            run_wasapi_capture_loop(
                is_running,
                is_paused,
                migration_requested,
                migration_counter,
                fps_mode,
                processor,
            );
        });

        self.capture_thread = Some(handle);
        Ok(())
    }

    /// Signals the capture loop to terminate and joins the thread.
    pub fn stop(&mut self) {
        self.is_running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.capture_thread.take() {
            let _ = handle.join();
        }
    }

    /// Feeds audio samples in volatile memory only; zero raw audio persisted (PRD [S08]).
    pub fn feed_samples(&mut self, samples: &[f32]) -> Vec<f32> {
        if self.is_paused() {
            return vec![0.0; self.band_count()];
        }
        self.processor
            .lock()
            .map(|mut p| p.process_samples(samples))
            .unwrap_or_else(|_| vec![0.0; 32])
    }

    pub fn current_bands(&self) -> Vec<f32> {
        self.processor
            .lock()
            .map(|p| p.current_bands().to_vec())
            .unwrap_or_default()
    }

    pub fn band_count(&self) -> usize {
        self.processor
            .lock()
            .map(|p| p.band_count())
            .unwrap_or_default()
    }

    pub fn is_silent(&self) -> bool {
        self.processor.lock().map(|p| p.is_silent()).unwrap_or(true)
    }

    pub fn should_auto_hide(&self) -> bool {
        self.processor
            .lock()
            .map(|p| p.should_auto_hide())
            .unwrap_or(true)
    }
}

impl Drop for WasapiLoopbackCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

unsafe fn convert_raw_audio_samples(
    p_data: *const u8,
    num_frames: usize,
    channels: usize,
    is_float: bool,
    bits_per_sample: u16,
) -> Vec<f32> {
    if num_frames == 0 || channels == 0 || p_data.is_null() {
        return Vec::new();
    }

    let mut out = Vec::with_capacity(num_frames);

    if is_float && bits_per_sample == 32 {
        let float_ptr = p_data as *const f32;
        for frame_idx in 0..num_frames {
            let mut sum = 0.0f32;
            for ch in 0..channels {
                sum += *float_ptr.add(frame_idx * channels + ch);
            }
            let mono = (sum / channels as f32).clamp(-1.0, 1.0);
            out.push(mono);
        }
    } else if !is_float && bits_per_sample == 16 {
        let i16_ptr = p_data as *const i16;
        for frame_idx in 0..num_frames {
            let mut sum = 0.0f32;
            for ch in 0..channels {
                sum += *i16_ptr.add(frame_idx * channels + ch) as f32 / 32768.0;
            }
            let mono = (sum / channels as f32).clamp(-1.0, 1.0);
            out.push(mono);
        }
    } else if !is_float && bits_per_sample == 24 {
        for frame_idx in 0..num_frames {
            let mut sum = 0.0f32;
            for ch in 0..channels {
                let offset = (frame_idx * channels + ch) * 3;
                let b0 = *p_data.add(offset) as i32;
                let b1 = *p_data.add(offset + 1) as i32;
                let b2 = *p_data.add(offset + 2) as i8 as i32;
                let sample_i24 = (b2 << 16) | (b1 << 8) | b0;
                sum += sample_i24 as f32 / 8388608.0;
            }
            let mono = (sum / channels as f32).clamp(-1.0, 1.0);
            out.push(mono);
        }
    } else if !is_float && bits_per_sample == 32 {
        let i32_ptr = p_data as *const i32;
        for frame_idx in 0..num_frames {
            let mut sum = 0.0f32;
            for ch in 0..channels {
                sum += *i32_ptr.add(frame_idx * channels + ch) as f32 / 2147483648.0;
            }
            let mono = (sum / channels as f32).clamp(-1.0, 1.0);
            out.push(mono);
        }
    } else {
        out.resize(num_frames, 0.0);
    }

    out
}

struct WasapiStream {
    audio_client: IAudioClient,
    capture_client: IAudioCaptureClient,
    pwfx_ptr: *mut windows::Win32::Media::Audio::WAVEFORMATEX,
    channels: u16,
    bits_per_sample: u16,
    is_float: bool,
}

impl Drop for WasapiStream {
    fn drop(&mut self) {
        unsafe {
            let _ = self.audio_client.Stop();
            if !self.pwfx_ptr.is_null() {
                CoTaskMemFree(Some(self.pwfx_ptr as *const _ as *const _));
                self.pwfx_ptr = std::ptr::null_mut();
            }
        }
    }
}

unsafe fn init_wasapi_stream_with_enumerator(
    enumerator: &IMMDeviceEnumerator,
) -> Result<WasapiStream, String> {
    let device = enumerator
        .GetDefaultAudioEndpoint(eRender, eConsole)
        .map_err(|e| format!("No default audio render endpoint available: {}", e))?;

    let audio_client: IAudioClient = device
        .Activate(CLSCTX_ALL, None)
        .map_err(|e| format!("Failed to activate IAudioClient: {}", e))?;

    let pwfx_ptr = audio_client
        .GetMixFormat()
        .map_err(|e| format!("GetMixFormat failed: {}", e))?;

    let pwfx = *pwfx_ptr;
    let channels = pwfx.nChannels;
    let bits_per_sample = pwfx.wBitsPerSample;
    let mut is_float = pwfx.wFormatTag == WAVE_FORMAT_IEEE_FLOAT;
    if pwfx.wFormatTag == WAVE_FORMAT_EXTENSIBLE {
        let ext = &*(pwfx_ptr as *const WAVEFORMATEXTENSIBLE);
        let sub_format = std::ptr::addr_of!(ext.SubFormat).read_unaligned();
        if sub_format == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT {
            is_float = true;
        }
    }

    let buffer_duration_hns = 2_000_000; // 200ms
    if let Err(e) = audio_client.Initialize(
        AUDCLNT_SHAREMODE_SHARED,
        AUDCLNT_STREAMFLAGS_LOOPBACK,
        buffer_duration_hns,
        0,
        pwfx_ptr,
        None,
    ) {
        CoTaskMemFree(Some(pwfx_ptr as *const _ as *const _));
        return Err(format!("AudioClient.Initialize loopback failed: {}", e));
    }

    let capture_client: IAudioCaptureClient = match audio_client.GetService() {
        Ok(cc) => cc,
        Err(e) => {
            CoTaskMemFree(Some(pwfx_ptr as *const _ as *const _));
            return Err(format!("GetService::<IAudioCaptureClient> failed: {}", e));
        }
    };

    if let Err(e) = audio_client.Start() {
        CoTaskMemFree(Some(pwfx_ptr as *const _ as *const _));
        return Err(format!("AudioClient.Start failed: {}", e));
    }

    Ok(WasapiStream {
        audio_client,
        capture_client,
        pwfx_ptr,
        channels,
        bits_per_sample,
        is_float,
    })
}

unsafe fn run_active_stream(
    is_running: &Arc<AtomicBool>,
    is_paused: &Arc<AtomicBool>,
    migration_requested: &Arc<AtomicBool>,
    fps_mode_atomic: &Arc<AtomicU8>,
    processor: &Arc<Mutex<AudioSpectrumProcessor>>,
    stream: &WasapiStream,
) -> Result<(), String> {
    let mut consecutive_silent_ticks = 0usize;

    while is_running.load(Ordering::Relaxed) {
        // 1. Check for audio endpoint migration request from IMMNotificationClient
        if migration_requested.swap(false, Ordering::SeqCst) {
            info!("IMMNotificationClient signaled default audio device migration. Restarting capture on new endpoint.");
            return Err("endpoint_migrated".to_string());
        }

        // 2. Compute dynamic power-aware polling interval based on silence and power profile
        let mode = FpsMode::from_u8(fps_mode_atomic.load(Ordering::Relaxed));
        let poll_interval = if consecutive_silent_ticks > 60 {
            mode.deep_silence_poll_interval()
        } else if consecutive_silent_ticks > 10 {
            mode.idle_poll_interval()
        } else {
            mode.active_poll_interval()
        };

        std::thread::sleep(poll_interval);

        if !is_running.load(Ordering::Relaxed) {
            break;
        }
        if migration_requested.swap(false, Ordering::SeqCst) {
            info!("IMMNotificationClient signaled default audio device migration during sleep. Restarting capture.");
            return Err("endpoint_migrated".to_string());
        }

        // 3. Handle pause state
        if is_paused.load(Ordering::Relaxed) {
            consecutive_silent_ticks = consecutive_silent_ticks.saturating_add(1);
            while let Ok(packet_size) = stream.capture_client.GetNextPacketSize() {
                if packet_size == 0 {
                    break;
                }
                let mut p_data = std::ptr::null_mut();
                let mut num_frames = 0u32;
                let mut flags = 0u32;
                if stream
                    .capture_client
                    .GetBuffer(&mut p_data, &mut num_frames, &mut flags, None, None)
                    .is_ok()
                {
                    let _ = stream.capture_client.ReleaseBuffer(num_frames);
                }
            }
            if let Ok(mut proc) = processor.lock() {
                proc.process_samples(&[]);
            }
            continue;
        }

        // 4. Drain available audio packets from loopback buffer
        let mut got_packets = false;
        loop {
            let packet_size = match stream.capture_client.GetNextPacketSize() {
                Ok(size) => size,
                Err(e) => {
                    // Endpoint error, device might have been unplugged or reset
                    return Err(format!("GetNextPacketSize failed: {}", e));
                }
            };

            if packet_size == 0 {
                break;
            }
            got_packets = true;

            let mut p_data = std::ptr::null_mut();
            let mut num_frames = 0u32;
            let mut flags = 0u32;

            if stream
                .capture_client
                .GetBuffer(&mut p_data, &mut num_frames, &mut flags, None, None)
                .is_ok()
            {
                if num_frames > 0 && !p_data.is_null() {
                    let is_silent = (flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0;
                    let samples = if is_silent {
                        vec![0.0f32; num_frames as usize]
                    } else {
                        convert_raw_audio_samples(
                            p_data,
                            num_frames as usize,
                            stream.channels as usize,
                            stream.is_float,
                            stream.bits_per_sample,
                        )
                    };

                    // Zero raw audio persistence (PRD [S08]): in volatile memory only, dropped immediately.
                    if let Ok(mut proc) = processor.lock() {
                        proc.process_samples(&samples);
                    }
                }
                let _ = stream.capture_client.ReleaseBuffer(num_frames);
            } else {
                break;
            }
        }

        // When no audio is playing on Windows, loopback produces 0 packets.
        // Feed empty samples to tick band decay and silence auto-hide.
        if got_packets {
            consecutive_silent_ticks = 0;
        } else {
            consecutive_silent_ticks = consecutive_silent_ticks.saturating_add(1);
            if let Ok(mut proc) = processor.lock() {
                proc.process_samples(&[]);
            }
        }
    }

    Ok(())
}

fn run_wasapi_capture_loop(
    is_running: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    migration_requested: Arc<AtomicBool>,
    migration_counter: Arc<AtomicUsize>,
    fps_mode_atomic: Arc<AtomicU8>,
    processor: Arc<Mutex<AudioSpectrumProcessor>>,
) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        {
            // Instantiate MMDeviceEnumerator within scoped block
            let enumerator: Result<IMMDeviceEnumerator, _> =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL);

            // Register IMMNotificationClient callback for dynamic default device migration
            let notification_client = AudioEndpointNotificationClient::new(
                Arc::clone(&migration_requested),
                Arc::clone(&migration_counter),
            );
            let notification_interface: IMMNotificationClient = notification_client.into();

            let callback_registered = if let Ok(ref enum_ref) = enumerator {
                match enum_ref.RegisterEndpointNotificationCallback(&notification_interface) {
                    Ok(_) => {
                        info!("IMMNotificationClient registered for audio device migration callbacks.");
                        true
                    }
                    Err(e) => {
                        warn!("Failed to register IMMNotificationClient callback: {}", e);
                        false
                    }
                }
            } else {
                false
            };

            while is_running.load(Ordering::Relaxed) {
                let stream_init_res = if let Ok(ref enum_ref) = enumerator {
                    init_wasapi_stream_with_enumerator(enum_ref)
                } else {
                    Err("No MMDeviceEnumerator available".to_string())
                };

                match stream_init_res {
                    Ok(stream) => {
                        let mode = FpsMode::from_u8(fps_mode_atomic.load(Ordering::Relaxed));
                        info!(
                            "WASAPI shared-loopback audio capture stream active (channels: {}, bits: {}, float: {}, target FPS: {})",
                            stream.channels,
                            stream.bits_per_sample,
                            stream.is_float,
                            mode.target_fps()
                        );

                        let res = run_active_stream(
                            &is_running,
                            &is_paused,
                            &migration_requested,
                            &fps_mode_atomic,
                            &processor,
                            &stream,
                        );

                        if let Err(e) = res {
                            if e == "endpoint_migrated" {
                                info!("Audio endpoint migrated. Immediately refreshing default endpoint...");
                                std::thread::sleep(Duration::from_millis(50));
                            } else {
                                warn!(
                                    "WASAPI stream error or device disconnected: {}. Re-enumerating default endpoint...",
                                    e
                                );
                                std::thread::sleep(Duration::from_millis(200));
                            }
                        }
                    }
                    Err(e) => {
                        info!(
                            "WASAPI: Default audio endpoint not active ({}). Operating in fallback mode.",
                            e
                        );
                        for _ in 0..10 {
                            if !is_running.load(Ordering::Relaxed) {
                                break;
                            }
                            if migration_requested.swap(false, Ordering::SeqCst) {
                                info!("Audio endpoint migration signaled during fallback mode; attempting immediate re-enumeration.");
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(100));
                            if let Ok(mut proc) = processor.lock() {
                                proc.process_samples(&[]);
                            }
                        }
                    }
                }
            }

            // Clean unregistration of notification callback before dropping COM interfaces
            if callback_registered {
                if let Ok(ref enum_ref) = enumerator {
                    let _ =
                        enum_ref.UnregisterEndpointNotificationCallback(&notification_interface);
                    info!("IMMNotificationClient unregistered cleanly.");
                }
            }

            drop(notification_interface);
            drop(enumerator);
        } // All COM objects safely dropped before CoUninitialize

        CoUninitialize();
        info!("WASAPI shared-loopback audio capture cleanly shut down.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Media::Audio::{
        eConsole, eRender, DEVICE_STATE_ACTIVE, DEVICE_STATE_UNPLUGGED,
    };

    #[test]
    fn test_adaptive_fps_intervals_and_power_aware_polling() {
        assert_eq!(FpsMode::Performance.target_fps(), 60);
        assert_eq!(FpsMode::AcBalanced.target_fps(), 30);
        assert_eq!(FpsMode::BatterySaver.target_fps(), 15);

        assert!(FpsMode::Performance.frame_interval() < FpsMode::AcBalanced.frame_interval());
        assert!(FpsMode::AcBalanced.frame_interval() < FpsMode::BatterySaver.frame_interval());

        // Active intervals
        assert_eq!(
            FpsMode::Performance.active_poll_interval(),
            Duration::from_millis(10)
        );
        assert_eq!(
            FpsMode::AcBalanced.active_poll_interval(),
            Duration::from_millis(15)
        );
        assert_eq!(
            FpsMode::BatterySaver.active_poll_interval(),
            Duration::from_millis(33)
        );

        // Idle / light silence intervals
        assert_eq!(
            FpsMode::Performance.idle_poll_interval(),
            Duration::from_millis(33)
        );
        assert_eq!(
            FpsMode::AcBalanced.idle_poll_interval(),
            Duration::from_millis(50)
        );
        assert_eq!(
            FpsMode::BatterySaver.idle_poll_interval(),
            Duration::from_millis(100)
        );

        // Deep silence non-waking intervals
        assert_eq!(
            FpsMode::Performance.deep_silence_poll_interval(),
            Duration::from_millis(50)
        );
        assert_eq!(
            FpsMode::AcBalanced.deep_silence_poll_interval(),
            Duration::from_millis(100)
        );
        assert_eq!(
            FpsMode::BatterySaver.deep_silence_poll_interval(),
            Duration::from_millis(200)
        );

        // Ensure deep silence backs off significantly compared to active
        assert!(
            FpsMode::BatterySaver.deep_silence_poll_interval()
                > FpsMode::BatterySaver.active_poll_interval() * 5
        );
        assert!(
            FpsMode::AcBalanced.deep_silence_poll_interval()
                > FpsMode::AcBalanced.active_poll_interval() * 5
        );
    }

    #[test]
    fn test_wasapi_capture_lifecycle_and_pause() {
        let mut capture = WasapiLoopbackCapture::new(32, FpsMode::AcBalanced);
        assert_eq!(capture.fps_mode(), FpsMode::AcBalanced);
        assert!(!capture.is_paused());

        let samples = vec![0.5f32; 128];
        let bands = capture.feed_samples(&samples);
        assert_eq!(bands.len(), 32);

        // Pause audio capture (e.g. from tray toggle)
        capture.set_paused(true);
        assert!(capture.is_paused());

        let muted = capture.feed_samples(&samples);
        assert_eq!(muted, vec![0.0; 32]);
    }

    #[test]
    fn test_wasapi_endpoint_migration_signaling() {
        let capture = WasapiLoopbackCapture::new(32, FpsMode::AcBalanced);
        assert_eq!(capture.migration_counter(), 0);
        assert!(!capture.is_migration_requested());

        // Simulate endpoint migration request
        capture.request_migration();
        assert_eq!(capture.migration_counter(), 1);
        assert!(capture.is_migration_requested());

        // Test dynamic thread-safe fps_mode switching
        capture.set_fps_mode(FpsMode::BatterySaver);
        assert_eq!(capture.fps_mode(), FpsMode::BatterySaver);

        capture.set_fps_mode(FpsMode::Performance);
        assert_eq!(capture.fps_mode(), FpsMode::Performance);
    }

    #[test]
    fn test_notification_client_com_interface() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

            let migration_requested = Arc::new(AtomicBool::new(false));
            let migration_counter = Arc::new(AtomicUsize::new(0));

            {
                let client: IMMNotificationClient = AudioEndpointNotificationClient::new(
                    Arc::clone(&migration_requested),
                    Arc::clone(&migration_counter),
                )
                .into();

                let dummy_id = windows::core::w!("test_device_id");

                // 1. Device state changed to active (should NOT trigger migration)
                let _ = client.OnDeviceStateChanged(dummy_id, DEVICE_STATE_ACTIVE);
                assert!(!migration_requested.load(Ordering::Relaxed));
                assert_eq!(migration_counter.load(Ordering::Relaxed), 0);

                // 2. Device state changed to unplugged (SHOULD trigger migration)
                let _ = client.OnDeviceStateChanged(dummy_id, DEVICE_STATE_UNPLUGGED);
                assert!(migration_requested.load(Ordering::Relaxed));
                assert_eq!(migration_counter.load(Ordering::Relaxed), 1);

                // Reset
                migration_requested.store(false, Ordering::Relaxed);

                // 3. Default render device changed (SHOULD trigger migration)
                let _ = client.OnDefaultDeviceChanged(eRender, eConsole, dummy_id);
                assert!(migration_requested.load(Ordering::Relaxed));
                assert_eq!(migration_counter.load(Ordering::Relaxed), 2);
            }

            CoUninitialize();
        }
    }

    #[test]
    fn test_wasapi_thread_start_and_graceful_fallback() {
        let mut capture = WasapiLoopbackCapture::new(16, FpsMode::AcBalanced);
        // Start capture thread (in test environment, will run real WASAPI or graceful fallback)
        let res = capture.start();
        assert!(res.is_ok());
        assert!(capture.is_running());

        // Allow thread to spin briefly
        std::thread::sleep(Duration::from_millis(60));

        // Stop capture thread
        capture.stop();
        assert!(!capture.is_running());
    }

    #[test]
    fn test_raw_audio_sample_conversion_multi_formats() {
        // 1. 32-bit float stereo: 2 frames = 4 floats
        let float_data: [f32; 4] = [0.5, 0.5, -0.5, -0.5];
        let converted =
            unsafe { convert_raw_audio_samples(float_data.as_ptr() as *const u8, 2, 2, true, 32) };
        assert_eq!(converted.len(), 2);
        assert!((converted[0] - 0.5).abs() < 1e-4);
        assert!((converted[1] - (-0.5)).abs() < 1e-4);

        // 2. 16-bit PCM stereo: 2 frames = 4 i16
        let i16_data: [i16; 4] = [16384, 16384, -16384, -16384];
        let converted_16 =
            unsafe { convert_raw_audio_samples(i16_data.as_ptr() as *const u8, 2, 2, false, 16) };
        assert_eq!(converted_16.len(), 2);
        assert!((converted_16[0] - 0.5).abs() < 1e-3);
        assert!((converted_16[1] - (-0.5)).abs() < 1e-3);

        // 3. 24-bit PCM mono: 1 frame = 3 bytes representing half scale (0x400000 = 4194304)
        let i24_data: [u8; 3] = [0x00, 0x00, 0x40];
        let converted_24 = unsafe { convert_raw_audio_samples(i24_data.as_ptr(), 1, 1, false, 24) };
        assert_eq!(converted_24.len(), 1);
        assert!((converted_24[0] - 0.5).abs() < 1e-3);

        // 4. 32-bit PCM integer mono: 1 frame = 4 bytes representing half scale (0x40000000 = 1073741824)
        let i32_data: [i32; 1] = [1073741824];
        let converted_32 =
            unsafe { convert_raw_audio_samples(i32_data.as_ptr() as *const u8, 1, 1, false, 32) };
        assert_eq!(converted_32.len(), 1);
        assert!((converted_32[0] - 0.5).abs() < 1e-3);
    }
}
