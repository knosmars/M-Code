/**
 * LLM pricing data and cost estimation.
 *
 * Per DEVELOPMENT_GUIDE §11: 每次调用记录 token 数及费用估算.
 *
 * Prices are per 1M tokens in USD. Sources:
 *   - OpenAI:  https://openai.com/api/pricing/
 *   - Anthropic: https://www.anthropic.com/pricing
 *   - Google:   https://ai.google.dev/pricing
 */

/** Pricing rate per model (USD per 1M tokens). */
interface ModelPricing {
  /** Price per 1M input (prompt) tokens */
  input: number;
  /** Price per 1M output (completion) tokens */
  output: number;
}

/** Pricing table keyed by model name substring match (case-insensitive). */
const PRICING: Record<string, ModelPricing> = {
  // OpenAI
  'gpt-4o': { input: 2.5, output: 10.0 },
  'gpt-4o-mini': { input: 0.15, output: 0.6 },
  'gpt-4-turbo': { input: 10.0, output: 30.0 },
  'gpt-3.5-turbo': { input: 0.5, output: 1.5 },
  // Anthropic
  'claude-sonnet-4': { input: 3.0, output: 15.0 },
  'claude-3-5-sonnet': { input: 3.0, output: 15.0 },
  'claude-3-opus': { input: 15.0, output: 75.0 },
  // Google
  'gemini-2.5-pro': { input: 1.25, output: 10.0 },
  'gemini-2.5-flash': { input: 0.15, output: 0.6 },
  'gemini-2.0-flash': { input: 0.1, output: 0.4 },

  // Fallback for unknown models
  'default': { input: 0.0, output: 0.0 },
};

/** Look up pricing for a model. Matches by substring, case-insensitive. */
function getPricing(model: string): ModelPricing {
  const lower = model.toLowerCase();
  // Longest match first for more specific models
  const keys = Object.keys(PRICING).sort((a, b) => b.length - a.length);
  for (const key of keys) {
    if (lower.includes(key)) {
      return PRICING[key];
    }
  }
  return PRICING['default'];
}

/**
 * Estimate cost for an API call.
 * @returns Cost in USD, or 0 if pricing is unavailable.
 */
export function estimateCost(
  model: string,
  promptTokens: number,
  completionTokens: number,
): number {
  const pricing = getPricing(model);
  if (pricing.input === 0 && pricing.output === 0) {
    return 0;
  }
  return (
    (promptTokens / 1_000_000) * pricing.input +
    (completionTokens / 1_000_000) * pricing.output
  );
}

/**
 * Format a cost value for display. Returns e.g. "$0.023" or "$0.00".
 * Shows up to 4 decimal places for sub-cent costs.
 */
export function formatCost(cost: number): string {
  if (cost === 0) return '$0.00';
  if (cost < 0.01) return `$${cost.toFixed(4)}`;
  return `$${cost.toFixed(2)}`;
}
