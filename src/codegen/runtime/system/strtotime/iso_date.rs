//! Purpose:
//! Emits the ISO `YYYY-MM-DD[ HH:MM:SS]` parser sub-routine consumed by the `__rt_strtotime` dispatcher.
//! Parses fixed-offset ASCII digits and builds a `struct tm` in the dispatcher-owned scratch slot.
//!
//! Called from:
//! - `crate::codegen::runtime::system::strtotime::mod::emit_strtotime()` via the dispatcher's first-byte digit branch.
//!
//! Key details:
//! - Entry label `__rt_strtotime_iso_entry` (ARM64) / `__rt_strtotime_iso_entry_linux_x86_64` (x86_64) expects the dispatcher frame already set up.
//! - Inputs come from `[sp+48]` (trimmed ptr) and `[sp+56]` (trimmed len); the result `struct tm` is built at `[sp+0..47]`.
//! - All exits branch to the shared `__rt_strtotime_ret` / `__rt_strtotime_fail` epilogues owned by the dispatcher (`mod.rs`).

use crate::codegen::{emit::Emitter, platform::Arch};

/// Emit the ISO date sub-routine on both targets.
pub(crate) fn emit_iso_date(emitter: &mut Emitter) {
    if emitter.target.arch == Arch::X86_64 {
        emit_iso_date_linux_x86_64(emitter);
        return;
    }

    emit_iso_date_arm64(emitter);
}

fn emit_iso_date_arm64(emitter: &mut Emitter) {
    emitter.blank();
    emitter.comment("--- strtotime: ISO date sub-routine ---");
    emitter.label("__rt_strtotime_iso_entry");

    // -- reload trimmed ptr/len from dispatcher slots --
    emitter.instruction("ldr x1, [sp, #48]");                                   // reload trimmed input pointer
    emitter.instruction("ldr x2, [sp, #56]");                                   // reload trimmed input length

    // -- validate minimum length (10 for YYYY-MM-DD) --
    emitter.instruction("cmp x2, #10");                                         // need at least 10 chars
    emitter.instruction("b.lt __rt_strtotime_fail");                            // fail if too short

    // -- parse YYYY (4 digits at offset 0) --
    emitter.instruction("ldrb w9, [x1, #0]");                                   // load 1st year digit
    emitter.instruction("sub w9, w9, #48");                                     // convert from ASCII
    emitter.instruction("mov w10, #1000");                                      // multiplier for thousands
    emitter.instruction("mul w9, w9, w10");                                     // thousands place
    emitter.instruction("ldrb w10, [x1, #1]");                                  // load 2nd year digit
    emitter.instruction("sub w10, w10, #48");                                   // convert from ASCII
    emitter.instruction("mov w11, #100");                                       // multiplier for hundreds
    emitter.instruction("mul w10, w10, w11");                                   // hundreds place
    emitter.instruction("add w9, w9, w10");                                     // accumulate
    emitter.instruction("ldrb w10, [x1, #2]");                                  // load 3rd year digit
    emitter.instruction("sub w10, w10, #48");                                   // convert from ASCII
    emitter.instruction("mov w11, #10");                                        // multiplier for tens
    emitter.instruction("mul w10, w10, w11");                                   // tens place
    emitter.instruction("add w9, w9, w10");                                     // accumulate
    emitter.instruction("ldrb w10, [x1, #3]");                                  // load 4th year digit
    emitter.instruction("sub w10, w10, #48");                                   // convert from ASCII
    emitter.instruction("add w9, w9, w10");                                     // w9 = year (e.g. 2024)
    emitter.instruction("mov w10, #1900");                                      // year base for struct tm
    emitter.instruction("sub w9, w9, w10");                                     // tm_year = year - 1900
    emitter.instruction("str w9, [sp, #20]");                                   // store tm_year

    // -- parse MM (2 digits at offset 5) --
    emitter.instruction("ldrb w9, [x1, #5]");                                   // load 1st month digit
    emitter.instruction("sub w9, w9, #48");                                     // convert from ASCII
    emitter.instruction("mov w10, #10");                                        // multiplier
    emitter.instruction("mul w9, w9, w10");                                     // tens place
    emitter.instruction("ldrb w10, [x1, #6]");                                  // load 2nd month digit
    emitter.instruction("sub w10, w10, #48");                                   // convert from ASCII
    emitter.instruction("add w9, w9, w10");                                     // w9 = month (1-12)
    emitter.instruction("sub w9, w9, #1");                                      // tm_mon = month - 1 (0-based)
    emitter.instruction("str w9, [sp, #16]");                                   // store tm_mon

    // -- parse DD (2 digits at offset 8) --
    emitter.instruction("ldrb w9, [x1, #8]");                                   // load 1st day digit
    emitter.instruction("sub w9, w9, #48");                                     // convert from ASCII
    emitter.instruction("mov w10, #10");                                        // multiplier
    emitter.instruction("mul w9, w9, w10");                                     // tens place
    emitter.instruction("ldrb w10, [x1, #9]");                                  // load 2nd day digit
    emitter.instruction("sub w10, w10, #48");                                   // convert from ASCII
    emitter.instruction("add w9, w9, w10");                                     // w9 = day
    emitter.instruction("str w9, [sp, #12]");                                   // store tm_mday

    // -- check if time component exists (length >= 19 for "YYYY-MM-DD HH:MM:SS") --
    emitter.instruction("cmp x2, #19");                                         // check for full datetime
    emitter.instruction("b.lt __rt_strtotime_iso_notime");                      // no time component

    // -- parse HH (2 digits at offset 11) --
    emitter.instruction("ldrb w9, [x1, #11]");                                  // load 1st hour digit
    emitter.instruction("sub w9, w9, #48");                                     // convert from ASCII
    emitter.instruction("mov w10, #10");                                        // multiplier
    emitter.instruction("mul w9, w9, w10");                                     // tens place
    emitter.instruction("ldrb w10, [x1, #12]");                                 // load 2nd hour digit
    emitter.instruction("sub w10, w10, #48");                                   // convert from ASCII
    emitter.instruction("add w9, w9, w10");                                     // w9 = hour
    emitter.instruction("str w9, [sp, #8]");                                    // store tm_hour

    // -- parse MM (2 digits at offset 14) --
    emitter.instruction("ldrb w9, [x1, #14]");                                  // load 1st minute digit
    emitter.instruction("sub w9, w9, #48");                                     // convert from ASCII
    emitter.instruction("mov w10, #10");                                        // multiplier
    emitter.instruction("mul w9, w9, w10");                                     // tens place
    emitter.instruction("ldrb w10, [x1, #15]");                                 // load 2nd minute digit
    emitter.instruction("sub w10, w10, #48");                                   // convert from ASCII
    emitter.instruction("add w9, w9, w10");                                     // w9 = minute
    emitter.instruction("str w9, [sp, #4]");                                    // store tm_min

    // -- parse SS (2 digits at offset 17) --
    emitter.instruction("ldrb w9, [x1, #17]");                                  // load 1st second digit
    emitter.instruction("sub w9, w9, #48");                                     // convert from ASCII
    emitter.instruction("mov w10, #10");                                        // multiplier
    emitter.instruction("mul w9, w9, w10");                                     // tens place
    emitter.instruction("ldrb w10, [x1, #18]");                                 // load 2nd second digit
    emitter.instruction("sub w10, w10, #48");                                   // convert from ASCII
    emitter.instruction("add w9, w9, w10");                                     // w9 = second
    emitter.instruction("str w9, [sp, #0]");                                    // store tm_sec
    emitter.instruction("b __rt_strtotime_iso_mktime");                         // proceed to mktime

    // -- no time component, default to 00:00:00 --
    emitter.label("__rt_strtotime_iso_notime");
    emitter.instruction("str wzr, [sp, #0]");                                   // tm_sec = 0
    emitter.instruction("str wzr, [sp, #4]");                                   // tm_min = 0
    emitter.instruction("str wzr, [sp, #8]");                                   // tm_hour = 0

    // -- fill remaining tm fields and call mktime --
    emitter.label("__rt_strtotime_iso_mktime");
    emitter.instruction("str wzr, [sp, #24]");                                  // tm_wday = 0
    emitter.instruction("str wzr, [sp, #28]");                                  // tm_yday = 0
    emitter.instruction("mov w9, #-1");                                         // tm_isdst = -1
    emitter.instruction("str w9, [sp, #32]");                                   // store tm_isdst
    emitter.instruction("mov x0, sp");                                          // x0 = pointer to struct tm
    emitter.bl_c("mktime");                                                     // mktime(&tm) → x0=timestamp
    emitter.instruction("b __rt_strtotime_ret");                                // return through shared epilogue
}

fn emit_iso_date_linux_x86_64(emitter: &mut Emitter) {
    emitter.blank();
    emitter.comment("--- strtotime: ISO date sub-routine ---");
    emitter.label("__rt_strtotime_iso_entry_linux_x86_64");

    // -- reload trimmed ptr/len from dispatcher slots --
    emitter.instruction("mov rdi, QWORD PTR [rsp + 48]");                       // reload trimmed input pointer from dispatcher slot
    emitter.instruction("mov rsi, QWORD PTR [rsp + 56]");                       // reload trimmed input length from dispatcher slot
    emitter.instruction("cmp rsi, 10");                                         // require at least the YYYY-MM-DD prefix
    emitter.instruction("jb __rt_strtotime_fail_linux_x86_64");                 // reject too-short inputs through the shared fail label

    emitter.instruction("mov r8, rdi");                                         // pin the date-string pointer for repeated relative byte loads
    emitter.instruction("movzx eax, BYTE PTR [r8 + 0]");                        // load the first year digit from the date string
    emitter.instruction("sub eax, 48");                                         // convert the first year digit from ASCII to its numeric value
    emitter.instruction("imul eax, eax, 1000");                                 // place the first year digit into the thousands column
    emitter.instruction("movzx ecx, BYTE PTR [r8 + 1]");                        // load the second year digit from the date string
    emitter.instruction("sub ecx, 48");                                         // convert the second year digit from ASCII to its numeric value
    emitter.instruction("imul ecx, ecx, 100");                                  // place the second year digit into the hundreds column
    emitter.instruction("add eax, ecx");                                        // accumulate the hundreds contribution
    emitter.instruction("movzx ecx, BYTE PTR [r8 + 2]");                        // load the third year digit from the date string
    emitter.instruction("sub ecx, 48");                                         // convert the third year digit from ASCII to its numeric value
    emitter.instruction("imul ecx, ecx, 10");                                   // place the third year digit into the tens column
    emitter.instruction("add eax, ecx");                                        // accumulate the tens contribution
    emitter.instruction("movzx ecx, BYTE PTR [r8 + 3]");                        // load the fourth year digit from the date string
    emitter.instruction("sub ecx, 48");                                         // convert the fourth year digit from ASCII to its numeric value
    emitter.instruction("add eax, ecx");                                        // finish assembling the full Gregorian year
    emitter.instruction("sub eax, 1900");                                       // convert the Gregorian year to libc's tm_year encoding
    emitter.instruction("mov DWORD PTR [rsp + 20], eax");                       // tm_year = parsed year - 1900

    emitter.instruction("movzx eax, BYTE PTR [r8 + 5]");                        // load the first month digit
    emitter.instruction("sub eax, 48");                                         // convert the first month digit from ASCII to its numeric value
    emitter.instruction("imul eax, eax, 10");                                   // place the first month digit into the tens column
    emitter.instruction("movzx ecx, BYTE PTR [r8 + 6]");                        // load the second month digit
    emitter.instruction("sub ecx, 48");                                         // convert the second month digit from ASCII to its numeric value
    emitter.instruction("add eax, ecx");                                        // finish assembling the calendar month
    emitter.instruction("sub eax, 1");                                          // convert the month from PHP's 1-12 to libc's 0-11 tm_mon
    emitter.instruction("mov DWORD PTR [rsp + 16], eax");                       // tm_mon = parsed month - 1

    emitter.instruction("movzx eax, BYTE PTR [r8 + 8]");                        // load the first day-of-month digit
    emitter.instruction("sub eax, 48");                                         // convert the first day-of-month digit from ASCII to its numeric value
    emitter.instruction("imul eax, eax, 10");                                   // place the first day-of-month digit into the tens column
    emitter.instruction("movzx ecx, BYTE PTR [r8 + 9]");                        // load the second day-of-month digit
    emitter.instruction("sub ecx, 48");                                         // convert the second day-of-month digit from ASCII to its numeric value
    emitter.instruction("add eax, ecx");                                        // finish assembling the day-of-month component
    emitter.instruction("mov DWORD PTR [rsp + 12], eax");                       // tm_mday = parsed day-of-month

    emitter.instruction("cmp rsi, 19");                                         // the full YYYY-MM-DD HH:MM:SS form requires at least 19 bytes
    emitter.instruction("jb __rt_strtotime_iso_notime_linux_x86_64");           // fall back to midnight when the time suffix is absent

    emitter.instruction("movzx eax, BYTE PTR [r8 + 11]");                       // load the first hour digit
    emitter.instruction("sub eax, 48");                                         // convert the first hour digit from ASCII to its numeric value
    emitter.instruction("imul eax, eax, 10");                                   // place the first hour digit into the tens column
    emitter.instruction("movzx ecx, BYTE PTR [r8 + 12]");                       // load the second hour digit
    emitter.instruction("sub ecx, 48");                                         // convert the second hour digit from ASCII to its numeric value
    emitter.instruction("add eax, ecx");                                        // finish assembling the hour component
    emitter.instruction("mov DWORD PTR [rsp + 8], eax");                        // tm_hour = parsed hour

    emitter.instruction("movzx eax, BYTE PTR [r8 + 14]");                       // load the first minute digit
    emitter.instruction("sub eax, 48");                                         // convert the first minute digit from ASCII to its numeric value
    emitter.instruction("imul eax, eax, 10");                                   // place the first minute digit into the tens column
    emitter.instruction("movzx ecx, BYTE PTR [r8 + 15]");                       // load the second minute digit
    emitter.instruction("sub ecx, 48");                                         // convert the second minute digit from ASCII to its numeric value
    emitter.instruction("add eax, ecx");                                        // finish assembling the minute component
    emitter.instruction("mov DWORD PTR [rsp + 4], eax");                        // tm_min = parsed minute

    emitter.instruction("movzx eax, BYTE PTR [r8 + 17]");                       // load the first second digit
    emitter.instruction("sub eax, 48");                                         // convert the first second digit from ASCII to its numeric value
    emitter.instruction("imul eax, eax, 10");                                   // place the first second digit into the tens column
    emitter.instruction("movzx ecx, BYTE PTR [r8 + 18]");                       // load the second second digit
    emitter.instruction("sub ecx, 48");                                         // convert the second second digit from ASCII to its numeric value
    emitter.instruction("add eax, ecx");                                        // finish assembling the second component
    emitter.instruction("mov DWORD PTR [rsp + 0], eax");                        // tm_sec = parsed second
    emitter.instruction("jmp __rt_strtotime_iso_mktime_linux_x86_64");          // skip the midnight-default path

    emitter.label("__rt_strtotime_iso_notime_linux_x86_64");
    emitter.instruction("mov DWORD PTR [rsp + 0], 0");                          // default tm_sec to zero when the date-only form was given
    emitter.instruction("mov DWORD PTR [rsp + 4], 0");                          // default tm_min to zero when the date-only form was given
    emitter.instruction("mov DWORD PTR [rsp + 8], 0");                          // default tm_hour to zero when the date-only form was given

    emitter.label("__rt_strtotime_iso_mktime_linux_x86_64");
    emitter.instruction("mov DWORD PTR [rsp + 24], 0");                         // tm_wday = 0 (mktime ignores)
    emitter.instruction("mov DWORD PTR [rsp + 28], 0");                         // tm_yday = 0 (mktime ignores)
    emitter.instruction("mov DWORD PTR [rsp + 32], -1");                        // tm_isdst = -1 so libc mktime infers DST automatically
    emitter.instruction("mov rdi, rsp");                                        // pass &tm as the first SysV argument to libc mktime
    emitter.instruction("call mktime");                                         // convert the parsed components into a Unix timestamp
    emitter.instruction("jmp __rt_strtotime_ret_linux_x86_64");                 // return through the shared epilogue
}
