//! Definition of each opcode of the EVM.
use crate::{
    circuit_input_builder::{
        CircuitInputStateRef, CopyBytes, CopyDataType, CopyEvent, ExecStep, NumberOrHash,
    },
    error::{
        ContractAddressCollisionError, DepthError, ExecError, InsufficientBalanceError,
        NonceUintOverflowError, OogError,
    },
    evm::OpcodeId,
    l2_predeployed::l1_gas_price_oracle,
    operation::{
        AccountField, AccountOp, CallContextField, StorageOp, TxAccessListAccountOp,
        TxReceiptField, TxRefundOp, RW,
    },
    state_db::CodeDB,
    Error,
};
use core::fmt::Debug;
use eth_types::{
    evm_types::{gas_utils::tx_data_gas_cost, GasCost, MAX_REFUND_QUOTIENT_OF_GAS_USED},
    evm_unimplemented, Bytecode, GethExecStep, GethExecTrace, ToAddress, ToWord, Word,
};
use ethers_core::utils::get_contract_address;

use crate::util::CHECK_MEM_STRICT;

#[cfg(any(feature = "test", test))]
pub use self::sha3::sha3_tests::{gen_sha3_code, MemoryKind};

mod address;
mod balance;
mod blockhash;
mod calldatacopy;
mod calldataload;
mod calldatasize;
mod caller;
mod callop;
mod callvalue;
mod chainid;
mod codecopy;
mod codesize;
mod create;
mod dup;
mod exp;
mod extcodecopy;
mod extcodehash;
mod extcodesize;
mod gasprice;
mod logs;
mod mload;
mod mstore;
mod number;
mod origin;
mod precompiles;
mod push0;
mod return_revert;
mod returndatacopy;
mod returndatasize;
mod selfbalance;
mod sha3;
mod sload;
mod sstore;
mod stackonlyop;
mod stop;
mod swap;

mod error_codestore;
mod error_contract_address_collision;
mod error_invalid_creation_code;
mod error_invalid_jump;
mod error_oog_account_access;
mod error_oog_call;
mod error_oog_log;
mod error_oog_memory_copy;
mod error_oog_precompile;
mod error_oog_sload_sstore;
mod error_precompile_failed;
mod error_return_data_outofbound;
mod error_write_protection;

#[cfg(test)]
mod memory_expansion_test;
#[cfg(feature = "test")]
pub use callop::tests::PrecompileCallArgs;

use self::sha3::Sha3;
use crate::precompile::is_precompiled;
use address::Address;
use balance::Balance;
use blockhash::Blockhash;
use calldatacopy::Calldatacopy;
use calldataload::Calldataload;
use calldatasize::Calldatasize;
use caller::Caller;
use callop::CallOpcode;
use callvalue::Callvalue;
use codecopy::Codecopy;
use codesize::Codesize;
use create::Create;
use dup::Dup;
use error_codestore::ErrorCodeStore;
use error_invalid_creation_code::ErrorCreationCode;
use error_invalid_jump::InvalidJump;
use error_oog_account_access::ErrorOOGAccountAccess;
use error_oog_call::OOGCall;
use error_oog_log::ErrorOOGLog;
use error_oog_memory_copy::OOGMemoryCopy;
use error_oog_sload_sstore::OOGSloadSstore;
use error_precompile_failed::PrecompileFailed;
use error_return_data_outofbound::ErrorReturnDataOutOfBound;
use error_write_protection::ErrorWriteProtection;
use exp::Exponentiation;
use extcodecopy::Extcodecopy;
use extcodehash::Extcodehash;
use extcodesize::Extcodesize;
use gasprice::GasPrice;
use logs::Log;
use mload::Mload;
use mstore::Mstore;
use origin::Origin;
use push0::Push0;
use return_revert::ReturnRevert;
use returndatacopy::Returndatacopy;
use returndatasize::Returndatasize;
use selfbalance::Selfbalance;
use sload::Sload;
use sstore::Sstore;
use stackonlyop::StackOnlyOpcode;
use stop::Stop;
use swap::Swap;

/// Generic opcode trait which defines the logic of the
/// [`Operation`](crate::operation::Operation) that should be generated for one
/// or multiple [`ExecStep`](crate::circuit_input_builder::ExecStep) depending
/// of the [`OpcodeId`] it contains.
pub trait Opcode: Debug {
    /// Generate the associated [`MemoryOp`](crate::operation::MemoryOp)s,
    /// [`StackOp`](crate::operation::StackOp)s, and
    /// [`StorageOp`](crate::operation::StorageOp)s associated to the Opcode
    /// is implemented for.
    fn gen_associated_ops(
        state: &mut CircuitInputStateRef,
        geth_steps: &[GethExecStep],
    ) -> Result<Vec<ExecStep>, Error>;
}

#[derive(Debug, Copy, Clone)]
struct Dummy;

impl Opcode for Dummy {
    fn gen_associated_ops(
        state: &mut CircuitInputStateRef,
        geth_steps: &[GethExecStep],
    ) -> Result<Vec<ExecStep>, Error> {
        Ok(vec![state.new_step(&geth_steps[0])?])
    }
}

type FnGenAssociatedOps = fn(
    state: &mut CircuitInputStateRef,
    geth_steps: &[GethExecStep],
) -> Result<Vec<ExecStep>, Error>;

fn fn_gen_associated_ops(opcode_id: &OpcodeId) -> FnGenAssociatedOps {
    if opcode_id.is_push_with_data() {
        return StackOnlyOpcode::<0, 1>::gen_associated_ops;
    }

    match opcode_id {
        OpcodeId::PUSH0 => Push0::gen_associated_ops,
        OpcodeId::STOP => Stop::gen_associated_ops,
        OpcodeId::ADD => StackOnlyOpcode::<2, 1>::gen_associated_ops,
        OpcodeId::MUL => StackOnlyOpcode::<2, 1>::gen_associated_ops,
        OpcodeId::SUB => StackOnlyOpcode::<2, 1>::gen_associated_ops,
        OpcodeId::DIV => StackOnlyOpcode::<2, 1>::gen_associated_ops,
        OpcodeId::SDIV => StackOnlyOpcode::<2, 1>::gen_associated_ops,
        OpcodeId::MOD => StackOnlyOpcode::<2, 1>::gen_associated_ops,
        OpcodeId::SMOD => StackOnlyOpcode::<2, 1>::gen_associated_ops,
        OpcodeId::ADDMOD => StackOnlyOpcode::<3, 1>::gen_associated_ops,
        OpcodeId::MULMOD => StackOnlyOpcode::<3, 1>::gen_associated_ops,
        OpcodeId::SIGNEXTEND => StackOnlyOpcode::<2, 1>::gen_associated_ops,
        OpcodeId::LT => StackOnlyOpcode::<2, 1>::gen_associated_ops,
        OpcodeId::GT => StackOnlyOpcode::<2, 1>::gen_associated_ops,
        OpcodeId::SLT => StackOnlyOpcode::<2, 1>::gen_associated_ops,
        OpcodeId::SGT => StackOnlyOpcode::<2, 1>::gen_associated_ops,
        OpcodeId::EQ => StackOnlyOpcode::<2, 1>::gen_associated_ops,
        OpcodeId::ISZERO => StackOnlyOpcode::<1, 1>::gen_associated_ops,
        OpcodeId::AND => StackOnlyOpcode::<2, 1>::gen_associated_ops,
        OpcodeId::OR => StackOnlyOpcode::<2, 1>::gen_associated_ops,
        OpcodeId::XOR => StackOnlyOpcode::<2, 1>::gen_associated_ops,
        OpcodeId::NOT => StackOnlyOpcode::<1, 1>::gen_associated_ops,
        OpcodeId::BYTE => StackOnlyOpcode::<2, 1>::gen_associated_ops,
        OpcodeId::SHL => StackOnlyOpcode::<2, 1>::gen_associated_ops,
        OpcodeId::SHR => StackOnlyOpcode::<2, 1>::gen_associated_ops,
        OpcodeId::SAR => StackOnlyOpcode::<2, 1>::gen_associated_ops,
        OpcodeId::SHA3 => Sha3::gen_associated_ops,
        OpcodeId::ADDRESS => Address::gen_associated_ops,
        OpcodeId::BALANCE => Balance::gen_associated_ops,
        OpcodeId::ORIGIN => Origin::gen_associated_ops,
        OpcodeId::CALLER => Caller::gen_associated_ops,
        OpcodeId::CALLVALUE => Callvalue::gen_associated_ops,
        OpcodeId::CALLDATASIZE => Calldatasize::gen_associated_ops,
        OpcodeId::CALLDATALOAD => Calldataload::gen_associated_ops,
        OpcodeId::CALLDATACOPY => Calldatacopy::gen_associated_ops,
        OpcodeId::GASPRICE => GasPrice::gen_associated_ops,
        OpcodeId::CODECOPY => Codecopy::gen_associated_ops,
        OpcodeId::CODESIZE => Codesize::gen_associated_ops,
        OpcodeId::EXP => Exponentiation::gen_associated_ops,
        OpcodeId::EXTCODESIZE => Extcodesize::gen_associated_ops,
        OpcodeId::EXTCODECOPY => Extcodecopy::gen_associated_ops,
        OpcodeId::RETURNDATASIZE => Returndatasize::gen_associated_ops,
        OpcodeId::RETURNDATACOPY => Returndatacopy::gen_associated_ops,
        OpcodeId::EXTCODEHASH => Extcodehash::gen_associated_ops,
        OpcodeId::BLOCKHASH => Blockhash::gen_associated_ops,
        OpcodeId::COINBASE => StackOnlyOpcode::<0, 1>::gen_associated_ops,
        OpcodeId::TIMESTAMP => StackOnlyOpcode::<0, 1>::gen_associated_ops,
        OpcodeId::NUMBER => StackOnlyOpcode::<0, 1>::gen_associated_ops,
        OpcodeId::DIFFICULTY => StackOnlyOpcode::<0, 1>::gen_associated_ops,
        OpcodeId::GASLIMIT => StackOnlyOpcode::<0, 1>::gen_associated_ops,
        OpcodeId::CHAINID => StackOnlyOpcode::<0, 1>::gen_associated_ops,
        OpcodeId::SELFBALANCE => Selfbalance::gen_associated_ops,
        OpcodeId::BASEFEE => StackOnlyOpcode::<0, 1>::gen_associated_ops,
        OpcodeId::POP => StackOnlyOpcode::<1, 0>::gen_associated_ops,
        OpcodeId::MLOAD => Mload::gen_associated_ops,
        OpcodeId::MSTORE => Mstore::<false>::gen_associated_ops,
        OpcodeId::MSTORE8 => Mstore::<true>::gen_associated_ops,
        OpcodeId::SLOAD => Sload::gen_associated_ops,
        OpcodeId::SSTORE => Sstore::gen_associated_ops,
        OpcodeId::JUMP => StackOnlyOpcode::<1, 0>::gen_associated_ops,
        OpcodeId::JUMPI => StackOnlyOpcode::<2, 0>::gen_associated_ops,
        OpcodeId::PC => StackOnlyOpcode::<0, 1>::gen_associated_ops,
        OpcodeId::MSIZE => StackOnlyOpcode::<0, 1>::gen_associated_ops,
        OpcodeId::GAS => StackOnlyOpcode::<0, 1>::gen_associated_ops,
        OpcodeId::JUMPDEST => Dummy::gen_associated_ops,
        OpcodeId::DUP1 => Dup::<1>::gen_associated_ops,
        OpcodeId::DUP2 => Dup::<2>::gen_associated_ops,
        OpcodeId::DUP3 => Dup::<3>::gen_associated_ops,
        OpcodeId::DUP4 => Dup::<4>::gen_associated_ops,
        OpcodeId::DUP5 => Dup::<5>::gen_associated_ops,
        OpcodeId::DUP6 => Dup::<6>::gen_associated_ops,
        OpcodeId::DUP7 => Dup::<7>::gen_associated_ops,
        OpcodeId::DUP8 => Dup::<8>::gen_associated_ops,
        OpcodeId::DUP9 => Dup::<9>::gen_associated_ops,
        OpcodeId::DUP10 => Dup::<10>::gen_associated_ops,
        OpcodeId::DUP11 => Dup::<11>::gen_associated_ops,
        OpcodeId::DUP12 => Dup::<12>::gen_associated_ops,
        OpcodeId::DUP13 => Dup::<13>::gen_associated_ops,
        OpcodeId::DUP14 => Dup::<14>::gen_associated_ops,
        OpcodeId::DUP15 => Dup::<15>::gen_associated_ops,
        OpcodeId::DUP16 => Dup::<16>::gen_associated_ops,
        OpcodeId::SWAP1 => Swap::<1>::gen_associated_ops,
        OpcodeId::SWAP2 => Swap::<2>::gen_associated_ops,
        OpcodeId::SWAP3 => Swap::<3>::gen_associated_ops,
        OpcodeId::SWAP4 => Swap::<4>::gen_associated_ops,
        OpcodeId::SWAP5 => Swap::<5>::gen_associated_ops,
        OpcodeId::SWAP6 => Swap::<6>::gen_associated_ops,
        OpcodeId::SWAP7 => Swap::<7>::gen_associated_ops,
        OpcodeId::SWAP8 => Swap::<8>::gen_associated_ops,
        OpcodeId::SWAP9 => Swap::<9>::gen_associated_ops,
        OpcodeId::SWAP10 => Swap::<10>::gen_associated_ops,
        OpcodeId::SWAP11 => Swap::<11>::gen_associated_ops,
        OpcodeId::SWAP12 => Swap::<12>::gen_associated_ops,
        OpcodeId::SWAP13 => Swap::<13>::gen_associated_ops,
        OpcodeId::SWAP14 => Swap::<14>::gen_associated_ops,
        OpcodeId::SWAP15 => Swap::<15>::gen_associated_ops,
        OpcodeId::SWAP16 => Swap::<16>::gen_associated_ops,
        OpcodeId::LOG0 => Log::gen_associated_ops,
        OpcodeId::LOG1 => Log::gen_associated_ops,
        OpcodeId::LOG2 => Log::gen_associated_ops,
        OpcodeId::LOG3 => Log::gen_associated_ops,
        OpcodeId::LOG4 => Log::gen_associated_ops,
        OpcodeId::CALL | OpcodeId::CALLCODE => CallOpcode::<7>::gen_associated_ops,
        OpcodeId::DELEGATECALL | OpcodeId::STATICCALL => CallOpcode::<6>::gen_associated_ops,
        OpcodeId::CREATE => Create::<false>::gen_associated_ops,
        OpcodeId::CREATE2 => Create::<true>::gen_associated_ops,
        OpcodeId::RETURN | OpcodeId::REVERT => ReturnRevert::gen_associated_ops,
        OpcodeId::INVALID(_) => Stop::gen_associated_ops,
        OpcodeId::SELFDESTRUCT => {
            log::debug!("Using dummy gen_selfdestruct_ops for opcode SELFDESTRUCT");
            DummySelfDestruct::gen_associated_ops
        }
        _ => {
            log::debug!("Using dummy gen_associated_ops for opcode {:?}", opcode_id);
            Dummy::gen_associated_ops
        }
    }
}

fn fn_gen_error_state_associated_ops(
    geth_step: &GethExecStep,
    error: &ExecError,
) -> Option<FnGenAssociatedOps> {
    match error {
        ExecError::InvalidJump => Some(InvalidJump::gen_associated_ops),
        ExecError::InvalidOpcode => Some(StackOnlyOpcode::<0, 0, true>::gen_associated_ops),
        // Depth error could occur in CALL, CALLCODE, DELEGATECALL and STATICCALL.
        ExecError::Depth(DepthError::Call) => match geth_step.op {
            OpcodeId::CALL | OpcodeId::CALLCODE => Some(CallOpcode::<7>::gen_associated_ops),
            OpcodeId::DELEGATECALL | OpcodeId::STATICCALL => {
                Some(CallOpcode::<6>::gen_associated_ops)
            }
            op => unreachable!("ErrDepth cannot occur in {op}"),
        },
        // Depth error could occur in CREATE and CREATE2.
        ExecError::Depth(DepthError::Create) => Some(Create::<false>::gen_associated_ops),
        ExecError::Depth(DepthError::Create2) => Some(Create::<true>::gen_associated_ops),
        ExecError::OutOfGas(OogError::Call) => Some(OOGCall::gen_associated_ops),
        ExecError::OutOfGas(OogError::Constant) => {
            Some(StackOnlyOpcode::<0, 0, true>::gen_associated_ops)
        }
        ExecError::OutOfGas(OogError::Create) => match geth_step.op {
            OpcodeId::CREATE => Some(StackOnlyOpcode::<3, 0, true>::gen_associated_ops),
            OpcodeId::CREATE2 => Some(StackOnlyOpcode::<4, 0, true>::gen_associated_ops),
            op => unreachable!("OOG Create cannot occur in {op}"),
        },
        ExecError::OutOfGas(OogError::Log) => Some(ErrorOOGLog::gen_associated_ops),
        ExecError::OutOfGas(OogError::DynamicMemoryExpansion) => {
            Some(StackOnlyOpcode::<2, 0, true>::gen_associated_ops)
        }
        ExecError::OutOfGas(OogError::StaticMemoryExpansion) => {
            Some(StackOnlyOpcode::<1, 0, true>::gen_associated_ops)
        }
        ExecError::OutOfGas(OogError::Exp) => {
            Some(StackOnlyOpcode::<2, 0, true>::gen_associated_ops)
        }
        ExecError::OutOfGas(OogError::MemoryCopy) => Some(OOGMemoryCopy::gen_associated_ops),
        ExecError::OutOfGas(OogError::Sha3) => {
            Some(StackOnlyOpcode::<2, 0, true>::gen_associated_ops)
        }
        ExecError::OutOfGas(OogError::SloadSstore) => Some(OOGSloadSstore::gen_associated_ops),
        ExecError::OutOfGas(OogError::AccountAccess) => {
            Some(ErrorOOGAccountAccess::gen_associated_ops)
        }
        // ExecError::
        ExecError::StackOverflow => Some(StackOnlyOpcode::<0, 0, true>::gen_associated_ops),
        ExecError::StackUnderflow => Some(StackOnlyOpcode::<0, 0, true>::gen_associated_ops),
        ExecError::CodeStoreOutOfGas => Some(ErrorCodeStore::gen_associated_ops),
        ExecError::MaxCodeSizeExceeded => Some(ErrorCodeStore::gen_associated_ops),
        // call & callcode can encounter InsufficientBalance error, Use pop-7 generic CallOpcode
        ExecError::InsufficientBalance(InsufficientBalanceError::Call) => {
            Some(CallOpcode::<7>::gen_associated_ops)
        }
        // create & create2 can encounter insufficient balance.
        ExecError::InsufficientBalance(InsufficientBalanceError::Create) => {
            Some(Create::<false>::gen_associated_ops)
        }
        ExecError::InsufficientBalance(InsufficientBalanceError::Create2) => {
            Some(Create::<true>::gen_associated_ops)
        }
        ExecError::PrecompileFailed => Some(PrecompileFailed::gen_associated_ops),
        ExecError::WriteProtection => Some(ErrorWriteProtection::gen_associated_ops),
        ExecError::ReturnDataOutOfBounds => Some(ErrorReturnDataOutOfBound::gen_associated_ops),
        // create & create2 can encounter contract address collision.
        ExecError::ContractAddressCollision(ContractAddressCollisionError::Create) => {
            Some(Create::<false>::gen_associated_ops)
        }
        ExecError::ContractAddressCollision(ContractAddressCollisionError::Create2) => {
            Some(Create::<true>::gen_associated_ops)
        }
        // create & create2 can encounter nonce uint overflow.
        ExecError::NonceUintOverflow(NonceUintOverflowError::Create) => {
            Some(Create::<false>::gen_associated_ops)
        }
        ExecError::NonceUintOverflow(NonceUintOverflowError::Create2) => {
            Some(Create::<true>::gen_associated_ops)
        }
        ExecError::InvalidCreationCode => Some(ErrorCreationCode::gen_associated_ops),
        // more future errors place here
        _ => {
            evm_unimplemented!("TODO: error state {:?} not implemented", error);
            None
        }
    }
}

#[allow(clippy::collapsible_else_if)]
/// Generate the associated operations according to the particular
/// [`OpcodeId`].
pub fn gen_associated_ops(
    opcode_id: &OpcodeId,
    state: &mut CircuitInputStateRef,
    geth_steps: &[GethExecStep],
) -> Result<Vec<ExecStep>, Error> {
    /////////// check stack  ///////////
    debug_assert_eq!(
        state.call_ctx()?.stack.0[0..geth_steps[0].stack.0.len()],
        geth_steps[0].stack.0
    );

    /////////// check memory ///////////
    let check_level = if *CHECK_MEM_STRICT { 2 } else { 0 }; // 0: no check, 1: check and log error and fix, 2: check and assert_eq
    if check_level >= 1 {
        let memory_enabled = !geth_steps.iter().all(|s| s.memory.is_empty());
        if memory_enabled {
            #[allow(clippy::collapsible_else_if)]
            if state.call_ctx()?.memory != geth_steps[0].memory {
                log::error!(
                    "wrong mem before {:?}. len in state {}, len in step {}",
                    opcode_id,
                    &state.call_ctx()?.memory.len(),
                    &geth_steps[0].memory.len(),
                );
                log::error!("state mem {:?}", &state.call_ctx()?.memory);
                log::error!("step  mem {:?}", &geth_steps[0].memory);

                for i in 0..std::cmp::min(
                    state.call_ctx()?.memory.0.len(),
                    geth_steps[0].memory.0.len(),
                ) {
                    let state_mem = state.call_ctx()?.memory.0[i];
                    let step_mem = geth_steps[0].memory.0[i];
                    if state_mem != step_mem {
                        log::error!(
                            "diff at {}: state {:?} != step {:?}",
                            i,
                            state_mem,
                            step_mem
                        );
                    }
                }
                if check_level >= 2 {
                    panic!("mem wrong");
                }
                state.call_ctx_mut()?.memory = geth_steps[0].memory.clone();
            }
        }
    }

    // check if have error
    let geth_step = &geth_steps[0];
    let mut exec_step = state.new_step(geth_step)?;
    let next_step = if geth_steps.len() > 1 {
        Some(&geth_steps[1])
    } else {
        None
    };
    if let Some(exec_error) = state.get_step_err(geth_step, next_step).unwrap() {
        log::debug!(
            "geth error {:?} occurred in  {:?} at pc {:?}",
            exec_error,
            geth_step.op,
            geth_step.pc,
        );

        exec_step.error = Some(exec_error.clone());
        // TODO: after more error state handled, refactor all error handling in
        // fn_gen_error_state_associated_ops method
        // For exceptions that have been implemented
        if let Some(fn_gen_error_ops) = fn_gen_error_state_associated_ops(geth_step, &exec_error) {
            let mut steps = fn_gen_error_ops(state, geth_steps)?;
            if let Some(e) = &steps[0].error {
                debug_assert_eq!(&exec_error, e);
            }
            steps[0].error = Some(exec_error.clone());
            return Ok(steps);
        } else {
            // For exceptions that fail to enter next call context, we need
            // to restore call context of current caller
            let mut need_restore = true;

            // For exceptions that already enter next call context, but fail immediately
            // (e.g. Depth, InsufficientBalance), we still need to parse the call.
            if geth_step.op.is_call_or_create()
                && !matches!(exec_error, ExecError::OutOfGas(OogError::Create))
            {
                let call = state.parse_call(geth_step)?;
                state.push_call(call);
                need_restore = false;
            }

            state.handle_return(&mut [&mut exec_step], geth_steps, need_restore)?;
            return Ok(vec![exec_step]);
        }
    }
    // if no errors, continue as normal
    let fn_gen_associated_ops = fn_gen_associated_ops(opcode_id);
    fn_gen_associated_ops(state, geth_steps)
}

pub fn gen_begin_tx_ops(
    state: &mut CircuitInputStateRef,
    geth_trace: &GethExecTrace,
) -> Result<(), Error> {
    let mut exec_step = state.new_begin_tx_step();
    let call = state.call()?.clone();

    let caller_address = call.caller_address;

    if state.tx.tx_type.is_l1_msg() {
        // for l1 message, no need to add rw op, but we must check
        // caller for its existent status

        // notice the caller must existed after a l1msg tx, so we
        // create it here
        let caller_acc = state.sdb.get_account(&caller_address).1.clone();

        state.account_read(
            &mut exec_step,
            caller_address,
            AccountField::CodeHash,
            caller_acc.code_hash_read().to_word(),
        )?;

        if caller_acc.is_empty() {
            log::info!("create account for {:?} inside l1msg tx", caller_address);

            // notice the op is not reversible, since the nonce increasing is
            // inreversible
            state.account_write(
                &mut exec_step,
                caller_address,
                AccountField::CodeHash,
                caller_acc.code_hash.to_word(),
                Word::zero(),
            )?;

            #[cfg(feature = "scroll")]
            {
                state.account_write(
                    &mut exec_step,
                    caller_address,
                    AccountField::KeccakCodeHash,
                    caller_acc.keccak_code_hash.to_word(),
                    Word::zero(),
                )?;
            }
        }
    } else {
        // else, add 3 RW read operations for transaction L1 fee.
        gen_tx_l1_fee_ops(state, &mut exec_step)?;
    }

    log::trace!("write tx l1fee {}", state.tx.l1_fee());
    state.call_context_write(
        &mut exec_step,
        call.call_id,
        CallContextField::L1Fee,
        Word::from(state.tx.l1_fee()),
    )?;

    // the rw delta before is:
    // + for non-l1 msg tx: 3 (rw for fee oracle contrace)
    // + for scroll l1-msg tx:
    //   * caller existed: 1 (read codehash)
    //   * caller not existed: 3 (read codehash and create account)
    // + for non-scroll l1-msg tx:
    //   * caller existed: 1 (read codehash)
    //   * caller not existed: 2 (read codehash and create account)
    // * write l1fee call context

    for (field, value) in [
        (CallContextField::TxId, state.tx_ctx.id().into()),
        (
            CallContextField::RwCounterEndOfReversion,
            call.rw_counter_end_of_reversion.into(),
        ),
        (
            CallContextField::IsPersistent,
            (call.is_persistent as usize).into(),
        ),
        (CallContextField::IsSuccess, call.is_success.to_word()),
    ] {
        state.call_context_write(&mut exec_step, call.call_id, field, value)?;
    }

    // Increase caller's nonce
    let nonce_prev = state.sdb.get_nonce(&caller_address);
    //debug_assert!(nonce_prev <= state.tx.nonce);
    //while nonce_prev < state.tx.nonce {
    //    state.sdb.increase_nonce(&caller_address);
    //    nonce_prev = state.sdb.get_nonce(&caller_address);
    //    log::warn!("[debug] increase nonce to {}", nonce_prev);
    //}
    state.account_write(
        &mut exec_step,
        caller_address,
        AccountField::Nonce,
        (nonce_prev + 1).into(),
        nonce_prev.into(),
    )?;

    // Add precompile contract address to access list
    for address in 1..=9 {
        let address = eth_types::Address::from_low_u64_be(address);
        let is_warm_prev = !state.sdb.add_account_to_access_list(address);
        state.tx_accesslist_account_write(
            &mut exec_step,
            state.tx_ctx.id(),
            address,
            true,
            is_warm_prev,
        )?;
    }

    // Add caller, callee and coinbase (only for Shanghai) to access list.
    #[cfg(feature = "shanghai")]
    let accessed_addresses = [
        call.caller_address,
        call.address,
        state
            .block
            .headers
            .get(&state.tx.block_num)
            .unwrap()
            .coinbase,
    ];
    #[cfg(not(feature = "shanghai"))]
    let accessed_addresses = [call.caller_address, call.address];
    for address in accessed_addresses {
        let is_warm_prev = !state.sdb.add_account_to_access_list(address);
        state.tx_accesslist_account_write(
            &mut exec_step,
            state.tx_ctx.id(),
            address,
            true,
            is_warm_prev,
        )?;
    }

    // Calculate gas cost of init code only for EIP-3860 of Shanghai.
    #[cfg(feature = "shanghai")]
    let init_code_gas_cost = if state.tx.is_create() {
        (state.tx.input.len() as u64 + 31) / 32 * eth_types::evm_types::INIT_CODE_WORD_GAS
    } else {
        0
    };
    #[cfg(not(feature = "shanghai"))]
    let init_code_gas_cost = 0;

    // Calculate intrinsic gas cost
    let call_data_gas_cost = tx_data_gas_cost(&state.tx.input);
    let intrinsic_gas_cost = if state.tx.is_create() {
        GasCost::CREATION_TX.as_u64()
    } else {
        GasCost::TX.as_u64()
    } + call_data_gas_cost
        + init_code_gas_cost;
    log::trace!("intrinsic_gas_cost {intrinsic_gas_cost}, call_data_gas_cost {call_data_gas_cost}, init_code_gas_cost {init_code_gas_cost}, exec_step.gas_cost {:?}", exec_step.gas_cost);
    exec_step.gas_cost = GasCost(intrinsic_gas_cost);

    // Get code_hash of callee account
    let callee_account = &state.sdb.get_account(&call.address).1.clone();
    let is_precompile = is_precompiled(&call.address);
    let callee_exists = !callee_account.is_empty();
    if !callee_exists && call.value.is_zero() {
        // The account is empty (codehash and nonce be 0) while storage is non empty.
        // It is an impossible case for any real world scenario.
        // The "clear" helps with testool.
        state.sdb.get_account_mut(&call.address).1.storage.clear();
    }
    let account_code_hash = if callee_exists {
        callee_account.code_hash.to_word()
    } else {
        Word::zero()
    };
    // call_code is code being executed
    let call_code_hash = call.code_hash.to_word();
    if !state.tx.is_create() && !account_code_hash.is_zero() {
        debug_assert_eq!(account_code_hash, call_code_hash);
    }
    let account_code_hash_is_empty_or_zero =
        account_code_hash.is_zero() || account_code_hash == CodeDB::empty_code_hash().to_word();

    state.account_read(
        &mut exec_step,
        call.address,
        AccountField::CodeHash,
        account_code_hash,
    )?;

    if state.tx.is_create()
        && ((!account_code_hash_is_empty_or_zero) || !callee_account.nonce.is_zero())
    {
        unimplemented!(
            "deployment collision at {:?}, account {:?}",
            call.address,
            callee_account
        );
    }

    // Transfer with fee
    let fee = if state.tx.tx_type.is_l1_msg() {
        0.into()
    } else {
        state.tx.gas_price * state.tx.gas + state.tx_ctx.l1_fee
    };
    state.transfer_with_fee(
        &mut exec_step,
        call.caller_address,
        call.address,
        callee_exists,
        call.is_create(),
        call.value,
        Some(fee),
    )?;

    // In case of contract creation we wish to verify the correctness of the
    // contract's address (callee). This address is defined as:
    //
    // Keccak256(RLP([tx_caller, tx_nonce]))[12:]
    //
    // We feed the RLP-encoded bytes to the block's SHA3 inputs, which gets assigned
    // to the Keccak circuit, so that the BeginTxGadget can do a lookup to the
    // Keccak table and verify the contract address.
    if state.tx.is_create() {
        // 1. add RLP-bytes for contract address to keccak circuit.
        state.block.sha3_inputs.push({
            let mut stream = ethers_core::utils::rlp::RlpStream::new();
            stream.begin_list(2);
            stream.append(&caller_address);
            stream.append(&nonce_prev);
            stream.out().to_vec()
        });
        // 2. add init code to keccak circuit.
        let init_code = state.tx.input.as_slice();
        let length = init_code.len();
        state.block.sha3_inputs.push(init_code.to_vec());
        // 3. add init code to copy circuit.
        let code_hash = CodeDB::hash(init_code);
        let bytes = Bytecode::from(init_code.to_vec())
            .code
            .iter()
            .map(|element| (element.value, element.is_code, false))
            .collect::<Vec<(u8, bool, bool)>>();

        let rw_counter_start = state.block_ctx.rwc;
        state.push_copy(
            &mut exec_step,
            CopyEvent {
                src_addr: 0,
                src_addr_end: length as u64,
                src_type: CopyDataType::TxCalldata,
                src_id: NumberOrHash::Number(state.tx_ctx.id()),
                dst_addr: 0,
                dst_type: CopyDataType::Bytecode,
                dst_id: NumberOrHash::Hash(code_hash),
                log_id: None,
                rw_counter_start,
                copy_bytes: CopyBytes::new(bytes, None, None),
            },
        );
    }

    // There are 4 branches from here.
    match (
        call.is_create(),
        is_precompile,
        account_code_hash_is_empty_or_zero,
    ) {
        // 1. Creation transaction.
        (true, _, _) => {
            state.push_op_reversible(
                &mut exec_step,
                AccountOp {
                    address: call.address,
                    field: AccountField::Nonce,
                    value: 1.into(),
                    value_prev: 0.into(),
                },
            )?;
            for (field, value) in [
                (CallContextField::Depth, call.depth.into()),
                (
                    CallContextField::CallerAddress,
                    call.caller_address.to_word(),
                ),
                (
                    CallContextField::CalleeAddress,
                    get_contract_address(caller_address, nonce_prev).to_word(),
                ),
                (
                    CallContextField::CallDataOffset,
                    call.call_data_offset.into(),
                ),
                (CallContextField::CallDataLength, 0.into()),
                (CallContextField::Value, call.value),
                (CallContextField::IsStatic, (call.is_static as usize).into()),
                (CallContextField::LastCalleeId, 0.into()),
                (CallContextField::LastCalleeReturnDataOffset, 0.into()),
                (CallContextField::LastCalleeReturnDataLength, 0.into()),
                (CallContextField::IsRoot, 1.into()),
                (CallContextField::IsCreate, 1.into()),
                (CallContextField::CodeHash, call.code_hash.to_word()),
            ] {
                state.call_context_write(&mut exec_step, call.call_id, field, value)?;
            }
        }
        // 2. Call to precompiled.
        (_, true, _) => (),
        (_, _, is_empty_code_hash) => {
            // 3. Call to account with empty code (is_empty_code_hash == true).
            // 4. Call to account with non-empty code (is_empty_code_hash == false).
            if !is_empty_code_hash {
                for (field, value) in [
                    (CallContextField::Depth, call.depth.into()),
                    (
                        CallContextField::CallerAddress,
                        call.caller_address.to_word(),
                    ),
                    (CallContextField::CalleeAddress, call.address.to_word()),
                    (
                        CallContextField::CallDataOffset,
                        call.call_data_offset.into(),
                    ),
                    (
                        CallContextField::CallDataLength,
                        call.call_data_length.into(),
                    ),
                    (CallContextField::Value, call.value),
                    (CallContextField::IsStatic, (call.is_static as usize).into()),
                    (CallContextField::LastCalleeId, 0.into()),
                    (CallContextField::LastCalleeReturnDataOffset, 0.into()),
                    (CallContextField::LastCalleeReturnDataLength, 0.into()),
                    (CallContextField::IsRoot, 1.into()),
                    (CallContextField::IsCreate, call.is_create().to_word()),
                    (CallContextField::CodeHash, call_code_hash),
                ] {
                    state.call_context_write(&mut exec_step, call.call_id, field, value)?;
                }
            }
        }
    }

    let real_gas_cost = if geth_trace.struct_logs.is_empty() {
        GasCost(geth_trace.gas.0)
    } else {
        GasCost(state.tx.gas - geth_trace.struct_logs[0].gas.0)
    };
    if is_precompile {
        // FIXME after we implement all precompiles
        if exec_step.gas_cost != real_gas_cost {
            log::warn!(
                "change begin tx precompile gas from {:?} to {real_gas_cost:?}, step {exec_step:?}",
                exec_step.gas_cost
            );
            exec_step.gas_cost = real_gas_cost;
        }
    } else {
        // EIP2930 not implemented
        if state.tx.access_list.is_none() {
            debug_assert_eq!(exec_step.gas_cost, real_gas_cost);
        }
    }

    log::trace!("begin_tx_step: {:?}", exec_step);
    if is_precompile && !state.call().unwrap().is_success {
        state.handle_reversion(&mut [&mut exec_step]);
    }
    state.tx.steps_mut().push(exec_step);

    Ok(())
}

pub fn gen_end_tx_ops(state: &mut CircuitInputStateRef) -> Result<ExecStep, Error> {
    let mut exec_step = state.new_end_tx_step();
    let call = state.tx.calls()[0].clone();

    state.call_context_read(
        &mut exec_step,
        call.call_id,
        CallContextField::TxId,
        state.tx_ctx.id().into(),
    )?;
    state.call_context_read(
        &mut exec_step,
        call.call_id,
        CallContextField::IsPersistent,
        Word::from(call.is_persistent as u8),
    )?;
    state.call_context_read(
        &mut exec_step,
        call.call_id,
        CallContextField::L1Fee,
        Word::from(state.tx.l1_fee()),
    )?;

    let refund = state.sdb.refund();
    state.push_op(
        &mut exec_step,
        RW::READ,
        TxRefundOp {
            tx_id: state.tx_ctx.id(),
            value: refund,
            value_prev: refund,
        },
    )?;

    let effective_refund =
        refund.min((state.tx.gas - exec_step.gas_left.0) / MAX_REFUND_QUOTIENT_OF_GAS_USED as u64);
    let (found, caller_account) = state.sdb.get_account(&call.caller_address);
    if !found {
        return Err(Error::AccountNotFound(call.caller_address));
    }
    let caller_balance_prev = caller_account.balance;
    let effective_refund_balance = state.tx.gas_price * (exec_step.gas_left.0 + effective_refund);
    let caller_balance = caller_balance_prev + effective_refund_balance;

    if !state.tx.tx_type.is_l1_msg() {
        log::trace!(
            "call balance refund {:?}, now {:?}",
            effective_refund_balance,
            caller_balance
        );
        state.account_write(
            &mut exec_step,
            call.caller_address,
            AccountField::Balance,
            caller_balance,
            caller_balance_prev,
        )?;
    } else {
        log::trace!("l1 tx, no refund");
    }

    let block_info = state
        .block
        .headers
        .get(&state.tx.block_num)
        .unwrap()
        .clone();
    let effective_tip = state.tx.gas_price - block_info.base_fee;
    let gas_cost = state.tx.gas - exec_step.gas_left.0 - effective_refund;
    let coinbase_reward = if state.tx.tx_type.is_l1_msg() {
        Word::zero()
    } else {
        effective_tip * gas_cost + state.tx_ctx.l1_fee
    };
    log::trace!(
        "coinbase reward = ({} - {}) * ({} - {} - {}) = {} or 0 for l1 msg",
        state.tx.gas_price,
        block_info.base_fee,
        state.tx.gas,
        exec_step.gas_left.0,
        effective_refund,
        coinbase_reward
    );

    let (found, coinbase_account) = state.sdb.get_account_mut(&block_info.coinbase);
    if !found {
        log::error!("coinbase account not found: {}", block_info.coinbase);
        return Err(Error::AccountNotFound(block_info.coinbase));
    }
    let coinbase_account = coinbase_account.clone();
    state.account_read(
        &mut exec_step,
        block_info.coinbase,
        AccountField::CodeHash,
        if coinbase_account.is_empty() {
            Word::zero()
        } else {
            coinbase_account.code_hash.to_word()
        },
    )?;

    if !state.tx.tx_type.is_l1_msg() {
        state.transfer_to(
            &mut exec_step,
            block_info.coinbase,
            !coinbase_account.is_empty(),
            false,
            coinbase_reward,
            false,
        )?;
    }

    // handle tx receipt tag
    state.tx_receipt_write(
        &mut exec_step,
        state.tx_ctx.id(),
        TxReceiptField::PostStateOrStatus,
        call.is_persistent as u64,
    )?;

    let log_id = exec_step.log_id;
    state.tx_receipt_write(
        &mut exec_step,
        state.tx_ctx.id(),
        TxReceiptField::LogLength,
        log_id as u64,
    )?;

    if state.tx_ctx.id() > 1 {
        // query pre tx cumulative gas
        state.tx_receipt_read(
            &mut exec_step,
            state.tx_ctx.id() - 1,
            TxReceiptField::CumulativeGasUsed,
            state.block_ctx.cumulative_gas_used,
        )?;
    }

    state.block_ctx.cumulative_gas_used += state.tx.gas - exec_step.gas_left.0;
    state.tx_receipt_write(
        &mut exec_step,
        state.tx_ctx.id(),
        TxReceiptField::CumulativeGasUsed,
        state.block_ctx.cumulative_gas_used,
    )?;

    if !state.tx_ctx.is_last_tx() {
        state.call_context_write(
            &mut exec_step,
            state.block_ctx.rwc.0 + 1,
            CallContextField::TxId,
            (state.tx_ctx.id() + 1).into(),
        )?;
    }

    Ok(exec_step)
}

// Add 3 RW read operations for transaction L1 fee.
fn gen_tx_l1_fee_ops(
    state: &mut CircuitInputStateRef,
    exec_step: &mut ExecStep,
) -> Result<(), Error> {
    let tx_id = state.tx_ctx.id();

    let base_fee = Word::from(state.tx.l1_fee.base_fee);
    let fee_overhead = Word::from(state.tx.l1_fee.fee_overhead);
    let fee_scalar = Word::from(state.tx.l1_fee.fee_scalar);

    let base_fee_committed = Word::from(state.tx.l1_fee_committed.base_fee);
    let fee_overhead_committed = Word::from(state.tx.l1_fee_committed.fee_overhead);
    let fee_scalar_committed = Word::from(state.tx.l1_fee_committed.fee_scalar);

    state.push_op(
        exec_step,
        RW::READ,
        StorageOp::new(
            *l1_gas_price_oracle::ADDRESS,
            *l1_gas_price_oracle::BASE_FEE_SLOT,
            base_fee,
            base_fee,
            tx_id,
            base_fee_committed,
        ),
    )?;
    state.push_op(
        exec_step,
        RW::READ,
        StorageOp::new(
            *l1_gas_price_oracle::ADDRESS,
            *l1_gas_price_oracle::OVERHEAD_SLOT,
            fee_overhead,
            fee_overhead,
            tx_id,
            fee_overhead_committed,
        ),
    )?;
    state.push_op(
        exec_step,
        RW::READ,
        StorageOp::new(
            *l1_gas_price_oracle::ADDRESS,
            *l1_gas_price_oracle::SCALAR_SLOT,
            fee_scalar,
            fee_scalar,
            tx_id,
            fee_scalar_committed,
        ),
    )?;
    Ok(())
}

#[derive(Debug, Copy, Clone)]
struct DummySelfDestruct;

impl Opcode for DummySelfDestruct {
    fn gen_associated_ops(
        state: &mut CircuitInputStateRef,
        geth_steps: &[GethExecStep],
    ) -> Result<Vec<ExecStep>, Error> {
        dummy_gen_selfdestruct_ops(state, geth_steps)
    }
}
fn dummy_gen_selfdestruct_ops(
    state: &mut CircuitInputStateRef,
    geth_steps: &[GethExecStep],
) -> Result<Vec<ExecStep>, Error> {
    let geth_step = &geth_steps[0];
    let mut exec_step = state.new_step(geth_step)?;
    let sender = state.call()?.address;
    let receiver = geth_step.stack.last()?.to_address();

    let is_warm = state.sdb.check_account_in_access_list(&receiver);
    state.push_op_reversible(
        &mut exec_step,
        TxAccessListAccountOp {
            tx_id: state.tx_ctx.id(),
            address: receiver,
            is_warm: true,
            is_warm_prev: is_warm,
        },
    )?;

    let (found, receiver_account) = state.sdb.get_account(&receiver);
    if !found {
        return Err(Error::AccountNotFound(receiver));
    }
    let receiver_account = &receiver_account.clone();
    let (found, sender_account) = state.sdb.get_account(&sender);
    if !found {
        return Err(Error::AccountNotFound(sender));
    }
    let sender_account = &sender_account.clone();
    let value = sender_account.balance;
    log::trace!(
        "self destruct, sender {:?} receiver {:?} value {:?}",
        sender,
        receiver,
        value
    );
    // NOTE: In this dummy implementation we assume that the receiver already
    // exists.

    state.push_op_reversible(
        &mut exec_step,
        AccountOp {
            address: sender,
            field: AccountField::Balance,
            value: Word::zero(),
            value_prev: value,
        },
    )?;
    state.push_op_reversible(
        &mut exec_step,
        AccountOp {
            address: sender,
            field: AccountField::Nonce,
            value: Word::zero(),
            value_prev: sender_account.nonce,
        },
    )?;
    state.push_op_reversible(
        &mut exec_step,
        AccountOp {
            address: sender,
            field: AccountField::CodeHash,
            value: Word::zero(),
            value_prev: sender_account.code_hash.to_word(),
        },
    )?;
    if receiver != sender {
        state.transfer_to(
            &mut exec_step,
            receiver,
            !receiver_account.is_empty(),
            false,
            value,
            true,
        )?;
    }

    if state.call()?.is_persistent {
        state.sdb.destruct_account(sender);
    }

    if let Ok(caller) = state.caller_ctx_mut() {
        caller.return_data.clear();
    }
    state.handle_return(&mut [&mut exec_step], geth_steps, !state.call()?.is_root)?;
    Ok(vec![exec_step])
}
