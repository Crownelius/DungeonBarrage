import type { Plugin } from "vite";

/**
 * Local Sites integration point.
 *
 * The packaged starter references this module, but the current Windows bundle
 * does not include its generated implementation. Keeping the plugin present as
 * a named, no-op Vite plugin preserves the starter contract while the Sites
 * service handles deployment packaging.
 */
export function sites(): Plugin {
  return {
    name: "openai-sites",
  };
}
