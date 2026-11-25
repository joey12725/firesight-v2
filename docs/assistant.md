# The assistant

Claude, working inside FIRESight on the user's plan. It drafts a plan from a description
or documents, answers what-if questions by running the engine, explains results, and
looks for what would change them. It is an analyst, not an adviser: the engine computes,
Claude explains, the user decides.

Code: `web/src/assistant/` (browser) and `api/chat.ts` (the optional hosted proxy).

## Principles

1. **The engine is the only source of numbers.** Every figure in an answer comes from a
   tool result in the conversation. The system prompt requires it, the tools make it
   easy (a what-if is one call), and each answer carries a "Based on" strip listing the
   tool calls behind it with their path counts and seed, each expandable to the exact
   request and result. This is a prompt rule plus an audit trail, not a mechanical
   check: nothing rejects an answer that contains an ungrounded number (see Limits).
2. **Claude proposes, the user disposes.** The plan changes only through
   `propose_changes`, which renders a card (before and after for each edit, the
   assumptions flagged as stated, inferred or default) with Accept, Edit and Decline.
   Accepted changes go through `store.edit`, so they are one undo step. A setting applies
   them without asking; they are still shown and still undoable.
3. **Every comparison is paired.** All simulation tools use the household's seed, so two
   runs meet the same simulated futures life by life. Results say so (`paired`, paths,
   seed, standard errors) and flag differences that cannot be told from zero
   (`within_noise`).
4. **The plan stays in the browser, apart from what the model needs to see.** See
   Privacy.

## Architecture

```
ui/Panel.tsx ── session.ts ──┬── agent/loop.ts ── transport/ ──► Anthropic API   (your key)
   cards, composer           │      streaming tool loop     └──► /api/chat ──►  (site's key)
                             ├── tools/  ──► profile store, worker pool (wasm), city table
                             └── conversation.ts ──► IndexedDB, one conversation per household
```

- `index.tsx` is what the app imports: `AssistantPanel`, `askAssistant`, `reviewPlan`,
  `registerAssistantChart`. The panel and everything behind it load on demand.
- `session.ts` owns a conversation: it builds each user turn, runs the loop, maps loop
  events onto the transcript the UI renders, holds change cards open until the user
  decides, and saves after every turn.
- `agent/loop.ts` is the tool loop. It is independent of the UI and of the tools, and is
  tested against a scripted model.
- `transport/` reaches Claude. Both modes use the official SDK (`@anthropic-ai/sdk`), so
  request building, stream parsing and typed errors are the same code.
- `tools/` are the twelve tools. Each takes a `ToolContext` (plan store, engine bridge,
  run registry), so the same executors run against fakes in tests.
- `prompt.ts` is the system prompt: role, the grounding rule, how to read and explain
  results, the model's main simplifications, guidance for startup equity, a map of the
  plan schema, the patch format with four worked examples, and intake guidance. Its
  first line is the marker `[FIRESight planning assistant, prompt v3]`, which is
  duplicated in `api/chat.ts` and compared by a test. The worked examples are data, and
  a test applies each to a real profile through the engine.

### The request

Built in one place (`transport/params.ts`), following the current Claude API guidance:

| Parameter | Value | Why |
|---|---|---|
| `model` | Settings; default `claude-fable-5-2` | Most capable current model. Opus 5.5, Sonnet 5.5 and Haiku 5.5 are selectable. |
| `thinking` | `{type: "adaptive", display: "summarized"}` | Adaptive is the only mode on these models. The summary doubles as a progress line. |
| `output_config.effort` | `medium`, pinned per model | Suits interactive analytical chat where tool calls carry the rigour. Changing it mid-conversation would invalidate the cache. |
| `max_tokens` | 32,000, always streamed | Thinking counts toward it. |
| `system` | the frozen prompt, with `cache_control` | Caches tools + system for every conversation. |
| top-level `cache_control` | ephemeral | Moves the second breakpoint along the conversation tail. |
| `tools` | twelve, name-sorted, `eager_input_streaming` | Large patches stream as written; inputs are therefore validated client-side before a tool runs. |
| `context_management` | `clear_tool_uses_20250919` past 100k prompt tokens (beta) | Server-side pruning of stale tool results. |
| `fallbacks` | `"default"` (beta; not on Haiku) | If a safety classifier declines a request, the API re-runs it on the model it recommends instead of failing the turn. |

Nothing that varies is in the system prompt. The plan, the date and any context from a
chart travel in the user turn, as `<plan_state>` (attached only when the plan has changed
since the model last saw it) and `<view_context>` blocks.

### History is append-only

Current models bind each reasoning block to the exact system prompt, tools and messages
that preceded it and reject edited histories. So the client never rewrites the
transcript:

- Reasoning blocks are replayed exactly as received.
- Old tool results are cleared by the server (context editing), not by the client.
- When a conversation gets long (about 150k prompt tokens, or 2.5 MB in hosted mode) the
  next turn starts with *simple compaction*: the model summarises the conversation and a
  new transcript starts from that summary alone. The UI shows a note where it happened.
- Regenerate and stop only ever cut the *end* of the transcript, which is allowed.
- A stored conversation records a fingerprint of the prompt and tools it was produced
  under. If they have changed (an app update), its reasoning blocks are dropped on load.
  If the API still rejects a history, the loop strips reasoning and retries once.

### Errors and retries

The SDK's own retries are off; the loop retries overload (529), server errors, rate
limits (honouring `retry-after`) and network failures with exponential backoff, visibly
and cancellably, up to four times. Errors are classified with the SDK's typed classes
into messages that say what to do (bad key, model not available to the key, rate limit,
request too large, hosted proxy refusals). Tools never run on a response that was
refused or cut off at `max_tokens`.

## Tools

All take and return compact JSON. No tool returns a raw `Summary` (hundreds of
kilobytes); results over 16,000 characters are refused as a bug.

| Tool | Does |
|---|---|
| `get_plan` | The plan with defaults pruned (ids kept), derived totals, the validation report, the engine's plan preview when available, scenarios, run settings. For each private company, `derived.private_equity` gives its paper value, exercise cost and the odds the model will use (see Startup equity). |
| `get_schema(section, with_nested?)` | Fields, types, defaults and documentation for one schema type, from the engine's JSON Schema. |
| `propose_changes(summary, ops, assumptions[])` | Validates a patch (errors go back verbatim, without bothering the user), then shows the change card and reports the decision. |
| `run_simulation(paths?)` | Runs the plan; returns the digest: success / FI / depletion with intervals, age quantiles, net worth and spending bands at six ages, spending delivery, failure causes, representative path ids, and for a plan with private-company equity the `equity` block. |
| `try_scenario(name, ops, keep?, paths?)` | Applies a patch to a copy, runs plan and copy on the same lives, returns the paired comparison (success change with McNemar statistics, lives rescued and harmed, per-life change in FI age, retirement age, terminal net worth). With private-company equity it also returns `by_equity_outcome` and each side's `equity` block. `keep` saves it as a scenario. |
| `rank_levers(metric, custom_levers?, paths?)` | The engine's sensitivity levers plus model-proposed ones as one paired batch at reduced paths, ranked by effect on a metric. |
| `solve(variable \| path, target_metric, target_value, lo?, hi?)` | Bisection on the same lives, with `scenario.rs` `Solver` semantics: both ends first, `out_of_range` with the closest end if the target is not bracketed, then halving. |
| `sweep(x, y?, metric, paths?)` | A grid up to 8 by 8 at reduced paths. |
| `inspect_life(pick \| quantile+by \| path, run_id?)` | One traced life as a year table (age, income, spending, tax, net worth, status), its event log and scalars. With private-company equity the picks include `equity_failed`, `equity_paid_median` and `equity_paid_p90`, the log carries exercises, tender offers, IPOs, acquisitions and company failures, and the scalars carry the six equity figures with the outcome in words. |
| `explain_tax(path, year, run_id?)` | The 1040-style lines of one simulated year, from the trace. |
| `lookup_cost_of_living(from, to)` | Category factors from the city table, ready for a `relocate` event. |
| `show_chart(kind, caption, run_id?, compare_run_id?, data_id?)` | Asks the UI to draw a chart from a result the model already has. The model cannot supply data. |

Variables for `solve` and `sweep`: `annual_spending`, `retirement_age`,
`withdrawal_rate`, `salary_scale`, `tender_sell_fraction`, `exit_price_multiple`, or
any numeric field by patch path. The last two named ones act on the first company that
is counted in the plan and has holdings.

Path counts: `run_simulation` and `try_scenario` default to the household's setting, at
most 10,000; `rank_levers` and `solve` to 2,000; `sweep` to 1,000 per cell. Identical
inputs are served from an in-memory registry, so a baseline is simulated once however
many scenarios are compared with it.

Follow-up suggestions are not a tool: the model ends an answer with a `<followups>`
block, which the app turns into buttons and never shows as text.

### What comes from the engine and what is TypeScript

The paired comparison, the plan preview and the equity preview all come from the Rust
engine through the wasm wrapper (`compare`, `planPreview`, `equityPreview`). The engine
bridge (`tools/engine-bridge.ts`) calls them directly; there is no TypeScript fallback.

- Paired statistics are core's `Comparison`, computed life by life over every life of
  the two runs, off the main thread. The result gives the number of lives matched
  (`paired_paths`).
- `get_plan` carries the engine's plan preview in its `preview` section (left out
  while the plan has errors, since the engine cannot compile it) and, for a plan
  with private-company equity, the equity preview under
  `derived.private_equity`.
- Only the search behind `solve` and the grid behind `sweep` are TypeScript. They
  call the engine for every point; the Rust `Solver` state machine is not exported.

## Startup equity

A plan can hold stock, options and RSUs in private companies under `private_equity`
(methodology section 6.1). The assistant has no extra tool for it. What changes is the
prompt (version 3), what three tools return, and two solver variables.

**What the prompt asks for.**

- Model such holdings under `private_equity`, not as a job's grant and not as a balance.
- Ask for what a holder can read off an equity portal or grant letter (type, count,
  strike, vested, months left, grant date; the last round's price, the 409A value, the
  stage), and never for hazard rates, volatilities or a valuation of their own stake.
- Start from the stage's typical odds and state them from the plan view. Write a custom
  outcome table only when the user states a view, ask for it in frequencies, and say
  once, plainly, when it is far more optimistic than the stage's.
- Paper value is not money: it is in no balance and no net worth figure.
- Never present the mean of equity proceeds as what the equity is worth, and never give
  one value for someone's options.
- Test decisions (exercise now or later, what to do at a deadline, selling in tenders,
  selling after an IPO, leaving) as paired scenarios, and report the bad outcomes as
  well as the middle.
- Do not restate tax rules from memory; `explain_tax` shows a year's AMT and credit
  lines.

**The both-ways rule.** Whenever the assistant reports how a plan with private equity
does, it reports it as modelled and with the equity counted at zero, as a paired
comparison: `try_scenario` with `included` set to false on each company. It leads with
the at-zero figure when the user is deciding whether the plan can be relied on. One of
the worked examples in the prompt is this patch.

**Where the odds come from.** The grounding rule covers odds too. For each company
with a price, `get_plan` returns under `derived.private_equity`:

```
{ id, name, included,
  paper_value_vested, paper_value_total, exercise_cost_vested,    // at the last round's price; strike only
  share_of_net_worth,                  // paper value over accounts, property equity less debts, plus the paper value
  odds_in_force:  { fails, acquired, ipo, still_private },        // futures of 100 over the next ten years
  typical_odds:   { fails, acquired, ipo, still_private },        // the stage's, for comparison
  implied_multiple, typical_implied_multiple,                     // mean common price at the outcome over the last round's
  nothing_share, tender_before_share,
  spread_iso_vested, spread_nso_vested,
  fmv_409a, fmv_fraction, fmv_409a_basis, tender_annual_probability }
```

The first group is computed in the browser (`web/src/engine/equity-math.ts`, compared
with the engine by a test); the rest is the engine's `equityPreview` of the company.
The four odds are rounded to one decimal so that they add up to exactly 100. These are
the model's inputs. What happened in a run is the digest's block, below, and differs a
little: a company still private after ten years is drawn again, and a life can end
first.

**The digest block.** For a plan with private-company equity the engine's digest
(about 2.2 KB instead of 1.7 KB) carries:

```
equity: { company,
          failed, still_private, acquired, ipo,     // shares of lives
          nothing: {p, lo, hi},                     // the equity pays nothing at all
          loss,                                     // more went in through exercise cost and tax than came back
          net_proceeds: {p10, p50, p90},            // after exercise cost and tax, all lives, zeros included
          net_proceeds_mean, net_proceeds_mean_se,
          success_by_outcome[4] }                   // failed, still_private, acquired, ipo; null under 30 lives
```

`run_simulation` returns it as it is. `try_scenario` returns it for both sides, and
adds to the comparison `by_equity_outcome`: for each of the four outcomes the number of
lives, success under the plan and under the scenario, the change with its 95% interval
and `within_noise`, and the median per-life change in final net worth. The outcome is
a fate drawn before any decision, so it is the same life by life in both runs unless
the scenario edits the outcome table.

**"What exit do I need?"** is `solve` on `exit_price_multiple`: the company's outcome
table is replaced by a certain listing at that multiple of the last round's price
(0.25 to 20), dated at the middle of the stage's typical years to an IPO. The result
carries a note saying exactly that, with the last round's price in dollars, and gives
the exit price itself in dollars per share (`value_dollars_per_share`, and
`dollars_per_share` on each side of the bracket; a sweep gives
`values_dollars_per_share`), so the model quotes the dollar figure from the result
and never multiplies. It is a way to ask what a goal needs, not a forecast.

Not built: a chart kind for equity (the at-zero pair uses `compare_delta`), and picking
a life by a quantile of equity proceeds (the engine's per-life sample does not carry
that figure; the three equity picks cover the common cases).

## Charts in the chat

`show_chart` renders a card. Chart components are registered per kind:

```ts
import { registerAssistantChart } from '@/assistant';
registerAssistantChart('fan', () => import('@/views/overview/FanCompact'));
```

The default export receives `AssistantChartProps` (`web/src/assistant/charts.ts`):
`{ kind, compact: true, caption, summary, profile, comparison?, ranking?, grid?, trace? }`.
Kinds: `fan`, `fi_age`, `status_stack`, `tornado`, `compare_delta`, `heatmap`,
`cash_flow`, `life_story`. Unregistered kinds show a labelled placeholder with the
caption. Runs live in memory only, so a chart in a restored conversation shows a note
instead.

## Entry points for other views

```ts
import { askAssistant, reviewPlan } from '@/assistant';
askAssistant('Why do 9% of lives run short?', { view: 'outcomes', depletion_rate: 0.09, paths: 10000 });
reviewPlan();
```

`askAssistant(prompt, context?)` opens the panel and sends the prompt. `context` is any
small JSON-serialisable object; the model receives it as data next to the question.

## Privacy: what leaves the browser

Simulation always runs on the device. Nothing is sent anywhere until the user sends a
message. Then one request goes to Claude containing:

- the user's messages and any files they attached (images and PDFs, read in the browser
  and sent as content blocks);
- the plan, as the compact `get_plan` rendering;
- the compact results of the tools the model called (digests, comparisons, one life's
  yearly table), never raw simulation output;
- the system prompt and tool definitions.

**With your own key** the request goes straight from the browser to `api.anthropic.com`.
The key is stored in this browser's `localStorage` only and is sent only there.
**In hosted mode** the same request goes to this site's `/api/chat`, which adds the
operator's key and forwards it; the function stores nothing and logs nothing (it contains
no logging call). Either way Anthropic processes the request under the terms of the API
account used; Claude Fable 5.2 requires that account to have 30-day data retention.

Conversations are saved in this browser's IndexedDB (`firesight-assistant`), per
household, including attached files. Clearing the conversation deletes them.

The prompt tells the model never to copy account numbers or other identifiers from
documents into the plan or its replies.

## Hosted mode on Vercel

`api/chat.ts` is a Node.js-runtime Vercel Function with the Web handler signature and no
dependencies. It is inactive until its key is set.

| Variable | Required | Meaning |
|---|---|---|
| `ANTHROPIC_API_KEY` | yes | Enables the proxy. Give it its own workspace with a spend limit. |
| `FIRESIGHT_ACCESS_CODE` | no | A shared code visitors must enter (assistant options) before the proxy serves them. |
| `FIRESIGHT_MODELS` | no | Comma-separated subset of the allow-list, to offer fewer (cheaper) models. |
| `FIRESIGHT_RATE_PER_MINUTE` / `FIRESIGHT_RATE_PER_DAY` | no | Requests per client address. Defaults 30 and 600. One user turn is several requests. |
| `FIRESIGHT_ALLOWED_ORIGINS` | no | Extra origins allowed to call the proxy (preview domains). |

`GET /api/chat` returns `{ available, models, access_code_required }`; the app probes it
when no key is set and prefers the user's own key when there is one. A long turn can
stream for minutes: if answers are cut off, raise the function's `maxDuration` in
`vercel.json` (`"functions": { "api/chat.ts": { "maxDuration": 300 } }`).

What the proxy checks before forwarding, and what each check is worth:

- **Same origin.** `Origin` must match the host. Stops other websites from spending the
  key through their visitors' browsers. Does not stop a script, which can send any header.
- **Access code**, if configured. The only check that keeps out someone who has read
  this source code; it is a shared secret, so treat it as such.
- **Rate limit** per address, in the function instance's memory. Each instance counts
  separately and forgets on recycle, so it is a speed bump, not a quota. For a real
  quota, put a shared store or Vercel's firewall rate limiting in front.
- **Size.** Bodies over 4 MB are refused (the platform's own limit is 4.5 MB), which also
  bounds attachments in hosted mode.
- **Shape.** Allow-listed model; `max_tokens` at most 32,000; streamed; the system prompt
  must start with the FIRESight marker (version 3; a browser tab still running an older
  build is refused until it reloads); the tools must be exactly the twelve by name with
  no server tools; adaptive thinking only; no other parameters (no MCP servers,
  containers, structured outputs, custom fallback chains); only the two beta features
  the app uses are forwarded. This makes the endpoint awkward as a general-purpose
  model API. It does not make it impossible: the marker and tool names are public, and
  a determined caller can wrap any question in a valid-looking request.

The backstop that actually bounds cost is the spend limit on the API key's workspace.

## Limits

- **Grounding is instructed and auditable, not enforced.** The stronger scheme in the
  research proposal (numbers written as references to tool-result fields and substituted
  by the client, with unreferenced literals rejected) is not built.
- **No evaluation set yet.** Prompt changes are not checked against profiles with known
  engine answers.
- **Live behaviour is unverified in this checkout.** The loop, tools, proxy and UI are
  tested against a scripted model and a fake simulation pool. `live.test.ts` runs a real
  conversation when given a key (`FIRESIGHT_LIVE_SMOKE=1`).
- **Reduced-path searches are approximate.** `rank_levers`, `solve` and `sweep` trade
  precision for speed; the prompt tells the model to confirm with `try_scenario`.
- **`solve` assumes a monotone response** and reports the bracket, not an interval on
  the solution. For a household that retires when funded, a better exit mostly buys an
  earlier retirement, so success does not rise steadily with `exit_price_multiple`;
  final net worth and retirement age do.
- **Startup equity guidance is instructed, not enforced.** Nothing checks that an
  answer gave the at-zero figure, or that it did not quote a single value for a
  position.
- **Charts and runs are session memory.** After a reload, earlier answers keep their
  text and their tool results, but charts need the run again.
- **Hosted mode caps attachments** at about 2.5 MB per message and compacts earlier.
- **Documents are read by the model, not parsed.** Extraction quality is the model's;
  every extracted value still goes through a change card.
- **One conversation per household**, in one browser. Deleting a household does not yet
  delete its conversation (`deleteConversation` is exported for that).
