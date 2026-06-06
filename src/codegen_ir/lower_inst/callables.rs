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
//! - Callable descriptors use a uniform invoker ABI with Mixed argument arrays;
//!   signature-dependent direct dispatch stays on explicit guarded paths.

use crate::codegen::{
    abi, callable_descriptor, emit_box_current_owned_value_as_mixed,
    emit_box_current_value_as_mixed, emit_release_pushed_refcounted_temp_after_array_push,
};
use crate::codegen::platform::Arch;
use crate::ir::{Instruction, ValueId};
use crate::names::function_symbol;
use crate::types::PhpType;

use super::super::context::FunctionContext;
use super::{direct_call_stack_pad_bytes, expect_operand, materialize_direct_call_args};
use crate::codegen_ir::{CodegenIrError, Result};

mod instance_expr;

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
        PhpType::Str => lower_runtime_string_call(ctx, inst, callable, "closure_call"),
        PhpType::Callable => instance_expr::lower_instance_method_closure_call(ctx, inst, callable)
            .or_else(|_| lower_descriptor_invoker_call(ctx, inst, callable, "closure_call")),
        other => Err(CodegenIrError::unsupported(format!(
            "closure_call for callable PHP type {:?}",
            other
        ))),
    }
}

/// Lowers expression-call forms like `($expr)(...)` when the callee is a runtime string.
pub(super) fn lower_expr_call(ctx: &mut FunctionContext<'_>, inst: &Instruction) -> Result<()> {
    let callable = expect_operand(inst, 0)?;
    match ctx.value_php_type(callable)?.codegen_repr() {
        PhpType::Str => lower_runtime_string_call(ctx, inst, callable, "expr_call"),
        PhpType::Callable => instance_expr::lower_instance_method_expr_call(ctx, inst, callable)
            .or_else(|_| lower_descriptor_invoker_call(ctx, inst, callable, "expr_call")),
        other => Err(CodegenIrError::unsupported(format!(
            "expr_call for callable PHP type {:?}",
            other
        ))),
    }
}

/// Lowers a callable descriptor call through its uniform invoker slot.
fn lower_descriptor_invoker_call(
    ctx: &mut FunctionContext<'_>,
    inst: &Instruction,
    callable: ValueId,
    op_name: &str,
) -> Result<()> {
    let visible_args = inst.operands.iter().skip(1).copied().collect::<Vec<_>>();
    let descriptor_reg = abi::nested_call_reg(ctx.emitter);
    let invoker_reg = abi::symbol_scratch_reg(ctx.emitter);
    ctx.load_value_to_reg(callable, descriptor_reg)?;
    callable_descriptor::emit_load_invoker_from_descriptor(ctx.emitter, invoker_reg, descriptor_reg);
    let ready_label = ctx.next_label(&format!("{}_descriptor_invoker_ready", op_name));
    emit_branch_if_invoker_present(ctx, invoker_reg, &ready_label);
    emit_missing_descriptor_invoker_fatal(ctx, op_name);

    ctx.emitter.label(&ready_label);
    emit_invoker_arg_mixed(ctx, &visible_args)?;
    abi::emit_push_reg(ctx.emitter, abi::int_result_reg(ctx.emitter));          // preserve the boxed Mixed argument array across descriptor register setup
    move_reg_to_arg(ctx, descriptor_reg, 0);
    let arg_reg = abi::int_arg_reg_name(ctx.emitter.target, 1);
    abi::emit_load_temporary_stack_slot(ctx.emitter, arg_reg, 0);
    callable_descriptor::emit_load_invoker_from_descriptor(ctx.emitter, invoker_reg, descriptor_reg);
    abi::emit_call_reg(ctx.emitter, invoker_reg);
    release_invoker_arg_preserving_result(ctx);
    store_descriptor_invoker_result(ctx, inst)
}

/// Branches to `ready_label` when a callable descriptor has a uniform invoker.
fn emit_branch_if_invoker_present(
    ctx: &mut FunctionContext<'_>,
    invoker_reg: &str,
    ready_label: &str,
) {
    match ctx.emitter.target.arch {
        Arch::AArch64 => {
            ctx.emitter.instruction(&format!("cbnz {}, {}", invoker_reg, ready_label)); // continue when the callable descriptor has a uniform invoker
        }
        Arch::X86_64 => {
            ctx.emitter.instruction(&format!("test {}, {}", invoker_reg, invoker_reg)); // check whether the callable descriptor has a uniform invoker
            ctx.emitter.instruction(&format!("jnz {}", ready_label));           // continue when the callable descriptor has a uniform invoker
        }
    }
}

/// Emits a fatal diagnostic for callable descriptors without a uniform invoker.
fn emit_missing_descriptor_invoker_fatal(ctx: &mut FunctionContext<'_>, op_name: &str) {
    let message = format!(
        "Fatal error: Unsupported EIR {} callable descriptor without invoker\n",
        op_name
    );
    let (message_label, message_len) = ctx.data.add_string(message.as_bytes());
    match ctx.emitter.target.arch {
        Arch::AArch64 => {
            ctx.emitter.instruction("mov x0, #2");                              // write the missing descriptor-invoker diagnostic to stderr
            ctx.emitter.adrp("x1", &message_label);                             // load the missing descriptor-invoker diagnostic page
            ctx.emitter.add_lo12("x1", "x1", &message_label);                  // resolve the missing descriptor-invoker diagnostic address
            ctx.emitter.instruction(&format!("mov x2, #{}", message_len));      // pass the descriptor-invoker diagnostic byte length to write
            ctx.emitter.syscall(4);
            abi::emit_exit(ctx.emitter, 1);
        }
        Arch::X86_64 => {
            ctx.emitter.instruction("mov edi, 2");                              // write the missing descriptor-invoker diagnostic to stderr
            abi::emit_symbol_address(ctx.emitter, "rsi", &message_label);
            ctx.emitter.instruction(&format!("mov edx, {}", message_len));      // pass the descriptor-invoker diagnostic byte length to write
            ctx.emitter.instruction("mov eax, 1");                              // Linux x86_64 syscall 1 = write
            ctx.emitter.instruction("syscall");                                 // emit the missing descriptor-invoker diagnostic before terminating
            abi::emit_exit(ctx.emitter, 1);
        }
    }
}

/// Creates an indexed argument array and boxes it as the descriptor-invoker container.
fn emit_invoker_arg_mixed(ctx: &mut FunctionContext<'_>, args: &[ValueId]) -> Result<()> {
    emit_invoker_arg_array(ctx, args)?;
    emit_box_current_owned_value_as_mixed(
        ctx.emitter,
        &PhpType::Array(Box::new(PhpType::Mixed)),
    );
    Ok(())
}

/// Creates the indexed array consumed by runtime callable descriptor invokers.
fn emit_invoker_arg_array(ctx: &mut FunctionContext<'_>, args: &[ValueId]) -> Result<()> {
    emit_new_invoker_arg_array(ctx, args.len());
    if args.is_empty() {
        return Ok(());
    }
    abi::emit_push_reg(ctx.emitter, abi::int_result_reg(ctx.emitter));          // preserve the in-progress invoker argument array across element boxing
    for arg in args {
        emit_box_invoker_arg(ctx, *arg)?;
        abi::emit_push_reg(ctx.emitter, abi::int_result_reg(ctx.emitter));      // preserve the boxed argument while loading the invoker array
        emit_append_boxed_invoker_arg(ctx);
        emit_release_pushed_refcounted_temp_after_array_push(ctx.emitter, &PhpType::Mixed);
        emit_store_result_to_top_stack_slot(ctx);
    }
    abi::emit_load_temporary_stack_slot(
        ctx.emitter,
        abi::int_result_reg(ctx.emitter),
        0,
    );
    abi::emit_release_temporary_stack(ctx.emitter, 16);
    Ok(())
}

/// Allocates the raw indexed array used to pass visible arguments to descriptor invokers.
fn emit_new_invoker_arg_array(ctx: &mut FunctionContext<'_>, arg_count: usize) {
    match ctx.emitter.target.arch {
        Arch::AArch64 => {
            abi::emit_load_int_immediate(ctx.emitter, "x0", arg_count as i64);
            abi::emit_load_int_immediate(ctx.emitter, "x1", 8);
        }
        Arch::X86_64 => {
            abi::emit_load_int_immediate(ctx.emitter, "rdi", arg_count as i64);
            abi::emit_load_int_immediate(ctx.emitter, "rsi", 8);
        }
    }
    abi::emit_call_label(ctx.emitter, "__rt_array_new");
}

/// Boxes or retains a visible descriptor-invoker argument as an owned Mixed cell.
fn emit_box_invoker_arg(ctx: &mut FunctionContext<'_>, arg: ValueId) -> Result<()> {
    let arg_ty = ctx.value_php_type(arg)?.codegen_repr();
    ctx.load_value_to_result(arg)?;
    if matches!(arg_ty, PhpType::Mixed | PhpType::Union(_)) {
        abi::emit_incref_if_refcounted(ctx.emitter, &arg_ty);
    } else if ctx.value_can_own_mixed_box_source(arg)? {
        emit_box_current_owned_value_as_mixed(ctx.emitter, &arg_ty);
    } else {
        emit_box_current_value_as_mixed(ctx.emitter, &arg_ty);
    }
    Ok(())
}

/// Appends the boxed top-of-stack Mixed cell into the saved invoker argument array.
fn emit_append_boxed_invoker_arg(ctx: &mut FunctionContext<'_>) {
    match ctx.emitter.target.arch {
        Arch::AArch64 => {
            abi::emit_load_temporary_stack_slot(ctx.emitter, "x9", 16);
            ctx.emitter.instruction("mov x1, x0");                              // pass the boxed visible argument to the invoker array append helper
            ctx.emitter.instruction("mov x0, x9");                              // pass the saved invoker argument array to the append helper
        }
        Arch::X86_64 => {
            abi::emit_load_temporary_stack_slot(ctx.emitter, "rdi", 16);
            ctx.emitter.instruction("mov rsi, rax");                            // pass the boxed visible argument to the invoker array append helper
        }
    }
    abi::emit_call_label(ctx.emitter, "__rt_array_push_refcounted");
}

/// Stores the current single-register result into the temporary stack slot at `sp`.
fn emit_store_result_to_top_stack_slot(ctx: &mut FunctionContext<'_>) {
    match ctx.emitter.target.arch {
        Arch::AArch64 => {
            ctx.emitter.instruction("str x0, [sp]");                            // update the saved invoker argument array after append growth
        }
        Arch::X86_64 => {
            ctx.emitter.instruction("mov QWORD PTR [rsp], rax");                // update the saved invoker argument array after append growth
        }
    }
}

/// Moves a general-purpose register into an ABI argument register.
fn move_reg_to_arg(ctx: &mut FunctionContext<'_>, source_reg: &str, arg_index: usize) {
    let arg_reg = abi::int_arg_reg_name(ctx.emitter.target, arg_index);
    if source_reg == arg_reg {
        return;
    }
    match ctx.emitter.target.arch {
        Arch::AArch64 => {
            ctx.emitter.instruction(&format!("mov {}, {}", arg_reg, source_reg)); // move the callable descriptor into the invoker ABI argument
        }
        Arch::X86_64 => {
            ctx.emitter.instruction(&format!("mov {}, {}", arg_reg, source_reg)); // move the callable descriptor into the invoker ABI argument
        }
    }
}

/// Releases the temporary invoker argument while preserving the Mixed call result.
fn release_invoker_arg_preserving_result(ctx: &mut FunctionContext<'_>) {
    abi::emit_push_result_value(ctx.emitter, &PhpType::Mixed);
    abi::emit_load_temporary_stack_slot(ctx.emitter, abi::int_result_reg(ctx.emitter), 16);
    abi::emit_decref_if_refcounted(ctx.emitter, &PhpType::Mixed);
    abi::emit_pop_reg(ctx.emitter, abi::int_result_reg(ctx.emitter));
    abi::emit_release_temporary_stack(ctx.emitter, 16);
}

/// Stores the Mixed descriptor-invoker result using the EIR result type.
fn store_descriptor_invoker_result(
    ctx: &mut FunctionContext<'_>,
    inst: &Instruction,
) -> Result<()> {
    let Some(result) = inst.result else {
        return Ok(());
    };
    match ctx.value_php_type(result)?.codegen_repr() {
        PhpType::Mixed | PhpType::Union(_) => ctx.store_result_value(result),
        PhpType::Void | PhpType::Never => {
            abi::emit_load_int_immediate(
                ctx.emitter,
                abi::int_result_reg(ctx.emitter),
                0x7fff_ffff_ffff_fffe,
            );
            ctx.store_result_value(result)
        }
        PhpType::Int => {
            move_result_to_arg(ctx, 0);
            abi::emit_call_label(ctx.emitter, "__rt_mixed_cast_int");
            ctx.store_result_value(result)
        }
        PhpType::Bool => {
            move_result_to_arg(ctx, 0);
            abi::emit_call_label(ctx.emitter, "__rt_mixed_cast_bool");
            ctx.store_result_value(result)
        }
        PhpType::Float => {
            move_result_to_arg(ctx, 0);
            abi::emit_call_label(ctx.emitter, "__rt_mixed_cast_float");
            ctx.store_result_value(result)
        }
        PhpType::Str => {
            move_result_to_arg(ctx, 0);
            abi::emit_call_label(ctx.emitter, "__rt_mixed_cast_string");
            ctx.store_result_value(result)
        }
        other => Err(CodegenIrError::unsupported(format!(
            "descriptor invoker result for PHP type {:?}",
            other
        ))),
    }
}

/// Moves the current integer result register into an ABI argument register.
fn move_result_to_arg(ctx: &mut FunctionContext<'_>, arg_index: usize) {
    let result_reg = abi::int_result_reg(ctx.emitter);
    move_reg_to_arg(ctx, result_reg, arg_index);
}

/// Lowers `value |> $callable` when `$callable` is a first-class user-function descriptor.
pub(super) fn lower_pipe_call(ctx: &mut FunctionContext<'_>, inst: &Instruction) -> Result<()> {
    if inst.operands.len() != 2 {
        return Err(CodegenIrError::invalid_module(format!(
            "pipe_call expected value and callable operands, got {}",
            inst.operands.len()
        )));
    }
    let value = expect_operand(inst, 0)?;
    let callable = expect_operand(inst, 1)?;
    let value_ty = ctx.value_php_type(value)?.codegen_repr();
    reject_signature_dependent_pipe_type("pipe value", &value_ty)?;
    if let Some(result) = inst.result {
        let result_ty = ctx.value_php_type(result)?.codegen_repr();
        reject_signature_dependent_pipe_type("pipe result", &result_ty)?;
    }

    let descriptor_reg = abi::nested_call_reg(ctx.emitter);
    ctx.load_value_to_reg(callable, descriptor_reg)?;
    let fatal_label = ctx.next_label("pipe_call_unsupported_descriptor");
    emit_branch_if_not_user_function_descriptor(ctx, descriptor_reg, &fatal_label);

    let overflow_bytes = materialize_direct_call_args(ctx, &[value], &[value_ty])?;
    let caller_stack_pad_bytes = direct_call_stack_pad_bytes(ctx, overflow_bytes);
    let entry_reg = abi::secondary_scratch_reg(ctx.emitter);
    callable_descriptor::emit_load_entry_from_descriptor(ctx.emitter, entry_reg, descriptor_reg);
    abi::emit_reserve_temporary_stack(ctx.emitter, caller_stack_pad_bytes);
    abi::emit_call_reg(ctx.emitter, entry_reg);
    abi::emit_release_temporary_stack(ctx.emitter, caller_stack_pad_bytes);
    abi::emit_release_temporary_stack(ctx.emitter, overflow_bytes);
    store_pipe_call_result(ctx, inst)?;

    let done_label = ctx.next_label("pipe_call_done");
    abi::emit_jump(ctx.emitter, &done_label);
    ctx.emitter.label(&fatal_label);
    emit_unsupported_pipe_call_fatal(ctx);
    ctx.emitter.label(&done_label);
    Ok(())
}

/// Rejects pipe-call shapes whose ABI cannot be recovered without callable signature metadata.
fn reject_signature_dependent_pipe_type(label: &str, ty: &PhpType) -> Result<()> {
    if matches!(ty.codegen_repr(), PhpType::Mixed | PhpType::Union(_) | PhpType::Void | PhpType::Never) {
        return Err(CodegenIrError::unsupported(format!(
            "{} PHP type {:?} without descriptor signature metadata",
            label, ty
        )));
    }
    Ok(())
}

/// Emits runtime guards that keep this Phase-04 pipe lowering on plain user functions.
fn emit_branch_if_not_user_function_descriptor(
    ctx: &mut FunctionContext<'_>,
    descriptor_reg: &str,
    fatal_label: &str,
) {
    let kind_reg = abi::secondary_scratch_reg(ctx.emitter);
    match ctx.emitter.target.arch {
        Arch::AArch64 => {
            ctx.emitter.instruction(&format!("cmp {}, #0", descriptor_reg));    // reject a missing first-class callable descriptor before dereferencing it
            ctx.emitter.instruction(&format!("b.eq {}", fatal_label));          // report unsupported pipe callable descriptors instead of branching through null
            abi::emit_load_from_address(ctx.emitter, kind_reg, descriptor_reg, 0);
            ctx.emitter.instruction(&format!(
                "cmp {}, #{}",
                kind_reg,
                callable_descriptor::CALLABLE_DESC_KIND_FUNCTION
            ));                                                                 // verify the descriptor targets a plain user function
            ctx.emitter.instruction(&format!("b.ne {}", fatal_label));          // keep non-function callable descriptors on the explicit unsupported path
        }
        Arch::X86_64 => {
            ctx.emitter.instruction(&format!("test {}, {}", descriptor_reg, descriptor_reg)); // reject a missing first-class callable descriptor before dereferencing it
            ctx.emitter.instruction(&format!("je {}", fatal_label));            // report unsupported pipe callable descriptors instead of branching through null
            abi::emit_load_from_address(ctx.emitter, kind_reg, descriptor_reg, 0);
            ctx.emitter.instruction(&format!(
                "cmp {}, {}",
                kind_reg,
                callable_descriptor::CALLABLE_DESC_KIND_FUNCTION
            ));                                                                 // verify the descriptor targets a plain user function
            ctx.emitter.instruction(&format!("jne {}", fatal_label));           // keep non-function callable descriptors on the explicit unsupported path
        }
    }
}

/// Stores an indirect pipe-call result using the EIR result type.
fn store_pipe_call_result(ctx: &mut FunctionContext<'_>, inst: &Instruction) -> Result<()> {
    let Some(result) = inst.result else {
        return Ok(());
    };
    if ctx.value_php_type(result)?.codegen_repr() == PhpType::Void {
        abi::emit_load_int_immediate(
            ctx.emitter,
            abi::int_result_reg(ctx.emitter),
            0x7fff_ffff_ffff_fffe,
        );
    }
    ctx.store_result_value(result)
}

/// Emits the fatal path for pipe-call callable descriptors not covered by Phase 04.
fn emit_unsupported_pipe_call_fatal(ctx: &mut FunctionContext<'_>) {
    let message = b"Fatal error: Unsupported EIR pipe callable descriptor\n";
    let (message_label, message_len) = ctx.data.add_string(message);
    match ctx.emitter.target.arch {
        Arch::AArch64 => {
            ctx.emitter.instruction("mov x0, #2");                              // write the unsupported pipe-call diagnostic to stderr
            ctx.emitter.adrp("x1", &message_label);                             // load the unsupported pipe-call diagnostic string page
            ctx.emitter.add_lo12("x1", "x1", &message_label);                  // resolve the unsupported pipe-call diagnostic string address
            ctx.emitter.instruction(&format!("mov x2, #{}", message_len));      // pass the unsupported pipe-call diagnostic byte length to write
            ctx.emitter.syscall(4);
            abi::emit_exit(ctx.emitter, 1);
        }
        Arch::X86_64 => {
            ctx.emitter.instruction("mov edi, 2");                              // write the unsupported pipe-call diagnostic to Linux stderr
            abi::emit_symbol_address(ctx.emitter, "rsi", &message_label);
            ctx.emitter.instruction(&format!("mov edx, {}", message_len));      // pass the unsupported pipe-call diagnostic byte length to write
            ctx.emitter.instruction("mov eax, 1");                              // Linux x86_64 syscall 1 = write
            ctx.emitter.instruction("syscall");                                 // emit the unsupported pipe-call diagnostic before terminating
            abi::emit_exit(ctx.emitter, 1);
        }
    }
}

/// Dispatches a runtime string callable across user functions with compatible ABI shape.
fn lower_runtime_string_call(
    ctx: &mut FunctionContext<'_>,
    inst: &Instruction,
    callable: ValueId,
    op_name: &str,
) -> Result<()> {
    let args = inst.operands.iter().skip(1).copied().collect::<Vec<_>>();
    let targets = runtime_string_function_targets(ctx, args.len(), inst)?;
    if targets.is_empty() {
        return Err(CodegenIrError::unsupported(format!(
            "{} with no compatible user-function targets",
            op_name
        )));
    }

    let (ptr_reg, len_reg) = abi::string_result_regs(ctx.emitter);
    ctx.load_string_value_to_regs(callable, ptr_reg, len_reg)?;
    abi::emit_push_reg_pair(ctx.emitter, ptr_reg, len_reg);

    let done_label = ctx.next_label(&format!("{}_done", op_name));
    let miss_label = ctx.next_label(&format!("{}_missing", op_name));
    let mut case_labels = Vec::with_capacity(targets.len());
    for target in &targets {
        let label = ctx.next_label(&format!("{}_{}", op_name, label_fragment(&target.name)));
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
