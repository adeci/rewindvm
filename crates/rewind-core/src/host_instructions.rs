//! The x86 instructions whose results come from the host rather than from
//! the guest's own state: what `rewind replay --bisect` names when the
//! first instruction whose result differs between two replays is one of
//! them, and says why its result can differ.

use serde::Serialize;

/// What an instruction that reads the host reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reads {
    /// A random number from the CPU: rdrand, rdseed.
    Random,
    /// The time stamp counter: rdtsc, rdtscp.
    TimeStamp,
    /// A performance counter: rdpmc.
    Counter,
    /// What the CPU says of itself: cpuid.
    Identity,
    /// A model-specific register: rdmsr.
    Msr,
    /// Which parts of the extended state are in use: xgetbv.
    ExtendedState,
    /// The processor's id: rdpid.
    ProcessorId,
}

/// An instruction the table knows, as a person reads it, and what it
/// reads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Host {
    pub mnemonic: String,
    pub reads: Reads,
}

/// Legacy prefixes an instruction may start with: operand and address
/// size, the segment overrides, lock and the two rep prefixes.
const OPERAND_SIZE: u8 = 0x66;
const REP: u8 = 0xf3;
const LEGACY_PREFIXES: [u8; 10] = [0x66, 0x67, 0x2e, 0x36, 0x3e, 0x26, 0x64, 0x65, 0xf0, 0xf2];

/// REX prefixes are 0x40 to 0x4f: W selects 64-bit operands, B extends
/// ModRM's rm field.
const REX_MASK: u8 = 0xf0;
const REX: u8 = 0x40;
const REX_W: u8 = 0x08;
const REX_B: u8 = 0x01;

/// Two-byte opcodes start with this escape.
const ESCAPE: u8 = 0x0f;

/// ModRM's fields: mod in the top two bits, reg in the middle three, rm
/// in the low three. Mod 3 names a register.
const MOD_REGISTER: u8 = 3;

const REGISTERS_64: [&str; 16] = [
    "rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi", "r8", "r9", "r10", "r11", "r12", "r13",
    "r14", "r15",
];
const REGISTERS_32: [&str; 16] = [
    "eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi", "r8d", "r9d", "r10d", "r11d", "r12d",
    "r13d", "r14d", "r15d",
];
const REGISTERS_16: [&str; 16] = [
    "ax", "cx", "dx", "bx", "sp", "bp", "si", "di", "r8w", "r9w", "r10w", "r11w", "r12w", "r13w",
    "r14w", "r15w",
];

/// The instruction `bytes` start with, when the table knows it.
pub fn decode(bytes: &[u8]) -> Option<Host> {
    // The prefixes, then the opcode.
    let mut at = 0;
    let mut operand_size = false;
    let mut rep = false;
    while let Some(&b) = bytes.get(at) {
        if b == REP {
            rep = true;
        } else if LEGACY_PREFIXES.contains(&b) {
            operand_size |= b == OPERAND_SIZE;
        } else {
            break;
        }
        at += 1;
    }
    let mut rex = 0;
    if bytes.get(at).is_some_and(|b| b & REX_MASK == REX) {
        rex = bytes[at];
        at += 1;
    }
    if *bytes.get(at)? != ESCAPE {
        return None;
    }
    let opcode = *bytes.get(at + 1)?;
    let host = |mnemonic: &str, reads| {
        Some(Host {
            mnemonic: mnemonic.to_string(),
            reads,
        })
    };
    match opcode {
        0x31 => host("rdtsc", Reads::TimeStamp),
        0x32 => host("rdmsr", Reads::Msr),
        0x33 => host("rdpmc", Reads::Counter),
        0xa2 => host("cpuid", Reads::Identity),
        0x01 => match *bytes.get(at + 2)? {
            0xf9 => host("rdtscp", Reads::TimeStamp),
            0xd0 => host("xgetbv", Reads::ExtendedState),
            _ => None,
        },

        // rdrand, rdseed and rdpid name their register in ModRM's rm.
        0xc7 => {
            let modrm = *bytes.get(at + 2)?;
            if modrm >> 6 != MOD_REGISTER {
                return None;
            }
            let reg = (modrm >> 3) & 7;
            let rm = (modrm & 7) as usize | if rex & REX_B != 0 { 8 } else { 0 };
            let register = if rex & REX_W != 0 {
                REGISTERS_64[rm]
            } else if operand_size {
                REGISTERS_16[rm]
            } else {
                REGISTERS_32[rm]
            };
            match (reg, rep) {
                (6, false) => host(&format!("rdrand {register}"), Reads::Random),
                (7, false) => host(&format!("rdseed {register}"), Reads::Random),
                (7, true) => host(&format!("rdpid {}", REGISTERS_64[rm]), Reads::ProcessorId),
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    // Instructions as a compiler emits them, decoded by the table, and
    // ones it does not know.
    use super::*;

    fn mnemonic(bytes: &[u8]) -> Option<String> {
        decode(bytes).map(|h| h.mnemonic)
    }

    #[test]
    fn the_random_number_instructions_name_their_register() {
        // rdrand rax is 48 0f c7 f0; rdrand r9d is 41 0f c7 f1; rdseed
        // with a 16-bit operand is 66 0f c7 f8; rdpid rax is f3 0f c7 f8.
        assert_eq!(mnemonic(&[0x48, 0x0f, 0xc7, 0xf0]).unwrap(), "rdrand rax");
        assert_eq!(mnemonic(&[0x41, 0x0f, 0xc7, 0xf1]).unwrap(), "rdrand r9d");
        assert_eq!(mnemonic(&[0x66, 0x0f, 0xc7, 0xf8]).unwrap(), "rdseed ax");
        assert_eq!(mnemonic(&[0xf3, 0x0f, 0xc7, 0xf8]).unwrap(), "rdpid rax");
        assert_eq!(
            decode(&[0x48, 0x0f, 0xc7, 0xf0]).unwrap().reads,
            Reads::Random
        );
    }

    #[test]
    fn the_other_host_reads_are_named() {
        // rdtsc, rdtscp, rdpmc, rdmsr, cpuid and xgetbv.
        assert_eq!(mnemonic(&[0x0f, 0x31]).unwrap(), "rdtsc");
        assert_eq!(mnemonic(&[0x0f, 0x01, 0xf9]).unwrap(), "rdtscp");
        assert_eq!(mnemonic(&[0x0f, 0x33]).unwrap(), "rdpmc");
        assert_eq!(mnemonic(&[0x0f, 0x32]).unwrap(), "rdmsr");
        assert_eq!(mnemonic(&[0x0f, 0xa2]).unwrap(), "cpuid");
        assert_eq!(mnemonic(&[0x0f, 0x01, 0xd0]).unwrap(), "xgetbv");
    }

    #[test]
    fn other_instructions_and_memory_forms_are_not_known() {
        // A mov, cmpxchg8b through memory (0f c7 /1, mod 0), and bytes cut
        // short.
        assert_eq!(decode(&[0x48, 0x89, 0xc3]), None);
        assert_eq!(decode(&[0x0f, 0xc7, 0x0e]), None);
        assert_eq!(decode(&[0x0f]), None);
        assert_eq!(decode(&[]), None);
    }
}
