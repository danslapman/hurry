use tower_lsp::{
    Client,
    lsp_types::{
        notification::Progress,
        request::WorkDoneProgressCreate,
        ProgressParams, ProgressParamsValue, ProgressToken,
        WorkDoneProgress, WorkDoneProgressBegin, WorkDoneProgressCreateParams,
        WorkDoneProgressEnd, WorkDoneProgressReport,
    },
};

/// Request the client to create a progress token, then send a Begin notification.
/// Ignores errors — clients that don't support work-done progress are unaffected.
/// Does nothing when `client` is `None` (e.g. standalone CLI mode).
pub async fn begin(client: Option<&Client>, token: &str, title: &str, message: Option<String>) {
    let Some(client) = client else { return };
    let tok = ProgressToken::String(token.to_string());
    let _ = client
        .send_request::<WorkDoneProgressCreate>(WorkDoneProgressCreateParams { token: tok.clone() })
        .await;
    client
        .send_notification::<Progress>(ProgressParams {
            token: tok,
            value: ProgressParamsValue::WorkDone(WorkDoneProgress::Begin(WorkDoneProgressBegin {
                title: title.to_string(),
                cancellable: Some(false),
                message,
                percentage: None,
            })),
        })
        .await;
}

/// Send a Report notification for an in-progress operation.
/// Does nothing when `client` is `None`.
pub async fn report(client: Option<&Client>, token: &str, message: String) {
    let Some(client) = client else { return };
    client
        .send_notification::<Progress>(ProgressParams {
            token: ProgressToken::String(token.to_string()),
            value: ProgressParamsValue::WorkDone(WorkDoneProgress::Report(
                WorkDoneProgressReport {
                    cancellable: Some(false),
                    message: Some(message),
                    percentage: None,
                },
            )),
        })
        .await;
}

/// Send an End notification, finishing the progress indicator.
/// Does nothing when `client` is `None`.
pub async fn end(client: Option<&Client>, token: &str, message: Option<String>) {
    let Some(client) = client else { return };
    client
        .send_notification::<Progress>(ProgressParams {
            token: ProgressToken::String(token.to_string()),
            value: ProgressParamsValue::WorkDone(WorkDoneProgress::End(WorkDoneProgressEnd {
                message,
            })),
        })
        .await;
}
