import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { register } from "node:module";
import { dirname, resolve } from "node:path";
import test, { beforeEach } from "node:test";
import { fileURLToPath } from "node:url";

import {
  handleJevRelay,
  JEV_DEFAULT_UPSTREAM,
  JEV_MAX_BODY_BYTES,
  JEV_MAX_IN_FLIGHT,
  resolveJevUpstream,
} from "../src/jev-relay.ts";

// ── The Worker's real entry point ──────────────────────────────────────────
//
// `index.ts` is loaded with the broker glue stubbed (see the loader), so
// `worker.fetch` below is the production HTTP entry: the same routing code that
// answers `lobby-preview.phase-rs.dev`.

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(HERE, "../..");
const LOBBY_DO_SOURCE = readFileSync(resolve(REPO_ROOT, "lobby-worker/src/lobby-do.ts"), "utf8");
const glueImport = LOBBY_DO_SOURCE.match(/import\s*\{([^}]*)\}\s*from\s*"\.\.\/broker-wasm-pkg\/broker\.js"/);
assert.ok(glueImport, "lobby-do.ts imports the broker glue");
register("./support/stub-broker-loader.mjs", {
  parentURL: import.meta.url,
  data: {
    names: glueImport[1]
      .split(",")
      .map((name) => name.trim())
      .filter(Boolean),
  },
});
const { default: worker } = await import("../src/index.ts");

const KEY = "jev-key-SECRET-9c41";
const originalFetch = globalThis.fetch;

beforeEach(() => {
  globalThis.fetch = originalFetch;
});

/** What the Worker's `LOBBY` binding saw: a request here reached the lobby DO. */
function fakeEnv(extra = {}) {
  const reachedDo = [];
  return {
    env: {
      LOBBY: {
        idFromName: (name) => name,
        get: () => ({
          fetch: async (request) => {
            reachedDo.push(new URL(request.url).pathname);
            return Response.json({ mode: "LobbyOnly" });
          },
        }),
      },
      ...extra,
    },
    reachedDo,
  };
}

const ctx = { waitUntil() {}, passThroughOnException() {} };

function envelope(request = { model: "jev-latest", questions: {} }, apiKey = KEY) {
  return JSON.stringify({ apiKey, request });
}

/** A browser's simple request: `text/plain`, an Origin, no Authorization. */
function simplePost(url, body, headers = {}) {
  return new Request(url, {
    method: "POST",
    headers: { "Content-Type": "text/plain;charset=UTF-8", Origin: "https://phase-rs.dev", ...headers },
    body,
  });
}

function stubUpstream(reply = () => new Response('{"answers":{}}', { status: 200, headers: { "Content-Type": "application/json" } })) {
  const calls = [];
  globalThis.fetch = async (input, init) => {
    calls.push({ url: String(input), init });
    return reply(input, init);
  };
  return calls;
}

// The URL the preview build is configured to use as its official server. Read
// from the workflow rather than restated, so a change to the deployed default
// moves this test with it.
const DEPLOY_YML = readFileSync(resolve(REPO_ROOT, ".github/workflows/deploy.yml"), "utf8");
const PREVIEW_WS_URL = DEPLOY_YML.match(/OFFICIAL_MULTIPLAYER_SERVER_URL:\s*"(wss:\/\/[^"]+)"/)?.[1];

test("the configured preview default is served by the Worker's relay route", async () => {
  assert.ok(PREVIEW_WS_URL, "deploy.yml configures the preview OFFICIAL_MULTIPLAYER_SERVER_URL");
  // The client derives the relay as the same host over https (`serverHttpOrigin`).
  const origin = new URL(PREVIEW_WS_URL.replace(/^wss:/, "https:")).origin;
  assert.equal(origin, "https://lobby-preview.phase-rs.dev");

  const upstream = stubUpstream();
  const { env, reachedDo } = fakeEnv();
  const response = await worker.fetch(simplePost(`${origin}/jev/systemone`, envelope()), env, ctx);

  assert.equal(response.status, 200);
  assert.equal(response.headers.get("Access-Control-Allow-Origin"), "*");
  assert.deepEqual(await response.json(), { answers: {} });
  assert.equal(upstream.length, 1);
  // Not the lobby DO, whose fallthrough answers a version document.
  assert.deepEqual(reachedDo, []);
});

test("the relay route does not capture the lobby's other paths", async () => {
  stubUpstream();
  const { env, reachedDo } = fakeEnv();
  await worker.fetch(new Request("https://lobby-preview.phase-rs.dev/health"), env, ctx);
  await worker.fetch(new Request("https://lobby-preview.phase-rs.dev/servers"), env, ctx);
  assert.deepEqual(reachedDo, ["/health", "/servers"]);
});

test("a preflight is answered by the Worker, not the lobby", async () => {
  const { env, reachedDo } = fakeEnv();
  const response = await worker.fetch(
    new Request("https://lobby-preview.phase-rs.dev/jev/systemone", {
      method: "OPTIONS",
      headers: { Origin: "https://phase-rs.dev", "Access-Control-Request-Method": "POST" },
    }),
    env,
    ctx,
  );
  assert.equal(response.status, 204);
  assert.equal(response.headers.get("Access-Control-Allow-Origin"), "*");
  assert.match(response.headers.get("Access-Control-Allow-Methods") ?? "", /POST/);
  assert.deepEqual(reachedDo, []);
});

// ── Relay behavior ─────────────────────────────────────────────────────────

const URL_ = "https://lobby.example/jev/systemone";

test("forwards the request with the key only in the Authorization header", async () => {
  const upstream = stubUpstream();
  const body = { model: "jev-latest", state: { position: "p" }, questions: { pick: { type: "choice" } } };
  const response = await handleJevRelay(simplePost(URL_, envelope(body)), {});

  assert.equal(response.status, 200);
  assert.equal(upstream.length, 1);
  const { url, init } = upstream[0];
  assert.equal(url, JEV_DEFAULT_UPSTREAM);
  assert.equal(init.method, "POST");
  assert.equal(init.headers.get("Authorization"), `Bearer ${KEY}`);
  assert.equal(init.headers.get("Content-Type"), "application/json");
  assert.deepEqual(JSON.parse(init.body), body);
  assert.ok(!init.body.includes(KEY), "the key is not forwarded as a body field");
  // Never follows a redirect with the bearer key.
  assert.equal(init.redirect, "manual");
});

test("ignores an incoming Authorization header", async () => {
  const upstream = stubUpstream();
  await handleJevRelay(simplePost(URL_, envelope(), { Authorization: "Bearer attacker" }), {});
  assert.equal(upstream[0].init.headers.get("Authorization"), `Bearer ${KEY}`);
});

test("passes the upstream status and content type through, with CORS", async () => {
  stubUpstream(() => new Response('{"error":"bad key"}', { status: 401, headers: { "Content-Type": "application/json", "Set-Cookie": "a=b", "X-Other": "1" } }));
  const response = await handleJevRelay(simplePost(URL_, envelope()), {});
  assert.equal(response.status, 401);
  assert.equal(response.headers.get("Content-Type"), "application/json");
  assert.equal(response.headers.get("Access-Control-Allow-Origin"), "*");
  assert.equal(response.headers.get("Set-Cookie"), null);
  assert.equal(response.headers.get("X-Other"), null);
  assert.deepEqual(await response.json(), { error: "bad key" });
});

test("a redirecting upstream never hands the browser a Location", async () => {
  stubUpstream(() => new Response(null, { status: 307, headers: { Location: "https://evil.example/collect" } }));
  const response = await handleJevRelay(simplePost(URL_, envelope()), {});
  assert.equal(response.status, 307);
  assert.equal(response.headers.get("Location"), null);
});

test("refuses anything that is not the exact envelope, without calling upstream", async () => {
  const upstream = stubUpstream();
  const bad = [
    "not json",
    "[]",
    "null",
    JSON.stringify({ apiKey: KEY }),
    JSON.stringify({ request: {} }),
    JSON.stringify({ apiKey: "", request: {} }),
    JSON.stringify({ apiKey: "   ", request: {} }),
    JSON.stringify({ apiKey: 7, request: {} }),
    JSON.stringify({ apiKey: KEY, request: [] }),
    JSON.stringify({ apiKey: KEY, request: "x" }),
    JSON.stringify({ apiKey: KEY, request: {}, extra: 1 }),
    JSON.stringify({ apiKey: `${KEY}\nX-Injected: 1`, request: {} }),
  ];
  for (const text of bad) {
    const response = await handleJevRelay(simplePost(URL_, text), {});
    assert.equal(response.status, 400, text);
    assert.equal(await response.text(), "invalid Jev relay request");
  }
  assert.equal(upstream.length, 0);
});

test("a refusal never echoes the key", async () => {
  stubUpstream();
  const response = await handleJevRelay(simplePost(URL_, JSON.stringify({ apiKey: KEY, request: [], extra: 1 })), {});
  assert.ok(!(await response.text()).includes(KEY));
});

test("only POST is relayed", async () => {
  const upstream = stubUpstream();
  const response = await handleJevRelay(new Request(URL_, { method: "GET" }), {});
  assert.equal(response.status, 405);
  assert.equal(upstream.length, 0);
});

test("an oversized body is refused, by header and by bytes read", async () => {
  const upstream = stubUpstream();
  const huge = envelope({ pad: "x".repeat(JEV_MAX_BODY_BYTES) });
  const declared = await handleJevRelay(simplePost(URL_, huge, { "Content-Length": String(huge.length) }), {});
  assert.equal(declared.status, 413);

  // No (or a lying) Content-Length: the bound is the bytes actually read.
  const chunked = new Request(URL_, {
    method: "POST",
    headers: { "Content-Type": "text/plain;charset=UTF-8" },
    body: new ReadableStream({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(huge));
        controller.close();
      },
    }),
    duplex: "half",
  });
  assert.equal((await handleJevRelay(chunked, {})).status, 413);
  assert.equal(upstream.length, 0);
});

test("a hung upstream is a 504 and an unreachable one a 502", async () => {
  globalThis.fetch = (_input, init) =>
    new Promise((_resolve, reject) => {
      init.signal.addEventListener("abort", () => reject(init.signal.reason));
    });
  const slow = await handleJevRelay(simplePost(URL_, envelope()), {}, { timeoutMs: 20 });
  assert.equal(slow.status, 504);
  assert.equal(await slow.text(), "Jev upstream timed out");

  globalThis.fetch = async () => {
    throw new TypeError("connect ECONNREFUSED");
  };
  const down = await handleJevRelay(simplePost(URL_, envelope()), {});
  assert.equal(down.status, 502);
  assert.equal(await down.text(), "Jev upstream unreachable");
});

test("the key reaches no log line", async () => {
  stubUpstream();
  const logged = [];
  const originalLog = console.log;
  console.log = (...args) => logged.push(JSON.stringify(args));
  try {
    await handleJevRelay(simplePost(URL_, envelope()), {});
  } finally {
    console.log = originalLog;
  }
  assert.ok(logged.length > 0);
  assert.ok(logged.every((line) => !line.includes(KEY)));
});

test("the upstream override is honored only when it cannot expose the key", () => {
  assert.equal(resolveJevUpstream({}), JEV_DEFAULT_UPSTREAM);
  assert.equal(resolveJevUpstream({ TYPESAFE_API_URL: "https://stub.example/v1/systemone" }), "https://stub.example/v1/systemone");
  assert.equal(resolveJevUpstream({ TYPESAFE_API_URL: "http://127.0.0.1:8080/x" }), "http://127.0.0.1:8080/x");
  for (const unsafe of ["http://stub.example/x", "ftp://stub.example", "not a url", "https://user:pw@stub.example/x"]) {
    assert.equal(resolveJevUpstream({ TYPESAFE_API_URL: unsafe }), JEV_DEFAULT_UPSTREAM, unsafe);
  }
});

// ── Abuse bounds ───────────────────────────────────────────────────────────

test("a per-IP limit refuses before the body is read or upstream is called", async () => {
  const upstream = stubUpstream();
  const keys = [];
  const env = {
    JEV_LIMIT: {
      async limit(options) {
        keys.push(options.key);
        return { success: false };
      },
    },
  };
  const response = await handleJevRelay(
    simplePost(URL_, envelope(), { "CF-Connecting-IP": "203.0.113.9" }),
    env,
  );
  assert.equal(response.status, 429);
  assert.equal(response.headers.get("Access-Control-Allow-Origin"), "*");
  assert.deepEqual(keys, ["jev:203.0.113.9"]);
  assert.equal(upstream.length, 0);
});

test("an admitted request goes through the limiter, and a failing limiter fails open", async () => {
  const upstream = stubUpstream();
  const admitting = { JEV_LIMIT: { limit: async () => ({ success: true }) } };
  assert.equal((await handleJevRelay(simplePost(URL_, envelope()), admitting)).status, 200);

  const broken = {
    JEV_LIMIT: {
      limit: async () => {
        throw new Error("limiter down");
      },
    },
  };
  const originalError = console.error;
  console.error = () => {};
  try {
    assert.equal((await handleJevRelay(simplePost(URL_, envelope()), broken)).status, 200);
  } finally {
    console.error = originalError;
  }
  assert.equal(upstream.length, 2);
});

test("calls beyond the in-flight cap are answered 503 and the cap recovers", async () => {
  const releases = [];
  globalThis.fetch = () =>
    new Promise((resolveUpstream) => {
      releases.push(() => resolveUpstream(new Response("{}", { status: 200 })));
    });
  const held = Array.from({ length: JEV_MAX_IN_FLIGHT }, () =>
    handleJevRelay(simplePost(URL_, envelope()), {}),
  );
  // Let every held request reach its upstream call.
  while (releases.length < JEV_MAX_IN_FLIGHT) await new Promise((r) => setImmediate(r));

  const refused = await handleJevRelay(simplePost(URL_, envelope()), {});
  assert.equal(refused.status, 503);
  assert.equal(await refused.text(), "Jev relay busy");

  releases.forEach((release) => release());
  assert.ok((await Promise.all(held)).every((response) => response.status === 200));

  stubUpstream();
  assert.equal((await handleJevRelay(simplePost(URL_, envelope()), {})).status, 200);
});
