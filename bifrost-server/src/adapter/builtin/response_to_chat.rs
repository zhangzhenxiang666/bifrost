use crate::adapter::Adapter;
use crate::adapter::converter::openai_responses::request::responses_to_chat_request;
use crate::adapter::converter::openai_responses::response::chat_to_responses_response;
use crate::adapter::converter::openai_responses::stream::processor::ChatToResponsesStreamProcessor;
use crate::error::LlmMapError;
use crate::model::{
    RequestContext, RequestTransform, ResponseContext, ResponseTransform, StreamChunkContext,
    StreamChunkTransform,
};
use async_trait::async_trait;
use http::HeaderMap;

pub struct ResponseToChatAdapter {
    stream_processor: ChatToResponsesStreamProcessor,
}

impl Default for ResponseToChatAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ResponseToChatAdapter {
    pub fn new() -> Self {
        Self {
            stream_processor: ChatToResponsesStreamProcessor::new(),
        }
    }
}

#[async_trait]
impl Adapter for ResponseToChatAdapter {
    type Error = LlmMapError;

    async fn transform_request(
        &self,
        context: RequestContext<'_>,
    ) -> Result<RequestTransform, Self::Error> {
        let body = responses_to_chat_request(context.body)?;
        let mut headers = HeaderMap::new();

        headers.insert(
            http::header::AUTHORIZATION,
            http::header::HeaderValue::from_bytes(
                format!("Bearer {}", context.provider_config.api_key).as_bytes(),
            )
            .expect("API key is valid header value"),
        );

        Ok(RequestTransform::new(body)
            .with_headers(headers)
            .with_url(crate::util::join_url_paths(
                &context.provider_config.base_url,
                "chat/completions",
            )))
    }

    async fn transform_response(
        &self,
        context: ResponseContext<'_>,
    ) -> Result<ResponseTransform, Self::Error> {
        let converted = chat_to_responses_response(context.body)?;
        Ok(ResponseTransform::new(converted))
    }

    async fn transform_stream_chunk(
        &self,
        context: StreamChunkContext<'_>,
    ) -> Result<StreamChunkTransform, Self::Error> {
        self.stream_processor
            .chat_stream_to_responses_stream(context.chunk)
    }
}
