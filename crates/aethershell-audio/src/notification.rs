use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tracing::{debug, info};
use windows::core::{implement, Result as WinResult, PCWSTR};
use windows::Win32::Media::Audio::{
    eConsole, eMultimedia, eRender, EDataFlow, ERole, IMMNotificationClient,
    IMMNotificationClient_Impl, DEVICE_STATE, DEVICE_STATE_DISABLED, DEVICE_STATE_NOTPRESENT,
    DEVICE_STATE_UNPLUGGED,
};
use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;

/// Implementation of the Windows COM `IMMNotificationClient` interface.
///
/// Seamlessly tracks default audio endpoint changes and device unplug/replug events
/// without Explorer injection or blocking the audio playback pipeline (PRD Gate G09).
#[implement(IMMNotificationClient)]
pub struct AudioEndpointNotificationClient {
    migration_requested: Arc<AtomicBool>,
    migration_counter: Arc<AtomicUsize>,
}

impl AudioEndpointNotificationClient {
    pub fn new(migration_requested: Arc<AtomicBool>, migration_counter: Arc<AtomicUsize>) -> Self {
        Self {
            migration_requested,
            migration_counter,
        }
    }
}

impl IMMNotificationClient_Impl for AudioEndpointNotificationClient_Impl {
    fn OnDeviceStateChanged(
        &self,
        _pwstrdeviceid: &PCWSTR,
        dwnewstate: DEVICE_STATE,
    ) -> WinResult<()> {
        let state_val = dwnewstate.0;
        if state_val == DEVICE_STATE_UNPLUGGED.0
            || state_val == DEVICE_STATE_NOTPRESENT.0
            || state_val == DEVICE_STATE_DISABLED.0
        {
            debug!(
                "IMMNotificationClient: Audio endpoint state changed to inactive (state: 0x{:08X}). Signaling device invalidation.",
                state_val
            );
            self.migration_requested.store(true, Ordering::SeqCst);
            self.migration_counter.fetch_add(1, Ordering::SeqCst);
        }
        Ok(())
    }

    fn OnDeviceAdded(&self, _pwstrdeviceid: &PCWSTR) -> WinResult<()> {
        debug!("IMMNotificationClient: Audio endpoint added.");
        Ok(())
    }

    fn OnDeviceRemoved(&self, _pwstrdeviceid: &PCWSTR) -> WinResult<()> {
        debug!("IMMNotificationClient: Audio endpoint removed.");
        self.migration_requested.store(true, Ordering::SeqCst);
        self.migration_counter.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn OnDefaultDeviceChanged(
        &self,
        flow: EDataFlow,
        role: ERole,
        _pwstrdefaultdeviceid: &PCWSTR,
    ) -> WinResult<()> {
        // We only care about render endpoints used for console/multimedia playback loopback
        if flow == eRender && (role == eConsole || role == eMultimedia) {
            info!(
                "IMMNotificationClient: Default audio render endpoint changed (flow: {:?}, role: {:?}). Requesting stream migration.",
                flow, role
            );
            self.migration_requested.store(true, Ordering::SeqCst);
            self.migration_counter.fetch_add(1, Ordering::SeqCst);
        }
        Ok(())
    }

    fn OnPropertyValueChanged(&self, _pwstrdeviceid: &PCWSTR, _key: &PROPERTYKEY) -> WinResult<()> {
        Ok(())
    }
}
