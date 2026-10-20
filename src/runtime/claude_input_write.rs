// Bounded streaming input writes; uncertain delivery never terminates the agent.
// Uses AsyncWrite so transport failure and timeout are tested without real model calls.
use std::time::Duration;
use tokio::io::{AsyncWrite, AsyncWriteExt};

pub(super) async fn send<W: AsyncWrite + Unpin>(
    writer: &mut W,
    bytes: &[u8],
    limit: Duration,
) -> bool {
    matches!(
        tokio::time::timeout(limit, async {
            writer.write_all(bytes).await?;
            writer.flush().await
        })
        .await,
        Ok(Ok(()))
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    struct Failed;

    impl AsyncWrite for Failed {
        fn poll_write(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            _: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            Poll::Ready(Err(std::io::Error::other("synthetic transport failure")))
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn non_pipe_write_errors_and_timeouts_are_unconfirmed() {
        assert!(!send(&mut Failed, b"steer", Duration::from_secs(1)).await);
        let (mut blocked, _reader) = tokio::io::duplex(1);
        assert!(!send(&mut blocked, b"steer", Duration::from_millis(10)).await);
    }
}
