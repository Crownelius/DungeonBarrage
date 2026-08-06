/**
 * Ambient Cloudflare Worker environment bindings.
 *
 * `cloudflare:workers` types its `env` export against the global `Env` interface, so it
 * must be declared globally rather than locally in each consumer — otherwise every module
 * re-declares a partial view and they drift apart.
 *
 * Bindings are declared **optional** because `.openai/hosting.json` currently sets both
 * `d1` and `r2` to `null`. Typing an unbound resource as present would let a missing
 * binding surface as a runtime `undefined` dereference instead of a type error, which is
 * precisely the check worth having.
 */

declare global {
  interface Env {
    /** Static asset fetcher, always present. */
    ASSETS: Fetcher;
    /** D1 database. Unbound until a milestone genuinely needs persistence. */
    DB?: D1Database;
    /** Cloudflare Images binding used by the vinext image optimizer. */
    IMAGES?: {
      input(stream: ReadableStream): {
        transform(options: Record<string, unknown>): {
          output(options: {
            format: string;
            quality: number;
          }): Promise<{ response(): Response }>;
        };
      };
    };
  }
}

export {};
