//!
//! ACPI Machine Language Opcode
//!

/* Data Objects */
pub const ZERO_OP: u8 = 0x00;
pub const ONE_OP: u8 = 0x01;
pub const ONES_OP: u8 = 0xFF;
pub const REVISION_OP: u8 = 0x30;
pub const EXT_OP_PREFIX: u8 = 0x5B;

/*  Expression Opcodes */
pub const VAR_PACKAGE_OP: u8 = 0x13;
pub const ACQUIRE_OP: u8 = 0x23;
pub const ADD_OP: u8 = 0x72;
pub const AND_OP: u8 = 0x7B;
pub const BUFFER_OP: u8 = 0x11;
pub const CONCAT_OP: u8 = 0x73;
pub const CONCAT_RES_OP: u8 = 0x84;
pub const COND_REF_OF_OP: u8 = 0x12;
pub const COPY_OBJECT_OP: u8 = 0x9D;
pub const DECREMENT_OP: u8 = 0x76;
pub const DEREF_OF_OP: u8 = 0x83;
pub const DIVIDE_OP: u8 = 0x78;
pub const FIND_SET_LEFT_BIT_OP: u8 = 0x81;
pub const FIND_SET_RIGHT_BIT_OP: u8 = 0x82;
pub const FROM_BCD_OP: u8 = 0x28;
pub const INCREMENT_OP: u8 = 0x75;
pub const INDEX_OP: u8 = 0x88;
pub const L_AND_OP: u8 = 0x90;
pub const L_EQUAL_OP: u8 = 0x93;
pub const L_GREATER_OP: u8 = 0x94;
pub const L_LESS_OP: u8 = 0x95;
pub const L_NOT_OP: u8 = 0x92;
pub const LOAD_OP: u8 = 0x20;
pub const LOAD_TABLE_OP: u8 = 0x1F;
pub const L_OR_OP: u8 = 0x91;
pub const MATCH_OP: u8 = 0x89;
pub const MID_OP: u8 = 0x9E;
pub const MOD_OP: u8 = 0x85;
pub const MULTIPLY_OP: u8 = 0x77;
pub const NAND_OP: u8 = 0x7C;
pub const NOR_OP: u8 = 0x7E;
pub const NOT_OP: u8 = 0x80;
pub const OBJECT_TYPE_OP: u8 = 0x8E;
pub const OR_OP: u8 = 0x7D;
pub const PACKAGE_OP: u8 = 0x12;
pub const REF_OF_OP: u8 = 0x71;
pub const SHIFT_LEFT_OP: u8 = 0x79;
pub const SHIFT_RIGHT_OP: u8 = 0x7A;
pub const SIZE_OF_OP: u8 = 0x87;
pub const STORE_OP: u8 = 0x70;
pub const SUBTRACT_OP: u8 = 0x74;
pub const TIMER_OP: u8 = 0x33;
pub const TO_BCD_OP: u8 = 0x29;
pub const TO_BUFFER_OP: u8 = 0x96;
pub const TO_DECIMAL_STRING_OP: u8 = 0x97;
pub const TO_HEX_STRING_OP: u8 = 0x98;
pub const TO_INTEGER_OP: u8 = 0x99;
pub const TO_STRING_OP: u8 = 0x9C;
pub const WAIT_OP: u8 = 0x25;
pub const XOR_OP: u8 = 0x7F;

/*  Namespace Modifier Objects */
pub const ALIAS_OP: u8 = 0x06;
pub const SCOPE_OP: u8 = 0x10;
pub const NAME_OP: u8 = 0x08;

/* Named Objects */
pub const BANK_FIELD_OP: u8 = 0x87;
pub const CREATE_BIT_FIELD_OP: u8 = 0x8D;
pub const CREATE_BYTE_FIELD_OP: u8 = 0x8C;
pub const CREATE_WORD_FIELD_OP: u8 = 0x8B;
pub const CREATE_DOUBLE_WORD_FIELD_OP: u8 = 0x8A;
pub const CREATE_QUAD_WORD_FIELD_OP: u8 = 0x8F;
pub const CREATE_FIELD_OP: u8 = 0x13;
pub const DATA_REGION_OP: u8 = 0x88;
pub const DEVICE_OP: u8 = 0x82;
pub const EVENT_OP: u8 = 0x02;
pub const EXTERNAL_OP: u8 = 0x15;
pub const FIELD_OP: u8 = 0x81;
pub const INDEX_FIELD_OP: u8 = 0x86;
pub const METHOD_OP: u8 = 0x14;
pub const MUTEX_OP: u8 = 0x01;
pub const OP_REGION_OP: u8 = 0x80;
pub const POWER_RES_OP: u8 = 0x84;
pub const PROCESSOR_OP: u8 = 0x83;
pub const THERMAL_ZONE_OP: u8 = 0x85;

/* Statement Opcodes */
pub const BREAK_OP: u8 = 0xA5;
pub const BREAK_POINT_OP: u8 = 0xCC;
pub const CONTINUE_OP: u8 = 0x9F;
pub const ELSE_OP: u8 = 0xA1;
pub const FATAL_OP: u8 = 0x32;
pub const IF_OP: u8 = 0xA0;
pub const NOOP_OP: u8 = 0xA3;
pub const NOTIFY_OP: u8 = 0x86;
pub const RELEASE_OP: u8 = 0x27;
pub const RESET_OP: u8 = 0x26;
pub const RETURN_OP: u8 = 0xA4;
pub const SIGNAL_OP: u8 = 0x24;
pub const SLEEP_OP: u8 = 0x22;
pub const STALL_OP: u8 = 0x21;
pub const WHILE_OP: u8 = 0xA2;

/* Local Objects */
pub const LOCAL0_OP: u8 = 0x60;
pub const LOCAL7_OP: u8 = 0x67;

/* Arg Objects */
pub const ARG0_OP: u8 = 0x68;
pub const ARG6_OP: u8 = 0x6E;

/* Debug Objects */
pub const DEBUG_OP: u8 = 0x31;
