/** Supported LLM provider identifiers.
 *
 * These match the provider adapter implementations in
 * `src-tauri/src/provider/`.
 */
export type ProviderId = 'meyatu' | 'openai-compatible' | 'anthropic' | 'google' | 'custom';

/** Configuration for a single LLM provider.
 *
 * Describes how the frontend should prompt the user for connection
 * details (base URL, API key) and what models are available.
 */
export interface ProviderConfig {
  /** Unique identifier matching a ProviderId */
  id: string;
  /** Human-readable display name (e.g. "OpenAI Compatible") */
  name: string;
  /** Base URL for the provider's API endpoint */
  baseUrl: string;
  /** List of model identifiers supported by this provider */
  models: string[];
  /** Whether this provider requires an API key for authentication */
  requiresApiKey: boolean;
}

/** A provider paired with the currently selected model.
 *
 * Represents the user's active choice of provider and model
 * for the current chat session.
 */
export interface ActiveProvider {
  /** The provider identifier */
  providerId: string;
  /** The selected model name (e.g. "gpt-4o", "claude-3-opus") */
  model: string;
}
