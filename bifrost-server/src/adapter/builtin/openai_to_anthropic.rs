use crate::adapter::Adapter;
use crate::adapter::converter::openai_anthropic::request::transform_request as transform_openai_request;
use crate::adapter::converter::openai_anthropic::response::anthropic_to_openai_response;
use crate::adapter::converter::openai_anthropic::stream::AnthropicToOpenAIStreamProcessor;
use crate::error::LlmMapError;
use crate::model::{
    RequestContext, RequestTransform, ResponseContext, ResponseTransform, StreamChunkContext,
    StreamChunkTransform,
};
use async_trait::async_trait;
use http::HeaderMap;

pub struct OpenAIToAnthropicAdapter {
    stream_processor: AnthropicToOpenAIStreamProcessor,
}

impl Default for OpenAIToAnthropicAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenAIToAnthropicAdapter {
    pub fn new() -> Self {
        Self {
            stream_processor: AnthropicToOpenAIStreamProcessor::new(),
        }
    }
}

#[async_trait]
impl Adapter for OpenAIToAnthropicAdapter {
    type Error = LlmMapError;

    async fn transform_request(
        &self,
        context: RequestContext<'_>,
    ) -> Result<RequestTransform, Self::Error> {
        let body = transform_openai_request(context.body)?;
        let mut headers = HeaderMap::new();

        headers.insert(
            http::header::AUTHORIZATION,
            http::header::HeaderValue::from_bytes(
                format!("Bearer {}", context.provider_config.api_key).as_bytes(),
            )
            .unwrap(),
        );

        // Anthropic requires x-api-key header
        headers.insert(
            "x-api-key",
            http::header::HeaderValue::from_bytes(context.provider_config.api_key.as_bytes())
                .unwrap(),
        );

        Ok(RequestTransform::new(body)
            .with_headers(headers)
            .with_url(crate::util::join_url_paths(
                &context.provider_config.base_url,
                "v1/messages",
            )))
    }

    async fn transform_response(
        &self,
        context: ResponseContext<'_>,
    ) -> Result<ResponseTransform, Self::Error> {
        let converted = anthropic_to_openai_response(context.body)?;
        Ok(ResponseTransform::new(converted))
    }

    async fn transform_stream_chunk(
        &self,
        context: StreamChunkContext<'_>,
    ) -> Result<StreamChunkTransform, Self::Error> {
        self.stream_processor
            .anthropic_to_openai_stream(context.event, context.chunk)
    }
}
