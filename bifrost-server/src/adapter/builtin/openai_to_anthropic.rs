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
        context: RequestContext,
    ) -> Result<RequestTransform, Self::Error> {
        let body = transform_openai_request(context.body)?;

        Ok(RequestTransform::new(body))
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
