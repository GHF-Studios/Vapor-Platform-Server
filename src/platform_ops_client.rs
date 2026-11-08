use serde::de::DeserializeOwned;
use serde::Serialize;
#[cfg(unix)]
use std::time::Duration;

#[cfg(unix)]
const DEFAULT_SOCKET: &str = "/run/vapor-platform-ops.sock";
const MAX_REQUEST_BYTES: usize = 8192;
#[cfg(unix)]
const MAX_RESPONSE_BYTES: usize = 16 * 1024;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OperationResult {
    pub ok: bool,
    pub operation_id: String,
    pub state: String,
    pub detail: String,
}

pub(crate) async fn request<T, R>(payload: &T) -> Result<R, &'static str>
where
    T: Serialize,
    R: DeserializeOwned,
{
    let bytes = serde_json::to_vec(payload).map_err(|_| "could not encode operation")?;
    if bytes.is_empty() || bytes.len() > MAX_REQUEST_BYTES {
        return Err("operation request exceeds its size limit");
    }

    #[cfg(unix)]
    {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::UnixStream;

        let exchange = async {
            let mut stream = UnixStream::connect(DEFAULT_SOCKET)
                .await
                .map_err(|_| "operation service unavailable")?;
            stream
                .write_all(&bytes)
                .await
                .map_err(|_| "could not send operation")?;
            stream
                .shutdown()
                .await
                .map_err(|_| "could not finish operation request")?;
            let mut response = Vec::with_capacity(1024);
            stream
                .take((MAX_RESPONSE_BYTES + 1) as u64)
                .read_to_end(&mut response)
                .await
                .map_err(|_| "could not read operation result")?;
            if response.is_empty() || response.len() > MAX_RESPONSE_BYTES {
                return Err("operation result exceeds its size limit");
            }
            serde_json::from_slice(&response).map_err(|_| "operation service returned invalid data")
        };
        tokio::time::timeout(Duration::from_secs(12), exchange)
            .await
            .map_err(|_| "operation service timed out")?
    }

    #[cfg(not(unix))]
    {
        let _ = bytes;
        Err("host operations require the local Unix socket")
    }
}
