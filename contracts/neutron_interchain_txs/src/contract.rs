// Copyright 2022 Neutron
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use cosmos_sdk_proto::cosmos::base::v1beta1::Coin;
use cosmos_sdk_proto::cosmos::staking::v1beta1::{
    MsgDelegate, MsgDelegateResponse, MsgUndelegate, MsgUndelegateResponse,
};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_binary, Binary, CosmosMsg, CustomQuery, Deps, DepsMut, Env, MessageInfo, Reply, Response,
    StdError, StdResult, SubMsg,
};
use osmosis_std::types::osmosis::gamm::v1beta1::{MsgSwapExactAmountIn, SwapAmountInRoute};
use osmosis_std::types::cosmos::base::v1beta1::{Coin as OsmoCoin}
use prost::Message;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::msg::{ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg};
use neutron_sdk::bindings::msg::NeutronMsg;
use neutron_sdk::bindings::query::{InterchainQueries, QueryInterchainAccountAddressResponse};
use neutron_sdk::bindings::types::ProtobufAny;
use neutron_sdk::interchain_txs::helpers::{
    get_port_id, parse_item, parse_response, parse_sequence,
};
use neutron_sdk::sudo::msg::{RequestPacket, SudoMsg};
use neutron_sdk::NeutronResult;

use crate::storage::{
    read_reply_payload, read_sudo_payload, save_reply_payload, save_sudo_payload,
    AcknowledgementResult, SudoPayload, ACKNOWLEDGEMENT_RESULTS, INTERCHAIN_ACCOUNTS,
    SUDO_PAYLOAD_REPLY_ID,
};

// Default timeout for SubmitTX is two weeks
const DEFAULT_TIMEOUT_SECONDS: u64 = 60 * 60 * 24 * 7 * 2;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
struct OpenAckVersion {
    version: String,
    controller_connection_id: String,
    host_connection_id: String,
    address: String,
    encoding: String,
    tx_type: String,
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    _deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    _msg: InstantiateMsg,
) -> NeutronResult<Response<NeutronMsg>> {
    Ok(Response::default())
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    _: MessageInfo,
    msg: ExecuteMsg,
) -> StdResult<Response<NeutronMsg>> {
    deps.api
        .debug(format!("WASMDEBUG: execute: received msg: {:?}", msg).as_str());
    match msg {
        ExecuteMsg::Register {
            connection_id,
            interchain_account_id,
        } => execute_register_ica(deps, env, connection_id, interchain_account_id),
        ExecuteMsg::Delegate {
            validator,
            interchain_account_id,
            amount,
            timeout,
        } => execute_delegate(deps, env, interchain_account_id, validator, amount, timeout),
        ExecuteMsg::Undelegate {
            validator,
            interchain_account_id,
            amount,
            timeout,
        } => execute_undelegate(deps, env, interchain_account_id, validator, amount, timeout),
        ExecuteMsg::CleanAckResults {} => execute_clean_ack_results(deps),
        ExecuteMsg::Swap {
            routes,
            connection_id,
            interchain_account_id,
            token_in,
            token_in_amount
        } => execute_swap(routes,
                          connection_id,
                          interchain_account_id,
                          token_in,
                          token_in_amount),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps<InterchainQueries>, env: Env, msg: QueryMsg) -> NeutronResult<Binary> {
    match msg {
        QueryMsg::InterchainAccountAddress {
            interchain_account_id,
            connection_id,
        } => query_interchain_address(deps, env, interchain_account_id, connection_id),
        QueryMsg::InterchainAccountAddressFromContract {
            interchain_account_id,
        } => query_interchain_address_contract(deps, env, interchain_account_id),
        QueryMsg::AcknowledgementResult {
            interchain_account_id,
        } => query_acknowledgement_result(deps, env, interchain_account_id),
    }
}

pub fn query_interchain_address(
    deps: Deps<InterchainQueries>,
    env: Env,
    interchain_account_id: String,
    connection_id: String,
) -> NeutronResult<Binary> {
    let query = InterchainQueries::InterchainAccountAddress {
        owner_address: env.contract.address.to_string(),
        interchain_account_id,
        connection_id,
    };

    let res: QueryInterchainAccountAddressResponse = deps.querier.query(&query.into())?;
    Ok(to_binary(&res)?)
}

pub fn query_interchain_address_contract(
    deps: Deps<InterchainQueries>,
    env: Env,
    interchain_account_id: String,
) -> NeutronResult<Binary> {
    Ok(to_binary(&get_ica(deps, &env, &interchain_account_id)?)?)
}

pub fn query_acknowledgement_result(
    deps: Deps<InterchainQueries>,
    env: Env,
    interchain_account_id: String,
) -> NeutronResult<Binary> {
    let key = get_port_id(env.contract.address.as_str(), &interchain_account_id);
    let res = ACKNOWLEDGEMENT_RESULTS.may_load(deps.storage, key)?;
    Ok(to_binary(&res)?)
}

fn msg_with_sudo_callback<C: Into<CosmosMsg<T>>, T>(
    deps: DepsMut,
    msg: C,
    payload: SudoPayload,
) -> StdResult<SubMsg<T>> {
    save_reply_payload(deps.storage, payload)?;
    Ok(SubMsg::reply_on_success(msg, SUDO_PAYLOAD_REPLY_ID))
}

fn execute_register_ica(
    deps: DepsMut,
    env: Env,
    connection_id: String,
    interchain_account_id: String,
) -> StdResult<Response<NeutronMsg>> {
    let register =
        NeutronMsg::register_interchain_account(connection_id, interchain_account_id.clone());
    let key = get_port_id(env.contract.address.as_str(), &interchain_account_id);
    INTERCHAIN_ACCOUNTS.save(deps.storage, key, &None)?;
    Ok(Response::new().add_message(register))
}

fn execute_delegate(
    mut deps: DepsMut,
    env: Env,
    interchain_account_id: String,
    validator: String,
    amount: u128,
    timeout: Option<u64>,
) -> StdResult<Response<NeutronMsg>> {
    let (delegator, connection_id) = get_ica(deps.as_ref(), &env, &interchain_account_id)?;
    let delegate_msg = MsgDelegate {
        delegator_address: delegator,
        validator_address: validator,
        amount: Some(Coin {
            denom: "stake".to_string(),
            amount: amount.to_string(),
        }),
    };
    let mut buf = Vec::new();
    buf.reserve(delegate_msg.encoded_len());

    if let Err(e) = delegate_msg.encode(&mut buf) {
        return Err(StdError::generic_err(format!("Encode error: {}", e)));
    }

    let any_msg = ProtobufAny {
        type_url: "/cosmos.staking.v1beta1.MsgDelegate".to_string(),
        value: Binary::from(buf),
    };

    let cosmos_msg = NeutronMsg::submit_tx(
        connection_id,
        interchain_account_id.clone(),
        vec![any_msg],
        "".to_string(),
        timeout.unwrap_or(DEFAULT_TIMEOUT_SECONDS),
    );

    let submsg = msg_with_sudo_callback(
        deps.branch(),
        cosmos_msg,
        SudoPayload {
            port_id: get_port_id(env.contract.address.as_str(), &interchain_account_id),
            message: "message".to_string(),
        },
    )?;

    Ok(Response::default().add_submessages(vec![submsg]))
}

fn execute_swap(
    routes: Vec<SwapAmountInRoute>,
    connection_id: String,
    interchain_account_id: String,
    token_in: String,
    token_in_amount: String,
) -> StdResult<Response<NeutronMsg>> {
    let swap_message = MsgSwapExactAmountIn {
        // NOTE: sender should not be user! because contract cannot swap from user account
        //       it can only swap from contract account, we should later send it back to user if needed.
        // sender: info.sender.to_string(),
        sender: _env.contract.address.to_string(),
        routes,
        token_in: Some(OsmoCoin { denom: token_in.clone(), amount: token_in_amount.into() }),
        token_out_min_amount: "1".to_string(),
    };

    let mut buf = Vec::new();
    buf.reserve(Message::encoded_len(&swap_message));
    if let Err(e) = swap_message.encode(&mut buf) {
        return Err(StdError::generic_err(format!("Encode error: {}", e)));
    }

    let cosmos_msg_swap = ProtobufAny {
        type_url: "/osmosis.gamm.v1beta1.MsgSwapExactAmountIn".to_string(),
        value: Binary::from(buf),
    };

    let cosmos_msg = NeutronMsg::submit_tx(
        connection_id,
        interchain_account_id.clone(),
        vec![cosmos_msg_swap],
        "".to_string(),
        timeout.unwrap_or(DEFAULT_TIMEOUT_SECONDS),
    );

    let submsg = SubMsg::new(cosmos_msg);

    Ok(Response::default().add_submessages(vec![submsg]))
}

fn execute_undelegate(
    mut deps: DepsMut,
    env: Env,
    interchain_account_id: String,
    validator: String,
    amount: u128,
    timeout: Option<u64>,
) -> StdResult<Response<NeutronMsg>> {
    let (delegator, connection_id) = get_ica(deps.as_ref(), &env, &interchain_account_id)?;
    let delegate_msg = MsgUndelegate {
        delegator_address: delegator,
        validator_address: validator,
        amount: Some(Coin {
            denom: "stake".to_string(),
            amount: amount.to_string(),
        }),
    };

    let any_msg = ProtobufAny {
        type_url: "/cosmos.staking.v1beta1.MsgUndelegate".to_string(),
        value: Binary::from(buf),
    };

    let cosmos_msg = NeutronMsg::submit_tx(
        connection_id,
        interchain_account_id.clone(),
        vec![any_msg],
        "".to_string(),
        timeout.unwrap_or(DEFAULT_TIMEOUT_SECONDS),
    );

    let submsg = msg_with_sudo_callback(
        deps.branch(),
        cosmos_msg,
        SudoPayload {
            port_id: get_port_id(env.contract.address.as_str(), &interchain_account_id),
            message: "message".to_string(),
        },
    )?;

    Ok(Response::default().add_submessages(vec![submsg]))
}

fn execute_clean_ack_results(deps: DepsMut) -> StdResult<Response<NeutronMsg>> {
    let keys: Vec<StdResult<String>> = ACKNOWLEDGEMENT_RESULTS
        .keys(deps.storage, None, None, cosmwasm_std::Order::Descending)
        .collect();
    for key in keys {
        ACKNOWLEDGEMENT_RESULTS.remove(deps.storage, key?);
    }
    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn sudo(deps: DepsMut, env: Env, msg: SudoMsg) -> StdResult<Response> {
    match msg {
        SudoMsg::Response { request, data } => sudo_response(deps, request, data),
        SudoMsg::Error { request, details } => sudo_error(deps, request, details),
        SudoMsg::Timeout { request } => sudo_timeout(deps, env, request),
        SudoMsg::OpenAck {
            port_id,
            channel_id,
            counterparty_channel_id,
            counterparty_version,
        } => sudo_open_ack(
            deps,
            env,
            port_id,
            channel_id,
            counterparty_channel_id,
            counterparty_version,
        ),
        _ => Ok(Response::default()),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> StdResult<Response> {
    Ok(Response::default())
}

fn sudo_open_ack(
    deps: DepsMut,
    _env: Env,
    port_id: String,
    _channel_id: String,
    _counterparty_channel_id: String,
    counterparty_version: String,
) -> StdResult<Response> {
    let parsed_version: Result<OpenAckVersion, _> =
        serde_json_wasm::from_str(counterparty_version.as_str());
    if let Ok(parsed_version) = parsed_version {
        INTERCHAIN_ACCOUNTS.save(
            deps.storage,
            port_id,
            &Some((
                parsed_version.address,
                parsed_version.controller_connection_id,
            )),
        )?;
        return Ok(Response::default());
    }
    Err(StdError::generic_err("Can't parse counterparty_version"))
}

fn sudo_response(deps: DepsMut, request: RequestPacket, data: Binary) -> StdResult<Response> {
    deps.api.debug(
        format!(
            "WASMDEBUG: sudo_response: sudo received: {:?} {:?}",
            request, data
        )
            .as_str(),
    );
    let seq_id = request
        .sequence
        .ok_or_else(|| StdError::generic_err("sequence not found"))?;
    let channel_id = request
        .source_channel
        .ok_or_else(|| StdError::generic_err("channel_id not found"))?;
    let payload = read_sudo_payload(deps.storage, channel_id, seq_id)?;
    deps.api
        .debug(format!("WASMDEBUG: sudo_response: sudo payload: {:?}", payload).as_str());

    // handle response
    let parsed_data = parse_response(data)?;

    let mut item_types = vec![];
    for item in parsed_data {
        let item_type = item.msg_type.as_str();
        item_types.push(item_type.to_string());
        match item_type {
            "/cosmos.staking.v1beta1.MsgUndelegate" => {
                let out: MsgUndelegateResponse = parse_item(&item.data)?;
                let completion_time = out
                    .completion_time
                    .ok_or_else(|| StdError::generic_err("failed to get completion time"))?;
                deps.api
                    .debug(format!("Undelegation completion time: {:?}", completion_time).as_str());
            }
            "/cosmos.staking.v1beta1.MsgDelegate" => {
                let _out: MsgDelegateResponse = parse_item(&item.data)?;
            }
            _ => {
                deps.api.debug(
                    format!(
                        "This type of acknowledgement is not implemented: {:?}",
                        payload
                    )
                        .as_str(),
                );
            }
        }
    }

    ACKNOWLEDGEMENT_RESULTS.save(
        deps.storage,
        payload.port_id,
        &AcknowledgementResult::Success(item_types),
    )?;

    Ok(Response::default())
}

fn sudo_timeout(deps: DepsMut, _env: Env, request: RequestPacket) -> StdResult<Response> {
    deps.api
        .debug(format!("WASMDEBUG: sudo timeout request: {:?}", request).as_str());

    let seq_id = request
        .sequence
        .ok_or_else(|| StdError::generic_err("sequence not found"))?;
    let channel_id = request
        .source_channel
        .ok_or_else(|| StdError::generic_err("channel_id not found"))?;
    let payload = read_sudo_payload(deps.storage, channel_id, seq_id)?;

    ACKNOWLEDGEMENT_RESULTS.save(
        deps.storage,
        payload.port_id,
        &AcknowledgementResult::Timeout(payload.message),
    )?;

    Ok(Response::default())
}

fn sudo_error(deps: DepsMut, request: RequestPacket, details: String) -> StdResult<Response> {
    deps.api
        .debug(format!("WASMDEBUG: sudo error: {}", details).as_str());
    let seq_id = request
        .sequence
        .ok_or_else(|| StdError::generic_err("sequence not found"))?;
    let channel_id = request
        .source_channel
        .ok_or_else(|| StdError::generic_err("channel_id not found"))?;
    let payload = read_sudo_payload(deps.storage, channel_id, seq_id)?;

    ACKNOWLEDGEMENT_RESULTS.save(
        deps.storage,
        payload.port_id,
        &AcknowledgementResult::Error((payload.message, details)),
    )?;

    Ok(Response::default())
}

fn prepare_sudo_payload(mut deps: DepsMut, _env: Env, msg: Reply) -> StdResult<Response> {
    let payload = read_reply_payload(deps.storage)?;
    let (channel_id, seq_id) = parse_sequence(deps.as_ref(), msg)?;
    save_sudo_payload(deps.branch().storage, channel_id, seq_id, payload)?;
    Ok(Response::new())
}

fn get_ica(
    deps: Deps<impl CustomQuery>,
    env: &Env,
    interchain_account_id: &str,
) -> Result<(String, String), StdError> {
    let key = get_port_id(env.contract.address.as_str(), interchain_account_id);

    INTERCHAIN_ACCOUNTS
        .load(deps.storage, key)?
        .ok_or_else(|| StdError::generic_err("Interchain account is not created yet"))
}

#[entry_point]
pub fn reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    match msg.id {
        SUDO_PAYLOAD_REPLY_ID => prepare_sudo_payload(deps, env, msg),
        _ => Err(StdError::generic_err(format!(
            "unsupported reply message id {}",
            msg.id
        ))),
    }
}
