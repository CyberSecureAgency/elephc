//! Purpose:
//! Emits the `__rt_hash` runtime helper and the shared `__rt_digest_to_string`
//! formatter that route PHP `hash()` through the elephc-crypto staticlib.
//! Keeps PHP byte-string pointer/length behavior and target-specific ABI variants
//! in one focused emitter.
//!
//! Called from:
//! - `crate::codegen::runtime::emitters::emit_runtime()` via `crate::codegen::runtime::strings`.
//!
//! Key details:
//! - `__rt_hash` calls `elephc_crypto_hash` indirectly through the
//!   `_elephc_crypto_hash_fn` slot (published at the call site), so the shared
//!   runtime never names elephc-crypto and non-hashing programs do not link it.
//! - An unknown algorithm (slot null or a -1 return) throws a catchable
//!   `\ValueError` through the shared clamp-style stamping sequence.
//! - `__rt_digest_to_string` turns a (raw digest ptr, length, binary flag) triple
//!   into a `_concat_buf`-backed PHP string: lowercase hex when the flag is 0, or
//!   the raw bytes verbatim when it is non-zero. md5/sha1 reuse it next.

use crate::codegen::abi;
use crate::codegen::builtins::hash_crypto;
use crate::codegen::emit::Emitter;
use crate::codegen::platform::Arch;
use crate::codegen::runtime::data::HASH_UNKNOWN_ALGO_MSG;

/// Emits the `__rt_hash` runtime helper for the `hash()` built-in.
///
/// Input registers:
///   AArch64: x1/x2 = algorithm name ptr/len, x3/x4 = data ptr/len,
///            x5 = binary flag (0 = hex output, non-zero = raw bytes).
///   x86_64:  rax/rdx = algorithm name ptr/len, rdi/rsi = data ptr/len,
///            r10 = binary flag.
///
/// Output registers (PHP string ptr/len pair):
///   AArch64: x1 = ptr, x2 = len.
///   x86_64:  rax = ptr, rdx = len.
///
/// Marshals the C ABI for `elephc_crypto_hash(name,name_len,data,data_len,out)`,
/// calls it indirectly through `_elephc_crypto_hash_fn`, throws a `\ValueError`
/// when the slot is null or the call returns -1, and otherwise formats the raw
/// digest through `__rt_digest_to_string`. Saves and restores fp/lr (rbp).
pub fn emit_hash(emitter: &mut Emitter) {
    if emitter.target.arch == Arch::X86_64 {
        emit_hash_linux_x86_64(emitter);
        emit_digest_to_string(emitter);
        return;
    }

    emitter.blank();
    emitter.comment("--- runtime: hash ---");
    emitter.label_global("__rt_hash");

    // -- set up frame: [sp,#0..64)=digest buffer, [sp,#64]=binary flag, [sp,#80]=fp/lr --
    emitter.instruction("sub sp, sp, #96");                                     // allocate 64B digest buffer + flag slot + saved fp/lr (16-byte aligned)
    emitter.instruction("stp x29, x30, [sp, #80]");                             // save frame pointer and return address
    emitter.instruction("add x29, sp, #80");                                    // set frame pointer
    emitter.instruction("str x5, [sp, #64]");                                   // save the binary flag across the clobbering C call

    // -- marshal the C ABI for elephc_crypto_hash(name,name_len,data,data_len,out) --
    emitter.instruction("mov x6, x1");                                          // stash algorithm name pointer before the argument shuffle
    emitter.instruction("mov x7, x2");                                          // stash algorithm name length before the argument shuffle
    emitter.instruction("mov x0, x6");                                          // C arg0 = algorithm name pointer
    emitter.instruction("mov x1, x7");                                          // C arg1 = algorithm name length
    emitter.instruction("mov x2, x3");                                          // C arg2 = data pointer
    emitter.instruction("mov x3, x4");                                          // C arg3 = data length
    emitter.instruction("add x4, sp, #0");                                      // C arg4 = stack-backed 64-byte raw-digest output buffer

    // -- call elephc_crypto_hash indirectly through the published slot --
    abi::emit_symbol_address(emitter, "x9", "_elephc_crypto_hash_fn");
    emitter.instruction("ldr x9, [x9]");                                        // load the published elephc_crypto_hash function pointer
    emitter.instruction("cbz x9, __rt_hash_unknown");                           // null slot means the program never linked elephc-crypto → unknown algo
    abi::emit_call_reg(emitter, "x9");                                          // compute the raw digest into the stack buffer; x0 = digest length or -1

    // -- handle an unknown algorithm (-1) before formatting --
    emitter.instruction("cmp x0, #0");                                          // did elephc_crypto_hash reject the algorithm name?
    emitter.instruction("b.lt __rt_hash_unknown");                              // a negative length means the algorithm is unknown

    // -- format the raw digest into a PHP string --
    emitter.instruction("mov x1, x0");                                          // digest length argument for the shared formatter
    emitter.instruction("add x0, sp, #0");                                      // raw digest pointer argument for the shared formatter
    emitter.instruction("ldr x2, [sp, #64]");                                   // reload the binary flag for the shared formatter
    emitter.instruction("bl __rt_digest_to_string");                            // turn (ptr,len,flag) into a _concat_buf string in x1/x2
    emitter.instruction("ldp x29, x30, [sp, #80]");                             // restore frame pointer and return address
    emitter.instruction("add sp, sp, #96");                                     // deallocate the helper frame
    emitter.instruction("ret");                                                 // return the PHP string ptr/len in x1/x2

    // -- unknown algorithm: throw a catchable \ValueError --
    emitter.label("__rt_hash_unknown");
    emitter.instruction("ldp x29, x30, [sp, #80]");                             // restore frame pointer and return address before throwing
    emitter.instruction("add sp, sp, #96");                                     // deallocate the helper frame before throwing
    hash_crypto::emit_throw_unknown_algorithm_value_error(
        emitter,
        "_hash_unknown_algo_msg",
        HASH_UNKNOWN_ALGO_MSG.len(),
    );

    emit_digest_to_string(emitter);
}

/// Emits the x86_64 Linux variant of the `__rt_hash` runtime helper.
///
/// See [`emit_hash`] for the register contract. Receives the binary flag in r10
/// (the 5th C argument register r8 is reserved for the output buffer), saves it
/// to the stack across the C call, calls `elephc_crypto_hash` indirectly, throws
/// a `\ValueError` on a null slot or -1 return, and otherwise formats the raw
/// digest through `__rt_digest_to_string`. Preserves rbp.
fn emit_hash_linux_x86_64(emitter: &mut Emitter) {
    emitter.blank();
    emitter.comment("--- runtime: hash ---");
    emitter.label_global("__rt_hash");
    emitter.instruction("push rbp");                                            // preserve the caller frame pointer before reserving the digest scratch space
    emitter.instruction("mov rbp, rsp");                                        // establish a stable frame base for the digest buffer and saved flag
    emitter.instruction("sub rsp, 96");                                         // reserve a 64-byte raw-digest buffer plus saved-flag scratch (16-byte aligned)
    emitter.instruction("mov QWORD PTR [rbp - 72], r10");                       // save the binary flag across the clobbering C call

    // -- marshal the C ABI for elephc_crypto_hash(name,name_len,data,data_len,out) --
    emitter.instruction("mov r8, rdi");                                         // stash the data pointer before rdi is overwritten by the algorithm name
    emitter.instruction("mov r9, rsi");                                         // stash the data length before rsi is overwritten by the algorithm name length
    emitter.instruction("mov rdi, rax");                                        // C arg0 = algorithm name pointer
    emitter.instruction("mov rsi, rdx");                                        // C arg1 = algorithm name length
    emitter.instruction("mov rdx, r8");                                         // C arg2 = data pointer
    emitter.instruction("mov rcx, r9");                                         // C arg3 = data length
    emitter.instruction("lea r8, [rbp - 64]");                                  // C arg4 = stack-backed 64-byte raw-digest output buffer

    // -- call elephc_crypto_hash indirectly through the published slot --
    emitter.instruction("mov r9, QWORD PTR [rip + _elephc_crypto_hash_fn]");    // load the published elephc_crypto_hash function pointer
    emitter.instruction("test r9, r9");                                         // a null slot means the program never linked elephc-crypto → unknown algo
    emitter.instruction("jz __rt_hash_unknown_linux_x86_64");                   // throw the unknown-algorithm ValueError when the slot is null
    emitter.instruction("call r9");                                             // compute the raw digest into the stack buffer; rax = digest length or -1

    // -- handle an unknown algorithm (-1) before formatting --
    emitter.instruction("test rax, rax");                                       // did elephc_crypto_hash reject the algorithm name?
    emitter.instruction("js __rt_hash_unknown_linux_x86_64");                   // a negative length means the algorithm is unknown

    // -- format the raw digest into a PHP string --
    emitter.instruction("mov rsi, rax");                                        // digest length argument for the shared formatter
    emitter.instruction("lea rdi, [rbp - 64]");                                 // raw digest pointer argument for the shared formatter
    emitter.instruction("mov rdx, QWORD PTR [rbp - 72]");                       // reload the binary flag for the shared formatter
    emitter.instruction("call __rt_digest_to_string");                          // turn (ptr,len,flag) into a _concat_buf string in rax/rdx
    emitter.instruction("add rsp, 96");                                         // release the helper frame
    emitter.instruction("pop rbp");                                             // restore the caller frame pointer
    emitter.instruction("ret");                                                 // return the PHP string ptr/len in rax/rdx

    // -- unknown algorithm: throw a catchable \ValueError --
    emitter.label("__rt_hash_unknown_linux_x86_64");
    emitter.instruction("add rsp, 96");                                         // release the helper frame before throwing
    emitter.instruction("pop rbp");                                             // restore the caller frame pointer before throwing
    hash_crypto::emit_throw_unknown_algorithm_value_error(
        emitter,
        "_hash_unknown_algo_msg",
        HASH_UNKNOWN_ALGO_MSG.len(),
    );
}

/// Emits the shared `__rt_digest_to_string` runtime helper.
///
/// Converts a raw digest into a `_concat_buf`-backed PHP string and advances
/// `_concat_off`. When the binary flag is zero it writes a lowercase hex string
/// (two chars per byte); otherwise it copies the raw bytes verbatim. The loops
/// are length-driven so any digest size works.
///
/// Input registers:
///   AArch64: x0 = raw digest ptr, x1 = length, x2 = binary flag.
///   x86_64:  rdi = raw digest ptr, rsi = length, rdx = binary flag.
///
/// Output registers (PHP string ptr/len pair):
///   AArch64: x1 = ptr, x2 = len.
///   x86_64:  rax = ptr, rdx = len.
fn emit_digest_to_string(emitter: &mut Emitter) {
    if emitter.target.arch == Arch::X86_64 {
        emit_digest_to_string_x86_64(emitter);
        return;
    }
    emit_digest_to_string_aarch64(emitter);
}

/// Emits the AArch64 variant of `__rt_digest_to_string`.
fn emit_digest_to_string_aarch64(emitter: &mut Emitter) {
    emitter.blank();
    emitter.comment("--- runtime: digest_to_string ---");
    emitter.label_global("__rt_digest_to_string");

    // -- resolve the concat-buffer destination cursor --
    abi::emit_symbol_address(emitter, "x6", "_concat_off");
    emitter.instruction("ldr x8, [x6]");                                        // load the current concat-buffer write offset
    abi::emit_symbol_address(emitter, "x7", "_concat_buf");
    emitter.instruction("add x9, x7, x8");                                      // compute the destination pointer for the formatted digest
    emitter.instruction("mov x10, x9");                                         // preserve the result start pointer across the write loop
    emitter.instruction("mov x11, x0");                                         // source = raw digest bytes
    emitter.instruction("mov x12, x1");                                         // remaining digest bytes to consume

    // -- binary flag chooses raw copy vs lowercase hex --
    emitter.instruction("cbnz x2, __rt_digest_raw_loop");                       // a non-zero binary flag copies the raw bytes verbatim

    // -- lowercase hex loop: two chars per digest byte --
    emitter.label("__rt_digest_hex_loop");
    emitter.instruction("cbz x12, __rt_digest_done");                           // all digest bytes converted to hex
    emitter.instruction("ldrb w13, [x11], #1");                                 // load one digest byte and advance the source cursor
    emitter.instruction("sub x12, x12, #1");                                    // decrement the remaining-byte counter
    // -- high nibble --
    emitter.instruction("lsr w14, w13, #4");                                    // extract the high 4 bits of the digest byte
    emitter.instruction("cmp w14, #10");                                        // does the high nibble need an 'a'-'f' digit?
    emitter.instruction("b.ge __rt_digest_hi_af");                              // map 10-15 to 'a'-'f'
    emitter.instruction("add w14, w14, #48");                                   // map 0-9 to '0'-'9'
    emitter.instruction("b __rt_digest_hi_st");                                 // store the high hex digit
    emitter.label("__rt_digest_hi_af");
    emitter.instruction("add w14, w14, #87");                                   // map 10-15 to 'a'-'f'
    emitter.label("__rt_digest_hi_st");
    emitter.instruction("strb w14, [x9], #1");                                  // write the high hex character and advance the destination
    // -- low nibble --
    emitter.instruction("and w14, w13, #0xf");                                  // extract the low 4 bits of the digest byte
    emitter.instruction("cmp w14, #10");                                        // does the low nibble need an 'a'-'f' digit?
    emitter.instruction("b.ge __rt_digest_lo_af");                              // map 10-15 to 'a'-'f'
    emitter.instruction("add w14, w14, #48");                                   // map 0-9 to '0'-'9'
    emitter.instruction("b __rt_digest_lo_st");                                 // store the low hex digit
    emitter.label("__rt_digest_lo_af");
    emitter.instruction("add w14, w14, #87");                                   // map 10-15 to 'a'-'f'
    emitter.label("__rt_digest_lo_st");
    emitter.instruction("strb w14, [x9], #1");                                  // write the low hex character and advance the destination
    emitter.instruction("b __rt_digest_hex_loop");                              // process the next digest byte

    // -- raw-bytes loop: copy each digest byte verbatim --
    emitter.label("__rt_digest_raw_loop");
    emitter.instruction("cbz x12, __rt_digest_done");                           // all raw digest bytes copied
    emitter.instruction("ldrb w13, [x11], #1");                                 // load one digest byte and advance the source cursor
    emitter.instruction("sub x12, x12, #1");                                    // decrement the remaining-byte counter
    emitter.instruction("strb w13, [x9], #1");                                  // write the raw digest byte and advance the destination
    emitter.instruction("b __rt_digest_raw_loop");                              // process the next raw digest byte

    // -- publish the result string and advance the concat offset --
    emitter.label("__rt_digest_done");
    emitter.instruction("mov x1, x10");                                         // result pointer = formatted-digest start
    emitter.instruction("sub x2, x9, x10");                                     // result length = bytes written
    emitter.instruction("ldr x8, [x6]");                                        // reload the concat-buffer write offset
    emitter.instruction("add x8, x8, x2");                                      // advance it past the formatted digest
    emitter.instruction("str x8, [x6]");                                        // persist the updated concat-buffer write offset
    emitter.instruction("ret");                                                 // return the PHP string ptr/len in x1/x2
}

/// Emits the x86_64 variant of `__rt_digest_to_string`.
fn emit_digest_to_string_x86_64(emitter: &mut Emitter) {
    emitter.blank();
    emitter.comment("--- runtime: digest_to_string ---");
    emitter.label_global("__rt_digest_to_string");

    // -- resolve the concat-buffer destination cursor --
    emitter.instruction("mov r8, QWORD PTR [rip + _concat_off]");               // load the current concat-buffer write offset
    abi::emit_symbol_address(emitter, "r10", "_concat_buf");
    emitter.instruction("lea r11, [r10 + r8]");                                 // compute the destination pointer for the formatted digest
    emitter.instruction("mov r8, r11");                                         // preserve the result start pointer across the write loop
    emitter.instruction("mov rcx, rdi");                                        // source = raw digest bytes
    emitter.instruction("mov r9, rsi");                                         // remaining digest bytes to consume

    // -- binary flag chooses raw copy vs lowercase hex --
    emitter.instruction("test rdx, rdx");                                       // is the binary flag set?
    emitter.instruction("jnz __rt_digest_raw_loop_linux_x86_64");               // a non-zero binary flag copies the raw bytes verbatim

    // -- lowercase hex loop: two chars per digest byte --
    emitter.label("__rt_digest_hex_loop_linux_x86_64");
    emitter.instruction("test r9, r9");                                         // any remaining digest bytes to format?
    emitter.instruction("jz __rt_digest_done_linux_x86_64");                    // all digest bytes converted to hex
    emitter.instruction("movzx edx, BYTE PTR [rcx]");                           // load one digest byte before splitting it into nibbles
    emitter.instruction("add rcx, 1");                                          // advance the source cursor past the consumed byte
    emitter.instruction("sub r9, 1");                                           // decrement the remaining-byte counter
    emitter.instruction("mov eax, edx");                                        // copy the digest byte before extracting its high nibble
    emitter.instruction("shr al, 4");                                           // isolate the high 4 bits of the digest byte
    emitter.instruction("cmp al, 10");                                          // does the high nibble need an 'a'-'f' digit?
    emitter.instruction("jae __rt_digest_hi_af_linux_x86_64");                  // map 10-15 to 'a'-'f'
    emitter.instruction("add al, 48");                                          // map 0-9 to '0'-'9'
    emitter.instruction("jmp __rt_digest_hi_store_linux_x86_64");               // store the high hex digit
    emitter.label("__rt_digest_hi_af_linux_x86_64");
    emitter.instruction("add al, 87");                                          // map 10-15 to 'a'-'f'
    emitter.label("__rt_digest_hi_store_linux_x86_64");
    emitter.instruction("mov BYTE PTR [r11], al");                              // write the high hex character
    emitter.instruction("add r11, 1");                                          // advance the destination past the high hex digit
    emitter.instruction("mov eax, edx");                                        // reload the digest byte before extracting its low nibble
    emitter.instruction("and al, 15");                                          // isolate the low 4 bits of the digest byte
    emitter.instruction("cmp al, 10");                                          // does the low nibble need an 'a'-'f' digit?
    emitter.instruction("jae __rt_digest_lo_af_linux_x86_64");                  // map 10-15 to 'a'-'f'
    emitter.instruction("add al, 48");                                          // map 0-9 to '0'-'9'
    emitter.instruction("jmp __rt_digest_lo_store_linux_x86_64");               // store the low hex digit
    emitter.label("__rt_digest_lo_af_linux_x86_64");
    emitter.instruction("add al, 87");                                          // map 10-15 to 'a'-'f'
    emitter.label("__rt_digest_lo_store_linux_x86_64");
    emitter.instruction("mov BYTE PTR [r11], al");                              // write the low hex character
    emitter.instruction("add r11, 1");                                          // advance the destination past the low hex digit
    emitter.instruction("jmp __rt_digest_hex_loop_linux_x86_64");               // process the next digest byte

    // -- raw-bytes loop: copy each digest byte verbatim --
    emitter.label("__rt_digest_raw_loop_linux_x86_64");
    emitter.instruction("test r9, r9");                                         // any remaining raw digest bytes to copy?
    emitter.instruction("jz __rt_digest_done_linux_x86_64");                    // all raw digest bytes copied
    emitter.instruction("movzx eax, BYTE PTR [rcx]");                           // load one raw digest byte
    emitter.instruction("add rcx, 1");                                          // advance the source cursor past the consumed byte
    emitter.instruction("sub r9, 1");                                           // decrement the remaining-byte counter
    emitter.instruction("mov BYTE PTR [r11], al");                              // write the raw digest byte verbatim
    emitter.instruction("add r11, 1");                                          // advance the destination past the raw byte
    emitter.instruction("jmp __rt_digest_raw_loop_linux_x86_64");               // process the next raw digest byte

    // -- publish the result string and advance the concat offset --
    emitter.label("__rt_digest_done_linux_x86_64");
    emitter.instruction("mov rax, r8");                                         // result pointer = formatted-digest start
    emitter.instruction("mov rdx, r11");                                        // copy the final destination cursor before computing the length
    emitter.instruction("sub rdx, r8");                                         // result length = bytes written
    emitter.instruction("mov rcx, QWORD PTR [rip + _concat_off]");              // reload the concat-buffer write offset
    emitter.instruction("add rcx, rdx");                                        // advance it past the formatted digest
    emitter.instruction("mov QWORD PTR [rip + _concat_off], rcx");              // persist the updated concat-buffer write offset
    emitter.instruction("ret");                                                 // return the PHP string ptr/len in rax/rdx
}
