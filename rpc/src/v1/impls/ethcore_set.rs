// Copyright 2015, 2016 Ethcore (UK) Ltd.
// This file is part of Parity.

// Parity is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Parity is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Parity.  If not, see <http://www.gnu.org/licenses/>.

/// Ethcore-specific rpc interface for operations altering the settings.
use std::sync::{Arc, Weak};
use jsonrpc_core::*;
use ethcore::miner::MinerService;
use ethcore::client::MiningBlockChainClient;
use ethsync::ManageNetwork;
use v1::helpers::errors;
use v1::helpers::params::expect_no_params;
use v1::traits::EthcoreSet;
use v1::types::{Bytes, H160, U256};

/// Ethcore-specific rpc interface for operations altering the settings.
pub struct EthcoreSetClient<C, M> where
	C: MiningBlockChainClient,
	M: MinerService
{
	client: Weak<C>,
	miner: Weak<M>,
	net: Weak<ManageNetwork>,
}

impl<C, M> EthcoreSetClient<C, M> where
	C: MiningBlockChainClient,
	M: MinerService {
	/// Creates new `EthcoreSetClient`.
	pub fn new(client: &Arc<C>, miner: &Arc<M>, net: &Arc<ManageNetwork>) -> Self {
		EthcoreSetClient {
			client: Arc::downgrade(client),
			miner: Arc::downgrade(miner),
			net: Arc::downgrade(net),
		}
	}

	fn active(&self) -> Result<(), Error> {
		// TODO: only call every 30s at most.
		take_weak!(self.client).keep_alive();
		Ok(())
	}
}

impl<C, M> EthcoreSet for EthcoreSetClient<C, M> where
	C: MiningBlockChainClient + 'static,
	M: MinerService + 'static {

	fn set_min_gas_price(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());
		from_params::<(U256,)>(params).and_then(|(gas_price,)| {
			take_weak!(self.miner).set_minimal_gas_price(gas_price.into());
			Ok(to_value(&true))
		})
	}

	fn set_gas_floor_target(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());
		from_params::<(U256,)>(params).and_then(|(target,)| {
			take_weak!(self.miner).set_gas_floor_target(target.into());
			Ok(to_value(&true))
		})
	}

	fn set_gas_ceil_target(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());
		from_params::<(U256,)>(params).and_then(|(target,)| {
			take_weak!(self.miner).set_gas_ceil_target(target.into());
			Ok(to_value(&true))
		})
	}

	fn set_extra_data(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());
		from_params::<(Bytes,)>(params).and_then(|(extra_data,)| {
			take_weak!(self.miner).set_extra_data(extra_data.to_vec());
			Ok(to_value(&true))
		})
	}

	fn set_author(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());
		from_params::<(H160,)>(params).and_then(|(author,)| {
			take_weak!(self.miner).set_author(author.into());
			Ok(to_value(&true))
		})
	}

	fn set_transactions_limit(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());
		from_params::<(usize,)>(params).and_then(|(limit,)| {
			take_weak!(self.miner).set_transactions_limit(limit);
			Ok(to_value(&true))
		})
	}

	fn set_tx_gas_limit(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());
		from_params::<(U256,)>(params).and_then(|(limit,)| {
			take_weak!(self.miner).set_tx_gas_limit(limit.into());
			Ok(to_value(&true))
		})
	}

	fn add_reserved_peer(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());
		from_params::<(String,)>(params).and_then(|(peer,)| {
			match take_weak!(self.net).add_reserved_peer(peer) {
				Ok(()) => Ok(to_value(&true)),
				Err(e) => Err(errors::invalid_params("Peer address", e)),
			}
		})
	}

	fn remove_reserved_peer(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());
		from_params::<(String,)>(params).and_then(|(peer,)| {
			match take_weak!(self.net).remove_reserved_peer(peer) {
				Ok(()) => Ok(to_value(&true)),
				Err(e) => Err(errors::invalid_params("Peer address", e)),
			}
		})
	}

	fn drop_non_reserved_peers(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());
		try!(expect_no_params(params));
		take_weak!(self.net).deny_unreserved_peers();
		Ok(to_value(&true))
	}

	fn accept_non_reserved_peers(&self, params: Params) -> Result<Value, Error> {
		try!(self.active());
		try!(expect_no_params(params));
		take_weak!(self.net).accept_unreserved_peers();
		Ok(to_value(&true))
	}

	fn start_network(&self, params: Params) -> Result<Value, Error> {
		try!(expect_no_params(params));
		take_weak!(self.net).start_network();
		Ok(Value::Bool(true))
	}

	fn stop_network(&self, params: Params) -> Result<Value, Error> {
		try!(expect_no_params(params));
		take_weak!(self.net).stop_network();
		Ok(Value::Bool(true))
	}
}
