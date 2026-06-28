import type { Message, ContentPart, ToolCall, ToolResult } from '../types/message';

/** Injectable id/time sources so the builders are deterministic in tests. */
export interface MessageBuildOpts {
  uuid?: () => string;
  now?: () => number;
}

function resolve(opts?: MessageBuildOpts) {
  return {
    uuid: opts?.uuid ?? (() => crypto.randomUUID()),
    now: opts?.now ?? (() => Date.now()),
  };
}

/**
 * Build the user message content for a turn: a plain string, or a multimodal
 * parts array when images are attached (text part first, if any).
 */
export function buildUserContent(
  text: string,
  images: { dataUrl: string }[],
): string | ContentPart[] {
  if (images.length === 0) return text;
  return [
    ...(text.length > 0 ? [{ type: 'text' as const, text }] : []),
    ...images.map((img) => ({ type: 'image_url' as const, image_url: { url: img.dataUrl } })),
  ];
}

/**
 * Turn streamed tool results into persisted `tool` messages. Each MUST carry
 * `toolCallId` (and `name`, looked up from the matching tool call) — otherwise
 * the next request 400s with an orphaned tool message.
 */
function toolMessages(
  toolCalls: ToolCall[],
  toolResults: ToolResult[],
  uuid: () => string,
  now: () => number,
): Message[] {
  return toolResults.map((result) => {
    const toolName = toolCalls.find((c) => c.id === result.toolCallId)?.name;
    return {
      id: uuid(),
      role: 'tool' as const,
      content: result.content,
      toolCallId: result.toolCallId,
      ...(toolName ? { name: toolName } : {}),
      timestamp: now(),
    };
  });
}

/**
 * Messages to persist for an interrupted turn (the assistant content gets an
 * `[interrupted]` marker). Returns `[]` when there is nothing to save.
 */
export function buildPartialMessages(
  assistantId: string,
  content: string,
  toolCalls: ToolCall[],
  toolResults: ToolResult[],
  opts?: MessageBuildOpts,
): Message[] {
  const { uuid, now } = resolve(opts);
  const hasContent = content.length > 0 || toolCalls.length > 0 || toolResults.length > 0;
  if (!hasContent) return [];

  const assistant: Message = {
    id: assistantId,
    role: 'assistant',
    content: content.length > 0 ? content + '\n\n[interrupted]' : '',
    toolCalls: toolCalls.length > 0 ? toolCalls : undefined,
    timestamp: now(),
  };
  return [assistant, ...toolMessages(toolCalls, toolResults, uuid, now)];
}

/**
 * Messages to persist when a turn completes normally: the assistant message
 * followed by one `tool` message per result.
 */
export function buildDoneMessages(
  assistantId: string,
  content: string,
  toolCalls: ToolCall[],
  toolResults: ToolResult[],
  opts?: MessageBuildOpts,
): Message[] {
  const { uuid, now } = resolve(opts);
  const assistant: Message = {
    id: assistantId,
    role: 'assistant',
    content,
    toolCalls: toolCalls.length > 0 ? toolCalls : undefined,
    timestamp: now(),
  };
  return [assistant, ...toolMessages(toolCalls, toolResults, uuid, now)];
}
