use crate::codegen::emit::Emitter;

pub(super) fn emit(emitter: &mut Emitter) {
    emitter.blank();
    emitter.comment("--- runtime: json_validate ---");
    emitter.label_global("__rt_json_validate");

    emitter.instruction("push rbp");                                            // preserve the caller frame pointer
    emitter.instruction("mov rbp, rsp");                                        // establish a stable frame base

    emitter.instruction("mov QWORD PTR [rip + _json_validate_ptr], rax");       // publish the source pointer
    emitter.instruction("mov QWORD PTR [rip + _json_validate_len], rdx");       // publish the source length
    emitter.instruction("mov QWORD PTR [rip + _json_validate_idx], 0");         // start at the beginning of the input
    emitter.instruction("mov QWORD PTR [rip + _json_active_depth], 0");         // begin at depth 0

    emitter.instruction("call __rt_json_validate_skip_ws_x");
    emitter.instruction("call __rt_json_validate_value_x");
    emitter.instruction("test rax, rax");
    emitter.instruction("je __rt_json_validate_fail_x");
    emitter.instruction("call __rt_json_validate_skip_ws_x");
    emitter.instruction("mov rcx, QWORD PTR [rip + _json_validate_idx]");
    emitter.instruction("cmp rcx, QWORD PTR [rip + _json_validate_len]");
    emitter.instruction("jl __rt_json_validate_syntax_error_x");

    emitter.instruction("mov QWORD PTR [rip + _json_last_error], 0");
    emitter.instruction("mov rax, 1");
    emitter.instruction("mov rsp, rbp");
    emitter.instruction("pop rbp");
    emitter.instruction("ret");

    emitter.label("__rt_json_validate_fail_x");
    emitter.instruction("mov rax, 0");
    emitter.instruction("mov rsp, rbp");
    emitter.instruction("pop rbp");
    emitter.instruction("ret");

    emitter.label("__rt_json_validate_syntax_error_x");
    emitter.instruction("mov rax, 4");
    emitter.instruction("call __rt_json_throw_error");
    emitter.instruction("mov rax, 0");
    emitter.instruction("mov rsp, rbp");
    emitter.instruction("pop rbp");
    emitter.instruction("ret");

    emit_skip_ws_x(emitter);
    emit_value_x(emitter);
    emit_match_literal_x(emitter, "true", &['t', 'r', 'u', 'e']);
    emit_match_literal_x(emitter, "null", &['n', 'u', 'l', 'l']);
    emit_match_literal_x(emitter, "false", &['f', 'a', 'l', 's', 'e']);
    emit_string_parser_x(emitter);
    emit_number_parser_x(emitter);
    emit_array_parser_x(emitter);
    emit_object_parser_x(emitter);
}

fn emit_skip_ws_x(emitter: &mut Emitter) {
    emitter.label("__rt_json_validate_skip_ws_x");
    emitter.instruction("push rbp");
    emitter.instruction("mov rbp, rsp");
    emitter.instruction("mov rcx, QWORD PTR [rip + _json_validate_idx]");
    emitter.instruction("mov rdx, QWORD PTR [rip + _json_validate_len]");
    emitter.instruction("mov rax, QWORD PTR [rip + _json_validate_ptr]");
    emitter.label("__rt_json_validate_skip_ws_loop_x");
    emitter.instruction("cmp rcx, rdx");
    emitter.instruction("jge __rt_json_validate_skip_ws_done_x");
    emitter.instruction("movzx r8, BYTE PTR [rax + rcx]");
    emitter.instruction("cmp r8, 32");
    emitter.instruction("je __rt_json_validate_skip_ws_step_x");
    emitter.instruction("cmp r8, 9");
    emitter.instruction("je __rt_json_validate_skip_ws_step_x");
    emitter.instruction("cmp r8, 10");
    emitter.instruction("je __rt_json_validate_skip_ws_step_x");
    emitter.instruction("cmp r8, 13");
    emitter.instruction("jne __rt_json_validate_skip_ws_done_x");
    emitter.label("__rt_json_validate_skip_ws_step_x");
    emitter.instruction("add rcx, 1");
    emitter.instruction("jmp __rt_json_validate_skip_ws_loop_x");
    emitter.label("__rt_json_validate_skip_ws_done_x");
    emitter.instruction("mov QWORD PTR [rip + _json_validate_idx], rcx");
    emitter.instruction("mov rsp, rbp");
    emitter.instruction("pop rbp");
    emitter.instruction("ret");
}

fn emit_value_x(emitter: &mut Emitter) {
    emitter.label("__rt_json_validate_value_x");
    emitter.instruction("push rbp");
    emitter.instruction("mov rbp, rsp");
    emitter.instruction("mov rcx, QWORD PTR [rip + _json_validate_idx]");
    emitter.instruction("mov rdx, QWORD PTR [rip + _json_validate_len]");
    emitter.instruction("cmp rcx, rdx");
    emitter.instruction("jge __rt_json_validate_value_syntax_x");
    emitter.instruction("mov rax, QWORD PTR [rip + _json_validate_ptr]");
    emitter.instruction("movzx r8, BYTE PTR [rax + rcx]");
    emitter.instruction("cmp r8, 34");                                          // string opener?
    emitter.instruction("je __rt_json_validate_value_string_x");
    emitter.instruction("cmp r8, 91");                                          // array opener?
    emitter.instruction("je __rt_json_validate_value_array_x");
    emitter.instruction("cmp r8, 123");                                         // object opener?
    emitter.instruction("je __rt_json_validate_value_object_x");
    emitter.instruction("cmp r8, 116");                                         // 't'?
    emitter.instruction("je __rt_json_validate_value_true_x");
    emitter.instruction("cmp r8, 102");                                         // 'f'?
    emitter.instruction("je __rt_json_validate_value_false_x");
    emitter.instruction("cmp r8, 110");                                         // 'n'?
    emitter.instruction("je __rt_json_validate_value_null_x");
    emitter.instruction("cmp r8, 45");                                          // negative number?
    emitter.instruction("je __rt_json_validate_value_number_x");
    emitter.instruction("cmp r8, 48");                                          // digit?
    emitter.instruction("jl __rt_json_validate_value_syntax_x");
    emitter.instruction("cmp r8, 57");
    emitter.instruction("jle __rt_json_validate_value_number_x");

    emitter.label("__rt_json_validate_value_syntax_x");
    emitter.instruction("mov rax, 4");
    emitter.instruction("call __rt_json_throw_error");
    emitter.instruction("mov rax, 0");
    emitter.instruction("mov rsp, rbp");
    emitter.instruction("pop rbp");
    emitter.instruction("ret");

    emitter.label("__rt_json_validate_value_string_x");
    emitter.instruction("call __rt_json_validate_string_x");
    emitter.instruction("mov rsp, rbp");
    emitter.instruction("pop rbp");
    emitter.instruction("ret");
    emitter.label("__rt_json_validate_value_number_x");
    emitter.instruction("call __rt_json_validate_number_x");
    emitter.instruction("mov rsp, rbp");
    emitter.instruction("pop rbp");
    emitter.instruction("ret");
    emitter.label("__rt_json_validate_value_array_x");
    emitter.instruction("call __rt_json_validate_array_x");
    emitter.instruction("mov rsp, rbp");
    emitter.instruction("pop rbp");
    emitter.instruction("ret");
    emitter.label("__rt_json_validate_value_object_x");
    emitter.instruction("call __rt_json_validate_object_x");
    emitter.instruction("mov rsp, rbp");
    emitter.instruction("pop rbp");
    emitter.instruction("ret");
    emitter.label("__rt_json_validate_value_true_x");
    emitter.instruction("call __rt_json_validate_match_true_x");
    emitter.instruction("mov rsp, rbp");
    emitter.instruction("pop rbp");
    emitter.instruction("ret");
    emitter.label("__rt_json_validate_value_false_x");
    emitter.instruction("call __rt_json_validate_match_false_x");
    emitter.instruction("mov rsp, rbp");
    emitter.instruction("pop rbp");
    emitter.instruction("ret");
    emitter.label("__rt_json_validate_value_null_x");
    emitter.instruction("call __rt_json_validate_match_null_x");
    emitter.instruction("mov rsp, rbp");
    emitter.instruction("pop rbp");
    emitter.instruction("ret");
}

fn emit_match_literal_x(emitter: &mut Emitter, suffix: &str, lit: &[char]) {
    let label = format!("__rt_json_validate_match_{}_x", suffix);
    let fail_label = format!("__rt_json_validate_match_{}_fail_x", suffix);
    emitter.label(&label);
    emitter.instruction("push rbp");
    emitter.instruction("mov rbp, rsp");
    emitter.instruction("mov rcx, QWORD PTR [rip + _json_validate_idx]");
    emitter.instruction("mov rdx, QWORD PTR [rip + _json_validate_len]");
    emitter.instruction(&format!("lea r8, [rcx + {}]", lit.len()));
    emitter.instruction("cmp r8, rdx");
    emitter.instruction(&format!("jg {}", fail_label));
    emitter.instruction("mov rax, QWORD PTR [rip + _json_validate_ptr]");
    for (offset, &c) in lit.iter().enumerate() {
        emitter.instruction(&format!("movzx r9, BYTE PTR [rax + rcx + {}]", offset));
        emitter.instruction(&format!("cmp r9, {}", c as u32));
        emitter.instruction(&format!("jne {}", fail_label));
    }
    emitter.instruction(&format!("add rcx, {}", lit.len()));
    emitter.instruction("mov QWORD PTR [rip + _json_validate_idx], rcx");
    emitter.instruction("mov rax, 1");
    emitter.instruction("mov rsp, rbp");
    emitter.instruction("pop rbp");
    emitter.instruction("ret");
    emitter.label(&fail_label);
    emitter.instruction("mov rax, 4");
    emitter.instruction("call __rt_json_throw_error");
    emitter.instruction("mov rax, 0");
    emitter.instruction("mov rsp, rbp");
    emitter.instruction("pop rbp");
    emitter.instruction("ret");
}

fn emit_string_parser_x(emitter: &mut Emitter) {
    emitter.label("__rt_json_validate_string_x");
    emitter.instruction("push rbp");
    emitter.instruction("mov rbp, rsp");
    emitter.instruction("mov rcx, QWORD PTR [rip + _json_validate_idx]");
    emitter.instruction("mov rdx, QWORD PTR [rip + _json_validate_len]");
    emitter.instruction("mov rax, QWORD PTR [rip + _json_validate_ptr]");
    emitter.instruction("cmp rcx, rdx");
    emitter.instruction("jge __rt_json_validate_string_syntax_x");
    emitter.instruction("movzx r8, BYTE PTR [rax + rcx]");
    emitter.instruction("cmp r8, 34");
    emitter.instruction("jne __rt_json_validate_string_syntax_x");
    emitter.instruction("add rcx, 1");                                          // consume opening quote

    emitter.label("__rt_json_validate_string_loop_x");
    emitter.instruction("cmp rcx, rdx");
    emitter.instruction("jge __rt_json_validate_string_syntax_x");
    emitter.instruction("movzx r8, BYTE PTR [rax + rcx]");
    emitter.instruction("cmp r8, 34");
    emitter.instruction("je __rt_json_validate_string_close_x");
    emitter.instruction("cmp r8, 92");
    emitter.instruction("je __rt_json_validate_string_escape_x");
    emitter.instruction("cmp r8, 32");
    emitter.instruction("jl __rt_json_validate_string_syntax_x");
    emitter.instruction("add rcx, 1");
    emitter.instruction("jmp __rt_json_validate_string_loop_x");

    emitter.label("__rt_json_validate_string_close_x");
    emitter.instruction("add rcx, 1");
    emitter.instruction("mov QWORD PTR [rip + _json_validate_idx], rcx");
    emitter.instruction("mov rax, 1");
    emitter.instruction("mov rsp, rbp");
    emitter.instruction("pop rbp");
    emitter.instruction("ret");

    emitter.label("__rt_json_validate_string_escape_x");
    emitter.instruction("add rcx, 1");
    emitter.instruction("cmp rcx, rdx");
    emitter.instruction("jge __rt_json_validate_string_syntax_x");
    emitter.instruction("movzx r8, BYTE PTR [rax + rcx]");
    emitter.instruction("cmp r8, 34");
    emitter.instruction("je __rt_json_validate_string_escape_simple_x");
    emitter.instruction("cmp r8, 92");
    emitter.instruction("je __rt_json_validate_string_escape_simple_x");
    emitter.instruction("cmp r8, 47");
    emitter.instruction("je __rt_json_validate_string_escape_simple_x");
    emitter.instruction("cmp r8, 98");
    emitter.instruction("je __rt_json_validate_string_escape_simple_x");
    emitter.instruction("cmp r8, 102");
    emitter.instruction("je __rt_json_validate_string_escape_simple_x");
    emitter.instruction("cmp r8, 110");
    emitter.instruction("je __rt_json_validate_string_escape_simple_x");
    emitter.instruction("cmp r8, 114");
    emitter.instruction("je __rt_json_validate_string_escape_simple_x");
    emitter.instruction("cmp r8, 116");
    emitter.instruction("je __rt_json_validate_string_escape_simple_x");
    emitter.instruction("cmp r8, 117");
    emitter.instruction("je __rt_json_validate_string_escape_unicode_x");
    emitter.instruction("jmp __rt_json_validate_string_syntax_x");

    emitter.label("__rt_json_validate_string_escape_simple_x");
    emitter.instruction("add rcx, 1");
    emitter.instruction("jmp __rt_json_validate_string_loop_x");

    emitter.label("__rt_json_validate_string_escape_unicode_x");
    emitter.instruction("add rcx, 1");                                          // consume the 'u'
    emitter.instruction("xor r10, r10");                                        // codepoint accumulator (16-bit)
    emitter.instruction("mov r9, 4");                                           // remaining hex-digit count
    emit_uhex_loop_x(emitter, "high", "__rt_json_validate_string_syntax_x");    // validate + accumulate 4 hex digits

    // -- surrogate-pair validation (mirrors ARM64) --
    emitter.instruction("cmp r10, 0xD800");                                     // codepoint < 0xD800?
    emitter.instruction("jl __rt_json_validate_string_loop_x");                 // not a surrogate → resume content scan
    emitter.instruction("cmp r10, 0xDFFF");                                     // codepoint > 0xDFFF?
    emitter.instruction("jg __rt_json_validate_string_loop_x");                 // not a surrogate → resume content scan
    emitter.instruction("cmp r10, 0xDC00");                                     // is the codepoint a low surrogate?
    emitter.instruction("jge __rt_json_validate_string_utf16_x");               // lone low surrogate → JSON_ERROR_UTF16

    // High surrogate: require an immediately following `\u`.
    emitter.instruction("cmp rcx, rdx");                                        // any byte left?
    emitter.instruction("jge __rt_json_validate_string_utf16_x");               // truncated → UTF16 error
    emitter.instruction("movzx r8, BYTE PTR [rax + rcx]");                      // peek the byte after the high surrogate
    emitter.instruction("cmp r8, 92");                                          // backslash?
    emitter.instruction("jne __rt_json_validate_string_utf16_x");               // anything else → UTF16 error
    emitter.instruction("add rcx, 1");                                          // consume the backslash
    emitter.instruction("cmp rcx, rdx");
    emitter.instruction("jge __rt_json_validate_string_utf16_x");
    emitter.instruction("movzx r8, BYTE PTR [rax + rcx]");
    emitter.instruction("cmp r8, 117");                                         // 'u'?
    emitter.instruction("jne __rt_json_validate_string_utf16_x");               // not a `\u` escape → UTF16 error
    emitter.instruction("add rcx, 1");                                          // consume the 'u'

    // Parse and accumulate the low surrogate's 4 hex digits.
    emitter.instruction("xor r10, r10");                                        // reset the accumulator for the second codepoint
    emitter.instruction("mov r9, 4");                                           // remaining hex-digit count
    emit_uhex_loop_x(emitter, "low", "__rt_json_validate_string_utf16_x");      // syntax errors in the second \u → UTF16 (PHP)

    // The second codepoint MUST be in the low-surrogate range.
    emitter.instruction("cmp r10, 0xDC00");                                     // is the second codepoint < 0xDC00?
    emitter.instruction("jl __rt_json_validate_string_utf16_x");                // not a low surrogate → UTF16 error
    emitter.instruction("cmp r10, 0xDFFF");                                     // is the second codepoint > 0xDFFF?
    emitter.instruction("jg __rt_json_validate_string_utf16_x");                // not a low surrogate → UTF16 error
    emitter.instruction("jmp __rt_json_validate_string_loop_x");                // valid surrogate pair → resume content scan

    emitter.label("__rt_json_validate_string_utf16_x");
    emitter.instruction("mov QWORD PTR [rip + _json_validate_idx], rcx");       // commit the failure index for diagnostics
    emitter.instruction("mov rax, 10");                                         // JSON_ERROR_UTF16
    emitter.instruction("call __rt_json_throw_error");                          // record the error and throw on JSON_THROW_ON_ERROR
    emitter.instruction("mov rax, 0");
    emitter.instruction("mov rsp, rbp");
    emitter.instruction("pop rbp");
    emitter.instruction("ret");

    emitter.label("__rt_json_validate_string_syntax_x");
    emitter.instruction("mov QWORD PTR [rip + _json_validate_idx], rcx");
    emitter.instruction("mov rax, 4");
    emitter.instruction("call __rt_json_throw_error");
    emitter.instruction("mov rax, 0");
    emitter.instruction("mov rsp, rbp");
    emitter.instruction("pop rbp");
    emitter.instruction("ret");
}

/// Emit a 4-hex-digit accumulator loop that walks the source slice and
/// builds up a 16-bit codepoint in `r10`. On entry: `r9 = 4`, `r10 = 0`,
/// `rcx` points at the first hex digit (already past `\u`),
/// `rdx = source length`, `rax = source pointer`. On exit (the
/// `__rt_json_validate_uhex_done_<suffix>_x` label) `r10` holds the
/// validated codepoint and `rcx` has advanced past the four digits.
fn emit_uhex_loop_x(emitter: &mut Emitter, suffix: &str, error_label: &str) {
    emitter.label(&format!("__rt_json_validate_uhex_loop_{suffix}_x"));
    emitter.instruction("cmp r9, 0");                                           // 4 digits consumed?
    emitter.instruction(&format!("je __rt_json_validate_uhex_done_{suffix}_x")); // exit loop with r10 = codepoint
    emitter.instruction("cmp rcx, rdx");                                        // bounds check
    emitter.instruction(&format!("jge {error_label}"));
    emitter.instruction("movzx r8, BYTE PTR [rax + rcx]");
    emitter.instruction("cmp r8, 48");                                          // '0'?
    emitter.instruction(&format!("jl {error_label}"));
    emitter.instruction("cmp r8, 57");                                          // ..'9'?
    emitter.instruction(&format!("jle __rt_json_validate_uhex_dec_{suffix}_x"));
    emitter.instruction("cmp r8, 65");                                          // 'A'?
    emitter.instruction(&format!("jl {error_label}"));
    emitter.instruction("cmp r8, 70");                                          // ..'F'?
    emitter.instruction(&format!("jle __rt_json_validate_uhex_upper_{suffix}_x"));
    emitter.instruction("cmp r8, 97");                                          // 'a'?
    emitter.instruction(&format!("jl {error_label}"));
    emitter.instruction("cmp r8, 102");                                         // ..'f'?
    emitter.instruction(&format!("jg {error_label}"));
    emitter.instruction("sub r8, 87");                                          // 'a'..'f' → 10..15
    emitter.instruction(&format!("jmp __rt_json_validate_uhex_acc_{suffix}_x"));
    emitter.label(&format!("__rt_json_validate_uhex_dec_{suffix}_x"));
    emitter.instruction("sub r8, 48");                                          // '0'..'9' → 0..9
    emitter.instruction(&format!("jmp __rt_json_validate_uhex_acc_{suffix}_x"));
    emitter.label(&format!("__rt_json_validate_uhex_upper_{suffix}_x"));
    emitter.instruction("sub r8, 55");                                          // 'A'..'F' → 10..15
    emitter.label(&format!("__rt_json_validate_uhex_acc_{suffix}_x"));
    emitter.instruction("shl r10, 4");                                          // shift accumulator nibble
    emitter.instruction("or r10, r8");                                          // OR in the digit value
    emitter.instruction("add rcx, 1");                                          // advance past the digit
    emitter.instruction("sub r9, 1");                                           // one fewer digit to scan
    emitter.instruction(&format!("jmp __rt_json_validate_uhex_loop_{suffix}_x"));
    emitter.label(&format!("__rt_json_validate_uhex_done_{suffix}_x"));
}

fn emit_number_parser_x(emitter: &mut Emitter) {
    emitter.label("__rt_json_validate_number_x");
    emitter.instruction("push rbp");
    emitter.instruction("mov rbp, rsp");
    emitter.instruction("mov rcx, QWORD PTR [rip + _json_validate_idx]");
    emitter.instruction("mov rdx, QWORD PTR [rip + _json_validate_len]");
    emitter.instruction("mov rax, QWORD PTR [rip + _json_validate_ptr]");

    emitter.instruction("cmp rcx, rdx");
    emitter.instruction("jge __rt_json_validate_number_syntax_x");
    emitter.instruction("movzx r8, BYTE PTR [rax + rcx]");
    emitter.instruction("cmp r8, 45");                                          // '-'?
    emitter.instruction("jne __rt_json_validate_number_int_start_x");
    emitter.instruction("add rcx, 1");
    emitter.instruction("cmp rcx, rdx");
    emitter.instruction("jge __rt_json_validate_number_syntax_x");

    emitter.label("__rt_json_validate_number_int_start_x");
    emitter.instruction("movzx r8, BYTE PTR [rax + rcx]");
    emitter.instruction("cmp r8, 48");
    emitter.instruction("je __rt_json_validate_number_zero_x");
    emitter.instruction("cmp r8, 49");
    emitter.instruction("jl __rt_json_validate_number_syntax_x");
    emitter.instruction("cmp r8, 57");
    emitter.instruction("jg __rt_json_validate_number_syntax_x");
    emitter.instruction("add rcx, 1");
    emitter.label("__rt_json_validate_number_int_loop_x");
    emitter.instruction("cmp rcx, rdx");
    emitter.instruction("jge __rt_json_validate_number_done_x");
    emitter.instruction("movzx r8, BYTE PTR [rax + rcx]");
    emitter.instruction("mov r9, r8");
    emitter.instruction("sub r9, 48");
    emitter.instruction("cmp r9, 9");
    emitter.instruction("ja __rt_json_validate_number_after_int_x");
    emitter.instruction("add rcx, 1");
    emitter.instruction("jmp __rt_json_validate_number_int_loop_x");

    emitter.label("__rt_json_validate_number_zero_x");
    emitter.instruction("add rcx, 1");
    emitter.instruction("cmp rcx, rdx");
    emitter.instruction("jge __rt_json_validate_number_done_x");

    emitter.label("__rt_json_validate_number_after_int_x");
    emitter.instruction("movzx r8, BYTE PTR [rax + rcx]");
    emitter.instruction("cmp r8, 46");
    emitter.instruction("je __rt_json_validate_number_frac_x");
    emitter.instruction("cmp r8, 101");
    emitter.instruction("je __rt_json_validate_number_exp_x");
    emitter.instruction("cmp r8, 69");
    emitter.instruction("je __rt_json_validate_number_exp_x");
    emitter.instruction("jmp __rt_json_validate_number_done_x");

    emitter.label("__rt_json_validate_number_frac_x");
    emitter.instruction("add rcx, 1");
    emitter.instruction("cmp rcx, rdx");
    emitter.instruction("jge __rt_json_validate_number_syntax_x");
    emitter.instruction("movzx r8, BYTE PTR [rax + rcx]");
    emitter.instruction("mov r9, r8");
    emitter.instruction("sub r9, 48");
    emitter.instruction("cmp r9, 9");
    emitter.instruction("ja __rt_json_validate_number_syntax_x");
    emitter.instruction("add rcx, 1");
    emitter.label("__rt_json_validate_number_frac_loop_x");
    emitter.instruction("cmp rcx, rdx");
    emitter.instruction("jge __rt_json_validate_number_done_x");
    emitter.instruction("movzx r8, BYTE PTR [rax + rcx]");
    emitter.instruction("mov r9, r8");
    emitter.instruction("sub r9, 48");
    emitter.instruction("cmp r9, 9");
    emitter.instruction("ja __rt_json_validate_number_after_frac_x");
    emitter.instruction("add rcx, 1");
    emitter.instruction("jmp __rt_json_validate_number_frac_loop_x");

    emitter.label("__rt_json_validate_number_after_frac_x");
    emitter.instruction("cmp r8, 101");
    emitter.instruction("je __rt_json_validate_number_exp_x");
    emitter.instruction("cmp r8, 69");
    emitter.instruction("je __rt_json_validate_number_exp_x");
    emitter.instruction("jmp __rt_json_validate_number_done_x");

    emitter.label("__rt_json_validate_number_exp_x");
    emitter.instruction("add rcx, 1");
    emitter.instruction("cmp rcx, rdx");
    emitter.instruction("jge __rt_json_validate_number_syntax_x");
    emitter.instruction("movzx r8, BYTE PTR [rax + rcx]");
    emitter.instruction("cmp r8, 43");
    emitter.instruction("je __rt_json_validate_number_exp_sign_consume_x");
    emitter.instruction("cmp r8, 45");
    emitter.instruction("je __rt_json_validate_number_exp_sign_consume_x");
    emitter.instruction("jmp __rt_json_validate_number_exp_first_x");
    emitter.label("__rt_json_validate_number_exp_sign_consume_x");
    emitter.instruction("add rcx, 1");
    emitter.instruction("cmp rcx, rdx");
    emitter.instruction("jge __rt_json_validate_number_syntax_x");
    emitter.instruction("movzx r8, BYTE PTR [rax + rcx]");
    emitter.label("__rt_json_validate_number_exp_first_x");
    emitter.instruction("mov r9, r8");
    emitter.instruction("sub r9, 48");
    emitter.instruction("cmp r9, 9");
    emitter.instruction("ja __rt_json_validate_number_syntax_x");
    emitter.instruction("add rcx, 1");
    emitter.label("__rt_json_validate_number_exp_loop_x");
    emitter.instruction("cmp rcx, rdx");
    emitter.instruction("jge __rt_json_validate_number_done_x");
    emitter.instruction("movzx r8, BYTE PTR [rax + rcx]");
    emitter.instruction("mov r9, r8");
    emitter.instruction("sub r9, 48");
    emitter.instruction("cmp r9, 9");
    emitter.instruction("ja __rt_json_validate_number_done_x");
    emitter.instruction("add rcx, 1");
    emitter.instruction("jmp __rt_json_validate_number_exp_loop_x");

    emitter.label("__rt_json_validate_number_done_x");
    emitter.instruction("mov QWORD PTR [rip + _json_validate_idx], rcx");
    emitter.instruction("mov rax, 1");
    emitter.instruction("mov rsp, rbp");
    emitter.instruction("pop rbp");
    emitter.instruction("ret");

    emitter.label("__rt_json_validate_number_syntax_x");
    emitter.instruction("mov QWORD PTR [rip + _json_validate_idx], rcx");
    emitter.instruction("mov rax, 4");
    emitter.instruction("call __rt_json_throw_error");
    emitter.instruction("mov rax, 0");
    emitter.instruction("mov rsp, rbp");
    emitter.instruction("pop rbp");
    emitter.instruction("ret");
}

fn emit_array_parser_x(emitter: &mut Emitter) {
    emitter.label("__rt_json_validate_array_x");
    emitter.instruction("push rbp");
    emitter.instruction("mov rbp, rsp");
    emitter.instruction("mov rcx, QWORD PTR [rip + _json_active_depth]");
    emitter.instruction("add rcx, 1");
    emitter.instruction("mov QWORD PTR [rip + _json_active_depth], rcx");
    emitter.instruction("cmp rcx, QWORD PTR [rip + _json_depth_limit]");
    emitter.instruction("jg __rt_json_validate_array_depth_x");

    emitter.instruction("mov rcx, QWORD PTR [rip + _json_validate_idx]");
    emitter.instruction("add rcx, 1");                                          // consume '['
    emitter.instruction("mov QWORD PTR [rip + _json_validate_idx], rcx");
    emitter.instruction("call __rt_json_validate_skip_ws_x");

    emitter.instruction("mov rcx, QWORD PTR [rip + _json_validate_idx]");
    emitter.instruction("mov rdx, QWORD PTR [rip + _json_validate_len]");
    emitter.instruction("cmp rcx, rdx");
    emitter.instruction("jge __rt_json_validate_array_syntax_x");
    emitter.instruction("mov rax, QWORD PTR [rip + _json_validate_ptr]");
    emitter.instruction("movzx r8, BYTE PTR [rax + rcx]");
    emitter.instruction("cmp r8, 93");                                          // ']'?
    emitter.instruction("je __rt_json_validate_array_close_x");

    emitter.label("__rt_json_validate_array_elem_x");
    emitter.instruction("call __rt_json_validate_value_x");
    emitter.instruction("test rax, rax");
    emitter.instruction("je __rt_json_validate_array_propagate_x");
    emitter.instruction("call __rt_json_validate_skip_ws_x");
    emitter.instruction("mov rcx, QWORD PTR [rip + _json_validate_idx]");
    emitter.instruction("mov rdx, QWORD PTR [rip + _json_validate_len]");
    emitter.instruction("cmp rcx, rdx");
    emitter.instruction("jge __rt_json_validate_array_syntax_x");
    emitter.instruction("mov rax, QWORD PTR [rip + _json_validate_ptr]");
    emitter.instruction("movzx r8, BYTE PTR [rax + rcx]");
    emitter.instruction("cmp r8, 93");
    emitter.instruction("je __rt_json_validate_array_close_x");
    emitter.instruction("cmp r8, 44");
    emitter.instruction("jne __rt_json_validate_array_syntax_x");
    emitter.instruction("add rcx, 1");
    emitter.instruction("mov QWORD PTR [rip + _json_validate_idx], rcx");
    emitter.instruction("call __rt_json_validate_skip_ws_x");
    emitter.instruction("jmp __rt_json_validate_array_elem_x");

    emitter.label("__rt_json_validate_array_close_x");
    emitter.instruction("add rcx, 1");
    emitter.instruction("mov QWORD PTR [rip + _json_validate_idx], rcx");
    emitter.instruction("mov rcx, QWORD PTR [rip + _json_active_depth]");
    emitter.instruction("sub rcx, 1");
    emitter.instruction("mov QWORD PTR [rip + _json_active_depth], rcx");
    emitter.instruction("mov rax, 1");
    emitter.instruction("mov rsp, rbp");
    emitter.instruction("pop rbp");
    emitter.instruction("ret");

    emitter.label("__rt_json_validate_array_propagate_x");
    emitter.instruction("mov rsp, rbp");
    emitter.instruction("pop rbp");
    emitter.instruction("ret");

    emitter.label("__rt_json_validate_array_syntax_x");
    emitter.instruction("mov rax, 4");
    emitter.instruction("call __rt_json_throw_error");
    emitter.instruction("mov rax, 0");
    emitter.instruction("mov rsp, rbp");
    emitter.instruction("pop rbp");
    emitter.instruction("ret");

    emitter.label("__rt_json_validate_array_depth_x");
    emitter.instruction("mov rax, 1");                                          // JSON_ERROR_DEPTH
    emitter.instruction("call __rt_json_throw_error");
    emitter.instruction("mov rax, 0");
    emitter.instruction("mov rsp, rbp");
    emitter.instruction("pop rbp");
    emitter.instruction("ret");
}

fn emit_object_parser_x(emitter: &mut Emitter) {
    emitter.label("__rt_json_validate_object_x");
    emitter.instruction("push rbp");
    emitter.instruction("mov rbp, rsp");
    emitter.instruction("mov rcx, QWORD PTR [rip + _json_active_depth]");
    emitter.instruction("add rcx, 1");
    emitter.instruction("mov QWORD PTR [rip + _json_active_depth], rcx");
    emitter.instruction("cmp rcx, QWORD PTR [rip + _json_depth_limit]");
    emitter.instruction("jg __rt_json_validate_object_depth_x");

    emitter.instruction("mov rcx, QWORD PTR [rip + _json_validate_idx]");
    emitter.instruction("add rcx, 1");                                          // consume '{'
    emitter.instruction("mov QWORD PTR [rip + _json_validate_idx], rcx");
    emitter.instruction("call __rt_json_validate_skip_ws_x");

    emitter.instruction("mov rcx, QWORD PTR [rip + _json_validate_idx]");
    emitter.instruction("mov rdx, QWORD PTR [rip + _json_validate_len]");
    emitter.instruction("cmp rcx, rdx");
    emitter.instruction("jge __rt_json_validate_object_syntax_x");
    emitter.instruction("mov rax, QWORD PTR [rip + _json_validate_ptr]");
    emitter.instruction("movzx r8, BYTE PTR [rax + rcx]");
    emitter.instruction("cmp r8, 125");                                         // '}'?
    emitter.instruction("je __rt_json_validate_object_close_x");

    emitter.label("__rt_json_validate_object_pair_x");
    emitter.instruction("call __rt_json_validate_string_x");
    emitter.instruction("test rax, rax");
    emitter.instruction("je __rt_json_validate_object_propagate_x");
    emitter.instruction("call __rt_json_validate_skip_ws_x");
    emitter.instruction("mov rcx, QWORD PTR [rip + _json_validate_idx]");
    emitter.instruction("mov rdx, QWORD PTR [rip + _json_validate_len]");
    emitter.instruction("cmp rcx, rdx");
    emitter.instruction("jge __rt_json_validate_object_syntax_x");
    emitter.instruction("mov rax, QWORD PTR [rip + _json_validate_ptr]");
    emitter.instruction("movzx r8, BYTE PTR [rax + rcx]");
    emitter.instruction("cmp r8, 58");
    emitter.instruction("jne __rt_json_validate_object_syntax_x");
    emitter.instruction("add rcx, 1");
    emitter.instruction("mov QWORD PTR [rip + _json_validate_idx], rcx");
    emitter.instruction("call __rt_json_validate_skip_ws_x");
    emitter.instruction("call __rt_json_validate_value_x");
    emitter.instruction("test rax, rax");
    emitter.instruction("je __rt_json_validate_object_propagate_x");
    emitter.instruction("call __rt_json_validate_skip_ws_x");
    emitter.instruction("mov rcx, QWORD PTR [rip + _json_validate_idx]");
    emitter.instruction("mov rdx, QWORD PTR [rip + _json_validate_len]");
    emitter.instruction("cmp rcx, rdx");
    emitter.instruction("jge __rt_json_validate_object_syntax_x");
    emitter.instruction("mov rax, QWORD PTR [rip + _json_validate_ptr]");
    emitter.instruction("movzx r8, BYTE PTR [rax + rcx]");
    emitter.instruction("cmp r8, 125");
    emitter.instruction("je __rt_json_validate_object_close_x");
    emitter.instruction("cmp r8, 44");
    emitter.instruction("jne __rt_json_validate_object_syntax_x");
    emitter.instruction("add rcx, 1");
    emitter.instruction("mov QWORD PTR [rip + _json_validate_idx], rcx");
    emitter.instruction("call __rt_json_validate_skip_ws_x");
    emitter.instruction("jmp __rt_json_validate_object_pair_x");

    emitter.label("__rt_json_validate_object_close_x");
    emitter.instruction("add rcx, 1");
    emitter.instruction("mov QWORD PTR [rip + _json_validate_idx], rcx");
    emitter.instruction("mov rcx, QWORD PTR [rip + _json_active_depth]");
    emitter.instruction("sub rcx, 1");
    emitter.instruction("mov QWORD PTR [rip + _json_active_depth], rcx");
    emitter.instruction("mov rax, 1");
    emitter.instruction("mov rsp, rbp");
    emitter.instruction("pop rbp");
    emitter.instruction("ret");

    emitter.label("__rt_json_validate_object_propagate_x");
    emitter.instruction("mov rsp, rbp");
    emitter.instruction("pop rbp");
    emitter.instruction("ret");

    emitter.label("__rt_json_validate_object_syntax_x");
    emitter.instruction("mov rax, 4");
    emitter.instruction("call __rt_json_throw_error");
    emitter.instruction("mov rax, 0");
    emitter.instruction("mov rsp, rbp");
    emitter.instruction("pop rbp");
    emitter.instruction("ret");

    emitter.label("__rt_json_validate_object_depth_x");
    emitter.instruction("mov rax, 1");                                          // JSON_ERROR_DEPTH
    emitter.instruction("call __rt_json_throw_error");
    emitter.instruction("mov rax, 0");
    emitter.instruction("mov rsp, rbp");
    emitter.instruction("pop rbp");
    emitter.instruction("ret");
}
