use crate::codegen::emit::Emitter;

/// __rt_json_decode_mixed_object_real (ARM64): recursive-descent parser for
/// non-empty JSON objects. Walks the slice between the leading `{` and
/// trailing `}`, parses each key (a JSON string) and value (any JSON
/// value, recursively decoded), and inserts the pair into a hash via
/// __rt_hash_set. Result boxes as Mixed(tag=5, lo=hash_ptr).
///
/// Input:  x1 = slice ptr (with leading `{` and trailing `}`),
///         x2 = slice length
/// Output: x0 = Mixed* on success, 0 on parse error
pub(super) fn emit_aarch64(emitter: &mut Emitter) {
    emitter.blank();
    emitter.comment("--- runtime: json_decode_mixed_object_real ---");
    emitter.label_global("__rt_json_decode_mixed_object_real");

    // Frame layout (80 bytes):
    //   [sp + 0]  = slice_ptr
    //   [sp + 8]  = slice_len
    //   [sp + 16] = cursor
    //   [sp + 24] = hash_ptr
    //   [sp + 32] = key_start (saved across the recursive key decode)
    //   [sp + 40] = key Mixed* (saved across the recursive value decode)
    //   [sp + 48] = value_start
    //   [sp + 56] = (reserved)
    //   [sp + 64] = saved x29
    //   [sp + 72] = saved x30
    emitter.instruction("sub sp, sp, #80");
    emitter.instruction("stp x29, x30, [sp, #64]");
    emitter.instruction("add x29, sp, #64");
    emitter.instruction("str x1, [sp, #0]");
    emitter.instruction("str x2, [sp, #8]");

    // Allocate the destination hash with capacity 4 and value_type 7
    // (boxed mixed slots — every value is a Mixed pointer).
    emitter.instruction("mov x0, #4");                                          // initial capacity
    emitter.instruction("mov x1, #7");                                          // value_type = 7 (boxed mixed)
    emitter.instruction("bl __rt_hash_new");
    emitter.instruction("str x0, [sp, #24]");                                   // park hash ptr

    emitter.instruction("mov x9, #1");                                          // cursor = 1 (skip leading `{`)
    emitter.instruction("str x9, [sp, #16]");

    emitter.label("__rt_json_decode_object_real_loop");

    // Skip whitespace before the key.
    emitter.instruction("ldr x1, [sp, #0]");
    emitter.instruction("ldr x2, [sp, #8]");
    emitter.instruction("ldr x9, [sp, #16]");
    emitter.label("__rt_json_decode_object_real_skip_key_ws");
    emitter.instruction("sub x10, x2, #1");
    emitter.instruction("cmp x9, x10");
    emitter.instruction("b.ge __rt_json_decode_object_real_close");
    emitter.instruction("ldrb w11, [x1, x9]");
    emitter.instruction("cmp w11, #32");
    emitter.instruction("b.eq __rt_json_decode_object_real_skip_key_ws_step");
    emitter.instruction("cmp w11, #9");
    emitter.instruction("b.eq __rt_json_decode_object_real_skip_key_ws_step");
    emitter.instruction("cmp w11, #10");
    emitter.instruction("b.eq __rt_json_decode_object_real_skip_key_ws_step");
    emitter.instruction("cmp w11, #13");
    emitter.instruction("b.ne __rt_json_decode_object_real_skip_key_ws_done");
    emitter.label("__rt_json_decode_object_real_skip_key_ws_step");
    emitter.instruction("add x9, x9, #1");
    emitter.instruction("b __rt_json_decode_object_real_skip_key_ws");
    emitter.label("__rt_json_decode_object_real_skip_key_ws_done");
    emitter.instruction("str x9, [sp, #16]");

    // After whitespace skip: if `}` we're done.
    emitter.instruction("ldrb w11, [x1, x9]");
    emitter.instruction("cmp w11, #125");                                       // '}'
    emitter.instruction("b.eq __rt_json_decode_object_real_close");

    // Key MUST be a JSON string starting with `"`.
    emitter.instruction("cmp w11, #34");                                        // '"'
    emitter.instruction("b.ne __rt_json_decode_object_real_fail");

    // Save key_start, then scan to the closing `"` (with backslash-escape
    // awareness so `\\\"` doesn't end the key prematurely).
    emitter.instruction("str x9, [sp, #32]");                                   // key_start (points at opening `"`)
    emitter.instruction("add x9, x9, #1");                                      // step past the opening `"`
    emitter.instruction("ldr x10, [sp, #8]");                                   // slice_len
    emitter.instruction("mov x12, #0");                                         // escape flag
    emitter.label("__rt_json_decode_object_real_key_scan");
    emitter.instruction("cmp x9, x10");
    emitter.instruction("b.ge __rt_json_decode_object_real_fail");
    emitter.instruction("ldrb w13, [x1, x9]");
    emitter.instruction("cbnz x12, __rt_json_decode_object_real_key_after_escape");
    emitter.instruction("cmp w13, #92");                                        // '\\'
    emitter.instruction("b.eq __rt_json_decode_object_real_key_set_escape");
    emitter.instruction("cmp w13, #34");                                        // '"' → key end
    emitter.instruction("b.eq __rt_json_decode_object_real_key_done");
    emitter.instruction("b __rt_json_decode_object_real_key_advance");
    emitter.label("__rt_json_decode_object_real_key_set_escape");
    emitter.instruction("mov x12, #1");
    emitter.instruction("b __rt_json_decode_object_real_key_advance");
    emitter.label("__rt_json_decode_object_real_key_after_escape");
    emitter.instruction("mov x12, #0");
    emitter.label("__rt_json_decode_object_real_key_advance");
    emitter.instruction("add x9, x9, #1");
    emitter.instruction("b __rt_json_decode_object_real_key_scan");
    emitter.label("__rt_json_decode_object_real_key_done");
    emitter.instruction("add x9, x9, #1");                                      // include the closing `"` in the sub-slice
    emitter.instruction("str x9, [sp, #16]");                                   // cursor at byte after closing `"`

    // Recursively decode the key sub-slice — must produce Mixed(str).
    emitter.instruction("ldr x11, [sp, #0]");                                   // slice_ptr
    emitter.instruction("ldr x10, [sp, #32]");                                  // key_start
    emitter.instruction("add x1, x11, x10");                                    // sub_ptr
    emitter.instruction("sub x2, x9, x10");                                     // sub_len
    emitter.instruction("bl __rt_json_decode_mixed");
    emitter.instruction("cbz x0, __rt_json_decode_object_real_fail");
    emitter.instruction("str x0, [sp, #40]");                                   // park key Mixed*

    // Skip whitespace, expect `:`, skip whitespace.
    emitter.instruction("ldr x9, [sp, #16]");
    emitter.instruction("ldr x1, [sp, #0]");
    emitter.instruction("ldr x2, [sp, #8]");
    emitter.label("__rt_json_decode_object_real_skip_colon_ws");
    emitter.instruction("cmp x9, x2");
    emitter.instruction("b.ge __rt_json_decode_object_real_fail");
    emitter.instruction("ldrb w11, [x1, x9]");
    emitter.instruction("cmp w11, #32");
    emitter.instruction("b.eq __rt_json_decode_object_real_skip_colon_step");
    emitter.instruction("cmp w11, #9");
    emitter.instruction("b.eq __rt_json_decode_object_real_skip_colon_step");
    emitter.instruction("cmp w11, #10");
    emitter.instruction("b.eq __rt_json_decode_object_real_skip_colon_step");
    emitter.instruction("cmp w11, #13");
    emitter.instruction("b.ne __rt_json_decode_object_real_at_colon");
    emitter.label("__rt_json_decode_object_real_skip_colon_step");
    emitter.instruction("add x9, x9, #1");
    emitter.instruction("b __rt_json_decode_object_real_skip_colon_ws");
    emitter.label("__rt_json_decode_object_real_at_colon");
    emitter.instruction("cmp w11, #58");                                        // ':'
    emitter.instruction("b.ne __rt_json_decode_object_real_fail");
    emitter.instruction("add x9, x9, #1");                                      // consume the colon
    emitter.instruction("str x9, [sp, #16]");

    // Skip whitespace before the value.
    emitter.label("__rt_json_decode_object_real_skip_value_ws");
    emitter.instruction("cmp x9, x2");
    emitter.instruction("b.ge __rt_json_decode_object_real_fail");
    emitter.instruction("ldrb w11, [x1, x9]");
    emitter.instruction("cmp w11, #32");
    emitter.instruction("b.eq __rt_json_decode_object_real_skip_value_step");
    emitter.instruction("cmp w11, #9");
    emitter.instruction("b.eq __rt_json_decode_object_real_skip_value_step");
    emitter.instruction("cmp w11, #10");
    emitter.instruction("b.eq __rt_json_decode_object_real_skip_value_step");
    emitter.instruction("cmp w11, #13");
    emitter.instruction("b.ne __rt_json_decode_object_real_value_start");
    emitter.label("__rt_json_decode_object_real_skip_value_step");
    emitter.instruction("add x9, x9, #1");
    emitter.instruction("b __rt_json_decode_object_real_skip_value_ws");
    emitter.label("__rt_json_decode_object_real_value_start");
    emitter.instruction("str x9, [sp, #16]");
    emitter.instruction("str x9, [sp, #48]");                                   // value_start

    // Boundary scanner for the value: advance to ',' or '}' at depth 0.
    emitter.instruction("ldr x10, [sp, #8]");                                   // slice_len
    emitter.instruction("mov x12, #0");                                         // depth
    emitter.instruction("mov x13, #0");                                         // in_string
    emitter.instruction("mov x14, #0");                                         // escape
    emitter.label("__rt_json_decode_object_real_value_scan");
    emitter.instruction("cmp x9, x10");
    emitter.instruction("b.ge __rt_json_decode_object_real_value_done");
    emitter.instruction("ldrb w15, [x1, x9]");
    emitter.instruction("cbnz x14, __rt_json_decode_object_real_value_after_escape");
    emitter.instruction("cbnz x13, __rt_json_decode_object_real_value_in_string");
    emitter.instruction("cmp w15, #34");
    emitter.instruction("b.eq __rt_json_decode_object_real_value_enter_string");
    emitter.instruction("cmp w15, #91");
    emitter.instruction("b.eq __rt_json_decode_object_real_value_open");
    emitter.instruction("cmp w15, #123");
    emitter.instruction("b.eq __rt_json_decode_object_real_value_open");
    emitter.instruction("cmp w15, #93");
    emitter.instruction("b.eq __rt_json_decode_object_real_value_close_inner");
    emitter.instruction("cmp w15, #125");
    emitter.instruction("b.eq __rt_json_decode_object_real_value_close_inner");
    emitter.instruction("cmp w15, #44");
    emitter.instruction("b.ne __rt_json_decode_object_real_value_advance");
    emitter.instruction("cbz x12, __rt_json_decode_object_real_value_done");
    emitter.instruction("b __rt_json_decode_object_real_value_advance");
    emitter.label("__rt_json_decode_object_real_value_open");
    emitter.instruction("add x12, x12, #1");
    emitter.instruction("b __rt_json_decode_object_real_value_advance");
    emitter.label("__rt_json_decode_object_real_value_close_inner");
    emitter.instruction("cbz x12, __rt_json_decode_object_real_value_done");
    emitter.instruction("sub x12, x12, #1");
    emitter.instruction("b __rt_json_decode_object_real_value_advance");
    emitter.label("__rt_json_decode_object_real_value_enter_string");
    emitter.instruction("mov x13, #1");
    emitter.instruction("b __rt_json_decode_object_real_value_advance");
    emitter.label("__rt_json_decode_object_real_value_in_string");
    emitter.instruction("cmp w15, #92");
    emitter.instruction("b.eq __rt_json_decode_object_real_value_set_escape");
    emitter.instruction("cmp w15, #34");
    emitter.instruction("b.ne __rt_json_decode_object_real_value_advance");
    emitter.instruction("mov x13, #0");
    emitter.instruction("b __rt_json_decode_object_real_value_advance");
    emitter.label("__rt_json_decode_object_real_value_set_escape");
    emitter.instruction("mov x14, #1");
    emitter.instruction("b __rt_json_decode_object_real_value_advance");
    emitter.label("__rt_json_decode_object_real_value_after_escape");
    emitter.instruction("mov x14, #0");
    emitter.label("__rt_json_decode_object_real_value_advance");
    emitter.instruction("add x9, x9, #1");
    emitter.instruction("b __rt_json_decode_object_real_value_scan");
    emitter.label("__rt_json_decode_object_real_value_done");
    emitter.instruction("str x9, [sp, #16]");                                   // cursor at separator

    // Recursively decode the value sub-slice.
    emitter.instruction("ldr x11, [sp, #0]");                                   // slice_ptr
    emitter.instruction("ldr x10, [sp, #48]");                                  // value_start
    emitter.instruction("add x1, x11, x10");
    emitter.instruction("sub x2, x9, x10");
    emitter.instruction("bl __rt_json_decode_mixed");
    emitter.instruction("cbz x0, __rt_json_decode_object_real_fail");

    // Insert (key, value) into the hash.
    //   __rt_hash_set: x0=hash, x1=key_lo, x2=key_hi, x3=value_lo,
    //                  x4=value_hi, x5=value_tag → returns x0=updated hash
    emitter.instruction("mov x9, x0");                                          // park value Mixed* in a non-arg reg
    emitter.instruction("ldr x10, [sp, #40]");                                  // key Mixed*
    emitter.instruction("ldr x1, [x10, #8]");                                   // key_lo = ptr (offset 8 in Mixed cell)
    emitter.instruction("ldr x2, [x10, #16]");                                  // key_hi = len (offset 16)
    emitter.instruction("ldr x0, [sp, #24]");                                   // hash ptr
    emitter.instruction("mov x3, x9");                                          // value_lo = Mixed*
    emitter.instruction("mov x4, #0");                                          // value_hi
    emitter.instruction("mov x5, #7");                                          // value_tag = boxed mixed
    emitter.instruction("bl __rt_hash_set");
    emitter.instruction("str x0, [sp, #24]");                                   // updated hash ptr

    // Look at the separator.
    emitter.instruction("ldr x1, [sp, #0]");
    emitter.instruction("ldr x9, [sp, #16]");
    emitter.instruction("ldr x10, [sp, #8]");
    emitter.instruction("cmp x9, x10");
    emitter.instruction("b.ge __rt_json_decode_object_real_fail");
    emitter.instruction("ldrb w11, [x1, x9]");
    emitter.instruction("cmp w11, #44");                                        // ','
    emitter.instruction("b.eq __rt_json_decode_object_real_after_comma");
    emitter.instruction("cmp w11, #125");                                       // '}'
    emitter.instruction("b.eq __rt_json_decode_object_real_close");
    emitter.instruction("b __rt_json_decode_object_real_fail");

    emitter.label("__rt_json_decode_object_real_after_comma");
    emitter.instruction("add x9, x9, #1");
    emitter.instruction("str x9, [sp, #16]");
    emitter.instruction("b __rt_json_decode_object_real_loop");

    emitter.label("__rt_json_decode_object_real_close");
    emitter.instruction("ldr x1, [sp, #24]");                                   // hash ptr
    // PHP json_decode default returns stdClass; assoc=true returns hash.
    // Read the runtime flag set by the json_decode codegen to decide which.
    crate::codegen::abi::emit_symbol_address(emitter, "x9", "_json_decode_assoc");
    emitter.instruction("ldr x9, [x9]");                                        // load the assoc flag (0 → stdClass, non-zero → assoc array)
    emitter.instruction("cbz x9, __rt_json_decode_object_real_close_stdclass"); // 0 means PHP's default → wrap hash in a stdClass

    emitter.instruction("mov x0, #5");                                          // tag = associative array
    emitter.instruction("mov x2, #0");                                          // mixed_from_value high word unused for assoc payload
    emitter.instruction("bl __rt_mixed_from_value");                            // box the hash as Mixed(assoc)
    emitter.instruction("ldp x29, x30, [sp, #64]");                             // restore frame pointer and return address
    emitter.instruction("add sp, sp, #80");                                     // release the local frame before returning
    emitter.instruction("ret");                                                 // return Mixed* (assoc array) in x0

    emitter.label("__rt_json_decode_object_real_close_stdclass");
    emitter.instruction("mov x0, x1");                                          // x0 = hash pointer for stdclass_from_hash
    emitter.instruction("bl __rt_stdclass_from_hash");                          // x0 = freshly allocated stdClass adopting the decoded hash
    emitter.instruction("mov x1, x0");                                          // shift the stdClass pointer into the mixed_from_value low-word slot
    emitter.instruction("mov x0, #6");                                          // tag = object
    emitter.instruction("mov x2, #0");                                          // mixed_from_value high word unused for object payload
    emitter.instruction("bl __rt_mixed_from_value");                            // box the stdClass as Mixed(object)
    emitter.instruction("ldp x29, x30, [sp, #64]");                             // restore frame pointer and return address
    emitter.instruction("add sp, sp, #80");                                     // release the local frame before returning
    emitter.instruction("ret");                                                 // return Mixed* (stdClass) in x0

    emitter.label("__rt_json_decode_object_real_fail");
    emitter.instruction("mov x0, #0");
    emitter.instruction("ldp x29, x30, [sp, #64]");
    emitter.instruction("add sp, sp, #80");
    emitter.instruction("ret");
}

/// __rt_json_decode_mixed_object_real (x86_64): mirrors the ARM64 recursive
/// object parser. See the ARM64 docstring for the parser's semantics.
pub(super) fn emit_x86_64(emitter: &mut Emitter) {
    emitter.blank();
    emitter.comment("--- runtime: json_decode_mixed_object_real ---");
    emitter.label_global("__rt_json_decode_mixed_object_real");

    // Frame layout (rbp-relative, 64 bytes reserved):
    //   [rbp - 8]  = slice_ptr
    //   [rbp - 16] = slice_len
    //   [rbp - 24] = cursor
    //   [rbp - 32] = hash_ptr
    //   [rbp - 40] = key_start
    //   [rbp - 48] = key Mixed*
    //   [rbp - 56] = value_start
    emitter.instruction("push rbp");
    emitter.instruction("mov rbp, rsp");
    emitter.instruction("sub rsp, 64");
    emitter.instruction("mov QWORD PTR [rbp - 8], rax");
    emitter.instruction("mov QWORD PTR [rbp - 16], rdx");

    emitter.instruction("mov rdi, 4");                                          // initial capacity
    emitter.instruction("mov rsi, 7");                                          // value_type = boxed mixed
    emitter.instruction("call __rt_hash_new");
    emitter.instruction("mov QWORD PTR [rbp - 32], rax");

    emitter.instruction("mov QWORD PTR [rbp - 24], 1");                         // cursor past `{`

    emitter.label("__rt_json_decode_object_real_loop_x");

    // Skip whitespace before key.
    emitter.instruction("mov rax, QWORD PTR [rbp - 8]");
    emitter.instruction("mov rdx, QWORD PTR [rbp - 16]");
    emitter.instruction("mov rcx, QWORD PTR [rbp - 24]");
    emitter.label("__rt_json_decode_object_real_skip_key_ws_x");
    emitter.instruction("mov r9, rdx");
    emitter.instruction("sub r9, 1");
    emitter.instruction("cmp rcx, r9");
    emitter.instruction("jge __rt_json_decode_object_real_close_x");
    emitter.instruction("movzx r8, BYTE PTR [rax + rcx]");
    emitter.instruction("cmp r8, 32");
    emitter.instruction("je __rt_json_decode_object_real_skip_key_ws_step_x");
    emitter.instruction("cmp r8, 9");
    emitter.instruction("je __rt_json_decode_object_real_skip_key_ws_step_x");
    emitter.instruction("cmp r8, 10");
    emitter.instruction("je __rt_json_decode_object_real_skip_key_ws_step_x");
    emitter.instruction("cmp r8, 13");
    emitter.instruction("jne __rt_json_decode_object_real_skip_key_ws_done_x");
    emitter.label("__rt_json_decode_object_real_skip_key_ws_step_x");
    emitter.instruction("add rcx, 1");
    emitter.instruction("jmp __rt_json_decode_object_real_skip_key_ws_x");
    emitter.label("__rt_json_decode_object_real_skip_key_ws_done_x");
    emitter.instruction("mov QWORD PTR [rbp - 24], rcx");

    emitter.instruction("movzx r8, BYTE PTR [rax + rcx]");
    emitter.instruction("cmp r8, 125");                                         // '}'
    emitter.instruction("je __rt_json_decode_object_real_close_x");
    emitter.instruction("cmp r8, 34");                                          // '"' — key must be JSON string
    emitter.instruction("jne __rt_json_decode_object_real_fail_x");

    // Save key_start, scan to closing `"`.
    emitter.instruction("mov QWORD PTR [rbp - 40], rcx");
    emitter.instruction("add rcx, 1");                                          // step past opening `"`
    emitter.instruction("push r12");                                            // preserve callee-saved
    emitter.instruction("xor r12, r12");                                        // escape flag
    emitter.label("__rt_json_decode_object_real_key_scan_x");
    emitter.instruction("cmp rcx, rdx");
    emitter.instruction("jge __rt_json_decode_object_real_key_fail_x");
    emitter.instruction("movzx r10, BYTE PTR [rax + rcx]");
    emitter.instruction("test r12, r12");
    emitter.instruction("jne __rt_json_decode_object_real_key_after_escape_x");
    emitter.instruction("cmp r10, 92");                                         // '\\'
    emitter.instruction("je __rt_json_decode_object_real_key_set_escape_x");
    emitter.instruction("cmp r10, 34");                                         // '"'
    emitter.instruction("je __rt_json_decode_object_real_key_done_x");
    emitter.instruction("jmp __rt_json_decode_object_real_key_advance_x");
    emitter.label("__rt_json_decode_object_real_key_set_escape_x");
    emitter.instruction("mov r12, 1");
    emitter.instruction("jmp __rt_json_decode_object_real_key_advance_x");
    emitter.label("__rt_json_decode_object_real_key_after_escape_x");
    emitter.instruction("xor r12, r12");
    emitter.label("__rt_json_decode_object_real_key_advance_x");
    emitter.instruction("add rcx, 1");
    emitter.instruction("jmp __rt_json_decode_object_real_key_scan_x");
    emitter.label("__rt_json_decode_object_real_key_done_x");
    emitter.instruction("pop r12");
    emitter.instruction("add rcx, 1");                                          // include the closing `"`
    emitter.instruction("mov QWORD PTR [rbp - 24], rcx");

    // Recursively decode the key sub-slice.
    emitter.instruction("mov rax, QWORD PTR [rbp - 8]");
    emitter.instruction("mov r10, QWORD PTR [rbp - 40]");
    emitter.instruction("add rax, r10");
    emitter.instruction("mov rdx, rcx");
    emitter.instruction("sub rdx, r10");
    emitter.instruction("call __rt_json_decode_mixed");
    emitter.instruction("test rax, rax");
    emitter.instruction("je __rt_json_decode_object_real_fail_x");
    emitter.instruction("mov QWORD PTR [rbp - 48], rax");                       // park key Mixed*

    // Skip whitespace, expect `:`, skip whitespace.
    emitter.instruction("mov rax, QWORD PTR [rbp - 8]");
    emitter.instruction("mov rdx, QWORD PTR [rbp - 16]");
    emitter.instruction("mov rcx, QWORD PTR [rbp - 24]");
    emitter.label("__rt_json_decode_object_real_skip_colon_ws_x");
    emitter.instruction("cmp rcx, rdx");
    emitter.instruction("jge __rt_json_decode_object_real_fail_x");
    emitter.instruction("movzx r8, BYTE PTR [rax + rcx]");
    emitter.instruction("cmp r8, 32");
    emitter.instruction("je __rt_json_decode_object_real_skip_colon_step_x");
    emitter.instruction("cmp r8, 9");
    emitter.instruction("je __rt_json_decode_object_real_skip_colon_step_x");
    emitter.instruction("cmp r8, 10");
    emitter.instruction("je __rt_json_decode_object_real_skip_colon_step_x");
    emitter.instruction("cmp r8, 13");
    emitter.instruction("jne __rt_json_decode_object_real_at_colon_x");
    emitter.label("__rt_json_decode_object_real_skip_colon_step_x");
    emitter.instruction("add rcx, 1");
    emitter.instruction("jmp __rt_json_decode_object_real_skip_colon_ws_x");
    emitter.label("__rt_json_decode_object_real_at_colon_x");
    emitter.instruction("cmp r8, 58");                                          // ':'
    emitter.instruction("jne __rt_json_decode_object_real_fail_x");
    emitter.instruction("add rcx, 1");
    emitter.instruction("mov QWORD PTR [rbp - 24], rcx");

    // Skip whitespace before value.
    emitter.label("__rt_json_decode_object_real_skip_value_ws_x");
    emitter.instruction("cmp rcx, rdx");
    emitter.instruction("jge __rt_json_decode_object_real_fail_x");
    emitter.instruction("movzx r8, BYTE PTR [rax + rcx]");
    emitter.instruction("cmp r8, 32");
    emitter.instruction("je __rt_json_decode_object_real_skip_value_step_x");
    emitter.instruction("cmp r8, 9");
    emitter.instruction("je __rt_json_decode_object_real_skip_value_step_x");
    emitter.instruction("cmp r8, 10");
    emitter.instruction("je __rt_json_decode_object_real_skip_value_step_x");
    emitter.instruction("cmp r8, 13");
    emitter.instruction("jne __rt_json_decode_object_real_value_start_x");
    emitter.label("__rt_json_decode_object_real_skip_value_step_x");
    emitter.instruction("add rcx, 1");
    emitter.instruction("jmp __rt_json_decode_object_real_skip_value_ws_x");
    emitter.label("__rt_json_decode_object_real_value_start_x");
    emitter.instruction("mov QWORD PTR [rbp - 24], rcx");
    emitter.instruction("mov QWORD PTR [rbp - 56], rcx");

    // Boundary scanner for the value.
    emitter.instruction("push r12");
    emitter.instruction("xor r10, r10");                                        // depth
    emitter.instruction("xor r11, r11");                                        // in_string
    emitter.instruction("xor r12, r12");                                        // escape
    emitter.label("__rt_json_decode_object_real_value_scan_x");
    emitter.instruction("cmp rcx, rdx");
    emitter.instruction("jge __rt_json_decode_object_real_value_done_x");
    emitter.instruction("movzx r8, BYTE PTR [rax + rcx]");
    emitter.instruction("test r12, r12");
    emitter.instruction("jne __rt_json_decode_object_real_value_after_escape_x");
    emitter.instruction("test r11, r11");
    emitter.instruction("jne __rt_json_decode_object_real_value_in_string_x");
    emitter.instruction("cmp r8, 34");
    emitter.instruction("je __rt_json_decode_object_real_value_enter_string_x");
    emitter.instruction("cmp r8, 91");
    emitter.instruction("je __rt_json_decode_object_real_value_open_x");
    emitter.instruction("cmp r8, 123");
    emitter.instruction("je __rt_json_decode_object_real_value_open_x");
    emitter.instruction("cmp r8, 93");
    emitter.instruction("je __rt_json_decode_object_real_value_close_inner_x");
    emitter.instruction("cmp r8, 125");
    emitter.instruction("je __rt_json_decode_object_real_value_close_inner_x");
    emitter.instruction("cmp r8, 44");
    emitter.instruction("jne __rt_json_decode_object_real_value_advance_x");
    emitter.instruction("test r10, r10");
    emitter.instruction("je __rt_json_decode_object_real_value_done_x");
    emitter.instruction("jmp __rt_json_decode_object_real_value_advance_x");
    emitter.label("__rt_json_decode_object_real_value_open_x");
    emitter.instruction("add r10, 1");
    emitter.instruction("jmp __rt_json_decode_object_real_value_advance_x");
    emitter.label("__rt_json_decode_object_real_value_close_inner_x");
    emitter.instruction("test r10, r10");
    emitter.instruction("je __rt_json_decode_object_real_value_done_x");
    emitter.instruction("sub r10, 1");
    emitter.instruction("jmp __rt_json_decode_object_real_value_advance_x");
    emitter.label("__rt_json_decode_object_real_value_enter_string_x");
    emitter.instruction("mov r11, 1");
    emitter.instruction("jmp __rt_json_decode_object_real_value_advance_x");
    emitter.label("__rt_json_decode_object_real_value_in_string_x");
    emitter.instruction("cmp r8, 92");
    emitter.instruction("je __rt_json_decode_object_real_value_set_escape_x");
    emitter.instruction("cmp r8, 34");
    emitter.instruction("jne __rt_json_decode_object_real_value_advance_x");
    emitter.instruction("xor r11, r11");
    emitter.instruction("jmp __rt_json_decode_object_real_value_advance_x");
    emitter.label("__rt_json_decode_object_real_value_set_escape_x");
    emitter.instruction("mov r12, 1");
    emitter.instruction("jmp __rt_json_decode_object_real_value_advance_x");
    emitter.label("__rt_json_decode_object_real_value_after_escape_x");
    emitter.instruction("xor r12, r12");
    emitter.label("__rt_json_decode_object_real_value_advance_x");
    emitter.instruction("add rcx, 1");
    emitter.instruction("jmp __rt_json_decode_object_real_value_scan_x");
    emitter.label("__rt_json_decode_object_real_value_done_x");
    emitter.instruction("pop r12");
    emitter.instruction("mov QWORD PTR [rbp - 24], rcx");

    // Recursively decode value sub-slice.
    emitter.instruction("mov rax, QWORD PTR [rbp - 8]");
    emitter.instruction("mov r10, QWORD PTR [rbp - 56]");
    emitter.instruction("add rax, r10");
    emitter.instruction("mov rdx, rcx");
    emitter.instruction("sub rdx, r10");
    emitter.instruction("call __rt_json_decode_mixed");
    emitter.instruction("test rax, rax");
    emitter.instruction("je __rt_json_decode_object_real_fail_x");

    // hash_set on x86_64: rdi=hash, rsi=key_lo, rdx=key_hi, rcx=value_lo,
    // r8=value_hi, r9=value_tag → returns rax=updated hash.
    emitter.instruction("mov rcx, rax");                                        // value_lo = value Mixed*
    emitter.instruction("mov r10, QWORD PTR [rbp - 48]");                       // key Mixed*
    emitter.instruction("mov rsi, QWORD PTR [r10 + 8]");                        // key_lo = key ptr
    emitter.instruction("mov rdx, QWORD PTR [r10 + 16]");                       // key_hi = key len
    emitter.instruction("mov rdi, QWORD PTR [rbp - 32]");                       // hash ptr
    emitter.instruction("xor r8, r8");                                          // value_hi
    emitter.instruction("mov r9, 7");                                           // value_tag = boxed mixed
    emitter.instruction("call __rt_hash_set");
    emitter.instruction("mov QWORD PTR [rbp - 32], rax");

    // Look at the separator.
    emitter.instruction("mov rax, QWORD PTR [rbp - 8]");
    emitter.instruction("mov rcx, QWORD PTR [rbp - 24]");
    emitter.instruction("mov rdx, QWORD PTR [rbp - 16]");
    emitter.instruction("cmp rcx, rdx");
    emitter.instruction("jge __rt_json_decode_object_real_fail_x");
    emitter.instruction("movzx r8, BYTE PTR [rax + rcx]");
    emitter.instruction("cmp r8, 44");                                          // ','
    emitter.instruction("je __rt_json_decode_object_real_after_comma_x");
    emitter.instruction("cmp r8, 125");                                         // '}'
    emitter.instruction("je __rt_json_decode_object_real_close_x");
    emitter.instruction("jmp __rt_json_decode_object_real_fail_x");

    emitter.label("__rt_json_decode_object_real_after_comma_x");
    emitter.instruction("add rcx, 1");
    emitter.instruction("mov QWORD PTR [rbp - 24], rcx");
    emitter.instruction("jmp __rt_json_decode_object_real_loop_x");

    emitter.label("__rt_json_decode_object_real_close_x");
    emitter.instruction("mov rdi, QWORD PTR [rbp - 32]");                       // rdi = hash pointer
    // PHP json_decode default returns stdClass; assoc=true returns hash.
    // Read the runtime flag set by the json_decode codegen to decide which.
    emitter.instruction("mov r10, QWORD PTR [rip + _json_decode_assoc]");       // load the assoc flag (0 → stdClass, non-zero → assoc array)
    emitter.instruction("test r10, r10");                                       // zero means PHP's default
    emitter.instruction("je __rt_json_decode_object_real_close_stdclass_x");    // dispatch to stdClass wrapping

    emitter.instruction("mov rax, 5");                                          // tag = associative array
    emitter.instruction("xor rsi, rsi");                                        // mixed_from_value high word unused for assoc payload
    emitter.instruction("call __rt_mixed_from_value");                          // box the hash as Mixed(assoc)
    emitter.instruction("mov rsp, rbp");                                        // restore stack pointer
    emitter.instruction("pop rbp");                                             // restore caller frame pointer
    emitter.instruction("ret");                                                 // return Mixed* (assoc array) in rax

    emitter.label("__rt_json_decode_object_real_close_stdclass_x");
    // rdi already holds the hash pointer (SysV first arg) for stdclass_from_hash.
    emitter.instruction("call __rt_stdclass_from_hash");                        // rax = freshly allocated stdClass adopting the decoded hash
    emitter.instruction("mov rdi, rax");                                        // shift the stdClass pointer into the mixed_from_value low-word slot
    emitter.instruction("mov rax, 6");                                          // tag = object
    emitter.instruction("xor rsi, rsi");                                        // mixed_from_value high word unused for object payload
    emitter.instruction("call __rt_mixed_from_value");                          // box the stdClass as Mixed(object)
    emitter.instruction("mov rsp, rbp");                                        // restore stack pointer
    emitter.instruction("pop rbp");                                             // restore caller frame pointer
    emitter.instruction("ret");                                                 // return Mixed* (stdClass) in rax

    emitter.label("__rt_json_decode_object_real_key_fail_x");
    emitter.instruction("pop r12");
    emitter.label("__rt_json_decode_object_real_fail_x");
    emitter.instruction("xor rax, rax");
    emitter.instruction("mov rsp, rbp");
    emitter.instruction("pop rbp");
    emitter.instruction("ret");
}
