# SSE fixtures

This directory stores protocol-level SSE fixtures shared by stream converter tests.

```text
input/
  openai_chat_full.sse
  anthropic_messages_full.sse

expected/
  anthropic_to_openai_chat_full.sse
  openai_chat_to_anthropic_full.sse
  openai_chat_to_responses_full.sse
```

Keep input fixtures close to the upstream provider format. OpenAI Chat stream fixtures should omit
`event:` lines when the provider does not send them; tests can normalize the missing event name.

Expected fixtures should be generated from the current converter output instead of being authored
by hand. The test-only `stream_test_utils` module provides helpers to render converter output back
to SSE files once the input fixture is defined.
