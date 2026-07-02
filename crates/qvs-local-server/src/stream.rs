use axum::{
    body::Body,
    response::{IntoResponse, Response},
};
use std::convert::Infallible;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

pub struct ChunkedStream {
    rx: mpsc::Receiver<Result<Vec<u8>, String>>,
}

impl ChunkedStream {
    #[must_use]
    pub fn new(rx: mpsc::Receiver<Result<Vec<u8>, String>>) -> Self {
        Self { rx }
    }
}

impl IntoResponse for ChunkedStream {
    fn into_response(self) -> Response {
        let stream = ReceiverStream::new(self.rx).map(|chunk| match chunk {
            Ok(data) => Ok::<_, Infallible>(data),
            Err(_) => Ok(Vec::new()),
        });
        Body::from_stream(stream).into_response()
    }
}
