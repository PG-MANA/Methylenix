//!
//! EFI Functions Manager
//!
//! This manager handles EFI services.
//! Currently, this is not used.

pub mod table;
pub mod text;

use self::table::EfiTableManager;
use self::text::output::EfiTextOutputManager;

pub type EfiStatus = usize;

pub const EFI_SUCCESS: EfiStatus = 0;

pub struct EfiManager {
    pub is_valid: bool,
    pub table_manager: EfiTableManager,
    pub output_manager: EfiTextOutputManager,
}

impl EfiManager {
    pub const fn new() -> Self {
        EfiManager {
            is_valid: false,
            table_manager: EfiTableManager::new(),
            output_manager: EfiTextOutputManager::new(),
        }
    }

    pub fn init(&mut self, table_address: usize) -> bool {
        self.table_manager.init(table_address);
        self.output_manager.init(
            self.table_manager
                .get_efi_system_table()
                .console_output_protocol,
        );

        self.is_valid = true;
        return true;
    }
}
