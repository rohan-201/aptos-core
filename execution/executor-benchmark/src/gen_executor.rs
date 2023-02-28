// Copyright © Aptos Foundation
// Parts of the project are originally copyright © Meta Platforms, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{time::Duration, sync::atomic::AtomicUsize};
use aptos_types::transaction::{SignedTransaction, Transaction};
use aptos_types::account_address::AccountAddress;
use aptos_transaction_generator_lib::TransactionExecutor as GenInitTransactionExecutor;
use anyhow::Result;
use aptos_storage_interface::DbReaderWriter;
use async_trait::async_trait;
use std::sync::mpsc;
use crate::{benchmark_transaction::BenchmarkTransaction, db_access::{DbAccessUtil, CoinStore}};
use aptos_storage_interface::state_view::LatestDbStateCheckpointView;
use aptos_state_view::account_with_state_view::AsAccountWithStateView;
use aptos_types::account_view::AccountView;

pub struct DbGenInitTransactionExecutor {
    pub db: DbReaderWriter,
    pub block_sender: mpsc::SyncSender<Vec<BenchmarkTransaction>>,
}

#[async_trait]
impl GenInitTransactionExecutor for DbGenInitTransactionExecutor {
    async fn get_account_balance(&self, account_address: AccountAddress) -> Result<u64> {
        let db_state_view = self.db.reader.latest_state_checkpoint_view().unwrap();
        let sender_coin_store_key = DbAccessUtil::new_state_key_aptos_coin(account_address);
        let sender_coin_store =
            DbAccessUtil::get_db_value::<CoinStore>(&sender_coin_store_key, &db_state_view)?.unwrap();

        Ok(sender_coin_store.coin)
    }

    async fn query_sequence_number(&self, address: AccountAddress) -> Result<u64> {
        let db_state_view = self.db.reader.latest_state_checkpoint_view().unwrap();
        let address_account_view = db_state_view.as_account_with_state_view(&address);
        Ok(
            address_account_view
                .get_account_resource()
                .unwrap()
                .unwrap()
                .sequence_number()
        )
    }

    async fn execute_transactions_with_counter(
        &self,
        txns: &[SignedTransaction],
        _failure_counter: &[AtomicUsize],
    ) -> Result<()> {
        self.block_sender.send(txns.iter().map(|t| BenchmarkTransaction{transaction: Transaction::UserTransaction(t.clone()), extra_info: None}).collect())?;

        for txn in txns {
            while txn.sequence_number() > self.query_sequence_number(txn.sender()).await? {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
        Ok(())
    }
}
