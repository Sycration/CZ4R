use tokio::signal;
use tokio::task::AbortHandle;

pub async fn shutdown_signal(
    deletion_task_abort_handle: AbortHandle,
    backup_task_abort_handle: Option<AbortHandle>,
) {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
                deletion_task_abort_handle.abort();
                if let Some(h) = backup_task_abort_handle {
                    h.abort();
                }
        },
        _ = terminate => {
            deletion_task_abort_handle.abort();
            if let Some(h) = backup_task_abort_handle {
                h.abort();
            }
         },

    }
}
