//! Purpose:
//! Lowers EIR callable invocation opcodes that need runtime dispatch.
//! Starts with runtime string callables that select among user functions.
//!
//! Called from:
//! - `crate::codegen_ir::lower_inst::lower_instruction()`.
//!
//! Key details:
//! - Runtime string callable dispatch preserves the callable name while
//!   comparing candidates, then reuses direct-call ABI materialization.
//! - This slice supports compatible user-function targets only; closures with
//!   captures, callable arrays, object `__invoke`, and builtin string names
//!   remain explicit unsupported paths.

use crate::codegen::{abi, emit_box_current_value_as_mixed};
use crate::codegen::platform::Arch;
use crate::ir::{Instruction, ValueId};
use crate::names::function_symbol;
use crate::types::PhpType;

use super::super::context::FunctionContext;
use super::{direct_call_stack_pad_bytes, expect_operand, materialize_direct_call_args};
use crate::codegen_ir::{CodegenIrError, Result};

/// Resolved user function candidate for a runtime string callable.
struct RuntimeStringFunctionTarget {
    name: String,
    param_types: Vec<PhpType>,
    return_ty: PhpType,
}

/// Lowers `$callable(...)` calls when the callable is a runtime string function name.
pub(super) fn lower_closure_call(ctx: &mut FunctionContext<'_>, inst: &Instruction) -> Result<()> {
    let callable = expect_operand(inst, 0)?;
    match ctx.value_php_type(callable)?.codegen_repr() {
        PhpType::Str => lower_runtime_string_closure_call(ctx, inst, callable),
        other => Err(CodegenIrError::unsupported(format!(
            "closure_call for callable PHP type {:?}",
            other
        ))),
    }
}

/// Dispatches a runtime string callable across user functions with compatible ABI shape.
fn lower_runtime_string_closure_call(
    ctx: &mut FunctionContext<'_>,
    inst: &Instruction,
    callable: ValueId,
) -> Result<()> {
    let args = inst.operands.iter().skip(1).copied().collect::<Vec<_>>();
    let targets = runtime_string_function_targets(ctx, args.len(), inst)?;
    if targets.is_empty() {
        return Err(CodegenIrError::unsupported(
            "closure_call with no compatible user-function targets",
        ));
    }

    let (ptr_reg, len_reg) = abi::string_result_regs(ctx.emitter);
    ctx.load_string_value_to_regs(callable, ptr_reg, len_reg)?;
    abi::emit_push_reg_pair(ctx.emitter, ptr_reg, len_reg);

    let done_label = ctx.next_label("closure_call_done");
    let miss_label = ctx.next_label("closure_call_missing");
    let mut case_labels = Vec::with_capacity(targets.len());
    for target in &targets {
        let label = ctx.next_label(&format!("closure_call_{}", label_fragment(&target.name)));
        emit_branch_if_runtime_callable_name_matches(ctx, &target.name, &label);
        case_labels.push(label);
    }
    abi::emit_jump(ctx.emitter, &miss_label);

    for (target, label) in targets.iter().zip(case_labels.iter()) {
        ctx.emitter.label(label);
        abi::emit_release_temporary_stack(ctx.emitter, 16);
        emit_runtime_string_function_call(ctx, inst, &args, target)?;
        abi::emit_jump(ctx.emitter, &done_label);
    }

    ctx.emitter.label(&miss_label);
    abi::emit_release_temporary_stack(ctx.emitter, 16);
    emit_undefined_runtime_string_call_fatal(ctx);

    ctx.emitter.label(&done_label);
    Ok(())
}

/// Collects compatible user functions that a runtime string callable may select.
fn runtime_string_function_targets(
    ctx: &FunctionContext<'_>,
    arg_count: usize,
    inst: &Instruction,
) -> Result<Vec<RuntimeStringFunctionTarget>> {
    let targets = ctx
        .module
        .functions
        .iter()
        .filter(|function| !function.flags.is_main)
        .filter(|function| function.params.len() == arg_count)
        .filter(|function| {
            function
                .params
                .iter()
                .all(|param| !param.by_ref && !param.variadic)
        })
        .filter_map(|function| {
            let return_ty = function.return_php_type.codegen_repr();
            if !runtime_string_result_type_supported(&inst.result_php_type.codegen_repr(), &return_ty) {
                return None;
            }
            Some(RuntimeStringFunctionTarget {
                name: function.name.clone(),
                param_types: function
                    .params
                    .iter()
                    .map(|param| param.php_type.codegen_repr())
                    .collect(),
                return_ty,
            })
        })
        .collect::<Vec<_>>();
    Ok(targets)
}

/// Returns true when the selected runtime function can be stored into the EIR result.
fn runtime_string_result_type_supported(result_ty: &PhpType, return_ty: &PhpType) -> bool {
    result_ty == return_ty || matches!(result_ty, PhpType::Mixed | PhpType::Union(_))
}

/// Converts arbitrary PHP function names into assembly-label-safe fragments.
fn label_fragment(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}

/// Emits one branch comparing the saved callable name with a candidate function name.
fn emit_branch_if_runtime_callable_name_matches(
    ctx: &mut FunctionContext<'_>,
    name: &str,
    matched_label: &str,
) {
    emit_runtime_callable_name_compare(ctx, name.as_bytes(), matched_label);
    let trimmed = name.trim_start_matches('\\');
    if trimmed == name {
        let qualified = format!("\\{}", name);
        emit_runtime_callable_name_compare(ctx, qualified.as_bytes(), matched_label);
    }
}

/// Emits a case-insensitive compare against the saved runtime callable name.
fn emit_runtime_callable_name_compare(
    ctx: &mut FunctionContext<'_>,
    candidate: &[u8],
    matched_label: &str,
) {
    let (candidate_label, candidate_len) = ctx.data.add_string(candidate);
    match ctx.emitter.target.arch {
        Arch::AArch64 => {
            abi::emit_load_temporary_stack_slot(ctx.emitter, "x1", 0);
            abi::emit_load_temporary_stack_slot(ctx.emitter, "x2", 8);
            abi::emit_symbol_address(ctx.emitter, "x3", &candidate_label);
            abi::emit_load_int_immediate(ctx.emitter, "x4", candidate_len as i64);
            abi::emit_call_label(ctx.emitter, "__rt_strcasecmp");
            ctx.emitter.instruction("cmp x0, #0");                              // did the runtime string callable name match this user function?
            ctx.emitter.instruction(&format!("b.eq {}", matched_label));        // dispatch to this user function when names match case-insensitively
        }
        Arch::X86_64 => {
            abi::emit_load_temporary_stack_slot(ctx.emitter, "rdi", 0);
            abi::emit_load_temporary_stack_slot(ctx.emitter, "rsi", 8);
            abi::emit_symbol_address(ctx.emitter, "rdx", &candidate_label);
            abi::emit_load_int_immediate(ctx.emitter, "rcx", candidate_len as i64);
            abi::emit_call_label(ctx.emitter, "__rt_strcasecmp");
            ctx.emitter.instruction("test rax, rax");                           // did the runtime string callable name match this user function?
            ctx.emitter.instruction(&format!("je {}", matched_label));          // dispatch to this user function when names match case-insensitively
        }
    }
}

/// Calls one resolved runtime string callable target and stores the converted result.
fn emit_runtime_string_function_call(
    ctx: &mut FunctionContext<'_>,
    inst: &Instruction,
    args: &[ValueId],
    target: &RuntimeStringFunctionTarget,
) -> Result<()> {
    let overflow_bytes = materialize_direct_call_args(ctx, args, &target.param_types)?;
    let caller_stack_pad_bytes = direct_call_stack_pad_bytes(ctx, overflow_bytes);
    abi::emit_reserve_temporary_stack(ctx.emitter, caller_stack_pad_bytes);
    abi::emit_call_label(ctx.emitter, &function_symbol(&target.name));
    abi::emit_release_temporary_stack(ctx.emitter, caller_stack_pad_bytes);
    abi::emit_release_temporary_stack(ctx.emitter, overflow_bytes);
    store_runtime_string_call_result(ctx, inst, &target.return_ty)
}

/// Stores a runtime string callable result, boxing scalar returns for Mixed slots.
fn store_runtime_string_call_result(
    ctx: &mut FunctionContext<'_>,
    inst: &Instruction,
    return_ty: &PhpType,
) -> Result<()> {
    let Some(result) = inst.result else {
        return Ok(());
    };
    let result_ty = ctx.value_php_type(result)?;
    if return_ty.codegen_repr() == PhpType::Void || result_ty == PhpType::Void {
        abi::emit_load_int_immediate(
            ctx.emitter,
            abi::int_result_reg(ctx.emitter),
            0x7fff_ffff_ffff_fffe,
        );
        if matches!(result_ty, PhpType::Mixed | PhpType::Union(_)) {
            emit_box_current_value_as_mixed(ctx.emitter, &PhpType::Void);
        }
        ctx.store_result_value(result)?;
        return Ok(());
    }
    if matches!(result_ty, PhpType::Mixed | PhpType::Union(_))
        && return_ty.codegen_repr() != PhpType::Mixed
    {
        emit_box_current_value_as_mixed(ctx.emitter, &return_ty.codegen_repr());
    }
    ctx.store_result_value(result)
}

/// Emits the fatal path for an unmatched runtime string callable name.
fn emit_undefined_runtime_string_call_fatal(ctx: &mut FunctionContext<'_>) {
    let message = b"Fatal error: Call to undefined function <dynamic>()\n";
    let (message_label, message_len) = ctx.data.add_string(message);
    match ctx.emitter.target.arch {
        Arch::AArch64 => {
            ctx.emitter.instruction("mov x0, #2");                              // write the undefined dynamic-call diagnostic to stderr
            ctx.emitter.adrp("x1", &message_label);                             // load the dynamic-call diagnostic string page
            ctx.emitter.add_lo12("x1", "x1", &message_label);                  // resolve the dynamic-call diagnostic string address
            ctx.emitter.instruction(&format!("mov x2, #{}", message_len));      // pass the dynamic-call diagnostic byte length to write
            ctx.emitter.syscall(4);
            abi::emit_exit(ctx.emitter, 1);
        }
        Arch::X86_64 => {
            ctx.emitter.instruction("mov edi, 2");                              // write the undefined dynamic-call diagnostic to Linux stderr
            abi::emit_symbol_address(ctx.emitter, "rsi", &message_label);
            ctx.emitter.instruction(&format!("mov edx, {}", message_len));      // pass the dynamic-call diagnostic byte length to write
            ctx.emitter.instruction("mov eax, 1");                              // Linux x86_64 syscall 1 = write
            ctx.emitter.instruction("syscall");                                 // emit the fatal diagnostic before terminating
            abi::emit_exit(ctx.emitter, 1);
        }
    }
}
