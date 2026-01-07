use std::sync::Arc;

use async_compat::CompatExt;
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
        let (parts, _body) = req.into_parts();
        let uri = parts.uri.to_string();
        let client = self.client.clone();

        async move {
            let response = client.get(&uri).send().compat().await?;
            let status = response.status().as_u16();
            let bytes = response.bytes().compat().await?;

            let async_body = AsyncBody::from_bytes(bytes);
            let mut http_response = Response::new(async_body);
            *http_response.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::OK);

            Ok(http_response)
        }
        .boxed()
    }
}
