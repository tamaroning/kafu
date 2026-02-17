use thiserror::Error;

pub type KafuResult<T> = Result<T, KafuError>;

#[derive(Debug, Error)]
pub enum KafuError {
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Failed to instantiate WASM module: {0}")]
    WasmInstantiationError(anyhow::Error),

    #[error("Failed to run WASM module: {0}")]
    WasmExecutionError(anyhow::Error),

    #[error("Failed to migrate the Wasm module: {0}")]
    WasmMigrationError(anyhow::Error),

    #[error("Failed to connect to the gRPC server: {0}")]
    GrpcTransportError(tonic::transport::Error),

    #[error("Failed to send a request to the gRPC server: {0}")]
    GrpcClientError(tonic::Status),

    #[error("Health check failed for node '{node_id}' ({endpoint}): {reason}")]
    HealthCheckFailed {
        node_id: String,
        endpoint: String,
        reason: String,
    },
}

impl From<tonic::transport::Error> for KafuError {
    fn from(e: tonic::transport::Error) -> Self {
        KafuError::GrpcTransportError(e)
    }
}

impl From<tonic::Status> for KafuError {
    fn from(e: tonic::Status) -> Self {
        KafuError::GrpcClientError(e)
    }
}
