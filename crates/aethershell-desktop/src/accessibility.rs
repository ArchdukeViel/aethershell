//! Windows UI Automation (UIA) Provider Implementation (Gate G14).
//!
//! Exposes the Direct2D desktop host overlay window and its interactive widget regions
//! (`InteractiveRegion`) to Windows Narrator and UI Automation clients via standard Win32 COM
//! provider interfaces (`IRawElementProviderSimple`, `IRawElementProviderFragment`, `IRawElementProviderFragmentRoot`,
//! and `IInvokeProvider`).
//!
//! Complies with PRD Gate G14 accessibility specifications and non-invasive Shell architecture.

#![allow(non_upper_case_globals)]

use crate::interaction::InteractiveRegion;
use crate::window::ActionCallback;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use windows::core::{implement, IUnknown, IUnknownImpl, Result as WinResult, VARIANT};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::System::Com::SAFEARRAY;
use windows::Win32::System::Ole::{
    SafeArrayAccessData, SafeArrayCreateVector, SafeArrayDestroy, SafeArrayUnaccessData,
};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Variant::{VT_ARRAY, VT_I4, VT_R8};
use windows::Win32::UI::Accessibility::{
    IInvokeProvider, IInvokeProvider_Impl, IRawElementProviderFragment,
    IRawElementProviderFragmentRoot, IRawElementProviderFragmentRoot_Impl,
    IRawElementProviderFragment_Impl, IRawElementProviderSimple, IRawElementProviderSimple_Impl,
    NavigateDirection, NavigateDirection_FirstChild, NavigateDirection_LastChild,
    NavigateDirection_NextSibling, NavigateDirection_Parent, NavigateDirection_PreviousSibling,
    NotifyWinEvent, ProviderOptions, ProviderOptions_ServerSideProvider,
    UIA_AutomationFocusChangedEventId, UIA_AutomationIdPropertyId, UIA_BoundingRectanglePropertyId,
    UIA_ButtonControlTypeId, UIA_CheckBoxControlTypeId, UIA_ControlTypePropertyId,
    UIA_CustomControlTypeId, UIA_GroupControlTypeId, UIA_HasKeyboardFocusPropertyId,
    UIA_HelpTextPropertyId, UIA_ImageControlTypeId, UIA_InvokePatternId, UIA_Invoke_InvokedEventId,
    UIA_IsContentElementPropertyId, UIA_IsControlElementPropertyId, UIA_IsEnabledPropertyId,
    UIA_IsKeyboardFocusablePropertyId, UIA_ItemStatusPropertyId, UIA_NamePropertyId,
    UIA_PaneControlTypeId, UIA_ProgressBarControlTypeId, UIA_SliderControlTypeId,
    UIA_TextControlTypeId, UiaAppendRuntimeId, UiaHostProviderFromHwnd, UiaRaiseAutomationEvent,
    UiaRect, UIA_PATTERN_ID, UIA_PROPERTY_ID,
};

/// Win32 client object identifier for accessibility events.
pub const OBJID_CLIENT: i32 = -4;

/// Standard WinEvent focus event ID.
pub const EVENT_OBJECT_FOCUS: u32 = 0x8005;

/// Creates a standard Win32 UI Automation Runtime ID SAFEARRAY for a child fragment.
///
/// In accordance with Microsoft UI Automation specification:
/// Elements that are not the root of a fragment must return a SAFEARRAY containing
/// `[UiaAppendRuntimeId, <unique_child_id>]`.
pub fn create_fragment_runtime_id(region_index: usize) -> *mut SAFEARRAY {
    unsafe {
        let sa = SafeArrayCreateVector(VT_I4, 0, 2);
        if sa.is_null() {
            return std::ptr::null_mut();
        }
        let mut ptr: *mut c_void = std::ptr::null_mut();
        if SafeArrayAccessData(sa, &mut ptr).is_err() {
            let _ = SafeArrayDestroy(sa);
            return std::ptr::null_mut();
        }
        let slice = std::slice::from_raw_parts_mut(ptr as *mut i32, 2);
        slice[0] = UiaAppendRuntimeId as i32;
        slice[1] = (region_index + 1) as i32;
        let _ = SafeArrayUnaccessData(sa);
        sa
    }
}

/// Converts a UI Automation bounding rectangle (`UiaRect`) into a standard COM `VT_ARRAY | VT_R8` VARIANT.
pub fn uia_rect_to_variant(rect: &UiaRect) -> VARIANT {
    unsafe {
        let sa = SafeArrayCreateVector(VT_R8, 0, 4);
        if sa.is_null() {
            return VARIANT::default();
        }
        let mut ptr: *mut c_void = std::ptr::null_mut();
        if SafeArrayAccessData(sa, &mut ptr).is_err() {
            let _ = SafeArrayDestroy(sa);
            return VARIANT::default();
        }
        let slice = std::slice::from_raw_parts_mut(ptr as *mut f64, 4);
        slice[0] = rect.left;
        slice[1] = rect.top;
        slice[2] = rect.width;
        slice[3] = rect.height;
        let _ = SafeArrayUnaccessData(sa);

        #[repr(C)]
        struct RawVariant {
            vt: u16,
            r1: u16,
            r2: u16,
            r3: u16,
            parray: *mut SAFEARRAY,
            pad: u64,
        }
        let raw = RawVariant {
            vt: VT_ARRAY.0 | VT_R8.0,
            r1: 0,
            r2: 0,
            r3: 0,
            parray: sa,
            pad: 0,
        };
        std::mem::transmute(raw)
    }
}

/// Thread-safe shared accessibility state between the desktop host window and its UIA COM providers.
#[derive(Clone)]
pub struct DesktopAccessibilityState {
    pub hwnd: HWND,
    pub is_interactive: Arc<AtomicBool>,
    pub interactive_regions: Arc<std::sync::Mutex<Vec<InteractiveRegion>>>,
    pub focused_region_index: Arc<std::sync::Mutex<Option<usize>>>,
    pub current_bounds: Arc<std::sync::Mutex<[i32; 4]>>,
    pub on_action: Arc<std::sync::Mutex<Option<ActionCallback>>>,
    pub last_activity_tick: Arc<AtomicU64>,
}

impl DesktopAccessibilityState {
    pub fn new(
        hwnd: HWND,
        is_interactive: Arc<AtomicBool>,
        interactive_regions: Arc<std::sync::Mutex<Vec<InteractiveRegion>>>,
        focused_region_index: Arc<std::sync::Mutex<Option<usize>>>,
        current_bounds: Arc<std::sync::Mutex<[i32; 4]>>,
        on_action: Arc<std::sync::Mutex<Option<ActionCallback>>>,
        last_activity_tick: Arc<AtomicU64>,
    ) -> Self {
        Self {
            hwnd,
            is_interactive,
            interactive_regions,
            focused_region_index,
            current_bounds,
            on_action,
            last_activity_tick,
        }
    }
}

/// Maps an interactive region role string to a standard UI Automation control type ID (Gate G14).
pub fn role_to_control_type_id(role: Option<&str>, has_action: bool) -> i32 {
    match role {
        Some(r) => match r.trim().to_ascii_lowercase().as_str() {
            "button" | "pushbutton" => UIA_ButtonControlTypeId.0,
            "text" | "label" | "static" | "heading" => UIA_TextControlTypeId.0,
            "group" | "section" => UIA_GroupControlTypeId.0,
            "image" | "icon" => UIA_ImageControlTypeId.0,
            "slider" => UIA_SliderControlTypeId.0,
            "progressbar" | "progress" => UIA_ProgressBarControlTypeId.0,
            "checkbox" | "check_box" => UIA_CheckBoxControlTypeId.0,
            "pane" | "window" => UIA_PaneControlTypeId.0,
            _ => {
                if has_action {
                    UIA_ButtonControlTypeId.0
                } else {
                    UIA_CustomControlTypeId.0
                }
            }
        },
        None => {
            if has_action {
                UIA_ButtonControlTypeId.0
            } else {
                UIA_CustomControlTypeId.0
            }
        }
    }
}

/// Creates a COM root provider for the desktop host overlay window (`IRawElementProviderSimple`).
pub fn create_root_provider(state: DesktopAccessibilityState) -> IRawElementProviderSimple {
    let root = DesktopHostRootProvider::new(state);
    root.into()
}

/// Root UI Automation provider for the desktop overlay host window (`HWND`).
///
/// Implements `IRawElementProviderSimple`, `IRawElementProviderFragment`, and `IRawElementProviderFragmentRoot`.
#[implement(
    IRawElementProviderSimple,
    IRawElementProviderFragment,
    IRawElementProviderFragmentRoot
)]
pub struct DesktopHostRootProvider {
    state: DesktopAccessibilityState,
}

impl DesktopHostRootProvider {
    pub fn new(state: DesktopAccessibilityState) -> Self {
        Self { state }
    }
}

impl IRawElementProviderSimple_Impl for DesktopHostRootProvider_Impl {
    fn ProviderOptions(&self) -> WinResult<ProviderOptions> {
        Ok(ProviderOptions_ServerSideProvider)
    }

    fn GetPatternProvider(&self, _patternid: UIA_PATTERN_ID) -> WinResult<IUnknown> {
        Err(windows::core::Error::empty())
    }

    fn GetPropertyValue(&self, propertyid: UIA_PROPERTY_ID) -> WinResult<VARIANT> {
        match propertyid {
            UIA_NamePropertyId => Ok(VARIANT::from("AetherShell Desktop Overlay")),
            UIA_ControlTypePropertyId => Ok(VARIANT::from(UIA_PaneControlTypeId.0)),
            UIA_BoundingRectanglePropertyId => {
                let rect = self.BoundingRectangle()?;
                Ok(uia_rect_to_variant(&rect))
            }
            UIA_IsEnabledPropertyId => Ok(VARIANT::from(true)),
            UIA_IsKeyboardFocusablePropertyId => Ok(VARIANT::from(true)),
            UIA_HasKeyboardFocusPropertyId => {
                let focused = self.state.is_interactive.load(Ordering::Relaxed);
                Ok(VARIANT::from(focused))
            }
            UIA_IsControlElementPropertyId => Ok(VARIANT::from(true)),
            UIA_IsContentElementPropertyId => Ok(VARIANT::from(true)),
            _ => Ok(VARIANT::default()),
        }
    }

    fn HostRawElementProvider(&self) -> WinResult<IRawElementProviderSimple> {
        if self.state.hwnd.0.is_null() {
            return Err(windows::core::Error::empty());
        }
        unsafe { UiaHostProviderFromHwnd(self.state.hwnd) }
    }
}

impl IRawElementProviderFragment_Impl for DesktopHostRootProvider_Impl {
    fn Navigate(&self, direction: NavigateDirection) -> WinResult<IRawElementProviderFragment> {
        let regions = self
            .state
            .interactive_regions
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();

        match direction {
            NavigateDirection_FirstChild => {
                if !regions.is_empty() {
                    let child = DesktopWidgetFragmentProvider::new(self.state.clone(), 0);
                    Ok(child.into())
                } else {
                    Err(windows::core::Error::empty())
                }
            }
            NavigateDirection_LastChild => {
                if !regions.is_empty() {
                    let child = DesktopWidgetFragmentProvider::new(
                        self.state.clone(),
                        regions.len().saturating_sub(1),
                    );
                    Ok(child.into())
                } else {
                    Err(windows::core::Error::empty())
                }
            }
            _ => Err(windows::core::Error::empty()),
        }
    }

    fn GetRuntimeId(&self) -> WinResult<*mut SAFEARRAY> {
        // HWND host root provider returns NULL runtime ID; UIA assigns ID automatically.
        Ok(std::ptr::null_mut())
    }

    fn BoundingRectangle(&self) -> WinResult<UiaRect> {
        let bounds = self
            .state
            .current_bounds
            .lock()
            .map(|b| *b)
            .unwrap_or([0, 0, 0, 0]);
        Ok(UiaRect {
            left: bounds[0] as f64,
            top: bounds[1] as f64,
            width: bounds[2] as f64,
            height: bounds[3] as f64,
        })
    }

    fn GetEmbeddedFragmentRoots(&self) -> WinResult<*mut SAFEARRAY> {
        Ok(std::ptr::null_mut())
    }

    fn SetFocus(&self) -> WinResult<()> {
        Ok(())
    }

    fn FragmentRoot(&self) -> WinResult<IRawElementProviderFragmentRoot> {
        self.to_object().cast()
    }
}

impl IRawElementProviderFragmentRoot_Impl for DesktopHostRootProvider_Impl {
    fn ElementProviderFromPoint(&self, x: f64, y: f64) -> WinResult<IRawElementProviderFragment> {
        let bounds = self
            .state
            .current_bounds
            .lock()
            .map(|b| *b)
            .unwrap_or([0, 0, 0, 0]);

        // Per UIA specification: Return NULL if point is outside the root element bounds.
        let left = bounds[0] as f64;
        let top = bounds[1] as f64;
        let right = (bounds[0] + bounds[2]) as f64;
        let bottom = (bounds[1] + bounds[3]) as f64;
        if x < left || x >= right || y < top || y >= bottom {
            return Err(windows::core::Error::empty());
        }

        let local_x = (x - left) as f32;
        let local_y = (y - top) as f32;

        let regions = self
            .state
            .interactive_regions
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();

        if let Some(idx) = regions
            .iter()
            .rposition(|r| r.contains_point(local_x, local_y))
        {
            let child = DesktopWidgetFragmentProvider::new(self.state.clone(), idx);
            Ok(child.into())
        } else {
            self.to_object().cast()
        }
    }

    fn GetFocus(&self) -> WinResult<IRawElementProviderFragment> {
        // In passive mode, neither the desktop overlay nor its children have keyboard input focus.
        if !self.state.is_interactive.load(Ordering::Relaxed) {
            return Err(windows::core::Error::empty());
        }

        let focused_idx = self.state.focused_region_index.lock().ok().and_then(|g| *g);
        let regions = self
            .state
            .interactive_regions
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();

        if let Some(idx) = focused_idx {
            if idx < regions.len() {
                let child = DesktopWidgetFragmentProvider::new(self.state.clone(), idx);
                return Ok(child.into());
            }
        }

        self.to_object().cast()
    }
}

/// UI Automation Fragment provider for individual Direct2D interactive widget regions (`InteractiveRegion`).
///
/// Implements `IRawElementProviderSimple`, `IRawElementProviderFragment`, and `IInvokeProvider`.
#[implement(
    IRawElementProviderSimple,
    IRawElementProviderFragment,
    IInvokeProvider
)]
pub struct DesktopWidgetFragmentProvider {
    state: DesktopAccessibilityState,
    region_index: usize,
}

impl DesktopWidgetFragmentProvider {
    pub fn new(state: DesktopAccessibilityState, region_index: usize) -> Self {
        Self {
            state,
            region_index,
        }
    }

    fn get_region(&self) -> Option<InteractiveRegion> {
        self.state
            .interactive_regions
            .lock()
            .ok()
            .and_then(|guard| guard.get(self.region_index).cloned())
    }
}

impl IRawElementProviderSimple_Impl for DesktopWidgetFragmentProvider_Impl {
    fn ProviderOptions(&self) -> WinResult<ProviderOptions> {
        Ok(ProviderOptions_ServerSideProvider)
    }

    fn GetPatternProvider(&self, patternid: UIA_PATTERN_ID) -> WinResult<IUnknown> {
        if patternid == UIA_InvokePatternId {
            let region = self.get_region();
            if region.as_ref().and_then(|r| r.action.as_ref()).is_some() {
                return self.to_object().cast();
            }
        }
        Err(windows::core::Error::empty())
    }

    fn GetPropertyValue(&self, propertyid: UIA_PROPERTY_ID) -> WinResult<VARIANT> {
        let region = match self.get_region() {
            Some(r) => r,
            None => return Ok(VARIANT::default()),
        };

        match propertyid {
            UIA_NamePropertyId => {
                let name = region
                    .accessible_name
                    .as_deref()
                    .unwrap_or(region.id.as_str());
                Ok(VARIANT::from(name))
            }
            UIA_ControlTypePropertyId => {
                let ctrl_type = role_to_control_type_id(
                    region.accessible_role.as_deref(),
                    region.action.is_some(),
                );
                Ok(VARIANT::from(ctrl_type))
            }
            UIA_BoundingRectanglePropertyId => {
                let rect = self.BoundingRectangle()?;
                Ok(uia_rect_to_variant(&rect))
            }
            UIA_IsEnabledPropertyId => Ok(VARIANT::from(true)),
            UIA_IsKeyboardFocusablePropertyId => Ok(VARIANT::from(true)),
            UIA_HasKeyboardFocusPropertyId => {
                let is_interactive = self.state.is_interactive.load(Ordering::Relaxed);
                let is_focused = is_interactive
                    && self
                        .state
                        .focused_region_index
                        .lock()
                        .map(|g| *g == Some(self.region_index))
                        .unwrap_or(false);
                Ok(VARIANT::from(is_focused))
            }
            UIA_HelpTextPropertyId => {
                if let Some(ref val) = region.accessible_value {
                    Ok(VARIANT::from(val.as_str()))
                } else {
                    Ok(VARIANT::default())
                }
            }
            UIA_ItemStatusPropertyId => {
                let desc = region.accessible_description();
                Ok(VARIANT::from(desc.as_str()))
            }
            UIA_AutomationIdPropertyId => Ok(VARIANT::from(region.id.as_str())),
            UIA_IsControlElementPropertyId => Ok(VARIANT::from(true)),
            UIA_IsContentElementPropertyId => Ok(VARIANT::from(true)),
            _ => Ok(VARIANT::default()),
        }
    }

    fn HostRawElementProvider(&self) -> WinResult<IRawElementProviderSimple> {
        // Child fragments are hosted by the root provider, not an HWND directly.
        Err(windows::core::Error::empty())
    }
}

impl IRawElementProviderFragment_Impl for DesktopWidgetFragmentProvider_Impl {
    fn Navigate(&self, direction: NavigateDirection) -> WinResult<IRawElementProviderFragment> {
        let regions = self
            .state
            .interactive_regions
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();

        match direction {
            NavigateDirection_Parent => {
                let root = DesktopHostRootProvider::new(self.state.clone());
                Ok(root.into())
            }
            NavigateDirection_NextSibling => {
                if self.region_index + 1 < regions.len() {
                    let next = DesktopWidgetFragmentProvider::new(
                        self.state.clone(),
                        self.region_index + 1,
                    );
                    Ok(next.into())
                } else {
                    Err(windows::core::Error::empty())
                }
            }
            NavigateDirection_PreviousSibling => {
                if self.region_index > 0 && self.region_index < regions.len() {
                    let prev = DesktopWidgetFragmentProvider::new(
                        self.state.clone(),
                        self.region_index - 1,
                    );
                    Ok(prev.into())
                } else {
                    Err(windows::core::Error::empty())
                }
            }
            _ => Err(windows::core::Error::empty()),
        }
    }

    fn GetRuntimeId(&self) -> WinResult<*mut SAFEARRAY> {
        let sa = create_fragment_runtime_id(self.region_index);
        if sa.is_null() {
            Err(windows::core::Error::empty())
        } else {
            Ok(sa)
        }
    }

    fn BoundingRectangle(&self) -> WinResult<UiaRect> {
        let bounds = self
            .state
            .current_bounds
            .lock()
            .map(|b| *b)
            .unwrap_or([0, 0, 0, 0]);

        if let Some(r) = self.get_region() {
            Ok(UiaRect {
                left: (bounds[0] as f32 + r.x) as f64,
                top: (bounds[1] as f32 + r.y) as f64,
                width: r.width as f64,
                height: r.height as f64,
            })
        } else {
            Ok(UiaRect {
                left: 0.0,
                top: 0.0,
                width: 0.0,
                height: 0.0,
            })
        }
    }

    fn GetEmbeddedFragmentRoots(&self) -> WinResult<*mut SAFEARRAY> {
        Ok(std::ptr::null_mut())
    }

    fn SetFocus(&self) -> WinResult<()> {
        if let Ok(mut guard) = self.state.focused_region_index.lock() {
            *guard = Some(self.region_index);
        }
        self.state
            .last_activity_tick
            .store(unsafe { GetTickCount64() }, Ordering::Relaxed);
        unsafe {
            let _ = InvalidateRect(self.state.hwnd, None, false);
            NotifyWinEvent(
                EVENT_OBJECT_FOCUS,
                self.state.hwnd,
                OBJID_CLIENT,
                (self.region_index + 1) as i32,
            );
            let child = DesktopWidgetFragmentProvider::new(self.state.clone(), self.region_index);
            let simple: IRawElementProviderSimple = child.into();
            let _ = UiaRaiseAutomationEvent(&simple, UIA_AutomationFocusChangedEventId);
        }
        Ok(())
    }

    fn FragmentRoot(&self) -> WinResult<IRawElementProviderFragmentRoot> {
        let root = DesktopHostRootProvider::new(self.state.clone());
        Ok(root.into())
    }
}

impl IInvokeProvider_Impl for DesktopWidgetFragmentProvider_Impl {
    fn Invoke(&self) -> WinResult<()> {
        self.state
            .last_activity_tick
            .store(unsafe { GetTickCount64() }, Ordering::Relaxed);
        if let Some(r) = self.get_region() {
            if let Ok(guard) = self.state.on_action.lock() {
                if let Some(ref cb) = *guard {
                    cb(&r, r.x, r.y);
                }
            }
            unsafe {
                let child =
                    DesktopWidgetFragmentProvider::new(self.state.clone(), self.region_index);
                let simple: IRawElementProviderSimple = child.into();
                let _ = UiaRaiseAutomationEvent(&simple, UIA_Invoke_InvokedEventId);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::core::Interface;
    use windows::Win32::System::Ole::{
        SafeArrayAccessData, SafeArrayCreateVector, SafeArrayUnaccessData,
    };
    use windows::Win32::System::Variant::{VT_ARRAY, VT_R8};

    #[test]
    fn test_role_to_control_type_mapping() {
        unsafe {
            let sa = SafeArrayCreateVector(VT_R8, 0, 4);
            let mut ptr = std::ptr::null_mut();
            let _ = SafeArrayAccessData(sa, &mut ptr);
            let slice = std::slice::from_raw_parts_mut(ptr as *mut f64, 4);
            slice[0] = 10.0;
            slice[1] = 20.0;
            slice[2] = 30.0;
            slice[3] = 40.0;
            let _ = SafeArrayUnaccessData(sa);

            #[repr(C)]
            struct RawVariant {
                vt: u16,
                r1: u16,
                r2: u16,
                r3: u16,
                parray: *mut windows::Win32::System::Com::SAFEARRAY,
                pad: u64,
            }
            let raw = RawVariant {
                vt: VT_ARRAY.0 | VT_R8.0,
                r1: 0,
                r2: 0,
                r3: 0,
                parray: sa,
                pad: 0,
            };
            let v: VARIANT = std::mem::transmute(raw);
            drop(v); // Ensures VariantClear runs cleanly
        }
        assert_eq!(
            role_to_control_type_id(Some("button"), false),
            UIA_ButtonControlTypeId.0
        );
        assert_eq!(
            role_to_control_type_id(Some("text"), false),
            UIA_TextControlTypeId.0
        );
        assert_eq!(
            role_to_control_type_id(Some("label"), false),
            UIA_TextControlTypeId.0
        );
        assert_eq!(
            role_to_control_type_id(Some("heading"), false),
            UIA_TextControlTypeId.0
        );
        assert_eq!(
            role_to_control_type_id(Some("group"), false),
            UIA_GroupControlTypeId.0
        );
        assert_eq!(
            role_to_control_type_id(Some("image"), false),
            UIA_ImageControlTypeId.0
        );
        assert_eq!(
            role_to_control_type_id(Some("slider"), false),
            UIA_SliderControlTypeId.0
        );
        assert_eq!(
            role_to_control_type_id(Some("progressbar"), false),
            UIA_ProgressBarControlTypeId.0
        );
        assert_eq!(
            role_to_control_type_id(Some("checkbox"), false),
            UIA_CheckBoxControlTypeId.0
        );
        assert_eq!(
            role_to_control_type_id(Some("pane"), false),
            UIA_PaneControlTypeId.0
        );
        assert_eq!(
            role_to_control_type_id(Some("unknown"), true),
            UIA_ButtonControlTypeId.0
        );
        assert_eq!(
            role_to_control_type_id(Some("unknown"), false),
            UIA_CustomControlTypeId.0
        );
        assert_eq!(
            role_to_control_type_id(None, true),
            UIA_ButtonControlTypeId.0
        );
        assert_eq!(
            role_to_control_type_id(None, false),
            UIA_CustomControlTypeId.0
        );
    }

    #[test]
    fn test_root_and_fragment_provider_lifecycle_and_properties() {
        let is_interactive = Arc::new(AtomicBool::new(true));
        let regions =
            vec![
                InteractiveRegion::new("btn_play", 10.0, 20.0, 100.0, 50.0, Some("play".into()))
                    .with_accessibility("Play Music", "button", Some("Playing".into())),
                InteractiveRegion::new("lbl_title", 120.0, 20.0, 200.0, 50.0, None)
                    .with_accessibility("Song Title", "text", Some("Track 01".into())),
            ];
        let interactive_regions = Arc::new(std::sync::Mutex::new(regions));
        let focused_region_index = Arc::new(std::sync::Mutex::new(Some(0)));
        let current_bounds = Arc::new(std::sync::Mutex::new([100, 200, 1920, 1080]));
        let action_fired = Arc::new(AtomicBool::new(false));
        let af_clone = Arc::clone(&action_fired);
        let on_action = Arc::new(std::sync::Mutex::new(Some(Arc::new(
            move |_reg: &InteractiveRegion, _x: f32, _y: f32| {
                af_clone.store(true, Ordering::SeqCst);
            },
        ) as ActionCallback)));

        let last_activity_tick = Arc::new(AtomicU64::new(0));

        let state = DesktopAccessibilityState::new(
            HWND(std::ptr::null_mut()),
            is_interactive,
            interactive_regions,
            focused_region_index,
            current_bounds,
            on_action,
            Arc::clone(&last_activity_tick),
        );

        // 1. Root Provider tests
        let root_simple = create_root_provider(state.clone());
        unsafe {
            let opts = root_simple.ProviderOptions().expect("ProviderOptions");
            assert_eq!(opts, ProviderOptions_ServerSideProvider);

            let v_name = root_simple
                .GetPropertyValue(UIA_NamePropertyId)
                .expect("GetPropertyValue Name");
            assert_eq!(v_name.to_string(), "AetherShell Desktop Overlay");

            let v_ctrl = root_simple
                .GetPropertyValue(UIA_ControlTypePropertyId)
                .expect("GetPropertyValue ControlType");
            assert_eq!(v_ctrl.to_string(), UIA_PaneControlTypeId.0.to_string());

            // Root BoundingRectangle property query
            let v_rect = root_simple
                .GetPropertyValue(UIA_BoundingRectanglePropertyId)
                .expect("Root BoundingRectangle Property");
            assert_ne!(v_rect, VARIANT::default());
        }

        let root_fragment: IRawElementProviderFragment =
            root_simple.cast().expect("cast to fragment");
        unsafe {
            let rect = root_fragment
                .BoundingRectangle()
                .expect("Root BoundingRectangle");
            assert_eq!(rect.left, 100.0);
            assert_eq!(rect.top, 200.0);
            assert_eq!(rect.width, 1920.0);
            assert_eq!(rect.height, 1080.0);

            // HWND-hosted root provider GetRuntimeId must return NULL
            let root_rtid = root_fragment.GetRuntimeId().expect("Root GetRuntimeId");
            assert!(root_rtid.is_null());
        }

        // 2. Navigation tests: Root -> FirstChild (btn_play)
        let first_child = unsafe {
            root_fragment
                .Navigate(NavigateDirection_FirstChild)
                .expect("Navigate FirstChild")
        };
        let first_simple: IRawElementProviderSimple =
            first_child.cast().expect("cast child to simple");
        unsafe {
            let v_name = first_simple
                .GetPropertyValue(UIA_NamePropertyId)
                .expect("Widget 0 Name");
            assert_eq!(v_name.to_string(), "Play Music");

            let v_ctrl = first_simple
                .GetPropertyValue(UIA_ControlTypePropertyId)
                .expect("Widget 0 ControlType");
            assert_eq!(v_ctrl.to_string(), UIA_ButtonControlTypeId.0.to_string());

            let v_focus = first_simple
                .GetPropertyValue(UIA_HasKeyboardFocusPropertyId)
                .expect("Widget 0 Focus");
            assert_eq!(v_focus.to_string(), "-1"); // VARIANT_TRUE (-1)

            let rect = first_child
                .BoundingRectangle()
                .expect("Widget 0 BoundingRectangle");
            assert_eq!(rect.left, 110.0); // 100 screen + 10 local
            assert_eq!(rect.top, 220.0); // 200 screen + 20 local
            assert_eq!(rect.width, 100.0);
            assert_eq!(rect.height, 50.0);

            // Child BoundingRectangle property query
            let v_rect = first_simple
                .GetPropertyValue(UIA_BoundingRectanglePropertyId)
                .expect("Child BoundingRectangle Property");
            assert_ne!(v_rect, VARIANT::default());

            // Non-root fragment provider GetRuntimeId must return valid SAFEARRAY with UiaAppendRuntimeId
            let child_rtid = first_child.GetRuntimeId().expect("Child GetRuntimeId");
            assert!(!child_rtid.is_null());
            let mut ptr: *mut c_void = std::ptr::null_mut();
            SafeArrayAccessData(child_rtid, &mut ptr).expect("SafeArrayAccessData");
            let slice = std::slice::from_raw_parts(ptr as *const i32, 2);
            assert_eq!(slice[0], UiaAppendRuntimeId as i32);
            assert_eq!(slice[1], 1); // 1-based index (0 + 1)
            let _ = SafeArrayUnaccessData(child_rtid);
            let _ = SafeArrayDestroy(child_rtid);
        }

        // 3. Navigation tests: FirstChild -> NextSibling (lbl_title)
        let second_child = unsafe {
            first_child
                .Navigate(NavigateDirection_NextSibling)
                .expect("Navigate NextSibling")
        };
        let second_simple: IRawElementProviderSimple =
            second_child.cast().expect("cast child 1 to simple");
        unsafe {
            let v_name = second_simple
                .GetPropertyValue(UIA_NamePropertyId)
                .expect("Widget 1 Name");
            assert_eq!(v_name.to_string(), "Song Title");

            let v_ctrl = second_simple
                .GetPropertyValue(UIA_ControlTypePropertyId)
                .expect("Widget 1 ControlType");
            assert_eq!(v_ctrl.to_string(), UIA_TextControlTypeId.0.to_string());

            let v_focus = second_simple
                .GetPropertyValue(UIA_HasKeyboardFocusPropertyId)
                .expect("Widget 1 Focus");
            assert_eq!(v_focus.to_string(), "0"); // VARIANT_FALSE (0)
        }

        // 4. Navigation tests: NextSibling on last child must return Error::empty (no next sibling)
        let no_sibling = unsafe { second_child.Navigate(NavigateDirection_NextSibling) };
        assert!(no_sibling.is_err());

        // 5. Navigation tests: SecondChild -> PreviousSibling (btn_play)
        let prev = unsafe {
            second_child
                .Navigate(NavigateDirection_PreviousSibling)
                .expect("Navigate PreviousSibling")
        };
        let prev_simple: IRawElementProviderSimple = prev.cast().expect("cast to simple");
        unsafe {
            let v_name = prev_simple
                .GetPropertyValue(UIA_NamePropertyId)
                .expect("Prev Name");
            assert_eq!(v_name.to_string(), "Play Music");
        }

        // PreviousSibling on first child must return Error::empty (no previous sibling)
        let no_prev = unsafe { first_child.Navigate(NavigateDirection_PreviousSibling) };
        assert!(no_prev.is_err());

        // 6. Navigation: Child -> Parent (Root)
        let parent = unsafe {
            first_child
                .Navigate(NavigateDirection_Parent)
                .expect("Navigate Parent")
        };
        let parent_simple: IRawElementProviderSimple =
            parent.cast().expect("cast parent to simple");
        unsafe {
            let v_name = parent_simple
                .GetPropertyValue(UIA_NamePropertyId)
                .expect("Parent Name");
            assert_eq!(v_name.to_string(), "AetherShell Desktop Overlay");
        }

        // 7. Spatial Hit-testing: ElementProviderFromPoint
        let root_root: IRawElementProviderFragmentRoot =
            root_simple.cast().expect("cast to fragment root");
        unsafe {
            // Screen coords: window is at (100, 200). btn_play is local (10, 20, 100, 50) -> screen (110..210, 220..270)
            let hit = root_root
                .ElementProviderFromPoint(150.0, 250.0)
                .expect("hit test on btn_play");
            let hit_simple: IRawElementProviderSimple = hit.cast().expect("hit simple");
            let v_name = hit_simple
                .GetPropertyValue(UIA_NamePropertyId)
                .expect("Hit name");
            assert_eq!(v_name.to_string(), "Play Music");

            // Point outside any widget but INSIDE root bounds: screen (500, 500) -> returns root
            let miss = root_root
                .ElementProviderFromPoint(500.0, 500.0)
                .expect("hit test inside root but outside child");
            let miss_simple: IRawElementProviderSimple = miss.cast().expect("miss simple");
            let v_name = miss_simple
                .GetPropertyValue(UIA_NamePropertyId)
                .expect("Miss name");
            assert_eq!(v_name.to_string(), "AetherShell Desktop Overlay");

            // Point OUTSIDE root bounds entirely: screen (-50, -50) and (3000, 3000) -> must return NULL per UIA spec
            let outside_neg = root_root.ElementProviderFromPoint(-50.0, -50.0);
            assert!(outside_neg.is_err());
            let outside_pos = root_root.ElementProviderFromPoint(3000.0, 3000.0);
            assert!(outside_pos.is_err());
        }

        // 8. Focus Query: GetFocus
        unsafe {
            let focused = root_root.GetFocus().expect("GetFocus");
            let focused_simple: IRawElementProviderSimple = focused.cast().expect("focused simple");
            let v_name = focused_simple
                .GetPropertyValue(UIA_NamePropertyId)
                .expect("Focused name");
            assert_eq!(v_name.to_string(), "Play Music");
        }

        // 9. Invoke Pattern on Widget 0
        unsafe {
            let unk = first_simple
                .GetPatternProvider(UIA_InvokePatternId)
                .expect("GetPatternProvider Invoke");
            let invoker: IInvokeProvider = unk.cast().expect("cast to IInvokeProvider");
            invoker.Invoke().expect("Invoke");
            assert!(action_fired.load(Ordering::SeqCst));
            assert!(last_activity_tick.load(Ordering::Relaxed) > 0);
        }

        // Widget 1 has no action: GetPatternProvider(UIA_InvokePatternId) must fail
        unsafe {
            let unk = second_simple.GetPatternProvider(UIA_InvokePatternId);
            assert!(unk.is_err());
        }

        // 10. SetFocus on Widget 1
        unsafe {
            let prev_tick = last_activity_tick.load(Ordering::Relaxed);
            second_child.SetFocus().expect("SetFocus");
            assert_eq!(*state.focused_region_index.lock().unwrap(), Some(1));
            assert!(last_activity_tick.load(Ordering::Relaxed) >= prev_tick);
        }
    }

    #[test]
    fn test_passive_mode_focus_and_hit_testing_spec_compliance() {
        let is_interactive = Arc::new(AtomicBool::new(false)); // Passive mode
        let regions = vec![InteractiveRegion::new("btn", 0.0, 0.0, 50.0, 50.0, None)
            .with_accessibility("Test", "button", None)];
        let interactive_regions = Arc::new(std::sync::Mutex::new(regions));
        let focused_region_index = Arc::new(std::sync::Mutex::new(Some(0)));
        let current_bounds = Arc::new(std::sync::Mutex::new([0, 0, 800, 600]));
        let last_activity_tick = Arc::new(AtomicU64::new(0));

        let state = DesktopAccessibilityState::new(
            HWND(std::ptr::null_mut()),
            is_interactive,
            interactive_regions,
            focused_region_index,
            current_bounds,
            Arc::new(std::sync::Mutex::new(None)),
            last_activity_tick,
        );

        let root_simple = create_root_provider(state.clone());
        let root_root: IRawElementProviderFragmentRoot = root_simple.cast().expect("cast root");

        // In passive mode, GetFocus must return NULL (Error)
        unsafe {
            let focus_result = root_root.GetFocus();
            assert!(focus_result.is_err());
        }

        // Child widget in passive mode must report HasKeyboardFocus == false (0) even if focused_region_index is set
        let child = DesktopWidgetFragmentProvider::new(state.clone(), 0);
        let child_simple: IRawElementProviderSimple = child.into();
        unsafe {
            let v_focus = child_simple
                .GetPropertyValue(UIA_HasKeyboardFocusPropertyId)
                .expect("Focus in passive mode");
            assert_eq!(v_focus.to_string(), "0");
        }
    }
}
