/**
 * Ambient Cloudflare Worker environment bindings.
 *
 * `cloudflare:workers` types its `env` export as `Cloudflare.Env` — a namespace, not a
 * global interface — so the declaration must merge into that namespace. Declaring a
 * global `Env` compiles but has no effect on `import { env } from "cloudflare:workers"`,
 * which is the mistake this comment exists to stop someone repeating.
 *
 * Bindings are declared **optional** because `.openai/hosting.json` currently sets both
 * `d1` and `r2` to `null`. Typing an unbound resource as present would let a missing
 * binding surface as a runtime `undefined` dereference instead of a compile error, which
 * is precisely the check worth having while persistence is still unbound.
 */

declare namespace Cloudflare {
  interface Env {
    /** Static asset fetcher. Always bound. */
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

/** Convenience alias so worker code can refer to the same shape as `Env`. */
type Env = Cloudflare.Env;
