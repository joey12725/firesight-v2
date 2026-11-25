// "Was this thrown because the work was cancelled?", by the DOM convention: an error
// named AbortError. Kept apart from `agent/errors.ts` so that the tools (and the in-page
// WebMCP layer that reuses them) do not pull the model SDK into their bundle.
export function isAbortSignalError(error: unknown): boolean {
  return typeof error === 'object' && error !== null && (error as { name?: unknown }).name === 'AbortError';
}
