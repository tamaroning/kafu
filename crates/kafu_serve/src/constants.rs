/// Maximum message size for the gRPC server and client. (150MB)
/// This is large enough to handle large ONNX models and memory snapshots.
pub const MAX_MESSAGE_SIZE: usize = 1024 * 1024 * 150;

/// Health service name used to indicate the coordinator has started program execution.
///
/// Followers use this as a gate so they don't treat "leader not started yet" as a failure.
pub const LEADER_EXECUTION_HEALTH_SERVICE: &str = "kafu.leader_execution";
