//! Purpose:
//! Holds per-function state while the EIR backend lowers SSA instructions to assembly.
//! Provides table lookups, value-slot loads/stores, data-pool access, and label creation.
//!
//! Called from:
//! - `crate::codegen_ir::block_emit`, `crate::codegen_ir::lower_inst`, and
//!   `crate::codegen_ir::lower_term`.
//!
//! Key details:
//! - Phase 04 stores every SSA value in a stack slot and reloads result registers at use sites.
//! - The context delegates target-specific movement to `crate::codegen::abi`.

use crate::codegen::abi;
use crate::codegen::data_section::DataSection;
use crate::codegen::emit::Emitter;
use crate::ir::{DataId, Function, Module, ValueId};
use crate::types::PhpType;

use super::value_placement::ValuePlacement;
use super::{CodegenIrError, Result};

/// Mutable backend state for one EIR function.
pub(super) struct FunctionContext<'a> {
    pub(super) module: &'a Module,
    pub(super) function: &'a Function,
    pub(super) emitter: &'a mut Emitter,
    pub(super) data: &'a mut DataSection,
    pub(super) placement: ValuePlacement,
    pub(super) frame_size: usize,
    pub(super) epilogue_emitted: bool,
    label_counter: usize,
}

impl<'a> FunctionContext<'a> {
    /// Creates a lowering context with finalized frame and value-placement metadata.
    pub(super) fn new(
        module: &'a Module,
        function: &'a Function,
        emitter: &'a mut Emitter,
        data: &'a mut DataSection,
        placement: ValuePlacement,
        frame_size: usize,
    ) -> Self {
        Self {
            module,
            function,
            emitter,
            data,
            placement,
            frame_size,
            epilogue_emitted: false,
            label_counter: 0,
        }
    }

    /// Returns a unique local label with a readable prefix.
    pub(super) fn next_label(&mut self, prefix: &str) -> String {
        let label = format!("_eir_{}_{}", prefix, self.label_counter);
        self.label_counter += 1;
        label
    }

    /// Returns the assembly label for a non-entry EIR block.
    pub(super) fn block_label(&self, block_name: &str, raw: u32) -> String {
        format!("_eir_{}_{}_{}", label_fragment(&self.function.name), label_fragment(block_name), raw)
    }

    /// Returns a function value or a structured backend error.
    pub(super) fn value_php_type(&self, value: ValueId) -> Result<PhpType> {
        self.function
            .value(value)
            .map(|metadata| metadata.php_type.codegen_repr())
            .ok_or_else(|| CodegenIrError::missing_entry("value", value.as_raw()))
    }

    /// Loads a stored SSA value into the target's canonical result register(s).
    pub(super) fn load_value_to_result(&mut self, value: ValueId) -> Result<PhpType> {
        let ty = self.value_php_type(value)?;
        let offset = self.value_offset(value)?;
        abi::emit_load(self.emitter, &ty, offset);
        Ok(ty)
    }

    /// Stores the current result register(s) into the SSA value's fixed stack slot.
    pub(super) fn store_result_value(&mut self, value: ValueId) -> Result<()> {
        let ty = self.value_php_type(value)?;
        let offset = self.value_offset(value)?;
        match &ty {
            PhpType::Str => {
                let (ptr_reg, len_reg) = abi::string_result_regs(self.emitter);
                abi::store_at_offset(self.emitter, ptr_reg, offset);
                abi::store_at_offset(self.emitter, len_reg, offset - 8);
            }
            PhpType::Float => {
                abi::store_at_offset(self.emitter, abi::float_result_reg(self.emitter), offset);
            }
            PhpType::Void | PhpType::Never => {}
            _ => {
                abi::store_at_offset(self.emitter, abi::int_result_reg(self.emitter), offset);
            }
        }
        Ok(())
    }

    /// Interns a module data-pool string into the assembly data section.
    pub(super) fn intern_string_data(&mut self, data_id: DataId) -> Result<(String, usize)> {
        let value = self
            .module
            .data
            .strings
            .get(data_id.as_raw() as usize)
            .ok_or_else(|| CodegenIrError::missing_entry("data string", data_id.as_raw()))?;
        Ok(self.data.add_string(value.as_bytes()))
    }

    /// Returns the frame offset assigned to a value by Phase 04 placement.
    fn value_offset(&self, value: ValueId) -> Result<usize> {
        self.placement
            .slot(value)
            .ok_or_else(|| CodegenIrError::missing_entry("value slot", value.as_raw()))
    }
}

/// Converts arbitrary names into assembly-label-safe fragments.
fn label_fragment(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}
