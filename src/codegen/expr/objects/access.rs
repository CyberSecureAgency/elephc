//! Purpose:
//! Lowers property reads, magic access paths, and nullable object field loads.
//! Produces object-related expression results while respecting runtime metadata and ownership rules.
//!
//! Called from:
//! - `crate::codegen::expr::objects`
//!
//! Key details:
//! - Object handles, property storage, and class ids must stay consistent with emitted class tables.

use crate::codegen::abi;
use crate::codegen::context::Context;
use crate::codegen::data_section::DataSection;
use crate::codegen::emit::Emitter;
use crate::codegen::functions;
use crate::codegen::platform::Arch;
use crate::codegen::UNINITIALIZED_TYPED_PROPERTY_SENTINEL;
use crate::parser::ast::Expr;
use crate::types::PhpType;

use super::super::emit_expr;

pub(super) fn emit_property_access(
    object: &Expr,
    property: &str,
    emitter: &mut Emitter,
    ctx: &mut Context,
    data: &mut DataSection,
) -> PhpType {
    // Resolve the receiver's static class up-front so a nullable object
    // union (`?Foo`) routes through the same path as a direct object type.
    // Direct object receivers produce a raw object pointer, while nullable
    // unions produce a boxed mixed cell that must be checked and unboxed
    // before the normal property load.
    let static_obj_ty = functions::infer_contextual_type(object, ctx);
    let static_class = functions::singular_object_class(&static_obj_ty)
        .map(|name| name.to_string());
    let obj_ty = emit_expr(object, emitter, ctx, data);
    if let Some(class_name) = static_class.as_ref() {
        if matches!(obj_ty, PhpType::Mixed | PhpType::Union(_)) {
            return emit_nullable_object_property_access(class_name, property, emitter, ctx, data);
        }
        if matches!(obj_ty, PhpType::Object(_)) {
            return emit_loaded_object_property_access(class_name, property, emitter, ctx, data);
        }
    }
    let (class_name, prop_ty, offset, needs_deref, is_reference) = match &obj_ty {
        PhpType::Object(class_name) => {
            return emit_loaded_object_property_access(class_name, property, emitter, ctx, data);
        }
        PhpType::Mixed => {
            return emit_mixed_property_access(property, emitter, data);
        }
        PhpType::Pointer(Some(class_name)) if ctx.extern_classes.contains_key(class_name) => {
            let class_info = match ctx.extern_classes.get(class_name).cloned() {
                Some(c) => c,
                None => {
                    emitter.comment(&format!("WARNING: undefined extern class {}", class_name));
                    return PhpType::Int;
                }
            };

            let field = match class_info
                .fields
                .iter()
                .find(|field| field.name == property)
            {
                Some(field) => field.clone(),
                None => {
                    emitter.comment(&format!("WARNING: undefined extern field {}", property));
                    return PhpType::Int;
                }
            };

            (class_name.clone(), field.php_type, field.offset, true, false)
        }
        PhpType::Pointer(Some(class_name)) if ctx.packed_classes.contains_key(class_name) => {
            let class_info = match ctx.packed_classes.get(class_name).cloned() {
                Some(c) => c,
                None => {
                    emitter.comment(&format!("WARNING: undefined packed class {}", class_name));
                    return PhpType::Int;
                }
            };

            let field = match class_info
                .fields
                .iter()
                .find(|field| field.name == property)
            {
                Some(field) => field.clone(),
                None => {
                    emitter.comment(&format!("WARNING: undefined packed field {}", property));
                    return PhpType::Int;
                }
            };

            (class_name.clone(), field.php_type, field.offset, true, false)
        }
        _ => {
            emitter.comment("WARNING: property access on non-object");
            return PhpType::Int;
        }
    };

    if needs_deref {
        abi::emit_call_label(emitter, "__rt_ptr_check_nonnull");               // abort with fatal error on null pointer dereference
        emitter.comment(&format!(
            "->{} via ptr<{}> (offset {})",
            property, class_name, offset
        ));
    } else {
        emitter.comment(&format!("->{}  (offset {})", property, offset));
    }

    let object_reg = abi::int_result_reg(emitter);

    if is_reference {
        let pointer_reg = abi::symbol_scratch_reg(emitter);
        abi::emit_load_from_address(emitter, pointer_reg, object_reg, offset);
        match &prop_ty {
            PhpType::Str => {
                let (ptr_reg, len_reg) = abi::string_result_regs(emitter);
                abi::emit_load_from_address(emitter, ptr_reg, pointer_reg, 0);
                abi::emit_load_from_address(emitter, len_reg, pointer_reg, 8);
            }
            PhpType::Float => {
                abi::emit_load_from_address(emitter, abi::float_result_reg(emitter), pointer_reg, 0);
            }
            PhpType::Bool | PhpType::Int | PhpType::Void | PhpType::Never | PhpType::Resource(_) => {
                abi::emit_load_from_address(emitter, abi::int_result_reg(emitter), pointer_reg, 0);
            }
            PhpType::Iterable
            | PhpType::Mixed
            | PhpType::Union(_)
            | PhpType::Array(_)
            | PhpType::AssocArray { .. }
            | PhpType::Buffer(_)
            | PhpType::Callable
            | PhpType::Object(_)
            | PhpType::Packed(_)
            | PhpType::Pointer(_) => {
                abi::emit_load_from_address(emitter, abi::int_result_reg(emitter), pointer_reg, 0);
            }
        }
        return prop_ty;
    }

    match &prop_ty {
        PhpType::Str => {
            let (ptr_reg, len_reg) = abi::string_result_regs(emitter);
            let base_reg = abi::symbol_scratch_reg(emitter);
            emitter.instruction(&format!("mov {}, {}", base_reg, object_reg));  // preserve the object base pointer while loading the two-word string property payload
            abi::emit_load_from_address(emitter, ptr_reg, base_reg, offset);
            abi::emit_load_from_address(emitter, len_reg, base_reg, offset + 8);
        }
        PhpType::Float => {
            abi::emit_load_from_address(emitter, abi::float_result_reg(emitter), object_reg, offset);
        }
        PhpType::Bool | PhpType::Int | PhpType::Void | PhpType::Never | PhpType::Resource(_) => {
            abi::emit_load_from_address(emitter, abi::int_result_reg(emitter), object_reg, offset);
        }
        PhpType::Iterable
        | PhpType::Mixed
        | PhpType::Union(_)
        | PhpType::Array(_)
        | PhpType::AssocArray { .. }
        | PhpType::Buffer(_)
        | PhpType::Callable
        | PhpType::Object(_)
        | PhpType::Packed(_)
        | PhpType::Pointer(_) => {
            abi::emit_load_from_address(emitter, abi::int_result_reg(emitter), object_reg, offset);
        }
    }

    prop_ty
}

/// Lower a `$obj->name` read where `$obj` has type `Object("stdClass")`.
///
/// stdClass has no static property layout, so route the access through the
/// runtime helper `__rt_stdclass_get`. The receiver is already in
/// int_result_reg (x0/rax) at this point thanks to `emit_property_access`.
fn emit_stdclass_property_access(
    property: &str,
    emitter: &mut Emitter,
    data: &mut DataSection,
) -> PhpType {
    emit_dynamic_property_access(
        property,
        emitter,
        data,
        "stdClass",
        "__rt_stdclass_get",
    )
}

/// Lower a `$obj->name` read where `$obj` has type `Mixed`.
///
/// The runtime helper unboxes the Mixed cell, validates that it carries a
/// stdClass instance, and routes to `__rt_stdclass_get`. Other payloads
/// return Mixed(null), matching PHP's "property access on non-object"
/// warning behaviour for the most common idiom (`json_decode($json)->name`).
fn emit_mixed_property_access(
    property: &str,
    emitter: &mut Emitter,
    data: &mut DataSection,
) -> PhpType {
    emit_dynamic_property_access(
        property,
        emitter,
        data,
        "mixed",
        "__rt_mixed_property_get",
    )
}

fn emit_dynamic_property_access(
    property: &str,
    emitter: &mut Emitter,
    data: &mut DataSection,
    receiver_label: &str,
    runtime_symbol: &str,
) -> PhpType {
    emitter.comment(&format!("{}->{}  (dynamic)", receiver_label, property));
    let (label, len) = data.add_string(property.as_bytes());
    match emitter.target.arch {
        Arch::AArch64 => {
            abi::emit_symbol_address(emitter, "x1", &label);
            abi::emit_load_int_immediate(emitter, "x2", len as i64);
            emitter.instruction(&format!("bl {}", runtime_symbol));             // call the dynamic-property reader; result Mixed* lands in x0
        }
        Arch::X86_64 => {
            emitter.instruction("mov rdi, rax");                                // shift the receiver into the SysV first-arg register
            abi::emit_symbol_address(emitter, "rsi", &label);
            abi::emit_load_int_immediate(emitter, "rdx", len as i64);
            emitter.instruction(&format!("call {}", runtime_symbol));           // call the dynamic-property reader; result Mixed* lands in rax
        }
    }
    PhpType::Mixed
}

pub(super) fn emit_nullable_object_property_access(
    class_name: &str,
    property: &str,
    emitter: &mut Emitter,
    ctx: &mut Context,
    data: &mut DataSection,
) -> PhpType {
    let null_label = ctx.next_label("nullable_prop_null");
    let done_label = ctx.next_label("nullable_prop_done");
    let message = format!("Warning: Attempt to read property \"{}\" on null\n", property);

    super::emit_unbox_mixed_object_or_null_branch(&null_label, emitter);
    let property_ty = emit_loaded_object_property_access(class_name, property, emitter, ctx, data);
    super::box_nullable_result(&property_ty, emitter);
    abi::emit_jump(emitter, &done_label);                                      // skip the nullable property null path after a real property read

    emitter.label(&null_label);
    super::emit_runtime_warning(message.as_bytes(), emitter, data);
    super::emit_boxed_null(emitter);

    emitter.label(&done_label);
    PhpType::Mixed
}

pub(super) fn emit_loaded_object_property_access(
    class_name: &str,
    property: &str,
    emitter: &mut Emitter,
    ctx: &mut Context,
    data: &mut DataSection,
) -> PhpType {
    if crate::types::checker::builtin_stdclass::is_stdclass(class_name) {
        return emit_stdclass_property_access(property, emitter, data);
    }
    let class_info = match ctx.classes.get(class_name).cloned() {
        Some(c) => c,
        None => {
            emitter.comment(&format!("WARNING: undefined class {}", class_name));
            return PhpType::Int;
        }
    };

    let prop_ty = match class_info
        .properties
        .iter()
        .find(|(n, _)| n == property)
        .map(|(_, t)| t.clone())
    {
        Some(v) => v,
        None => {
            if class_info.methods.contains_key("__get") {
                emitter.comment(&format!("magic __get('{}')", property));
                let object_reg = abi::symbol_scratch_reg(emitter);
                emitter.instruction(&format!("mov {}, {}", object_reg, abi::int_result_reg(emitter))); // preserve $this while the magic-property name setup clobbers normal result registers
                super::push_magic_property_name_arg(property, emitter, data);
                abi::emit_push_reg(emitter, object_reg);                      // push $this pointer for __get dispatch using the preserved object register
                return super::emit_method_call_with_pushed_args(
                    class_name,
                    "__get",
                    &[PhpType::Str],
                    emitter,
                    ctx,
                );
            }
            if class_info.allow_dynamic_properties {
                let dyn_slot_offset = 8 + class_info.properties.len() * 16;
                return crate::codegen::stmt::emit_dynamic_property_get(
                    property,
                    dyn_slot_offset,
                    emitter,
                    ctx,
                    data,
                );
            }
            emitter.comment(&format!("WARNING: undefined property {}", property));
            return PhpType::Int;
        }
    };
    let offset = match class_info.property_offsets.get(property) {
        Some(offset) => *offset,
        None => {
            emitter.comment(&format!("WARNING: missing property offset {}", property));
            return PhpType::Int;
        }
    };

    emit_loaded_object_property_value(
        class_name,
        property,
        prop_ty,
        offset,
        class_info.declared_properties.contains(property),
        false,
        class_info.reference_properties.contains(property),
        ctx,
        data,
        emitter,
    )
}

fn emit_loaded_object_property_value(
    class_name: &str,
    property: &str,
    prop_ty: PhpType,
    offset: usize,
    is_declared: bool,
    needs_deref: bool,
    is_reference: bool,
    ctx: &mut Context,
    data: &mut DataSection,
    emitter: &mut Emitter,
) -> PhpType {
    if needs_deref {
        abi::emit_call_label(emitter, "__rt_ptr_check_nonnull");               // abort with fatal error on null pointer dereference
        emitter.comment(&format!(
            "->{} via ptr<{}> (offset {})",
            property, class_name, offset
        ));
    } else {
        emitter.comment(&format!("->{}  (offset {})", property, offset));
    }

    let object_reg = abi::int_result_reg(emitter);

    if is_declared {
        emit_uninitialized_typed_property_guard(
            class_name, property, offset, object_reg, emitter, ctx, data,
        );
    }

    if is_reference {
        let pointer_reg = abi::symbol_scratch_reg(emitter);
        abi::emit_load_from_address(emitter, pointer_reg, object_reg, offset);
        match &prop_ty {
            PhpType::Str => {
                let (ptr_reg, len_reg) = abi::string_result_regs(emitter);
                abi::emit_load_from_address(emitter, ptr_reg, pointer_reg, 0);
                abi::emit_load_from_address(emitter, len_reg, pointer_reg, 8);
            }
            PhpType::Float => {
                abi::emit_load_from_address(emitter, abi::float_result_reg(emitter), pointer_reg, 0);
            }
            PhpType::Bool | PhpType::Int | PhpType::Void | PhpType::Never | PhpType::Resource(_) => {
                abi::emit_load_from_address(emitter, abi::int_result_reg(emitter), pointer_reg, 0);
            }
            PhpType::Iterable
            | PhpType::Mixed
            | PhpType::Union(_)
            | PhpType::Array(_)
            | PhpType::AssocArray { .. }
            | PhpType::Buffer(_)
            | PhpType::Callable
            | PhpType::Object(_)
            | PhpType::Packed(_)
            | PhpType::Pointer(_) => {
                abi::emit_load_from_address(emitter, abi::int_result_reg(emitter), pointer_reg, 0);
            }
        }
        return prop_ty;
    }

    match &prop_ty {
        PhpType::Str => {
            let (ptr_reg, len_reg) = abi::string_result_regs(emitter);
            let base_reg = abi::symbol_scratch_reg(emitter);
            emitter.instruction(&format!("mov {}, {}", base_reg, object_reg));  // preserve the object base pointer while loading the two-word string property payload
            abi::emit_load_from_address(emitter, ptr_reg, base_reg, offset);
            abi::emit_load_from_address(emitter, len_reg, base_reg, offset + 8);
        }
        PhpType::Float => {
            abi::emit_load_from_address(emitter, abi::float_result_reg(emitter), object_reg, offset);
        }
        PhpType::Bool | PhpType::Int | PhpType::Void | PhpType::Never | PhpType::Resource(_) => {
            abi::emit_load_from_address(emitter, abi::int_result_reg(emitter), object_reg, offset);
        }
        PhpType::Iterable
        | PhpType::Mixed
        | PhpType::Union(_)
        | PhpType::Array(_)
        | PhpType::AssocArray { .. }
        | PhpType::Buffer(_)
        | PhpType::Callable
        | PhpType::Object(_)
        | PhpType::Packed(_)
        | PhpType::Pointer(_) => {
            abi::emit_load_from_address(emitter, abi::int_result_reg(emitter), object_reg, offset);
        }
    }

    prop_ty
}

fn emit_uninitialized_typed_property_guard(
    class_name: &str,
    property: &str,
    offset: usize,
    object_reg: &str,
    emitter: &mut Emitter,
    ctx: &mut Context,
    data: &mut DataSection,
) {
    let initialized_label = ctx.next_label("typed_prop_initialized");
    let marker_reg = abi::secondary_scratch_reg(emitter);
    let sentinel_reg = abi::tertiary_scratch_reg(emitter);
    abi::emit_load_from_address(emitter, marker_reg, object_reg, offset + 8);
    abi::emit_load_int_immediate(emitter, sentinel_reg, UNINITIALIZED_TYPED_PROPERTY_SENTINEL);
    match emitter.target.arch {
        Arch::AArch64 => {
            emitter.instruction(&format!("cmp {}, {}", marker_reg, sentinel_reg)); // check whether the typed property still carries the uninitialized marker
            emitter.instruction(&format!("b.ne {}", initialized_label));        // continue the property read once the slot has been initialized
        }
        Arch::X86_64 => {
            emitter.instruction(&format!("cmp {}, {}", marker_reg, sentinel_reg)); // check whether the typed property still carries the uninitialized marker
            emitter.instruction(&format!("jne {}", initialized_label));         // continue the property read once the slot has been initialized
        }
    }
    emit_uninitialized_typed_property_fatal(class_name, property, emitter, data);
    emitter.label(&initialized_label);
}

fn emit_uninitialized_typed_property_fatal(
    class_name: &str,
    property: &str,
    emitter: &mut Emitter,
    data: &mut DataSection,
) {
    let message = format!(
        "Fatal error: Typed property {}::${} must not be accessed before initialization\n",
        class_name, property
    );
    let (label, len) = data.add_string(message.as_bytes());
    match emitter.target.arch {
        Arch::AArch64 => {
            emitter.instruction("mov x0, #2");                                  // fd = stderr for the typed-property initialization fatal
            abi::emit_symbol_address(emitter, "x1", &label);                    // point write() at the typed-property initialization diagnostic
            emitter.instruction(&format!("mov x2, #{}", len));                  // pass the diagnostic byte length to write()
            emitter.syscall(4);
            emitter.instruction("mov x0, #1");                                  // exit status 1 indicates abnormal termination
            emitter.syscall(1);
        }
        Arch::X86_64 => {
            abi::emit_symbol_address(emitter, "rsi", &label);                   // point write() at the typed-property initialization diagnostic
            emitter.instruction(&format!("mov edx, {}", len));                  // pass the diagnostic byte length to write()
            emitter.instruction("mov edi, 2");                                  // fd = stderr for the typed-property initialization fatal
            emitter.instruction("mov eax, 1");                                  // Linux x86_64 syscall 1 = write
            emitter.instruction("syscall");                                     // emit the fatal diagnostic before terminating
            emitter.instruction("mov edi, 1");                                  // exit status 1 indicates abnormal termination
            emitter.instruction("mov eax, 60");                                 // Linux x86_64 syscall 60 = exit
            emitter.instruction("syscall");                                     // terminate after the typed-property initialization fatal
        }
    }
}
