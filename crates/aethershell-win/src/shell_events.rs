use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use windows::Win32::Foundation::{HMODULE, HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, PeekMessageW, PostThreadMessageW, TranslateMessage, EVENT_OBJECT_DESTROY,
    EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND, EVENT_SYSTEM_MINIMIZESTART,
    EVENT_SYSTEM_MOVESIZEEND, MSG, PM_REMOVE, WM_QUIT,
};

/// WinEvent hook flags from winuser.h
pub const WINEVENT_OUTOFCONTEXT: u32 = 0x0000;
pub const WINEVENT_SKIPOWNPROCESS: u32 = 0x0002;

/// Normalized shell window events captured out-of-context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellEvent {
    ForegroundChanged { hwnd: isize },
    WindowMovedResized { hwnd: isize },
    WindowMinimized { hwnd: isize },
    WindowRestored { hwnd: isize },
    WindowDestroyed { hwnd: isize },
}

static EVENT_DISPATCHER: Mutex<Option<Sender<ShellEvent>>> = Mutex::new(None);

unsafe extern "system" fn win_event_proc(
    _h_hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    id_child: i32,
    _id_event_thread: u32,
    _dwms_event_time: u32,
) {
    // Only capture top-level window events (OBJID_WINDOW == 0, CHILDID_SELF == 0)
    if id_object != 0 || id_child != 0 || hwnd.0.is_null() {
        return;
    }

    let raw_hwnd = hwnd.0 as isize;

    let shell_event = match event {
        EVENT_SYSTEM_FOREGROUND => ShellEvent::ForegroundChanged { hwnd: raw_hwnd },
        EVENT_SYSTEM_MOVESIZEEND => ShellEvent::WindowMovedResized { hwnd: raw_hwnd },
        EVENT_SYSTEM_MINIMIZESTART => ShellEvent::WindowMinimized { hwnd: raw_hwnd },
        EVENT_SYSTEM_MINIMIZEEND => ShellEvent::WindowRestored { hwnd: raw_hwnd },
        EVENT_OBJECT_DESTROY => ShellEvent::WindowDestroyed { hwnd: raw_hwnd },
        _ => return,
    };

    if let Ok(guard) = EVENT_DISPATCHER.lock() {
        if let Some(ref sender) = *guard {
            let _ = sender.send(shell_event);
        }
    }
}

/// Out-of-context WinEvent monitor managing hook registration and message pumping.
pub struct ShellEventMonitor {
    thread_handle: Option<JoinHandle<()>>,
    thread_id: u32,
    is_running: Arc<AtomicBool>,
}

impl ShellEventMonitor {
    /// Spawns the shell event monitor on a dedicated OS thread.
    ///
    /// Returns the monitor controller and an event receiver channel.
    pub fn start() -> Result<(Self, Receiver<ShellEvent>), String> {
        let (tx, rx) = channel();

        {
            let mut guard = EVENT_DISPATCHER
                .lock()
                .map_err(|e| format!("Mutex poisoned: {}", e))?;
            *guard = Some(tx);
        }

        let is_running = Arc::new(AtomicBool::new(true));
        let is_running_clone = Arc::clone(&is_running);

        let (tid_tx, tid_rx) = channel();

        let thread_handle = std::thread::spawn(move || {
            let tid = unsafe { GetCurrentThreadId() };
            let _ = tid_tx.send(tid);

            // Register out-of-context hooks (never injecting DLLs)
            let hook_foreground = unsafe {
                SetWinEventHook(
                    EVENT_SYSTEM_FOREGROUND,
                    EVENT_SYSTEM_MINIMIZEEND,
                    HMODULE::default(),
                    Some(win_event_proc),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                )
            };

            let hook_destroy = unsafe {
                SetWinEventHook(
                    EVENT_OBJECT_DESTROY,
                    EVENT_OBJECT_DESTROY,
                    HMODULE::default(),
                    Some(win_event_proc),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                )
            };

            tracing::info!(
                "WinEvent hooks installed (Thread ID: {}, Foreground: {:?}, Destroy: {:?})",
                tid,
                hook_foreground.0,
                hook_destroy.0
            );

            // Active message loop on the hook thread
            unsafe {
                let mut msg = MSG::default();
                while is_running_clone.load(Ordering::Relaxed) {
                    if PeekMessageW(&mut msg, HWND::default(), 0, 0, PM_REMOVE).as_bool() {
                        if msg.message == WM_QUIT {
                            break;
                        }
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    } else {
                        std::thread::sleep(std::time::Duration::from_millis(15));
                    }
                }

                if !hook_foreground.0.is_null() {
                    let _ = UnhookWinEvent(hook_foreground);
                }
                if !hook_destroy.0.is_null() {
                    let _ = UnhookWinEvent(hook_destroy);
                }
            }

            tracing::info!("WinEvent monitor thread unhooked and terminated.");
        });

        let thread_id = tid_rx
            .recv()
            .map_err(|e| format!("Failed to receive thread ID: {}", e))?;

        Ok((
            Self {
                thread_handle: Some(thread_handle),
                thread_id,
                is_running,
            },
            rx,
        ))
    }

    /// Stops the monitor and unhooks all WinEvent hooks.
    pub fn stop(&mut self) {
        if self.is_running.swap(false, Ordering::SeqCst) {
            unsafe {
                let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
            }

            if let Some(handle) = self.thread_handle.take() {
                let _ = handle.join();
            }

            if let Ok(mut guard) = EVENT_DISPATCHER.lock() {
                *guard = None;
            }
        }
    }
}

impl Drop for ShellEventMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_event_monitor_lifecycle() {
        let (mut monitor, _rx) = ShellEventMonitor::start().expect("Failed to start monitor");
        assert!(monitor.thread_id > 0);

        // Sleep briefly to ensure thread is pumping
        std::thread::sleep(std::time::Duration::from_millis(50));

        monitor.stop();
    }
}
