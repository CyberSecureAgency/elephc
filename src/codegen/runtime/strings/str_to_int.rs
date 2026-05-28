//! Purpose:
//! Emits the `__rt_str_to_int` runtime helper for PHP string-to-int casts.
//! Bridges bounded PHP strings through numeric-string parsing so whitespace, signs, and exponents match PHP.
//!
//! Called from:
//! - `crate::codegen::runtime::emitters::emit_runtime()` via `crate::codegen::runtime::strings`.
//!
//! Key details:
//! - Reuses `__rt_str_to_number`, then truncates the parsed double toward zero like PHP casts.

use crate::codegen::{abi, emit::Emitter, platform::Arch};

/// Emits `__rt_str_to_int` for PHP string-to-int conversion.
///
/// Input follows the active string-result convention:
/// AArch64 uses `x1`/`x2`; x86_64 uses `rax`/`rdx`.
/// The helper preserves nested-call return state, reuses `__rt_str_to_number`
/// for PHP numeric-prefix parsing, and returns the truncated integer result.
pub fn emit_str_to_int(emitter: &mut Emitter) {
    if emitter.target.arch == Arch::X86_64 {
        emit_str_to_int_linux_x86_64(emitter);
        return;
    }

    emitter.blank();
    emitter.comment("--- runtime: str_to_int ---");
    emitter.label_global("__rt_str_to_int");

    emitter.instruction("sub sp, sp, #16");                                     // allocate a frame for preserving the return address across parsing
    emitter.instruction("stp x29, x30, [sp]");                                  // save frame pointer and return address before the nested helper call
    emitter.instruction("mov x29, sp");                                         // establish a stable helper frame pointer
    abi::emit_call_label(emitter, "__rt_str_to_number");                        // parse the PHP string as a numeric prefix and leave the double result live
    abi::emit_float_result_to_int_result(emitter);                              // truncate the parsed numeric value toward zero for PHP int casts
    emitter.instruction("ldp x29, x30, [sp]");                                  // restore the caller frame pointer and return address
    emitter.instruction("add sp, sp, #16");                                     // release the helper frame
    emitter.instruction("ret");                                                 // return the integer cast result
}

/// Emits the Linux x86_64 `__rt_str_to_int` runtime helper.
///
/// The input string arrives in the elephc string-result registers (`rax`/`rdx`).
/// The nested numeric parser returns the parsed double in `xmm0`; this helper
/// truncates it into `rax`, matching PHP's string-to-int cast behavior.
fn emit_str_to_int_linux_x86_64(emitter: &mut Emitter) {
    emitter.blank();
    emitter.comment("--- runtime: str_to_int ---");
    emitter.label_global("__rt_str_to_int");

    emitter.instruction("push rbp");                                            // preserve the caller frame pointer before calling the numeric parser
    emitter.instruction("mov rbp, rsp");                                        // establish a stable helper frame pointer and aligned call stack
    abi::emit_call_label(emitter, "__rt_str_to_number");                        // parse the PHP string as a numeric prefix and leave the double result live
    abi::emit_float_result_to_int_result(emitter);                              // truncate the parsed numeric value toward zero for PHP int casts
    emitter.instruction("pop rbp");                                             // restore the caller frame pointer
    emitter.instruction("ret");                                                 // return the integer cast result
}
