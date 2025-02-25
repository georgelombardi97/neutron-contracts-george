use crate::bindings::msg::NeutronMsg;
use crate::bindings::query::InterchainQueries;
use crate::bindings::types::{KVKey, KVKeys};
use crate::errors::error::NeutronResult;
use crate::interchain_queries::helpers::{
    create_account_denom_balance_key, create_delegation_key, create_params_store_key,
    create_validator_key, decode_and_convert,
};
use crate::interchain_queries::types::{
    QueryType, TransactionFilterItem, TransactionFilterOp, TransactionFilterValue, BANK_STORE_KEY,
    HEIGHT_FIELD, KEY_BOND_DENOM, PARAMS_STORE_KEY, RECIPIENT_FIELD, STAKING_STORE_KEY,
};
use cosmwasm_std::{attr, Attribute, Binary, DepsMut, Env, Response, StdError};
use schemars::_serde_json::to_string;

#[allow(clippy::too_many_arguments)]
/// Registers an Interchain Query with provided params
fn register_interchain_query(
    _deps: DepsMut<InterchainQueries>,
    _env: Env,
    connection_id: String,
    zone_id: String,
    query_type: QueryType,
    kv_keys: Vec<KVKey>,
    transactions_filter: String,
    update_period: u64,
) -> NeutronResult<Response<NeutronMsg>> {
    let register_msg = NeutronMsg::register_interchain_query(
        query_type.into(),
        kv_keys.clone(),
        transactions_filter.clone(),
        zone_id.clone(),
        connection_id.clone(),
        update_period,
    );

    let mut attrs: Vec<Attribute> = vec![
        attr("action", "register_interchain_query"),
        attr("connection_id", connection_id.as_str()),
        attr("zone_id", zone_id.as_str()),
        attr("query_type", query_type),
        attr("update_period", update_period.to_string()),
    ];

    if !transactions_filter.is_empty() {
        attrs.push(attr("transactions_filter", transactions_filter.as_str()))
    }

    if !kv_keys.is_empty() {
        attrs.push(attr("kv_keys", KVKeys(kv_keys)))
    }

    Ok(Response::new()
        .add_message(register_msg)
        .add_attributes(attrs))
}

/// Registers an Interchain Query to get balance of account on remote chain for particular denom
///
/// * **connection_id** is an IBC connection identifier between Neutron and remote chain;
/// * **zone_id** is used to identify the chain of interest;
/// * **addr** address of an account on remote chain for which you want to get balances;
/// * **denom** denomination of the coin for which you want to get balance;
/// * **update_period** is used to say how often the query must be updated.
pub fn register_balance_query(
    deps: DepsMut<InterchainQueries>,
    env: Env,
    connection_id: String,
    zone_id: String,
    addr: String,
    denom: String,
    update_period: u64,
) -> NeutronResult<Response<NeutronMsg>> {
    let converted_addr_bytes = decode_and_convert(addr.as_str())?;

    let balance_key = create_account_denom_balance_key(converted_addr_bytes, denom)?;

    let kv_key = KVKey {
        path: BANK_STORE_KEY.to_string(),
        key: Binary(balance_key),
    };

    register_interchain_query(
        deps,
        env,
        connection_id,
        zone_id,
        QueryType::KV,
        vec![kv_key],
        String::new(),
        update_period,
    )
}

/// Registers an Interchain Query to get delegations of particular delegator on remote chain.
///
/// * **connection_id** is an IBC connection identifier between Neutron and remote chain;
/// * **zone_id** is used to identify the chain of interest;
/// * **delegator** is an address of an account on remote chain for which you want to get list of delegations;
/// * **validators** is a list of validators addresses for which you want to get delegations from particular **delegator**;
/// * **update_period** is used to say how often the query must be updated.
pub fn register_delegator_delegations_query(
    deps: DepsMut<InterchainQueries>,
    env: Env,
    connection_id: String,
    zone_id: String,
    delegator: String,
    validators: Vec<String>,
    update_period: u64,
) -> NeutronResult<Response<NeutronMsg>> {
    let delegator_addr = decode_and_convert(delegator.as_str())?;

    // Allocate memory for such KV keys as:
    // * staking module params to get staking denomination
    // * validators structures to calculate amount of delegated tokens
    // * delegations structures to get info about delegations itself
    let mut keys: Vec<KVKey> = Vec::with_capacity(validators.len() * 2 + 1);

    // create KV key to get BondDenom from staking module params
    keys.push(KVKey {
        path: PARAMS_STORE_KEY.to_string(),
        key: Binary(create_params_store_key(STAKING_STORE_KEY, KEY_BOND_DENOM)),
    });

    for v in &validators {
        // create delegation key to get delegation structure
        let val_addr = decode_and_convert(v.as_str())?;
        keys.push(KVKey {
            path: STAKING_STORE_KEY.to_string(),
            key: Binary(create_delegation_key(&delegator_addr, &val_addr)?),
        });

        // create validator key to get validator structure
        keys.push(KVKey {
            path: STAKING_STORE_KEY.to_string(),
            key: Binary(create_validator_key(&val_addr)?),
        })
    }

    register_interchain_query(
        deps,
        env,
        connection_id,
        zone_id,
        QueryType::KV,
        keys,
        String::new(),
        update_period,
    )
}

/// Registers an Interchain Query to get transfer events to a recipient on a remote chain.
///
/// * **connection_id** is an IBC connection identifier between Neutron and remote chain;
/// * **zone_id** is used to identify the chain of interest;
/// * **recipient** is an address of an account on remote chain for which you want to get list of transfer transactions;
/// * **update_period** is used to say how often the query must be updated.
/// * **min_height** is used to set min height for query (by default = 0).
pub fn register_transfers_query(
    deps: DepsMut<InterchainQueries>,
    env: Env,
    connection_id: String,
    zone_id: String,
    recipient: String,
    update_period: u64,
    min_height: Option<u128>,
) -> NeutronResult<Response<NeutronMsg>> {
    let mut query_data: Vec<TransactionFilterItem> = vec![TransactionFilterItem {
        field: RECIPIENT_FIELD.to_string(),
        op: TransactionFilterOp::Eq,
        value: TransactionFilterValue::String(recipient),
    }];
    if let Some(min_height) = min_height {
        query_data.push(TransactionFilterItem {
            field: HEIGHT_FIELD.to_string(),
            op: TransactionFilterOp::Gte,
            value: TransactionFilterValue::Int(min_height),
        })
    }
    let query_data_json_encoded =
        to_string(&query_data).map_err(|e| StdError::generic_err(e.to_string()))?;

    deps.api.debug(
        format!(
            "WASMDEBUG: query_data_json_encoded: {:?}",
            query_data_json_encoded,
        )
        .as_str(),
    );

    register_interchain_query(
        deps,
        env,
        connection_id,
        zone_id,
        QueryType::TX,
        vec![],
        query_data_json_encoded,
        update_period,
    )
}

/// Updates a registered Interchain Query.
/// Only the owner of the query can execute this message.
///
/// * **query_id** is an identifier of a registered Interchain Query;
/// * **new_keys** is a new Vec of KV keys you want your query to handle. Optional, if None, the old value stays the same;
/// * **new_update_period** is a new update period for your query. Optional, if None, the old value stays the same.
pub fn update_interchain_query(
    query_id: u64,
    new_keys: Option<Vec<KVKey>>,
    new_update_period: Option<u64>,
) -> NeutronResult<Response<NeutronMsg>> {
    let mut attributes = vec![
        attr("action", "update_interchain_query"),
        attr("query_id", query_id.to_string()),
    ];
    if let Some(keys) = new_keys.clone() {
        attributes.push(attr("new_keys", KVKeys(keys)))
    }
    if let Some(update_period) = new_update_period {
        attributes.push(attr("new_update_period", update_period.to_string()))
    }
    let update_msg = NeutronMsg::update_interchain_query(query_id, new_keys, new_update_period);
    Ok(Response::new()
        .add_message(update_msg)
        .add_attributes(attributes))
}

/// Removes a registered Interchain Query from the Interchain Queries Module.
/// Only the owner of the query can execute this message.
///
/// * **query_id** is an identifier of the query you want to remove.
pub fn remove_interchain_query(query_id: u64) -> NeutronResult<Response<NeutronMsg>> {
    let remove_msg = NeutronMsg::remove_interchain_query(query_id);
    Ok(Response::new().add_message(remove_msg).add_attributes(vec![
        attr("action", "remove_interchain_query"),
        attr("query_id", query_id.to_string()),
    ]))
}
