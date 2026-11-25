# The hosted MCP server

An AI client that is not in the user's browser (Claude, Claude Code, ChatGPT, Cursor,
VS Code) can work with the plans in a user's FIRESight account through a
[Model Context Protocol](https://modelcontextprotocol.io) server at `/api/mcp`. It can
read a plan, build one from a description, change one, simulate it and compare a
what-if, with the same engine the website runs. What it writes appears on the website
for that user, at a link every result carries, and for nobody else.

This is the counterpart of the in-page tools in [webmcp.md](webmcp.md). Those act on the
plan open in a browser tab, in front of the user. These act on plans saved to an
account, so they need an account: see [accounts.md](accounts.md). A site deployed
without accounts has no MCP endpoint, and a plan that was never synced to an account is
out of its reach.

Code: `api/mcp.ts` (the HTTP side), `api/_lib/mcp/` (the server and its tools),
`api/_lib/engine.ts` (the engine under Node). Tests: `api/_tests/mcp*.test.ts`,
`api/_tests/engine.test.ts`. End-to-end check: `npm run mcp:smoke`.

## Principles

1. **The engine is the only source of numbers.** Every figure in a result comes from the
   same WebAssembly engine as the website, given the same input. A run here and a run in
   the browser with the same plan, seed and number of lives are the same lives. An agent
   that follows `get_guide` never has to estimate anything.
2. **Charts stay on the site.** Results are compact JSON: the engine's digest of a run
   (about 2 KB), never its full summary. Every result about a plan has a `url`
   (`<site>/p/<plan id>`), and says to give it to the user. The picture is there.
3. **A caller is a verified person, and only that person.** Every query is scoped to the
   user behind the credential. A plan id that belongs to someone else answers exactly as
   an id that does not exist. Nothing takes a user from the arguments.
4. **Writes cannot silently undo each other.** `update_plan` needs the `rev` the agent
   last read. If the user edited the plan on the website in the meantime, nothing is
   written and the agent is told to read again.
5. **A failure says what to do next.** Anything the agent can act on (a patch that does
   not apply, a stale rev, a read-only key) is a tool result with `isError` and a message
   written to be acted on, not a protocol error. Like every result it is one JSON object
   (`{"error": {"code", "message"}, …}`), as text and as structured content: a message
   can quote the engine, which quotes ids from the plan, so text that somebody typed
   into a plan only ever reaches the agent as the value of a JSON field, cleaned and cut
   to a bounded length.
6. **Nothing is logged.** Like the other functions, this one writes no logs, so neither
   a credential nor a plan can end up in one.

## Connecting

The endpoint is `https://<site>/api/mcp`. The account page on the website shows it with
copy-and-paste instructions.

**With sign-in (OAuth).** Clients that sign the user in need nothing but the address:
Claude (add a custom connector and paste the address; the account page gives the menu
path, which comes from Anthropic's documentation and has not been tried here), ChatGPT,
and Claude Code or Cursor when no key is given. The first call is answered with `401` and a pointer to the site's
sign-in metadata; the client sends the user to the site's login and consent pages, and
comes back with a token that is bound to this endpoint. The user chooses whether the app
may only read plans or also change them, and can disconnect it on the account page,
which cuts it off on its next call.

**With an API key.** Clients that can send a header can use a key created on the account
page (`fs_…`, read-only or read-write, shown once):

```bash
claude mcp add --transport http firesight https://<site>/api/mcp \
  --header "Authorization: Bearer fs_…"
```

```json
{ "mcpServers": { "firesight": { "type": "http", "url": "https://<site>/api/mcp",
  "headers": { "Authorization": "Bearer ${FIRESIGHT_API_KEY}" } } } }
```

A key is as good as a password for the plans in the account. Keep it out of files that
are shared or committed.

## Tools

| Tool | Needs | What it does |
|---|---|---|
| `get_guide` | | The rules for working with a plan: where figures may come from, how to read results, the patch syntax, what the model leaves out. The same text the in-page tools serve, with this server's tool names. |
| `get_plan_schema` | | The plan format: the map of every field, or one type's exact fields, allowed values and defaults. |
| `validate_plan` | | Checks profile JSON with the engine without saving it. Returns the validation report and, for a valid plan, what it implies before any simulation. |
| `list_plans` | read | The account's plans: id, name, rev, who last wrote it, link. No contents. |
| `get_plan` | read | One plan: compact (only what differs from the defaults, with the ids patches use) or `full`. |
| `create_plan` | write | Saves a new plan from full or partial profile JSON. Returns id, rev, link and the validation report. |
| `update_plan` | write | Applies a patch to a plan, all or nothing, if it is still at `expected_rev`. |
| `run_simulation` | read | Simulates a plan and returns the engine's digest: success, FI and depletion with intervals, ages, net worth, spending delivery, failure causes. |
| `compare_scenario` | read; write to save | Simulates a what-if against the plan on the same lives and returns the paired comparison. `save: true` adds it to the plan as a named scenario. |

There is no tool that deletes a plan. The user does that on the website.

Every tool has a title and all four annotations (`readOnlyHint`, `destructiveHint`,
`idempotentHint`, `openWorldHint`) set explicitly. Inputs are strict: an argument the
tool does not know is refused. A result is one JSON object, as text and as
`structuredContent`; a result about a plan adds a second line of text with the link.

### Simulation

A run uses the plan's own seed, so it meets the same simulated futures as the website
and as every other run of that plan. It simulates 2,000 lives unless asked (200 to
5,000); the website simulates the plan's own setting, usually 10,000. The first 2,000
lives are the same lives in both, but a figure from 2,000 can differ from one from
10,000 within its sampling interval, and each result says so. A function has one thread
where the browser has several: 2,000 lives took about half a second on an idle laptop
(Apple M-series, Node 25); the time on Vercel has not been measured. A run is cut into
stretches of about 30 milliseconds with a pause between them, so that other requests
are served in the gaps and the run stops when the client goes away; it is stopped if it
has not finished in a minute. A stretch is sized by the time the last one took (4 lives
to begin with, at most 200), because a life of a large plan costs many times a life of
a small one. The sizes do not affect the figures: the engine merges the stretches
exactly, so the result is the same bytes however the lives are cut.

## What the endpoint checks

In order, before the MCP server sees a request:

1. **Origin.** A request that names an `Origin` must name an allowed one: the site
   itself, `localhost` (for tools such as the MCP Inspector), `claude.ai`, `claude.com`,
   `chatgpt.com`, and any hosts in `FIRESIGHT_MCP_ALLOWED_ORIGINS`. Others get `403`. A
   request without an `Origin`, which is every client that is not a web page, is not
   blocked. This grants nothing: a browser attaches no credential by itself.
2. **Credential.** `Authorization: Bearer` with an API key or an access token issued by
   this site for this endpoint (signature, issuer, audience and expiry are checked, and
   that the user still consents to the app). Cookies are never read: the header is
   removed before anything else looks at the request. Without a credential the answer is
   `401` with `WWW-Authenticate: Bearer resource_metadata="…"` ([RFC 9728](https://www.rfc-editor.org/rfc/rfc9728)).
3. **Rate limits**, counted in the database so that they hold across function instances:

   | What | Limit |
   |---|---|
   | Calls, per account | 240 a minute |
   | Simulations (`run_simulation`, `compare_scenario`), per account | 20 a minute |
   | Writes, per account | 60 a minute |
   | Calls, per API key | 120 a minute |
   | Refused credentials, per client address | 30 a minute |

   Over a limit, a request gets `429` with `Retry-After`, and a tool call gets an
   `isError` result that says how long to wait. The one exception is `compare_scenario`
   with `save: true` at the write limit: the simulations are already done, so the
   comparison is returned with `saved_scenario: { saved: false, reason }` instead.

A request body larger than 1 MB is refused before it is parsed.

## Protocol

Streamable HTTP, without sessions: every request is answered by a server built for that
request and that caller, which is what a serverless function needs and what the
2026-07-28 revision of the protocol expects. Clients that still speak a 2025 revision
are served the same tools through the SDK's stateless fallback (`initialize`, then one
request per call; a `GET` for a stream is answered `405`). The server offers tools only:
no resources, no prompts, and no change notifications, so nothing holds a function
open.

Built on `@modelcontextprotocol/server` 2.4 (`createMcpHandler`). The server's
`instructions` are under 4,000 characters, with what matters most first, because some
clients cut them there.

## The engine on the server

The engine is the wasm package the website uses (`web/src/engine/pkg/`, built from
`crates/firesight-wasm`). `npm run engine` copies it to `api/_engine/`, which is not in
git, and `api/_lib/engine.ts` loads it once per function instance. The npm scripts that
need it (`typecheck`, `test`, `dev:api`, `e2e:api`) refresh the copy first.

On Vercel, `vercel.json` gives `api/mcp.ts` its own install step
(`npm run vercel-engine`), which builds the wasm if the web build has not done so yet
and makes the copy, and lists `api/_engine/**` as files to include with the function.

A few pure functions (the compact view of a plan, the schema lookups, the digest of a
comparison) exist both here and in the web app, because a function cannot import the web
app's modules. `api/_tests/mcp-parity.test.ts` loads the web app's originals and holds
the server's copies equal to them on the example households. The guide text is not
copied: both sides import `web/src/webmcp/guide.ts`.

## Try it

```bash
npm install && npm install --prefix web
npm run wasm --prefix web        # once: builds the engine (needs Rust)
npm run mcp:smoke                # starts a throwaway server and plays an agent through it
```

Against a server you are running (`npm run dev:api`), keeping the account so that the
plan can be opened in the app:

```bash
BASE_URL=http://localhost:8787 npm run mcp:smoke -- --keep
```

With the [MCP Inspector](https://github.com/modelcontextprotocol/inspector):

```bash
npx @modelcontextprotocol/inspector --cli http://localhost:8787/api/mcp --transport http \
  --method tools/list --header "Authorization: Bearer fs_…"
```

## Privacy

- A plan reaches this server only if its owner synced it to their account, or an agent
  they authorised created it there. Plans kept in the browser are not reachable.
- What a tool returns goes to the AI client that called it, and from there to whoever
  runs that client's model. The user chose that client and gave it the credential.
- Names and notes inside a plan are text a person typed. They are returned only as JSON
  values, never as part of a tool description or an instruction, and the guide tells
  agents to treat them as data.
- Synced plans are encrypted at rest with a key the server holds. The server can read
  them: it has to, to simulate them. See [accounts.md](accounts.md).

## Limits

- Fewer tools than in the page. Lever ranking, goal seek, sweeps, single-life inspection
  and tax explanations are in the in-page tools and the in-app assistant, not here.
- No charts or images in results, by design: the link opens the plan on the site.
- At most 5,000 lives a run, 50 plans an account, 20 scenarios a plan, 256 KB a plan.
- `create_plan` keeps the scenarios of a plan file but not always their ids: an id that
  is repeated, is `base` (the website's name for the plan itself) or is not a usable id
  is replaced by one made from the scenario's name, and the result lists the changes
  under `scenario_ids_changed`. Scenario names are cleaned and cut to 60 characters.
- Which `Origin`, if any, the hosted assistants send has not been observed. If one of
  them is turned away with `403`, add its host to `FIRESIGHT_MCP_ALLOWED_ORIGINS`.
- Not yet verified against a real deployment or a real hosted client. Locally it has
  been driven by the MCP SDK's client, the MCP Inspector, and a public OAuth client
  through the site's login and consent pages; claude.ai and ChatGPT have not been tried.
  [accounts.md](accounts.md#self-hosting-on-vercel) has the deployment steps.
