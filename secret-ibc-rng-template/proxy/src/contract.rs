use cosmwasm_std::{
    // import necessary types and traits from cosmwasm_std
    entry_point,
    from_binary,
    to_binary,
    Binary,
    Deps,
    DepsMut,
    Env,
    Ibc3ChannelOpenResponse,
    IbcBasicResponse,
    IbcChannelCloseMsg,
    IbcChannelConnectMsg,
    IbcChannelOpenMsg,
    IbcChannelOpenResponse,
    IbcMsg,
    IbcPacketAckMsg,
    IbcPacketReceiveMsg,
    IbcPacketTimeoutMsg,
    IbcReceiveResponse,
    IbcTimeout,
    MessageInfo,
    Response,
    StdResult,
    WasmMsg,
};
use crate::msg::{CallbackInfo, ExecuteMsg, InstantiateMsg, PacketMsg, QueryMsg, RandomCallback, ResultBytes};
use crate::state::{load_callback, save_callback, Channel, StoredRandomAnswer};
// use crate::utils::verify_callback;
//use secret_toolkit_crypto::Prng;
use secret_toolkit_crypto::ContractPrng;
// Define a constant for the IBC app version
pub const IBC_APP_VERSION: &str = "ibc-v1";
// Define a constant for the packet lifetime in seconds
const PACKET_LIFETIME: u64 = 60 * 60;

// Instantiate entry point
#[entry_point]
pub fn instantiate(
    _deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    _msg: InstantiateMsg,
) -> StdResult<Response> {
    // Return a response with an attribute "init" containing the serialized last operation
    Ok(Response::default()
        .add_attribute("init", to_binary(&"Initialized".to_string())?.to_string()))
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    msg: ExecuteMsg,
) -> StdResult<Response> {
    match msg {
        ExecuteMsg::SendIbcPacket { message } => {
            let channel_id = Channel::get_last_opened(deps.storage)?;
            let packet = PacketMsg::Message {
                value: channel_id.clone() + &message,
            };

            // Create a new IBC message to send the packet
            Ok(Response::new().add_message(IbcMsg::SendPacket {
                channel_id,
                data: to_binary(&packet)?,
                timeout: IbcTimeout::with_timestamp(env.block.time.plus_seconds(PACKET_LIFETIME)),
            }))
        }

        ExecuteMsg::RequestRandom { job_id, callback } => {
            // Get the last opened channel ID
            let channel_id = Channel::get_last_opened(deps.storage)?;
            // Create a new packet message to request a random value
            let packet = PacketMsg::RequestRandom {
                job_id: job_id.clone(),
                length: None,
            };

            save_callback(deps.storage, callback)?;

            Ok(Response::new().add_message(IbcMsg::SendPacket {
                channel_id,
                data: to_binary(&packet)?,
                timeout: IbcTimeout::with_timestamp(env.block.time.plus_seconds(PACKET_LIFETIME)),
            }))
        }
    }
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::LastIbcOperation {} => Ok(to_binary(&"No operations".to_string())?),

        QueryMsg::ViewReceivedLifeAnswer {} => {
            Ok(to_binary(&StoredRandomAnswer::get(deps.storage)?)?)
        }
    }
}

#[entry_point]
pub fn ibc_channel_open(
    _deps: DepsMut,
    _env: Env,
    _msg: IbcChannelOpenMsg,
) -> StdResult<IbcChannelOpenResponse> {
    Ok(Some(Ibc3ChannelOpenResponse {
        version: IBC_APP_VERSION.to_string(),
    }))
}

#[entry_point]
pub fn ibc_channel_connect(
    deps: DepsMut,
    _env: Env,
    msg: IbcChannelConnectMsg,
) -> StdResult<IbcBasicResponse> {
    #[allow(unreachable_patterns)]
    match msg {
        IbcChannelConnectMsg::OpenAck { channel, .. } => {
            // save channel to state
            let channel_id = channel.endpoint.channel_id;
            Channel::save_last_opened(deps.storage, channel_id)?;
        }

        IbcChannelConnectMsg::OpenConfirm { channel } => {
            // save channel to state
            let channel_id = channel.endpoint.channel_id;
            Channel::save_last_opened(deps.storage, channel_id)?;
        }

        _ => {}
    };

    Ok(IbcBasicResponse::default())
}

#[entry_point]
pub fn ibc_packet_receive(
    deps: DepsMut,
    env: Env,
    msg: IbcPacketReceiveMsg,
) -> StdResult<IbcReceiveResponse> {
    let mut response = IbcReceiveResponse::new();

    let packet: PacketMsg = from_binary(&msg.packet.data)?;
    match packet {
        PacketMsg::Message { value } => {
            let res = PacketMsg::Message {
                value: format!("got your message: {}", value),
            };
            response = response.set_ack(to_binary(&res).unwrap());
        }

        // todo: return random with different lengths
        PacketMsg::RequestRandom { job_id, .. } => {
            deps.api.debug(&format!("{:?}", env));

            // todo: handle random not in block for some reason?
            let random = env.block.random.unwrap();

            let mut rng = ContractPrng::new(random.as_slice(), job_id.as_bytes());
            let rand_for_job = hex::encode(rng.rand_bytes());

            let res = PacketMsg::RandomResponse {
                random: rand_for_job,
                job_id,
            };

            let json_bytes = serde_json_wasm::to_vec(&res).unwrap();

            // Serialize the result_json into bytes
            let result_bytes = ResultBytes {
                result: (Binary(json_bytes))
            };

            response = response.set_ack(to_binary(&result_bytes).unwrap());
        }

        _ => {}
    }

    Ok(response)
}

#[entry_point]
pub fn ibc_packet_ack(
    deps: DepsMut,
    _env: Env,
    msg: IbcPacketAckMsg,
) -> StdResult<IbcBasicResponse> {
    let ack_data = from_binary(&msg.acknowledgement.data)?;
    match ack_data {
        PacketMsg::Message { .. } => Ok(IbcBasicResponse::default()),

        PacketMsg::RandomResponse { job_id, random } => {
            // todo: support more than 1 concurrent job
            let callback = load_callback(deps.storage)?;

            let msg = create_random_response_callback(callback, job_id, random)?;

            Ok(IbcBasicResponse::default().add_message(msg))
        }

        _ => Ok(IbcBasicResponse::default()),
    }
}

fn create_random_response_callback(
    callback: CallbackInfo,
    job_id: String,
    random: String,
) -> StdResult<WasmMsg> {
    Ok(WasmMsg::Execute {
        contract_addr: callback.contract.address.to_string(),
        code_hash: callback.contract.code_hash,
        msg: to_binary(&RandomCallback::Result {
            job_id,
            random,
            msg: callback.msg,
        })?,
        funds: vec![],
    })
}

#[entry_point]
pub fn ibc_channel_close(
    _deps: DepsMut,
    _env: Env,
    _msg: IbcChannelCloseMsg,
) -> StdResult<IbcBasicResponse> {
    Ok(IbcBasicResponse::default())
}

#[entry_point]
pub fn ibc_packet_timeout(
    _deps: DepsMut,
    _env: Env,
    _msg: IbcPacketTimeoutMsg,
) -> StdResult<IbcBasicResponse> {
    Ok(IbcBasicResponse::default())
}
