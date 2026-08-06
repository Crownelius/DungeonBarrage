/**
 * Server-render assertions for the Dungeon Barrage web shell.
 *
 * These replace the vinext starter's loading-skeleton tests, which asserted a page that
 * no longer exists — `app/page.tsx` renders the game. Those assertions had been failing
 * since the game replaced the starter, which is worse than having no test: a permanently
 * red suite stops being read.
 *
 * The focus here is what server rendering is actually responsible for: the document
 * arrives complete, the accessibility affordances are in the HTML rather than added by
 * client script, and no server-side identity leaks into the markup. Gameplay correctness
 * is covered by the simulation tests, not by scraping HTML.
 */

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

async function render(headers = { accept: "text/html" }) {
  const workerUrl = new URL("../dist/server/index.js", import.meta.url);
  // Cache-bust the module so repeated renders in one process are independent.
  workerUrl.searchParams.set("test", `${process.pid}-${Date.now()}`);
  const { default: worker } = await import(workerUrl.href);

  return worker.fetch(
    new Request("http://localhost/", { headers }),
    { ASSETS: { fetch: async () => new Response("Not found", { status: 404 }) } },
    { waitUntil() {}, passThroughOnException() {} },
  );
}

test("server-renders a complete game document", async () => {
  const response = await render();
  assert.equal(response.status, 200);
  assert.match(response.headers.get("content-type") ?? "", /^text\/html\b/i);

  const html = await response.text();

  assert.match(html, /<title>[^<]*Dungeon Barrage[^<]*<\/title>/i);
  assert.match(html, /<html lang="en"/);
  assert.match(html, /rel="manifest"/, "PWA manifest must be linked");
  assert.match(html, /<canvas/, "battlefield canvas must be server-rendered");
});

test("critical match state is present as text, not only on the canvas", async () => {
  // Accessibility baseline (PRODUCT_SPEC.md §6): wind, angle, power, ammunition, and
  // health must have text or numeric forms. Canvas pixels are invisible to a screen
  // reader, so these must exist in the DOM.
  const html = await (await render()).text();

  for (const label of ["WIND", "ANGLE", "POWER", "TURN"]) {
    assert.match(html, new RegExp(`>${label}<`), `${label} must be readable as text`);
  }
  assert.match(html, /aria-label=/, "interactive controls must be labelled");
  assert.match(html, /aria-live="polite"/, "turn state changes must be announced");
  assert.match(html, /role="tablist"/, "the weapon rack must expose its roles");
});

test("the canvas carries a control description for non-visual users", async () => {
  const html = await (await render()).text();
  const canvasTag = html.match(/<canvas[^>]*>/i)?.[0] ?? "";

  assert.ok(canvasTag.length > 0, "expected a canvas element");
  assert.match(
    canvasTag,
    /aria-label="[^"]{40,}"/,
    "the canvas needs a substantive description of the controls, not a bare name",
  );
});

test("no server-side identity or secret material reaches the markup", async () => {
  // SECURITY_BASELINE.md §7: email and provider identifiers never appear in client
  // payloads. The starter's ChatGPT identity headers make this a live risk rather than a
  // theoretical one, so it is asserted at the boundary where it would leak.
  const html = await (
    await render({
      accept: "text/html",
      "oai-authenticated-user-id": "test-user-id-should-not-render",
      "oai-authenticated-user-email": "leak-canary@example.com",
    })
  ).text();

  assert.doesNotMatch(html, /leak-canary@example\.com/, "email leaked into HTML");
  assert.doesNotMatch(html, /test-user-id-should-not-render/, "user id leaked into HTML");
  assert.doesNotMatch(html, /\bsk-[A-Za-z0-9]{16,}/, "credential-shaped string in HTML");
});

test("the page entry point renders the game rather than the starter skeleton", async () => {
  const page = await readFile(new URL("../app/page.tsx", import.meta.url), "utf8");

  assert.match(page, /export const metadata:\s*Metadata/);
  assert.match(page, /<DungeonBarrageGame \/>/);
  assert.doesNotMatch(
    page,
    /SkeletonPreview|codex-preview/,
    "starter preview scaffolding must not return to the entry point",
  );
});
