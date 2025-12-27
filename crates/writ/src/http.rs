use std::sync::Arc;

use futures::{FutureExt, future::BoxFuture};
use gpui::http_client::{
    AsyncBody, HttpClient, Request, Response, Url,
    http::{HeaderValue, StatusCode},
};

pub struct Client {
    client: reqwest::Client,
}

impl Client {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            client: reqwest::Client::new(),
        })
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
        let client = self.client.clone();
        let (parts, _body) = req.into_parts();
        let uri = parts.uri.to_string();

        async move {
            let response = client.get(&uri).send().await?;
            let status = response.status();
            let bytes = response.bytes().await?;

            let async_body = AsyncBody::from_bytes(bytes);
            let mut http_response = Response::new(async_body);
            *http_response.status_mut() =
                StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::OK);

            Ok(http_response)
        }
        .boxed()
    }
}
