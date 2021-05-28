// AluRE: AluVM runtime environment.
// This is rust implementation of AluVM (arithmetic logic unit virtual machine).
//
// Designed & written in 2021 by
//     Dr. Maxim Orlovsky <orlovsky@pandoracore.com>
//
// This software is licensed under the terms of MIT License.
// You should have received a copy of the MIT License along with this software.
// If not, see <https://opensource.org/licenses/MIT>.

use core::cmp::Ordering;
use core::ops::{BitAnd, BitOr, BitXor, Shl, Shr};

use amplify_num::u5;

use super::{
    ArithmeticOp, BitwiseOp, Bytecode, BytesOp, CmpOp, ControlFlowOp, Curve25519Op, DigestOp,
    Instr, MoveOp, NOp, PutOp, SecpOp,
};
use crate::reg::{Reg32, RegVal, Registers, Value};
use crate::LibSite;

/// Turing machine movement after instruction execution
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum ExecStep {
    /// Stop program execution
    Stop,

    /// Move to the next instruction
    Next,

    /// Jump to the offset from the origin
    Jump(u16),

    /// Jump to another code fragment
    Call(LibSite),
}

#[cfg(not(feature = "std"))]
/// Trait for instructions
pub trait Instruction: Bytecode {
    /// Executes given instruction taking all registers as input and output.
    /// The method is provided with the current code position which may be
    /// used by the instruction for constructing call stack.
    ///
    /// Returns whether further execution should be stopped.
    fn exec(self, regs: &mut Registers, site: LibSite) -> ExecStep;
}

#[cfg(feature = "std")]
/// Trait for instructions
pub trait InstructionSet: Bytecode + std::fmt::Display {
    /// Executes given instruction taking all registers as input and output.
    /// The method is provided with the current code position which may be
    /// used by the instruction for constructing call stack.
    ///
    /// Returns whether further execution should be stopped.
    fn exec(self, regs: &mut Registers, site: LibSite) -> ExecStep;
}

impl<Extension> InstructionSet for Instr<Extension>
where
    Extension: InstructionSet,
{
    fn exec(self, regs: &mut Registers, site: LibSite) -> ExecStep {
        match self {
            Instr::ControlFlow(instr) => instr.exec(regs, site),
            Instr::Put(instr) => instr.exec(regs, site),
            Instr::Move(instr) => instr.exec(regs, site),
            Instr::Cmp(instr) => instr.exec(regs, site),
            Instr::Arithmetic(instr) => instr.exec(regs, site),
            Instr::Bitwise(instr) => instr.exec(regs, site),
            Instr::Bytes(instr) => instr.exec(regs, site),
            Instr::Digest(instr) => instr.exec(regs, site),
            Instr::Secp256k1(instr) => instr.exec(regs, site),
            Instr::Curve25519(instr) => instr.exec(regs, site),
            Instr::ExtensionCodes(instr) => instr.exec(regs, site),
            Instr::Nop => ExecStep::Next,
        }
    }
}

impl InstructionSet for ControlFlowOp {
    fn exec(self, regs: &mut Registers, site: LibSite) -> ExecStep {
        match self {
            ControlFlowOp::Fail => {
                regs.st0 = false;
                ExecStep::Stop
            }
            ControlFlowOp::Succ => {
                regs.st0 = true;
                ExecStep::Stop
            }
            ControlFlowOp::Jmp(offset) => regs
                .jmp()
                .map(|_| ExecStep::Jump(offset))
                .unwrap_or(ExecStep::Stop),
            ControlFlowOp::Jif(offset) => {
                if regs.st0 == true {
                    regs.jmp()
                        .map(|_| ExecStep::Jump(offset))
                        .unwrap_or(ExecStep::Stop)
                } else {
                    ExecStep::Next
                }
            }
            ControlFlowOp::Routine(offset) => regs
                .call(site)
                .map(|_| ExecStep::Jump(offset))
                .unwrap_or(ExecStep::Stop),
            ControlFlowOp::Call(site) => regs
                .call(site)
                .map(|_| ExecStep::Call(site))
                .unwrap_or(ExecStep::Stop),
            ControlFlowOp::Exec(site) => regs
                .jmp()
                .map(|_| ExecStep::Call(site))
                .unwrap_or(ExecStep::Stop),
            ControlFlowOp::Ret => regs.ret().map(ExecStep::Call).unwrap_or(ExecStep::Stop),
        }
    }
}

impl InstructionSet for PutOp {
    fn exec(self, regs: &mut Registers, _: LibSite) -> ExecStep {
        match self {
            PutOp::ZeroA(reg, index) => regs.set(reg, index, 0),
            PutOp::ZeroR(reg, index) => regs.set(reg, index, 0),
            PutOp::ClA(reg, index) => regs.set(reg, index, RegVal::none()),
            PutOp::ClR(reg, index) => regs.set(reg, index, RegVal::none()),
            PutOp::PutA(reg, index, blob) => regs.set(reg, index, blob),
            PutOp::PutR(reg, index, blob) => regs.set(reg, index, blob),
            PutOp::PutIfA(reg, index, blob) => regs.set_if(reg, index, blob),
            PutOp::PutIfR(reg, index, blob) => regs.set_if(reg, index, blob),
        }
        ExecStep::Next
    }
}

impl InstructionSet for MoveOp {
    fn exec(self, regs: &mut Registers, _: LibSite) -> ExecStep {
        match self {
            MoveOp::SwpA(reg1, index1, reg2, index2) => {
                regs.set(reg1, index1, regs.get(reg2, index2));
                regs.set(reg2, index2, regs.get(reg1, index1));
            }
            MoveOp::SwpR(reg1, index1, reg2, index2) => {
                regs.set(reg1, index1, regs.get(reg2, index2));
                regs.set(reg2, index2, regs.get(reg1, index1));
            }
            MoveOp::SwpAR(reg1, index1, reg2, index2) => {
                regs.set(reg1, index1, regs.get(reg2, index2));
                regs.set(reg2, index2, regs.get(reg1, index1));
            }
            MoveOp::AMov(reg1, reg2, num_type) => {
                for idx in 0u8..32 {
                    regs.set(reg2, u5::with(idx), regs.get(reg1, u5::with(idx)));
                }
            }
            MoveOp::MovA(sreg, sidx, dreg, didx) => {
                regs.set(dreg, didx, regs.get(sreg, sidx));
            }
            MoveOp::MovR(sreg, sidx, dreg, didx) => {
                regs.set(dreg, didx, regs.get(sreg, sidx));
            }
            MoveOp::MovAR(sreg, sidx, dreg, didx) => {
                regs.set(dreg, didx, regs.get(sreg, sidx));
            }
            MoveOp::MovRA(sreg, sidx, dreg, didx) => {
                regs.set(dreg, didx, regs.get(sreg, sidx));
            }
        }
        ExecStep::Next
    }
}

impl InstructionSet for CmpOp {
    fn exec(self, regs: &mut Registers, _: LibSite) -> ExecStep {
        match self {
            CmpOp::GtA(num_type, reg, idx1, idx2) => {
                regs.st0 =
                    RegVal::partial_cmp_op(num_type)(regs.get(reg, idx1), regs.get(reg, idx2))
                        == Some(Ordering::Greater);
            }
            CmpOp::GtR(reg1, idx1, reg2, idx2) => {
                regs.st0 = regs.get(reg1, idx1).partial_cmp_uint(regs.get(reg2, idx2))
                    == Some(Ordering::Greater);
            }
            CmpOp::LtA(num_type, reg, idx1, idx2) => {
                regs.st0 =
                    RegVal::partial_cmp_op(num_type)(regs.get(reg, idx1), regs.get(reg, idx2))
                        == Some(Ordering::Less);
            }
            CmpOp::LtR(reg1, idx1, reg2, idx2) => {
                regs.st0 = regs.get(reg1, idx1).partial_cmp_uint(regs.get(reg2, idx2))
                    == Some(Ordering::Less);
            }
            CmpOp::EqA(reg1, idx1, reg2, idx2) => {
                regs.st0 = regs.get(reg1, idx1) == regs.get(reg2, idx2);
            }
            CmpOp::EqR(reg1, idx1, reg2, idx2) => {
                regs.st0 = regs.get(reg1, idx1) == regs.get(reg2, idx2);
            }
            CmpOp::Len(reg, idx) => {
                regs.a16[0] = regs.get(reg, idx).map(|v| v.len);
            }
            CmpOp::Cnt(reg, idx) => {
                regs.a16[0] = regs.get(reg, idx).map(|v| v.count_ones());
            }
            CmpOp::St2A => {
                regs.a8[0] = if regs.st0 == true { Some(1) } else { Some(0) };
            }
            CmpOp::A2St => {
                regs.st0 = regs.a8[1].map(|val| val != 0).unwrap_or(false);
            }
        }
        ExecStep::Next
    }
}

impl InstructionSet for ArithmeticOp {
    fn exec(self, regs: &mut Registers, _: LibSite) -> ExecStep {
        match self {
            ArithmeticOp::Neg(reg, index) => {
                regs.get(reg, index).map(|mut blob| {
                    blob.bytes[reg as usize] = 0xFF ^ blob.bytes[reg as usize];
                    regs.set(reg, index, Some(blob));
                });
            }
            ArithmeticOp::Stp(dir, arithm, reg, index, step) => {
                regs.op_ap1(
                    reg,
                    index,
                    arithm.is_ap(),
                    Reg32::Reg1,
                    Value::step_op(arithm, step.as_u8() as i8 * dir.multiplier()),
                );
            }
            ArithmeticOp::Add(arithm, reg, src1, src2) => {
                regs.op_ap2(
                    reg,
                    src1,
                    src2,
                    arithm.is_ap(),
                    Reg32::Reg1,
                    Value::add_op(arithm),
                );
            }
            ArithmeticOp::Sub(arithm, reg, src1, src2) => {
                regs.op_ap2(
                    reg,
                    src1,
                    src2,
                    arithm.is_ap(),
                    Reg32::Reg1,
                    Value::sub_op(arithm),
                );
            }
            ArithmeticOp::Mul(arithm, reg, src1, src2) => {
                regs.op_ap2(
                    reg,
                    src1,
                    src2,
                    arithm.is_ap(),
                    Reg32::Reg1,
                    Value::mul_op(arithm),
                );
            }
            ArithmeticOp::Div(arithm, reg, src1, src2) => {
                regs.op_ap2(
                    reg,
                    src1,
                    src2,
                    arithm.is_ap(),
                    Reg32::Reg1,
                    Value::div_op(arithm),
                );
            }
            ArithmeticOp::Rem(arithm, reg, src1, src2) => {
                regs.op_ap2(
                    reg,
                    src1,
                    src2,
                    arithm.is_ap(),
                    Reg32::Reg1,
                    Value::rem_op(arithm),
                );
            }
            ArithmeticOp::Abs(reg, index) => {
                todo!()
            }
        }
        ExecStep::Next
    }
}

impl InstructionSet for BitwiseOp {
    fn exec(self, regs: &mut Registers, site: LibSite) -> ExecStep {
        match self {
            BitwiseOp::And(reg, src1, src2, dst) => regs.op(reg, src1, src2, dst, BitAnd::bitand),
            BitwiseOp::Or(reg, src1, src2, dst) => regs.op(reg, src1, src2, dst, BitOr::bitor),
            BitwiseOp::Xor(reg, src1, src2, dst) => regs.op(reg, src1, src2, dst, BitXor::bitxor),
            BitwiseOp::Not(reg, idx) => regs.set(reg, idx, !regs.get(reg, idx)),
            BitwiseOp::Shl(reg, src1, src2, dst) => regs.op(reg, src1, src2, dst, Shl::shl),
            BitwiseOp::Shr(reg, src1, src2, dst) => regs.op(reg, src1, src2, dst, Shr::shr),
            BitwiseOp::Scl(reg, src1, src2, dst) => regs.op(reg, src1, src2, dst, Value::scl),
            BitwiseOp::Scr(reg, src1, src2, dst) => regs.op(reg, src1, src2, dst, Value::scr),
        }
        ExecStep::Next
    }
}

impl InstructionSet for BytesOp {
    fn exec(self, regs: &mut Registers, site: LibSite) -> ExecStep {
        todo!()
    }
}

impl InstructionSet for DigestOp {
    fn exec(self, regs: &mut Registers, site: LibSite) -> ExecStep {
        todo!()
    }
}

impl InstructionSet for SecpOp {
    fn exec(self, regs: &mut Registers, site: LibSite) -> ExecStep {
        todo!()
    }
}

impl InstructionSet for Curve25519Op {
    fn exec(self, regs: &mut Registers, site: LibSite) -> ExecStep {
        todo!()
    }
}

impl InstructionSet for NOp {
    fn exec(self, regs: &mut Registers, site: LibSite) -> ExecStep {
        ExecStep::Next
    }
}