// Initial prompt writes propagate errors; bounded steer writes remain unconfirmed on failure.
// Uses AsyncWrite so transport failure and timeout are tested without real model calls.
use std::time::Duration;
use tokio::io::{AsyncWrite, AsyncWriteExt};

pub(super) async fn initial<W: AsyncWrite + Unpin>(
    writer: &mut W,
    bytes: &[u8],
) -> std::io::Result<bool> {
    let result = async {
        writer.write_all(bytes).await?;
        writer.flush().await
    }
    .await;
    match result {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(false),
        Err(error) => Err(error),
    }
}

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

    struct Failed(std::io::ErrorKind);

    impl AsyncWrite for Failed {
        fn poll_write(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            _: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            Poll::Ready(Err(std::io::Error::new(
                self.0,
                "synthetic transport failure",
            )))
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
        assert!(
            !send(
                &mut Failed(std::io::ErrorKind::Other),
                b"steer",
                Duration::from_secs(1)
            )
            .await
        );
        let (mut blocked, _reader) = tokio::io::duplex(1);
        assert!(!send(&mut blocked, b"steer", Duration::from_millis(10)).await);
    }

    #[tokio::test]
    async fn initial_write_propagates_real_errors_and_preserves_broken_pipe_exit() {
        let error = initial(&mut Failed(std::io::ErrorKind::Other), b"prompt")
            .await
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::Other);
        assert!(
            !initial(&mut Failed(std::io::ErrorKind::BrokenPipe), b"prompt")
                .await
                .unwrap()
        );
    }
}
