// WebMCP: offers this page's tools to an AI agent running in the browser.
//
// This is the only file that touches the browser's API, `document.modelContext` (the
// W3C Web Machine Learning group's WebMCP draft; in Chrome behind a flag or an origin
// trial as of late 2026). Where the API is absent, which is almost everywhere today,
// `registerWebMcp` returns at once: nothing is downloaded and nothing is logged.
//
// The API has changed shape between Chrome versions, so this registers defensively:
// - `registerTool` returns nothing (Chrome 149 and 150) or a promise (151 on); it may
//   throw or reject. Both are handled the same way.
// - A tool is unregistered by aborting the signal it was registered with. Chrome 146 to
//   148 and Firefox's prototype ignore the signal and have `unregisterTool(name)`.
// - Early builds had the object on `navigator`, not `document`.
import { useAgent } from './agent-store';
import type { WebMcpToolDefinition } from './core';

/** The part of `ModelContext` this page uses. */
export interface ModelContextLike {
  registerTool(tool: WebMcpToolDefinition, options?: { signal?: AbortSignal }): unknown;
  unregisterTool?: (name: string) => unknown;
}

interface Holder {
  modelContext?: unknown;
}

/** The browser's model context, or undefined when the page has none (no flag, no trial token, an insecure context). */
export function findModelContext(doc: Holder | undefined = globalThis.document as Holder | undefined, nav: Holder | undefined = globalThis.navigator as Holder | undefined): ModelContextLike | undefined {
  const candidate = (doc?.modelContext ?? nav?.modelContext) as Partial<ModelContextLike> | null | undefined;
  return candidate && typeof candidate.registerTool === 'function' ? (candidate as ModelContextLike) : undefined;
}

export interface Registration {
  /** Unregisters every tool. Safe to call more than once. */
  dispose(): void;
  /** Resolves when the browser has accepted every tool; rejects with the first refusal, after the rest are withdrawn. */
  done: Promise<void>;
}

/**
 * Registers the tools as one batch: if the browser refuses any of them (a name it does
 * not accept, a permissions policy), the others are withdrawn, so an agent never sees
 * half a tool set.
 */
export function registerTools(context: ModelContextLike, tools: WebMcpToolDefinition[]): Registration {
  const controller = new AbortController();
  const accepted = new Set<string>();
  let disposed = false;
  const dispose = () => {
    if (disposed) return;
    disposed = true;
    controller.abort();
    if (typeof context.unregisterTool !== 'function') return;
    for (const name of accepted) {
      try {
        context.unregisterTool(name);
      } catch {
        // Already gone: the browser honoured the signal.
      }
    }
  };
  const each = tools.map(async (tool) => {
    // `registerTool` may return undefined or a promise, and may throw: awaiting inside an async function covers all three.
    await context.registerTool(tool, { signal: controller.signal });
    accepted.add(tool.name);
  });
  const done = Promise.all(each).then(
    () => undefined,
    (error: unknown) => {
      // Let every registration settle before withdrawing, so none is left behind.
      return Promise.allSettled(each).then(() => {
        dispose();
        throw error;
      });
    },
  );
  // The caller decides what a refusal means; an unobserved `done` must not surface as an unhandled rejection.
  done.catch(() => {});
  return { dispose, done };
}

const SLOT = '__firesightWebMcpDispose';

/**
 * Call once at startup. Registers the tools if the browser offers WebMCP; otherwise does
 * nothing. Tools are registered before the engine has loaded: each one waits for
 * `ready` itself. Calling it again replaces the earlier registration.
 */
export function registerWebMcp(options: { ready: () => Promise<void>; load?: () => Promise<{ definitions: (options: { ready: () => Promise<void> }) => WebMcpToolDefinition[] }> }): void {
  const context = findModelContext();
  if (!context) return;
  const slot = globalThis as unknown as Record<string, (() => void) | undefined>;
  slot[SLOT]?.();
  let cancelled = false;
  let registration: Registration | null = null;
  slot[SLOT] = () => {
    cancelled = true;
    registration?.dispose();
    useAgent.setState({ available: false, toolCount: 0 });
  };
  const failed = (error: unknown) => {
    if (cancelled) return;
    useAgent.setState({ available: false, toolCount: 0, registrationError: error instanceof Error ? error.message : String(error) });
  };
  // The tools and everything they pull in are a separate chunk, fetched only here. `load` is for tests.
  void (options.load ?? (() => import('./runtime')))()
    .then(({ definitions }) => {
      if (cancelled) return;
      const tools = definitions({ ready: options.ready });
      registration = registerTools(context, tools);
      return registration.done.then(() => {
        if (!cancelled) useAgent.setState({ available: true, toolCount: tools.length, registrationError: null });
      });
    })
    .catch(failed);
  import.meta.hot?.dispose(() => slot[SLOT]?.());
}
