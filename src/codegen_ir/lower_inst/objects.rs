//! Purpose:
//! Lowers object metadata opcodes for the Phase 04 EIR backend.
//! Supports simple object allocation, declared property access, and named `instanceof` checks.
//!
//! Called from:
//! - `crate::codegen_ir::lower_inst::lower_instruction()`.
//!
//! Key details:
//! - Object payload layout must match the legacy backend and runtime helpers:
//!   heap kind word before payload, class id at payload offset 0, then 16 bytes
//!   per declared property slot.
//! - This slice intentionally rejects constructors, dynamic properties, references,
//!   default property expressions, and method metadata until their runtime paths land.

use crate::codegen::abi;
use crate::codegen::platform::Arch;
use crate::codegen::UNINITIALIZED_TYPED_PROPERTY_SENTINEL;
use crate::ir::Instruction;
use crate::types::PhpType;

use super::super::context::FunctionContext;
use super::{expect_data, expect_operand, store_if_result};
use crate::codegen_ir::{CodegenIrError, Result};

const X86_64_HEAP_MAGIC_HI32: u64 = 0x454C5048;

/// Resolved declared-property storage metadata for a known object receiver.
struct PropertySlot {
    class_name: String,
    property: String,
    php_type: PhpType,
    offset: usize,
    is_declared: bool,
}

/// Lowers fixed-class object allocation for classes without constructor work.
pub(super) fn lower_object_new(ctx: &mut FunctionContext<'_>, inst: &Instruction) -> Result<()> {
    if !inst.operands.is_empty() {
        return Err(CodegenIrError::unsupported(
            "object construction with constructor arguments",
        ));
    }
    let class_name = class_name_immediate(ctx, inst)?.to_string();
    let (class_id, property_count, uninitialized_marker_offsets) = {
        let class_info = ctx
            .module
            .class_infos
            .get(&class_name)
            .ok_or_else(|| CodegenIrError::unsupported(format!("unknown class {}", class_name)))?;
        if class_info.allow_dynamic_properties
            || class_info.defaults.iter().any(Option::is_some)
            || !class_info.vtable_methods.is_empty()
            || !class_info.static_vtable_methods.is_empty()
        {
            return Err(CodegenIrError::unsupported(format!(
                "object allocation requiring dynamic, default, or method metadata for {}",
                class_name
            )));
        }
        let marker_offsets = uninitialized_property_marker_offsets(class_info);
        (class_info.class_id, class_info.properties.len(), marker_offsets)
    };
    emit_object_allocation(
        ctx,
        class_id,
        property_count,
        &uninitialized_marker_offsets,
    )?;
    store_if_result(ctx, inst)
}

/// Lowers a declared object property read for statically known object receivers.
pub(super) fn lower_prop_get(ctx: &mut FunctionContext<'_>, inst: &Instruction) -> Result<()> {
    let object = expect_operand(inst, 0)?;
    let property = property_name_immediate(ctx, inst)?.to_string();
    let slot = resolve_property_slot(ctx, object, &property, inst)?;
    let base_reg = abi::symbol_scratch_reg(ctx.emitter);
    ctx.load_value_to_reg(object, base_reg)?;
    if slot.is_declared {
        emit_uninitialized_typed_property_guard(ctx, &slot, base_reg);
    }
    emit_property_load(ctx, &slot, base_reg)?;
    store_if_result(ctx, inst)
}

/// Lowers a declared object property write for statically known object receivers.
pub(super) fn lower_prop_set(ctx: &mut FunctionContext<'_>, inst: &Instruction) -> Result<()> {
    let object = expect_operand(inst, 0)?;
    let value = expect_operand(inst, 1)?;
    let property = property_name_immediate(ctx, inst)?.to_string();
    let slot = resolve_property_slot(ctx, object, &property, inst)?;
    let value_ty = ctx.value_php_type(value)?;
    ensure_property_value_supported(&slot, &value_ty, inst)?;
    let base_reg = abi::symbol_scratch_reg(ctx.emitter);
    ctx.load_value_to_reg(object, base_reg)?;
    emit_property_store(ctx, value, &slot, base_reg)
}

/// Lowers named `instanceof` using runtime class/interface metadata.
pub(super) fn lower_instanceof(ctx: &mut FunctionContext<'_>, inst: &Instruction) -> Result<()> {
    let value = expect_operand(inst, 0)?;
    let value_ty = ctx.value_php_type(value)?;
    if !matches!(value_ty, PhpType::Object(_) | PhpType::Mixed | PhpType::Union(_)) {
        emit_false(ctx);
        return store_if_result(ctx, inst);
    }
    let class_name = class_name_immediate(ctx, inst)?;
    let Some((target_id, target_kind)) = classify_named_target(ctx, class_name) else {
        emit_false(ctx);
        return store_if_result(ctx, inst);
    };
    reject_method_metadata_target(ctx, class_name)?;
    match value_ty {
        PhpType::Object(_) => {
            ctx.load_value_to_reg(value, abi::int_arg_reg_name(ctx.emitter.target, 0))?;
            emit_match_call(ctx, target_id, target_kind, "__rt_exception_matches");
        }
        PhpType::Mixed | PhpType::Union(_) => {
            ctx.load_value_to_reg(value, abi::int_arg_reg_name(ctx.emitter.target, 0))?;
            emit_match_call(ctx, target_id, target_kind, "__rt_mixed_instanceof");
        }
        _ => emit_false(ctx),
    }
    store_if_result(ctx, inst)
}

/// Emits allocation, class-id stamping, and declared-property slot initialization.
fn emit_object_allocation(
    ctx: &mut FunctionContext<'_>,
    class_id: u64,
    property_count: usize,
    uninitialized_marker_offsets: &[usize],
) -> Result<()> {
    let payload_size = 8 + property_count * 16;
    match ctx.emitter.target.arch {
        Arch::AArch64 => {
            ctx.emitter.instruction(&format!("mov x0, #{}", payload_size));     // request object payload storage for the class id and property slots
            abi::emit_call_label(ctx.emitter, "__rt_heap_alloc");
            ctx.emitter.instruction("mov x9, #4");                              // heap kind 4 marks object instances for ownership helpers
            ctx.emitter.instruction("str x9, [x0, #-8]");                       // stamp the heap header before the object payload
            ctx.emitter.instruction(&format!("mov x10, #{}", class_id));        // materialize the compile-time class id
            ctx.emitter.instruction("str x10, [x0]");                           // store the class id at object payload offset zero
        }
        Arch::X86_64 => {
            ctx.emitter.instruction(&format!("mov rax, {}", payload_size));     // request object payload storage for the class id and property slots
            abi::emit_call_label(ctx.emitter, "__rt_heap_alloc");
            ctx.emitter.instruction(&format!("mov r10, 0x{:x}", (X86_64_HEAP_MAGIC_HI32 << 32) | 4)); // materialize the x86_64 object heap kind word
            ctx.emitter.instruction("mov QWORD PTR [rax - 8], r10");            // stamp the heap header before the object payload
            ctx.emitter.instruction(&format!("mov r10, {}", class_id));         // materialize the compile-time class id
            ctx.emitter.instruction("mov QWORD PTR [rax], r10");                // store the class id at object payload offset zero
        }
    }
    let object_reg = abi::int_result_reg(ctx.emitter);
    for index in 0..property_count {
        let offset = 8 + index * 16;
        abi::emit_store_zero_to_address(ctx.emitter, object_reg, offset);
        abi::emit_store_zero_to_address(ctx.emitter, object_reg, offset + 8);
    }
    if !uninitialized_marker_offsets.is_empty() {
        let marker_reg = abi::secondary_scratch_reg(ctx.emitter);
        abi::emit_load_int_immediate(ctx.emitter, marker_reg, UNINITIALIZED_TYPED_PROPERTY_SENTINEL);
        for offset in uninitialized_marker_offsets {
            abi::emit_store_to_address(ctx.emitter, marker_reg, object_reg, *offset);
        }
    }
    Ok(())
}

/// Collects property high-word offsets that should start with the typed-property sentinel.
fn uninitialized_property_marker_offsets(class_info: &crate::types::ClassInfo) -> Vec<usize> {
    class_info
        .properties
        .iter()
        .enumerate()
        .filter_map(|(index, (property, _))| {
            let starts_uninitialized = class_info.declared_properties.contains(property)
                && class_info.defaults.get(index).is_some_and(|default| default.is_none());
            if starts_uninitialized {
                Some(
                    class_info
                        .property_offsets
                        .get(property)
                        .copied()
                        .unwrap_or(8 + index * 16)
                        + 8,
                )
            } else {
                None
            }
        })
        .collect()
}

/// Resolves the property slot for a concrete object receiver and declared property name.
fn resolve_property_slot(
    ctx: &FunctionContext<'_>,
    object: crate::ir::ValueId,
    property: &str,
    inst: &Instruction,
) -> Result<PropertySlot> {
    let object_ty = ctx.value_php_type(object)?;
    let PhpType::Object(class_name) = object_ty else {
        return Err(CodegenIrError::unsupported(format!(
            "{} for receiver PHP type {:?}",
            inst.op.name(),
            object_ty
        )));
    };
    let normalized = class_name.trim_start_matches('\\');
    let class_info = ctx
        .module
        .class_infos
        .get(normalized)
        .ok_or_else(|| CodegenIrError::unsupported(format!("unknown class {}", normalized)))?;
    if class_info.reference_properties.contains(property) {
        return Err(CodegenIrError::unsupported(format!(
            "{} for reference property {}::${}",
            inst.op.name(),
            normalized,
            property
        )));
    }
    let Some((index, (_, php_type))) = class_info
        .properties
        .iter()
        .enumerate()
        .find(|(_, (name, _))| name == property)
    else {
        return Err(CodegenIrError::unsupported(format!(
            "{} for dynamic or missing property {}::${}",
            inst.op.name(),
            normalized,
            property
        )));
    };
    ensure_property_type_supported(php_type, inst)?;
    let offset = class_info
        .property_offsets
        .get(property)
        .copied()
        .unwrap_or(8 + index * 16);
    Ok(PropertySlot {
        class_name: normalized.to_string(),
        property: property.to_string(),
        php_type: php_type.clone(),
        offset,
        is_declared: class_info.declared_properties.contains(property),
    })
}

/// Verifies that this slice knows how to represent the property type in an object slot.
fn ensure_property_type_supported(php_type: &PhpType, inst: &Instruction) -> Result<()> {
    match php_type {
        PhpType::Bool | PhpType::Int | PhpType::Float | PhpType::Str => Ok(()),
        _ => Err(CodegenIrError::unsupported(format!(
            "{} for property PHP type {:?}",
            inst.op.name(),
            php_type
        ))),
    }
}

/// Verifies the assigned value already has the property storage representation.
fn ensure_property_value_supported(
    slot: &PropertySlot,
    value_ty: &PhpType,
    inst: &Instruction,
) -> Result<()> {
    if value_ty == &slot.php_type {
        return Ok(());
    }
    Err(CodegenIrError::unsupported(format!(
        "{} assigning PHP type {:?} to {}::${} with PHP type {:?}",
        inst.op.name(),
        value_ty,
        slot.class_name,
        slot.property,
        slot.php_type
    )))
}

/// Emits the declared-property load into the canonical result register(s).
fn emit_property_load(
    ctx: &mut FunctionContext<'_>,
    slot: &PropertySlot,
    base_reg: &str,
) -> Result<()> {
    match &slot.php_type {
        PhpType::Str => {
            let (ptr_reg, len_reg) = abi::string_result_regs(ctx.emitter);
            abi::emit_load_from_address(ctx.emitter, ptr_reg, base_reg, slot.offset);
            abi::emit_load_from_address(ctx.emitter, len_reg, base_reg, slot.offset + 8);
        }
        PhpType::Float => {
            let float_reg = abi::float_result_reg(ctx.emitter);
            abi::emit_load_from_address(ctx.emitter, float_reg, base_reg, slot.offset);
        }
        PhpType::Bool | PhpType::Int => {
            let int_reg = abi::int_result_reg(ctx.emitter);
            abi::emit_load_from_address(ctx.emitter, int_reg, base_reg, slot.offset);
        }
        _ => return Err(CodegenIrError::unsupported(format!(
            "property load for PHP type {:?}",
            slot.php_type
        ))),
    }
    Ok(())
}

/// Emits a declared-property store from an SSA value into the object slot.
fn emit_property_store(
    ctx: &mut FunctionContext<'_>,
    value: crate::ir::ValueId,
    slot: &PropertySlot,
    base_reg: &str,
) -> Result<()> {
    match &slot.php_type {
        PhpType::Str => {
            let (ptr_reg, len_reg) = abi::string_result_regs(ctx.emitter);
            ctx.load_string_value_to_regs(value, ptr_reg, len_reg)?;
            abi::emit_store_to_address(ctx.emitter, ptr_reg, base_reg, slot.offset);
            abi::emit_store_to_address(ctx.emitter, len_reg, base_reg, slot.offset + 8);
        }
        PhpType::Float => {
            let float_reg = abi::float_result_reg(ctx.emitter);
            ctx.load_value_to_reg(value, float_reg)?;
            abi::emit_store_to_address(ctx.emitter, float_reg, base_reg, slot.offset);
            abi::emit_store_zero_to_address(ctx.emitter, base_reg, slot.offset + 8);
        }
        PhpType::Bool | PhpType::Int => {
            let int_reg = abi::int_result_reg(ctx.emitter);
            ctx.load_value_to_reg(value, int_reg)?;
            abi::emit_store_to_address(ctx.emitter, int_reg, base_reg, slot.offset);
            abi::emit_store_zero_to_address(ctx.emitter, base_reg, slot.offset + 8);
        }
        _ => return Err(CodegenIrError::unsupported(format!(
            "property store for PHP type {:?}",
            slot.php_type
        ))),
    }
    Ok(())
}

/// Emits a fatal guard for reads from uninitialized typed properties.
fn emit_uninitialized_typed_property_guard(
    ctx: &mut FunctionContext<'_>,
    slot: &PropertySlot,
    object_reg: &str,
) {
    let initialized_label = ctx.next_label("typed_prop_initialized");
    let marker_reg = abi::secondary_scratch_reg(ctx.emitter);
    let sentinel_reg = abi::tertiary_scratch_reg(ctx.emitter);
    abi::emit_load_from_address(ctx.emitter, marker_reg, object_reg, slot.offset + 8);
    abi::emit_load_int_immediate(ctx.emitter, sentinel_reg, UNINITIALIZED_TYPED_PROPERTY_SENTINEL);
    match ctx.emitter.target.arch {
        Arch::AArch64 => {
            ctx.emitter.instruction(&format!("cmp {}, {}", marker_reg, sentinel_reg)); // compare the property marker against the uninitialized sentinel
            ctx.emitter.instruction(&format!("b.ne {}", initialized_label));    // continue the property read once the slot has been initialized
        }
        Arch::X86_64 => {
            ctx.emitter.instruction(&format!("cmp {}, {}", marker_reg, sentinel_reg)); // compare the property marker against the uninitialized sentinel
            ctx.emitter.instruction(&format!("jne {}", initialized_label));     // continue the property read once the slot has been initialized
        }
    }
    emit_uninitialized_typed_property_fatal(ctx, slot);
    ctx.emitter.label(&initialized_label);
}

/// Emits the runtime fatal diagnostic for an uninitialized typed-property read.
fn emit_uninitialized_typed_property_fatal(
    ctx: &mut FunctionContext<'_>,
    slot: &PropertySlot,
) {
    let message = format!(
        "Fatal error: Typed property {}::${} must not be accessed before initialization\n",
        slot.class_name, slot.property
    );
    let (message_label, message_len) = ctx.data.add_string(message.as_bytes());
    match ctx.emitter.target.arch {
        Arch::AArch64 => {
            ctx.emitter.instruction("mov x0, #2");                              // select stderr for the uninitialized typed-property fatal
            ctx.emitter.adrp("x1", &message_label);
            ctx.emitter.add_lo12("x1", "x1", &message_label);
            ctx.emitter.instruction(&format!("mov x2, #{}", message_len));      // pass the fatal diagnostic byte length to write()
            ctx.emitter.syscall(4);
        }
        Arch::X86_64 => {
            abi::emit_symbol_address(ctx.emitter, "rsi", &message_label);
            ctx.emitter.instruction(&format!("mov edx, {}", message_len));      // pass the fatal diagnostic byte length to write()
            ctx.emitter.instruction("mov edi, 2");                              // select stderr for the uninitialized typed-property fatal
            ctx.emitter.instruction("mov eax, 1");                              // select Linux write syscall
            ctx.emitter.instruction("syscall");                                 // write the uninitialized typed-property fatal diagnostic
        }
    }
    abi::emit_exit(ctx.emitter, 1);
}

/// Emits the metadata matcher call with object-or-mixed input already in argument 0.
fn emit_match_call(
    ctx: &mut FunctionContext<'_>,
    target_id: u64,
    target_kind: i64,
    helper: &str,
) {
    abi::emit_load_int_immediate(
        ctx.emitter,
        abi::int_arg_reg_name(ctx.emitter.target, 1),
        target_id as i64,
    );
    abi::emit_load_int_immediate(
        ctx.emitter,
        abi::int_arg_reg_name(ctx.emitter.target, 2),
        target_kind,
    );
    abi::emit_call_label(ctx.emitter, helper);
}

/// Classifies a named target as a class `(kind 0)` or interface `(kind 1)`.
fn classify_named_target(
    ctx: &FunctionContext<'_>,
    class_name: &str,
) -> Option<(u64, i64)> {
    let normalized = class_name.trim_start_matches('\\');
    if let Some(class_info) = ctx.module.class_infos.get(normalized) {
        return Some((class_info.class_id, 0));
    }
    ctx.module
        .interface_infos
        .get(normalized)
        .map(|interface_info| (interface_info.interface_id, 1))
}

/// Rejects targets whose runtime metadata would reference method symbols not emitted yet.
fn reject_method_metadata_target(ctx: &FunctionContext<'_>, class_name: &str) -> Result<()> {
    let normalized = class_name.trim_start_matches('\\');
    if let Some(class_info) = ctx.module.class_infos.get(normalized) {
        if !class_info.vtable_methods.is_empty() || !class_info.static_vtable_methods.is_empty() {
            return Err(CodegenIrError::unsupported(format!(
                "instanceof target with method metadata {}",
                normalized
            )));
        }
    }
    if let Some(interface_info) = ctx.module.interface_infos.get(normalized) {
        if !interface_info.method_order.is_empty() {
            return Err(CodegenIrError::unsupported(format!(
                "instanceof interface target with method metadata {}",
                normalized
            )));
        }
    }
    Ok(())
}

/// Emits a boolean false result for non-object values or unresolved targets.
fn emit_false(ctx: &mut FunctionContext<'_>) {
    abi::emit_load_int_immediate(ctx.emitter, abi::int_result_reg(ctx.emitter), 0);
}

/// Resolves an instruction property-name immediate into the module data pool.
fn property_name_immediate<'a>(
    ctx: &'a FunctionContext<'_>,
    inst: &Instruction,
) -> Result<&'a str> {
    let data = expect_data(inst)?;
    ctx.module
        .data
        .strings
        .get(data.as_raw() as usize)
        .map(String::as_str)
        .ok_or_else(|| CodegenIrError::missing_entry("data string", data.as_raw()))
}

/// Resolves an instruction class-name immediate into the module data pool.
fn class_name_immediate<'a>(
    ctx: &'a FunctionContext<'_>,
    inst: &Instruction,
) -> Result<&'a str> {
    let data = expect_data(inst)?;
    ctx.module
        .data
        .class_names
        .get(data.as_raw() as usize)
        .map(String::as_str)
        .ok_or_else(|| CodegenIrError::missing_entry("class data", data.as_raw()))
}
