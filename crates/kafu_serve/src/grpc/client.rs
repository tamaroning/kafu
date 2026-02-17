use std::time::Duration;

use tokio::time::timeout;
use tonic::transport::Endpoint;
use tonic_health::{
    ServingStatus,
    pb::{HealthCheckRequest, health_client::HealthClient},
};

use crate::grpc::kafu_proto::{
    CheckSnapshotCacheRequest, CheckSnapshotCacheResponse, HeartbeatRequest, HeartbeatResponse,
    MigrateRequest, MigrateResponse, ShutdownRequest, ShutdownResponse,
    command_client::CommandClient,
};
use crate::{
    constants::MAX_MESSAGE_SIZE,
    error::{KafuError, KafuResult},
};

const GRPC_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const GRPC_RPC_TIMEOUT: Duration = Duration::from_secs(10);

async fn connect_with_timeouts(endpoint: Endpoint) -> KafuResult<tonic::transport::Channel> {
    let endpoint = endpoint
        .connect_timeout(GRPC_CONNECT_TIMEOUT)
        .timeout(GRPC_RPC_TIMEOUT)
        .tcp_keepalive(Some(Duration::from_secs(30)));

    match timeout(GRPC_CONNECT_TIMEOUT, endpoint.connect()).await {
        Ok(Ok(channel)) => Ok(channel),
        Ok(Err(e)) => Err(KafuError::GrpcTransportError(e)),
        Err(_) => Err(KafuError::GrpcClientError(
            tonic::Status::deadline_exceeded("gRPC connect timeout"),
        )),
    }
}

pub async fn check_snapshot_cache(endpoint: Endpoint) -> KafuResult<CheckSnapshotCacheResponse> {
    let channel = connect_with_timeouts(endpoint).await?;
    let mut client = CommandClient::new(channel);
    let response = client
        .check_snapshot_cache(CheckSnapshotCacheRequest {})
        .await?;
    Ok(response.into_inner())
}

pub async fn send_migration_request(
    request: MigrateRequest,
    endpoint: Endpoint,
) -> KafuResult<MigrateResponse> {
    let channel = connect_with_timeouts(endpoint).await?;
    let mut client = CommandClient::new(channel)
        .max_decoding_message_size(MAX_MESSAGE_SIZE)
        .max_encoding_message_size(MAX_MESSAGE_SIZE);
    let response = client.migrate(request).await?;
    Ok(response.into_inner())
}

pub async fn send_shutdown_request(
    request: ShutdownRequest,
    endpoint: Endpoint,
) -> KafuResult<ShutdownResponse> {
    let channel = connect_with_timeouts(endpoint).await?;
    let mut client = CommandClient::new(channel);
    let response = client.shutdown(request).await?;
    Ok(response.into_inner())
}

pub async fn send_heartbeat(
    request: HeartbeatRequest,
    endpoint: Endpoint,
) -> KafuResult<HeartbeatResponse> {
    let channel = connect_with_timeouts(endpoint).await?;
    let mut client = CommandClient::new(channel);
    let response = client.heartbeat(request).await?;
    Ok(response.into_inner())
}

pub async fn health_check(endpoint: Endpoint) -> KafuResult<ServingStatus> {
    health_check_service(endpoint, "kafu.Command").await
}

pub async fn health_check_service(
    endpoint: Endpoint,
    service: impl AsRef<str>,
) -> KafuResult<ServingStatus> {
    let conn = connect_with_timeouts(endpoint).await?;
    let mut client = HealthClient::new(conn);
    let response = client
        .check(HealthCheckRequest {
            service: service.as_ref().to_string(),
        })
        .await?;
    match response.into_inner().status {
        0 => Ok(ServingStatus::Unknown),
        1 => Ok(ServingStatus::Serving),
        2 => Ok(ServingStatus::NotServing),
        _ => Err(KafuError::GrpcClientError(tonic::Status::invalid_argument(
            "Invalid status code returned from health check",
        ))),
    }
}
