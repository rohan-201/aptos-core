// Copyright (c) Aptos
// SPDX-License-Identifier: Apache-2.0

use better_any::{Tid, TidAble};
use move_binary_format::errors::PartialVMResult;
use move_core_types::gas_algebra::InternalGas;
use move_vm_runtime::native_functions::{NativeContext, NativeFunction};
use move_vm_types::{
    loaded_data::runtime_types::Type, natives::function::NativeResult, values::Value,
};
use smallvec::smallvec;
use std::collections::VecDeque;
use std::fmt::Debug;
use std::sync::Arc;

pub const NUM_BUCKETS: u128 = 10;

/// The native transaction context extension. This needs to be attached to the
/// NativeContextExtensions value which is passed into session functions, so its accessible from
/// natives of this extension.
#[derive(Tid)]
pub struct NativeTransactionContext {
    script_hash: Vec<u8>,
    txn_hash: u128,
    chain_id: u8,
}

impl NativeTransactionContext {
    /// Create a new instance of a native transaction context. This must be passed in via an
    /// extension into VM session functions.
    pub fn new(script_hash: Vec<u8>, txn_hash: u128, chain_id: u8) -> Self {
        Self {
            script_hash,
            txn_hash,
            chain_id,
        }
    }

    pub fn chain_id(&self) -> u8 {
        self.chain_id
    }
}

/***************************************************************************************************
 * native fun get_bucket
 *
 *   gas cost: base_cost
 *
 **************************************************************************************************/
#[derive(Clone, Debug)]
pub struct GetBucketGasParameters {
    pub base: InternalGas,
}

fn native_get_bucket(
    gas_params: &GetBucketGasParameters,
    context: &mut NativeContext,
    mut _ty_args: Vec<Type>,
    _args: VecDeque<Value>,
) -> PartialVMResult<NativeResult> {
    let transaction_context = context.extensions().get::<NativeTransactionContext>();

    let index = (transaction_context.txn_hash % NUM_BUCKETS) as u64;
    Ok(NativeResult::ok(
        gas_params.base,
        smallvec![Value::u64(index)],
    ))
}

pub fn make_native_get_bucket(gas_params: GetBucketGasParameters) -> NativeFunction {
    Arc::new(move |context, ty_args, args| native_get_bucket(&gas_params, context, ty_args, args))
}

/***************************************************************************************************
 * native fun get_script_hash
 *
 *   gas cost: base_cost
 *
 **************************************************************************************************/
#[derive(Clone, Debug)]
pub struct GetScriptHashGasParameters {
    pub base: InternalGas,
}

fn native_get_script_hash(
    gas_params: &GetScriptHashGasParameters,
    context: &mut NativeContext,
    mut _ty_args: Vec<Type>,
    _args: VecDeque<Value>,
) -> PartialVMResult<NativeResult> {
    let transaction_context = context.extensions().get::<NativeTransactionContext>();

    Ok(NativeResult::ok(
        gas_params.base,
        smallvec![Value::vector_u8(transaction_context.script_hash.clone())],
    ))
}

pub fn make_native_get_script_hash(gas_params: GetScriptHashGasParameters) -> NativeFunction {
    Arc::new(move |context, ty_args, args| {
        native_get_script_hash(&gas_params, context, ty_args, args)
    })
}

/***************************************************************************************************
 * module
 *
 **************************************************************************************************/
#[derive(Debug, Clone)]
pub struct GasParameters {
    pub get_script_hash: GetScriptHashGasParameters,
    pub get_bucket: GetBucketGasParameters,
}

pub fn make_all(gas_params: GasParameters) -> impl Iterator<Item = (String, NativeFunction)> {
    let natives = [
        (
            "get_script_hash",
            make_native_get_script_hash(gas_params.get_script_hash),
        ),
        ("get_bucket", make_native_get_bucket(gas_params.get_bucket)),
    ];

    crate::natives::helpers::make_module_natives(natives)
}
