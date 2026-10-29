//! Process memory management and working set optimization utilities.

use windows::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};

/// Trims the current process working set using Win32 `SetProcessWorkingSetSize`.
///
/// Under Decision Q47 and PRD Section 14 (Gate G13):
/// Calling `SetProcessWorkingSetSize(handle, usize::MAX, usize::MAX)` requests the Windows
/// Memory Manager to remove unreferenced/unpaged pages from the process working set,
/// releasing physical RAM back to the operating system without terminating or corrupting process state.
pub fn trim_working_set() -> bool {
    unsafe {
        let handle = GetCurrentProcess();
        SetProcessWorkingSetSize(handle, usize::MAX, usize::MAX).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trim_working_set_succeeds() {
        // Calling trim_working_set on the current process must succeed without errors
        let res = trim_working_set();
        assert!(
            res,
            "SetProcessWorkingSetSize(MAX, MAX) must return true for current process"
        );
    }
}
