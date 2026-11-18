import type { Report } from './types';

export type EngineErrorKind =
  /** The simulation core is still a stub in this build of the wasm package. */
  | 'not_implemented'
  /** The profile has validation errors; `report` carries them. */
  | 'invalid_profile'
  /** The run was cancelled through its AbortSignal. */
  | 'aborted'
  /** A worker crashed or failed to load the wasm module. */
  | 'worker'
  /** Anything else the engine threw. */
  | 'engine';

export class EngineError extends Error {
  readonly kind: EngineErrorKind;
  readonly report?: Report;

  constructor(kind: EngineErrorKind, message: string, report?: Report) {
    super(message);
    this.name = kind === 'aborted' ? 'AbortError' : 'EngineError';
    this.kind = kind;
    this.report = report;
  }
}

/** Classifies a message thrown by the wasm module. */
export function classifyEngineMessage(message: string): EngineError {
  if (/not implemented/i.test(message)) return new EngineError('not_implemented', message);
  if (message.startsWith('{')) {
    try {
      const report = JSON.parse(message) as Report;
      if (Array.isArray(report.errors)) {
        const first = report.errors[0]?.message ?? 'The profile has errors.';
        return new EngineError('invalid_profile', first, report);
      }
    } catch {
      // Not a report after all.
    }
  }
  return new EngineError('engine', message);
}

export function toEngineError(error: unknown): EngineError {
  if (error instanceof EngineError) return error;
  if (error instanceof Error) return classifyEngineMessage(error.message);
  return classifyEngineMessage(String(error));
}

export function isAbort(error: unknown): boolean {
  return error instanceof EngineError
    ? error.kind === 'aborted'
    : error instanceof DOMException && error.name === 'AbortError';
}
