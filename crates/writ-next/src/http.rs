use std::sync::Arc;

use futures::{future::BoxFuture, FutureExt};
use gpui::http_client::{
    http::{HeaderValue, StatusCode},
    AsyncBody, HttpClient, Request, Response, Url,
};

pub struct Client;

impl Client {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl HttpClient for Client {
    fn type_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    fn user_agent(&self) -> Option<&HeaderValue> {
        None
    }

    fn proxy(&self) -> Option<&Url> {
        None
    }

    fn send(
        &self,
        req: Request<AsyncBody>,
    ) -> BoxFuture<'static, anyhow::Result<Response<AsyncBody>>> {
        let (parts, _body) = req.into_parts();
        let uri = parts.uri.to_string();

        // Use smol's blocking task spawner to run ureq in a thread pool
        smol::unblock(move || {
            let response = ureq::get(&uri).call()?;
            let status = response.status().as_u16();
            let bytes = response.into_body().read_to_vec()?;

            let async_body = AsyncBody::from_bytes(bytes.into());
            let mut http_response = Response::new(async_body);
            *http_response.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::OK);

            Ok(http_response)
        })
        .boxed()
    }
}
