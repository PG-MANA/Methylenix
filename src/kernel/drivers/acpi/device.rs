//!
//! ACPI Devices
//!

pub mod ec;
pub mod pm_timer;

use self::ec::EmbeddedController;
use self::pm_timer::AcpiPmTimer;

pub struct AcpiDeviceManager {
    pub(super) ec: Option<EmbeddedController>,
    pub(super) pm_timer: Option<AcpiPmTimer>,
}

impl AcpiDeviceManager {
    pub const fn new() -> Self {
        Self {
            ec: None,
            pm_timer: None,
        }
    }

    pub const fn is_pm_timer_available(&self) -> bool {
        self.pm_timer.is_some()
    }

    pub const fn get_pm_timer(&self) -> Option<&AcpiPmTimer> {
        self.pm_timer.as_ref()
    }

    pub const fn get_embedded_controller(&self) -> Option<&EmbeddedController> {
        self.ec.as_ref()
    }
}
