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

pub struct ResponsesToChatAdapter {
    stream_processor: ChatToResponsesStreamProcessor,
}

impl Default for ResponsesToChatAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ResponsesToChatAdapter {
    pub fn new() -> Self {
        Self {
            stream_processor: ChatToResponsesStreamProcessor::new(),
        }
    }
}

#[async_trait]
impl Adapter for ResponsesToChatAdapter {
    type Error = LlmMapError;

    async fn transform_request(
        &self,
        context: RequestContext,
    ) -> Result<RequestTransform, Self::Error> {
        let (body, mappings) = responses_to_chat_request(context.body)?;
        self.stream_processor.set_namespace_mappings(mappings);
        Ok(RequestTransform::new(body))
    }

    async fn transform_response(
        &self,
        context: ResponseContext<'_>,
    ) -> Result<ResponseTransform, Self::Error> {
        let mappings = self.stream_processor.namespace_mappings();
        let converted = chat_to_responses_response(context.body, &mappings)?;
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
