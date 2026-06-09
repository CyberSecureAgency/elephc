//! Purpose:
//! Emits PHP `hash` string transformation or formatting calls.
//! Marshals string/scalar arguments into runtime helpers that allocate returned PHP strings.
//!
//! Called from:
//! - `crate::codegen::builtins::strings::emit()`.
//!
//! Key details:
//! - Returned string pointer/length pairs must be treated as owned runtime values when the helper allocates.

use super::hash_crypto;
use crate::codegen::context::Context;
use crate::codegen::data_section::DataSection;
use crate::codegen::emit::Emitter;
use crate::codegen::expr::{coerce_to_truthiness, emit_expr};
use crate::codegen::{abi, platform::Arch};
use crate::parser::ast::Expr;
use crate::types::PhpType;

/// Emits a PHP `hash($algo, $data, $binary = false)` call as a runtime helper invocation.
///
/// The algorithm string is evaluated first and preserved on the stack while the
/// optional `$binary` flag is evaluated, coerced to a 0/1 integer, and preserved
/// in turn; the data string is evaluated last. Before the `__rt_hash` call the
/// arguments are materialised in the runtime ABI registers (algo ptr/len, data
/// ptr/len, and the binary flag in AArch64 `x5` / x86_64 `r10`). The
/// `elephc_crypto_hash` entry point is published into its runtime fn-pointer slot
/// immediately before the call so only hashing programs link `-lelephc_crypto`.
///
/// # Arguments
/// - `_name`: Unused; the runtime helper handles algorithm dispatch internally.
/// - `args`: Two or three expressions — algorithm name, data string, and the
///   optional `$binary` flag (defaults to `false`/`0` when omitted).
/// - `emitter`: Target-aware assembly emitter.
/// - `ctx`: Codegen context carrying variable layout and metadata.
/// - `data`: Data section for relocatable constants.
///
/// # Returns
/// `Some(PhpType::Str)` indicating the result is a PHP string.
///
/// # Side effects
/// - Clobbers caller-saved registers appropriate to each target's ABI.
/// - The runtime helper allocates a PHP string; caller owns the returned value.
/// - An unknown algorithm throws a catchable `\ValueError` from the runtime.
pub fn emit(
    _name: &str,
    args: &[Expr],
    emitter: &mut Emitter,
    ctx: &mut Context,
    data: &mut DataSection,
) -> Option<PhpType> {
    emitter.comment("hash()");
    // hash($algo, $data, $binary) — evaluate algo string first
    emit_expr(&args[0], emitter, ctx, data);
    match emitter.target.arch {
        Arch::AArch64 => {
            emitter.instruction("stp x1, x2, [sp, #-16]!");                     // preserve the algorithm string while evaluating the binary flag and data string
            emit_binary_flag(args, emitter, ctx, data);
            emitter.instruction("str x0, [sp, #-16]!");                         // preserve the 0/1 binary flag while evaluating the data string expression
            emit_expr(&args[1], emitter, ctx, data);
            emitter.instruction("mov x3, x1");                                  // move the data string pointer into the secondary runtime argument register pair on AArch64
            emitter.instruction("mov x4, x2");                                  // move the data string length into the secondary runtime argument register pair on AArch64
            emitter.instruction("ldr x5, [sp], #16");                           // restore the binary flag into its runtime argument register on AArch64
            emitter.instruction("ldp x1, x2, [sp], #16");                       // restore the algorithm string after evaluating the data string expression
        }
        Arch::X86_64 => {
            abi::emit_push_reg_pair(emitter, "rax", "rdx");                     // preserve the algorithm string ptr/len while evaluating the binary flag and data string on x86_64
            emit_binary_flag(args, emitter, ctx, data);
            abi::emit_push_reg(emitter, "rax");                                 // preserve the 0/1 binary flag while evaluating the data string expression
            emit_expr(&args[1], emitter, ctx, data);
            emitter.instruction("mov rdi, rax");                                // move the data string pointer into the secondary x86_64 runtime argument register
            emitter.instruction("mov rsi, rdx");                                // move the data string length into the secondary x86_64 runtime argument register
            abi::emit_pop_reg(emitter, "r10");                                  // restore the binary flag into its runtime argument register on x86_64
            abi::emit_pop_reg_pair(emitter, "rax", "rdx");                      // restore the algorithm string ptr/len after evaluating the data string expression
        }
    }
    hash_crypto::publish_elephc_crypto_function_pointers(emitter);
    abi::emit_call_label(emitter, "__rt_hash");                                 // call the target-aware runtime helper that hashes through elephc-crypto and returns the PHP string
    Some(PhpType::Str)
}

/// Materialises the optional `$binary` flag as a 0/1 integer in the int result
/// register, defaulting to `0` (PHP `false`) when the third argument is omitted.
fn emit_binary_flag(
    args: &[Expr],
    emitter: &mut Emitter,
    ctx: &mut Context,
    data: &mut DataSection,
) {
    if args.len() >= 3 {
        let ty = emit_expr(&args[2], emitter, ctx, data);
        coerce_to_truthiness(emitter, ctx, &ty);
    } else {
        abi::emit_load_int_immediate(emitter, abi::int_result_reg(emitter), 0); // default $binary to false (hex output) when omitted
    }
}
