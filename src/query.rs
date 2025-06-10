use serde::{Deserialize, Serialize};

#[derive(Serialize, Debug)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub id: u32,
    pub method: String,
    pub params: RpcParams,
}

#[derive(Serialize, Debug)]
pub struct RpcParams {
    pub path: String,
    pub data: String,
}

#[derive(Deserialize, Debug)]
pub struct RpcResponse {
    pub jsonrpc: String,
    pub id: u32,
    pub result: RpcResult,
}

#[derive(Deserialize, Debug)]
pub struct RpcResult {
    pub response: Response,
}

#[derive(Deserialize, Debug)]
pub struct Response {
    #[serde(rename = "ResponseBase")]
    pub response_base: ResponseBase,
}

#[derive(Deserialize, Debug)]
pub struct ResponseBase {
    #[serde(rename = "Error")]
    pub error: Option<serde_json::Value>,
    #[serde(rename = "Data")]
    pub data: String,
    #[serde(rename = "Log")]
    pub log: String,
}
