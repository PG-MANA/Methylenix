//!
//! ACPI Machine Language Named Objects
//!
#![allow(dead_code)]

use super::data_object::PkgLength;
use super::name_object::NameString;
use super::opcode;
use super::term_object::{TermArg, TermList};
use super::{AcpiInt, AmlError, AmlStream, Evaluator};

#[derive(Debug, Clone)]
pub struct BankField {
    region_name: NameString,
    bank_name: NameString,
    bank_value: TermArg,
    field_flags: u8,
    field_list: FieldList,
}

impl BankField {
    fn parse(
        stream: &mut AmlStream,
        current_scope: &NameString,
        evaluator: &mut Evaluator,
    ) -> Result<Self, AmlError> {
        /* BankFieldOp was read */
        let pkg_length = PkgLength::parse(stream)?;
        let mut bank_field_stream = stream.clone();
        stream.seek(pkg_length.actual_length)?;
        bank_field_stream.change_size(pkg_length.actual_length)?;
        let region_name = NameString::parse(&mut bank_field_stream, Some(current_scope))?;
        let bank_name = NameString::parse(&mut bank_field_stream, Some(current_scope))?;
        let bank_value = TermArg::parse_integer(&mut bank_field_stream, current_scope, evaluator)?;
        let field_flags = bank_field_stream.read_byte()?;
        let field_list = FieldList::new(bank_field_stream, current_scope)?;
        Ok(Self {
            region_name,
            bank_name,
            bank_value,
            field_flags,
            field_list,
        })
    }
}

#[derive(Eq, PartialEq, Debug, Clone)]
pub enum CreateFieldType {
    Bit,
    Byte = 0x1,
    Word = 0x2,
    DWord = 0x4,
    QWord = 0x8,
    Other,
}

#[derive(Debug, Clone)]
pub struct CreateField {
    size: CreateFieldType,
    source_buffer: TermArg,
    index: TermArg,
    name: NameString,
    optional_size: Option<TermArg>,
}

impl CreateField {
    fn parse(
        stream: &mut AmlStream,
        current_scope: &NameString,
        field_type: CreateFieldType,
        evaluator: &mut Evaluator,
    ) -> Result<Self, AmlError> {
        /* Op was read */
        let source_buffer = TermArg::try_parse(stream, current_scope, evaluator)?;
        let index = TermArg::parse_integer(stream, current_scope, evaluator)?;
        let optional_size = if field_type == CreateFieldType::Other {
            Some(TermArg::parse_integer(stream, current_scope, evaluator)?)
        } else {
            None
        };
        let name = NameString::parse(stream, Some(current_scope))?;
        Ok(Self {
            size: field_type,
            source_buffer,
            index,
            name,
            optional_size,
        })
    }

    pub fn get_name(&self) -> &NameString {
        &self.name
    }

    pub fn get_source_buffer(&self) -> &TermArg {
        &self.source_buffer
    }

    pub fn get_index(&self) -> &TermArg {
        &self.index
    }

    pub fn is_bit_field(&self) -> bool {
        self.size == CreateFieldType::Bit || self.size == CreateFieldType::Other
    }

    pub fn get_source_size(&self) -> Option<usize> {
        if self.size == CreateFieldType::Other {
            None
        } else if self.size == CreateFieldType::Bit {
            Some(1)
        } else {
            Some(self.size.clone() as usize)
        }
    }

    pub fn get_source_size_term_arg(&self) -> &Option<TermArg> {
        &self.optional_size
    }
}

#[derive(Debug, Clone)]
pub struct DataRegion {
    name: NameString,
    term_args: [TermArg; 3],
}

impl DataRegion {
    fn parse(
        stream: &mut AmlStream,
        current_scope: &NameString,
        evaluator: &mut Evaluator,
    ) -> Result<Self, AmlError> {
        /* DataRegionOp was read */
        let name = NameString::parse(stream, Some(current_scope))?;
        let term_arg1 = TermArg::try_parse(stream, current_scope, evaluator)?;
        let term_arg2 = TermArg::try_parse(stream, current_scope, evaluator)?;
        let term_arg3 = TermArg::try_parse(stream, current_scope, evaluator)?;
        Ok(Self {
            name,
            term_args: [term_arg1, term_arg2, term_arg3],
        })
    }

    pub fn get_name(&self) -> &NameString {
        &self.name
    }
}

#[derive(Debug, Clone)]
pub struct External {
    name: NameString,
    object_type: u8,
    argument_count: u8,
}

impl External {
    fn parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        /* ExternalOp was read */
        let name = NameString::parse(stream, Some(current_scope))?;
        let object_type = stream.read_byte()?;
        let argument_count = stream.read_byte()?;
        Ok(Self {
            name,
            object_type,
            argument_count,
        })
    }

    pub fn new(name: NameString, object_type: u8, argument_count: u8) -> Self {
        /* Used to define builtin functions */
        Self {
            name,
            object_type,
            argument_count,
        }
    }

    pub fn get_name(&self) -> &NameString {
        &self.name
    }

    pub fn get_argument_count(&self) -> AcpiInt {
        self.argument_count as _
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
#[repr(u8)]
pub enum OperationRegionType {
    SystemMemory = 0,
    SystemIO = 1,
    PciConfig,
    EmbeddedControl,
    SMBus,
    SystemCMOS,
    PciBarTarget,
    IPMI,
    GeneralPurposeIO,
    GenericSerialBus,
    PCC,
}

#[derive(Debug, Clone)]
pub struct OpRegion {
    name: NameString,
    region_scope: u8,
    region_offset: TermArg,
    region_len: TermArg,
}

impl OpRegion {
    fn parse(
        stream: &mut AmlStream,
        current_scope: &NameString,
        evaluator: &mut Evaluator,
    ) -> Result<Self, AmlError> {
        /* OpRegionOp was read */
        let name = NameString::parse(stream, Some(current_scope))?;
        let region_scope = stream.read_byte()?;
        let region_offset = TermArg::parse_integer(stream, current_scope, evaluator)?;
        let region_len = TermArg::parse_integer(stream, current_scope, evaluator)?;
        Ok(Self {
            name,
            region_scope,
            region_offset,
            region_len,
        })
    }

    pub fn get_name(&self) -> &NameString {
        &self.name
    }

    pub fn get_operation_type(&self) -> Result<OperationRegionType, AmlError> {
        if self.region_scope > 0x0A {
            Err(AmlError::UnsupportedType)
        } else {
            Ok(unsafe { core::mem::transmute::<u8, OperationRegionType>(self.region_scope) })
        }
    }

    pub fn get_region_offset(&self) -> &TermArg {
        &self.region_offset
    }

    pub fn get_region_length(&self) -> &TermArg {
        &self.region_len
    }
}

#[derive(Debug, Clone)]
pub struct PowerRes {
    name: NameString,
    system_level: u8,
    resource_order: u16,
    term_list: TermList,
}

impl PowerRes {
    fn parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        /* PowerResOp was read */
        let pkg_length = PkgLength::parse(stream)?;
        let mut power_res_stream = stream.clone();
        stream.seek(pkg_length.actual_length)?;
        power_res_stream.change_size(pkg_length.actual_length)?;

        let name = NameString::parse(&mut power_res_stream, Some(current_scope))?;
        let system_level = power_res_stream.read_byte()?;
        let resource_order = power_res_stream.read_word()?;
        let term_list = TermList::new(power_res_stream, name.clone());
        Ok(Self {
            name,
            system_level,
            resource_order,
            term_list,
        })
    }

    pub fn get_name(&self) -> &NameString {
        &self.name
    }
}

#[derive(Debug, Clone)]
pub struct Device {
    device_name: NameString,
    term_list: TermList,
}

impl Device {
    fn parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        /* DeviceOp was read */
        let pkg_length = PkgLength::parse(stream)?;
        let mut device_stream = stream.clone();
        stream.seek(pkg_length.actual_length)?;
        device_stream.change_size(pkg_length.actual_length)?;
        let device_name = NameString::parse(&mut device_stream, Some(current_scope))?;
        let term_list = TermList::new(device_stream, device_name.clone());
        Ok(Self {
            device_name,
            term_list,
        })
    }

    pub const fn get_name(&self) -> &NameString {
        &self.device_name
    }

    pub const fn get_term_list(&self) -> &TermList {
        &self.term_list
    }
}

#[derive(Debug, Clone)]
pub struct ThermalZone {
    name: NameString,
    term_list: TermList,
}

impl ThermalZone {
    fn parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        /* PowerResOp was read */
        let pkg_length = PkgLength::parse(stream)?;
        let mut thermal_zone_stream = stream.clone();
        stream.seek(pkg_length.actual_length)?;
        thermal_zone_stream.change_size(pkg_length.actual_length)?;

        let name = NameString::parse(&mut thermal_zone_stream, Some(current_scope))?;
        let term_list = TermList::new(thermal_zone_stream, name.clone());
        Ok(Self { name, term_list })
    }

    pub fn get_name(&self) -> &NameString {
        &self.name
    }
}

#[derive(Debug, Clone)]
pub struct Method {
    name: NameString,
    method_flags: u8,
    term_list: TermList,
}

impl Method {
    pub fn parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        /* MethodOp was read */
        let pkg_length = PkgLength::parse(stream)?;
        let mut method_stream = stream.clone();
        stream.seek(pkg_length.actual_length)?;
        method_stream.change_size(pkg_length.actual_length)?;
        let name = NameString::parse(&mut method_stream, Some(current_scope))?;
        let method_flags = method_stream.read_byte()?;
        let term_list = TermList::new(method_stream, name.clone());
        Ok(Self {
            name,
            method_flags,
            term_list,
        })
    }

    pub fn get_name(&self) -> &NameString {
        &self.name
    }

    pub fn get_argument_count(&self) -> AcpiInt {
        (self.method_flags & 0b111) as _
    }

    pub fn get_term_list(&self) -> &TermList {
        &self.term_list
    }
}

#[derive(Debug, Clone)]
pub struct Field {
    region_name: NameString,
    field_flags: u8,
    field_list: FieldList,
}

impl Field {
    pub fn parse(stream: &mut AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        /* FieldOp was read */
        let pkg_length = PkgLength::parse(stream)?;
        let mut field_stream = stream.clone();
        stream.seek(pkg_length.actual_length)?;
        field_stream.change_size(pkg_length.actual_length)?;
        let region_name = NameString::parse(&mut field_stream, Some(current_scope))?;
        let field_flags = field_stream.read_byte()?;
        let field_list = FieldList::new(field_stream, current_scope)?;
        Ok(Self {
            region_name,
            field_flags,
            field_list,
        })
    }

    pub fn get_source_region_name(&self) -> &NameString {
        &self.region_name
    }

    pub fn convert_to_access_size(flags: u8) -> usize {
        match flags & 0b111 {
            0 => {
                0 /*Any Access*/
            }
            1 => 1,
            2 => 2,
            3 => 4,
            4 => 8,
            5 => {
                pr_warn!("Buffer Access was not supported.");
                0
            }
            _ => {
                pr_warn!("Unknown Access Type.");
                0
            }
        }
    }

    pub fn get_access_size(&self) -> usize {
        Self::convert_to_access_size(self.field_flags)
    }

    pub fn should_lock(&self) -> bool {
        (self.field_flags & (1 << 4)) != 0
    }

    pub fn get_update_rule(&self) -> u8 {
        (self.field_flags >> 5) & 0b11
    }

    pub fn get_field_list(&self) -> &FieldList {
        &self.field_list
    }
}

#[derive(Debug, Clone)]
pub struct IndexField {
    index_register_name: NameString,
    /* The register(ByteField) to send access index */
    data_register_name: NameString,
    /* The register(ByteField) to read/write data.*/
    field_flags: u8,
    field_list: FieldList,
}

impl IndexField {
    pub(crate) fn parse(
        stream: &mut AmlStream,
        current_scope: &NameString,
    ) -> Result<Self, AmlError> {
        /* IndexFieldOp was read */
        let pkg_length = PkgLength::parse(stream)?;
        let mut index_field_stream = stream.clone();
        stream.seek(pkg_length.actual_length)?;
        index_field_stream.change_size(pkg_length.actual_length)?;
        let index_register_name = NameString::parse(&mut index_field_stream, Some(current_scope))?;
        let data_register_name = NameString::parse(&mut index_field_stream, Some(current_scope))?;
        let field_flags = index_field_stream.read_byte()?;
        let field_list = FieldList::new(index_field_stream, current_scope)?;
        Ok(Self {
            index_register_name,
            data_register_name,
            field_flags,
            field_list,
        })
    }

    pub fn get_index_register(&self) -> &NameString {
        &self.index_register_name
    }

    pub fn get_data_register(&self) -> &NameString {
        &self.data_register_name
    }

    pub fn get_access_size(&self) -> usize {
        Field::convert_to_access_size(self.field_flags)
    }

    pub fn should_lock(&self) -> bool {
        (self.field_flags & (1 << 4)) != 0
    }

    pub fn get_field_list(&self) -> &FieldList {
        &self.field_list
    }
}

#[derive(Debug, Clone)]
pub enum NamedObject {
    DefBankField(BankField),
    DefCreateField(CreateField),
    DefDataRegion(DataRegion),
    DefDevice(Device),
    DefField(Field),
    DefEvent(NameString),
    DefIndexField(IndexField),
    DefMethod(Method),
    DefMutex((NameString, u8)),
    DefExternal(External),
    DefOpRegion(OpRegion),
    DefPowerRes(PowerRes),
    DefThermalZone(ThermalZone),
}

impl NamedObject {
    pub fn try_parse(
        stream: &mut AmlStream,
        current_scope: &NameString,
        evaluator: &mut Evaluator,
    ) -> Result<Self, AmlError> {
        let first_byte = stream.peek_byte()?;
        /* println!("NamedObject: {:#X}", first_byte); */
        match first_byte {
            opcode::EXT_OP_PREFIX => {
                match stream.peek_byte_with_pos(1)? {
                    opcode::BANK_FIELD_OP => {
                        /* DefBankField */
                        stream.seek(2)?;
                        Ok(Self::DefBankField(BankField::parse(
                            stream,
                            current_scope,
                            evaluator,
                        )?))
                    }
                    opcode::CREATE_FIELD_OP => {
                        /* DefCreateField */
                        stream.seek(2)?;
                        Ok(Self::DefCreateField(CreateField::parse(
                            stream,
                            current_scope,
                            CreateFieldType::Other,
                            evaluator,
                        )?))
                    }
                    opcode::DATA_REGION_OP => {
                        /* DefDataRegion */
                        stream.seek(2)?;
                        Ok(Self::DefDataRegion(DataRegion::parse(
                            stream,
                            current_scope,
                            evaluator,
                        )?))
                    }
                    opcode::DEVICE_OP => {
                        stream.seek(2)?;
                        Ok(Self::DefDevice(Device::parse(stream, current_scope)?))
                    }
                    opcode::MUTEX_OP => {
                        stream.seek(2)?;
                        let name = NameString::parse(stream, Some(current_scope))?;
                        let flags = stream.read_byte()?;
                        Ok(Self::DefMutex((name, flags)))
                    }
                    opcode::FIELD_OP => {
                        stream.seek(2)?;
                        Ok(Self::DefField(Field::parse(stream, current_scope)?))
                    }
                    opcode::INDEX_FIELD_OP => {
                        stream.seek(2)?;
                        Ok(Self::DefIndexField(IndexField::parse(
                            stream,
                            current_scope,
                        )?))
                    }
                    opcode::EVENT_OP => {
                        stream.seek(2)?;
                        Ok(Self::DefEvent(NameString::parse(
                            stream,
                            Some(current_scope),
                        )?))
                    }
                    opcode::OP_REGION_OP => {
                        /* DefOpRegion */
                        stream.seek(2)?;
                        Ok(Self::DefOpRegion(OpRegion::parse(
                            stream,
                            current_scope,
                            evaluator,
                        )?))
                    }
                    opcode::POWER_RES_OP => {
                        /* DefPowerRes */
                        stream.seek(2)?;
                        Ok(Self::DefPowerRes(PowerRes::parse(stream, current_scope)?))
                    }
                    opcode::THERMAL_ZONE_OP => {
                        /* DefThermalZone */
                        stream.seek(2)?;
                        Ok(Self::DefThermalZone(ThermalZone::parse(
                            stream,
                            current_scope,
                        )?))
                    }
                    _ => Err(AmlError::InvalidType),
                }
            }
            opcode::CREATE_BIT_FIELD_OP => {
                /* DefCreateBitField */
                stream.seek(1)?;
                Ok(Self::DefCreateField(CreateField::parse(
                    stream,
                    current_scope,
                    CreateFieldType::Bit,
                    evaluator,
                )?))
            }
            opcode::CREATE_BYTE_FIELD_OP => {
                /* DefCreateByteField */
                stream.seek(1)?;
                Ok(Self::DefCreateField(CreateField::parse(
                    stream,
                    current_scope,
                    CreateFieldType::Byte,
                    evaluator,
                )?))
            }
            opcode::CREATE_WORD_FIELD_OP => {
                /* DefCreateWordField */
                stream.seek(1)?;
                Ok(Self::DefCreateField(CreateField::parse(
                    stream,
                    current_scope,
                    CreateFieldType::Word,
                    evaluator,
                )?))
            }
            opcode::CREATE_DOUBLE_WORD_FIELD_OP => {
                /* DefCreateDWordField */
                stream.seek(1)?;
                Ok(Self::DefCreateField(CreateField::parse(
                    stream,
                    current_scope,
                    CreateFieldType::DWord,
                    evaluator,
                )?))
            }
            opcode::CREATE_QUAD_WORD_FIELD_OP => {
                /* DefCreateQWordField */
                stream.seek(1)?;
                Ok(Self::DefCreateField(CreateField::parse(
                    stream,
                    current_scope,
                    CreateFieldType::QWord,
                    evaluator,
                )?))
            }
            opcode::EXTERNAL_OP => {
                /* DefExternal */
                stream.seek(1)?;
                Ok(Self::DefExternal(External::parse(stream, current_scope)?))
            }
            opcode::METHOD_OP => {
                /* DefMethod */
                stream.seek(1)?;
                Ok(Self::DefMethod(Method::parse(stream, current_scope)?))
            }
            _ => Err(AmlError::InvalidType),
        }
    }

    pub fn get_name(&self) -> Option<&NameString> {
        match self {
            Self::DefCreateField(c) => Some(c.get_name()),
            Self::DefMethod(m) => Some(m.get_name()),
            Self::DefMutex((n, _)) => Some(n),
            Self::DefDataRegion(d) => Some(d.get_name()),
            Self::DefExternal(d) => Some(d.get_name()),
            Self::DefThermalZone(z) => Some(z.get_name()),
            Self::DefPowerRes(p) => Some(p.get_name()),
            Self::DefOpRegion(o) => Some(o.get_name()),
            Self::DefDevice(d) => Some(d.get_name()),
            Self::DefEvent(n) => Some(n),
            Self::DefBankField(_) => None,
            Self::DefField(_) => None,
            Self::DefIndexField(_) => None,
        }
    }

    pub fn get_argument_count(&self) -> Option<AcpiInt> {
        match self {
            NamedObject::DefMethod(m) => Some(m.get_argument_count()),
            NamedObject::DefExternal(e) => Some(e.get_argument_count()),
            _ => Some(0),
        }
    }

    pub fn get_field_list(&self) -> Option<FieldList> {
        match self {
            NamedObject::DefBankField(b_f) => Some(b_f.field_list.clone()),
            NamedObject::DefField(d_f) => Some(d_f.field_list.clone()),
            NamedObject::DefIndexField(d_i) => Some(d_i.field_list.clone()),
            _ => None,
        }
    }

    pub fn get_term_list(&self) -> Option<TermList> {
        match self {
            NamedObject::DefDevice(d_d) => Some(d_d.term_list.clone()),
            NamedObject::DefMethod(d_m) => Some(d_m.term_list.clone()),
            NamedObject::DefPowerRes(d_p) => Some(d_p.term_list.clone()),
            NamedObject::DefThermalZone(d_t) => Some(d_t.term_list.clone()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FieldList {
    stream: AmlStream,
    current_scope: NameString,
}

impl FieldList {
    fn new(stream: AmlStream, current_scope: &NameString) -> Result<Self, AmlError> {
        Ok(Self {
            stream,
            current_scope: current_scope.clone(),
        })
    }

    pub fn next(&mut self) -> Result<Option<FieldElement>, AmlError> {
        if self.stream.is_end_of_stream() {
            Ok(None)
        } else {
            Ok(Some(match self.stream.peek_byte()? {
                0 => {
                    self.stream.seek(1)?;
                    FieldElement::ReservedField(PkgLength::parse(&mut self.stream)?)
                }
                1 => {
                    self.stream.seek(1)?;
                    let access_type = self.stream.read_byte()?;
                    let access_attribute = self.stream.read_byte()?;
                    FieldElement::AccessField((access_type, access_attribute))
                }
                2 => {
                    self.stream.seek(1)?;
                    FieldElement::ConnectField(
                        NameString::parse(&mut self.stream, Some(&self.current_scope))
                            .or(Err(AmlError::UnsupportedType))?,
                    )
                    /* BufferData was not supported */
                }
                3 => {
                    self.stream.seek(1)?;
                    let access_type = self.stream.read_byte()?;
                    let extended_access_attribute = self.stream.read_byte()?;
                    let access_length = self.stream.read_byte()?;
                    FieldElement::ExtendedAccessField([
                        access_type,
                        extended_access_attribute,
                        access_length,
                    ])
                }
                _ => {
                    let name = NameString::parse(&mut self.stream, Some(&self.current_scope))
                        .or(Err(AmlError::InvalidType))?;
                    let pkg_length = PkgLength::parse(&mut self.stream)?;
                    FieldElement::NameField((name, pkg_length))
                }
            }))
        }
    }
}

#[derive(Debug)]
pub enum FieldElement {
    ReservedField(PkgLength),
    AccessField((u8, u8)),
    ExtendedAccessField([u8; 3] /* Temporary */),
    ConnectField(NameString),
    NameField((NameString, PkgLength)),
}
