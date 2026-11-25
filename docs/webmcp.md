# Browser agents (WebMCP)

FIRESight offers its own tools to an AI agent running in the user's browser, through
[WebMCP](https://github.com/webmachinelearning/webmcp): the page registers named tools
with `document.modelContext`, and an agent calls them instead of reading pixels and
clicking. The agent can read the open plan, build one from a description, simulate,
compare what-ifs, propose changes and put the right chart in front of the user. The user
sees all of it and decides what is allowed.

Code: `web/src/webmcp/`. End-to-end check: `web/e2e/webmcp.mjs`.

WebMCP is experimental. As of November 2026 it exists only in Chromium browsers behind a
flag or an origin trial (see Browser support). Where the API is absent, which is almost
everywhere today, FIRESight does nothing: no code is downloaded and nothing is logged.

## Principles

1. **The user sees what the agent does, and decides.** The first call that would read or
   act on the plan stops at a prompt on the page. Changes to the plan are shown on a card
   and wait for Accept, Edit or Decline. A pill in the top bar says when an agent is at
   work, and a log lists its recent calls. Settings can switch the whole thing off.
2. **The engine is the only source of numbers.** The tools are the in-app assistant's
   executors, reused unchanged, plus tools that read what the page itself has computed.
   An agent that follows `get_guide` never has to estimate a figure.
3. **A tool call always answers with JSON.** `{ ok: true, … }` or
   `{ ok: false, error: { code, message }, … }`. Nothing is thrown at the agent except
   when the agent itself aborts the call. Error messages say what to do next.
4. **Results are small and say what they are.** Headline figures and digests, never the
   raw simulation output; every result is capped at 16,000 characters of JSON. Units are
   stated in the descriptions and in the results.
5. **Charts stay on the site.** A WebMCP result is text. The agent navigates the visible
   app to a view (`show_view`) so the user sees the picture, and reads the same chart as
   words and rows (`list_charts`, `get_chart_data`).

## Try it

Chrome 150 or later, desktop. (The end-to-end script was last run on 2026-11-24 on
Chrome 158.0.8127.38, and on 2026-11-20 on Chrome 157.0.8103.80 and Chromium
159.0.8154.6.)

1. Open `chrome://flags/#enable-webmcp-testing`, enable "WebMCP for testing" and
   relaunch. Or start Chrome with `--enable-features=WebMCP`.
2. Open FIRESight. Settings, under **Browser agents**, says whether this browser offers
   WebMCP to the page and how many tools were registered.
3. DevTools, Application, **WebMCP** lists the tools and has "Run tool". Or from the
   console:

   ```js
   const tools = await document.modelContext.getTools();
   const call = async (name, input = {}) =>
     JSON.parse(await document.modelContext.executeTool(tools.find((t) => t.name === name), input));
   await call('get_app_state');
   await call('open_demo', { slug: 'dual-income-family' });   // the page asks you first
   await call('get_results');
   ```

4. From a coding agent, the Chrome DevTools MCP server can drive the same tools:
   `chrome-devtools-mcp --categoryExperimentalWebmcp --chromeArg=--enable-features=WebMCP`.

The automated check plays an agent through the browser's own API:

```
cd web
npm run dev                                   # or: npx vite --port 5174 --strictPort
BASE_URL=http://localhost:5173 npm run webmcp
```

It launches the installed Chrome (falling back to Playwright's Chromium; set
`WEBMCP_BROWSER=chromium` to use that one, after `npx playwright install chromium`) with
the flag, prints the browser version, and exits 0 with a "SKIPPED" line if that browser
has no `document.modelContext`.

## Tools

Twenty-four, registered at startup before the engine has loaded; each call waits for the
app to be ready. Annotations: R = `readOnlyHint`, C = `consequentialHint`,
U = `untrustedContentHint` (the result can carry text a person typed).

| Tool | What it does | Hints |
|---|---|---|
| `get_app_state` | Call first. Readiness, whether agents are allowed, the open plan (id, name, error and warning counts), the active view and the views there are, the page's run status, saved plans (id and name), demos, signed-in flag. No figures. | R U |
| `get_guide` | The rules an outside agent needs: grounding, units, the definitions of success, fi, depletion and home_sale, paired runs and `within_noise`, patch syntax, model simplifications. `topic: "schema"` returns the map of every plan field. Each answer is under 8,000 characters. | R |
| `create_plan` | A new plan from full or partial profile JSON (or an exported plan file), completed and validated by the engine, saved in this browser and opened. Returns the validation report. | C U |
| `open_demo` | Opens a built-in example household as a new plan. | C U |
| `open_plan` | Opens a plan saved in this browser, by id. | C U |
| `get_plan` | The open plan, pruned of defaults, with ids, derived facts, the pre-simulation preview, validation and scenarios. | R U |
| `get_schema` | One type of the plan schema: fields, types, enums, defaults, documentation. | R |
| `edit_plan` | A patch to the open plan. Validated first, then shown to the user on a change card. | C U |
| `run_simulation` | A detailed digest of a fresh run of the plan (probabilities with intervals, ages, net worth and spending by age, failure causes, representative lives). | R |
| `try_scenario` | A what-if on a copy, paired with the plan on the same lives. `keep: true` asks the user to save it as a scenario. | U |
| `rank_levers` | The engine's sensitivity levers, and any the agent adds, ranked by effect. | R U |
| `solve` | Goal seek: the value of one quantity at which a metric reaches a target. | R |
| `sweep` | One metric over a grid of one or two quantities. | R |
| `inspect_life` | One simulated life, year by year, with its event log. | R U |
| `explain_tax` | The tax return of one life in one year, line by line. | R |
| `lookup_cost_of_living` | Price multipliers between two cities, from the app's table. | R |
| `show_view` | Navigates the visible app to a results view, or opens one simulated life in Life stories. | none |
| `get_results` | Waits for the page's own run to settle, including this run's scenario comparisons, then returns the headline figures the user sees, the plan facts and each enabled scenario's paired difference. The run store keeps the previous comparison on show while a new run is under way (`ScenarioRun.pairing` says which it is); `get_results` never reports that one. | R U |
| `list_charts` | The charts of the visible view: id, title, the takeaway sentence, the caption, whether there is a table. | R U |
| `get_chart_data` | The rows behind one chart (the same table the page offers), paged. | R U |
| `list_scenarios` | The saved scenarios with their patches. | R U |
| `set_scenario_enabled` | Switches a scenario on or off. | U |
| `remove_scenario` | Deletes a scenario, after the user confirms. | C U |
| `undo_agent_edit` | Undoes the latest change, only if this agent made it. | C U |

Twelve of these wrap the assistant's tools (`web/src/assistant/tools`) through the same
`executeTool`, with their input schemas unchanged and descriptions rewritten for an agent
that has no system prompt of ours. `edit_plan` is `propose_changes` under a plainer name.
`show_chart` is not offered: it draws in the assistant panel, which a browser agent has
no part in. The agent surface keeps its own run registry, apart from the assistant's;
run ids are dropped when another plan is opened.

Never offered, by any tool: the Claude API key, the hosted access code, share links (a
share link is the whole plan), assistant conversations, `askAssistant` (it would spend
the user's key), the contents of plans that are not open, settings, and anything to do
with an account or its API keys.

### How a call runs

`core.ts` is the one path every call takes:

1. Read the abort signal from the second argument if the browser passed one (Chrome 153
   and later); otherwise the call simply cannot be cancelled.
2. Refuse if agent access is off.
3. Validate the input against the tool's JSON Schema. WebMCP does not do this, so the
   page does, with the validator the assistant uses. Chrome before 155 passed input as a
   JSON string; that is parsed first.
4. Wait for the app to be ready (the engine loaded, the last plan restored).
5. Ask the user, if the tool needs consent and they have not answered yet.
6. Run the tool, then refuse a result over the size cap rather than truncate it blindly.

### Architecture

```
main.tsx ── register.ts ──(only if document.modelContext exists)──► runtime.ts ── tools.ts
              feature detect,                                         real stores,   24 tool specs
              AbortSignal per batch                                   worker pool    over a context
host.tsx (pill, log, Settings section) ── agent-store.ts ── ui.tsx (consent prompt, cards; lazy)
```

- `register.ts` is the only file that touches the browser's API. It feature-detects
  `document.modelContext` (falling back to `navigator.modelContext` for early builds),
  registers every tool under one `AbortController`, tolerates `registerTool` returning
  nothing or a promise, and withdraws the whole batch if the browser refuses one tool.
  Calling it again replaces the earlier registration, so StrictMode and hot reloads are
  safe.
- `runtime.ts`, `tools.ts` and `guide.ts` are a separate chunk, fetched only when the
  browser has WebMCP. `ui.tsx` (the prompt and the cards) loads when an agent first
  needs the user.
- Tools are specs over an injected context (`types.ts`), as the assistant's are, so the
  unit tests run all of them in node against the real engine with a stubbed model
  context (`webmcp.test.ts`).
- `guide.ts` imports nothing and takes the tool names as parameters, so the hosted MCP
  server can serve the same text.
- `chart-registry.ts`: every `ChartCard` announces itself while mounted. Only charts
  that are drawn in the results view count; the previous view, which React keeps mounted
  but hidden while the next one loads, does not.

## Consent

Two settings, under Settings, **Browser agents**, saved in this browser:

- **Agent access**: *Ask first* (the default), *Always allow*, or *Off*.
- **Apply agent changes without asking**: off by default.

| Access | What an agent gets |
|---|---|
| Off | Every tool answers `agent_access_off`, except `get_app_state`, which reports the mode and nothing about the plan. Switching to Off also takes down any card that is waiting, and a call that was already running answers `agent_access_off` instead of its result. |
| Ask first | `get_app_state`, `get_guide` and `get_schema` answer freely; none of them returns anything from the plan (before consent, `get_app_state` says only that a plan is open). The first other call shows a prompt: "An AI agent in this browser wants to read and work with your plan", with **Allow for this session**, **Always allow** and **Don't allow**. Calls wait for the answer. The answer lasts until the tab is reloaded or closed. |
| Always allow | No prompt. Changes still need approval unless the second setting is on. |

The prompt is a card in the corner, not a modal dialog: the page stays usable, for the
user and for an agent driving it. If the user does not answer within two minutes the
call returns `user_did_not_respond` and the prompt stays up; the answer, when it comes,
covers later calls. Declining returns `agent_access_denied` for the rest of the page
session without asking again.

Changes go through approval unless the user turned on automatic apply:

- `edit_plan` shows the same change card the assistant uses (each edit as before and
  after, the agent's stated assumptions), with Accept, Edit and Decline. It works with
  the assistant panel closed and with no API key. A patch that does not apply, or that
  would add validation errors, goes back to the agent without the user being shown it.
- `try_scenario` with `keep`, `remove_scenario`, and `create_plan`, `open_demo` or
  `open_plan` when the open plan has changes from this session, show a plain confirmation.
- An unanswered card is withdrawn after five minutes, so a stale proposal cannot be
  accepted later. Aborting the call withdraws it at once.
- A card belongs to the plan it was raised for. When another plan is opened (by the
  user, by account sync, or by the agent's own `open_demo`, `open_plan` or
  `create_plan`), or the same plan is loaded again, every waiting card is withdrawn
  and its call returns `plan_changed` with nothing written (`withdraw.ts`). The tools
  check the open plan's id again after the answer, so this holds even for an answer
  already on its way. `try_scenario` with `keep` returns its comparison with
  `saved_scenario: { saved: false, reason: "plan_changed" }`.

Every applied change is one undo step on the baseline plan, labelled "Agent: …".
`undo_agent_edit` reverts the latest change only if the agent made it; it never steps
back over something the user, or the in-app assistant, did. With automatic apply on,
each change raises a notice with an Undo button.

There is no page-side consent API in WebMCP itself; `consequentialHint` only tells the
agent's host that a tool matters. The prompts above are FIRESight's own.

## Privacy

Nothing changes for a visitor whose browser has no WebMCP, or who never uses an agent.

When an agent calls a tool, the result leaves the page: it goes to that agent and to the
AI service behind it, which FIRESight neither runs nor controls. That is the point of
the consent prompt, and it says so. What can go: the plan as `get_plan` renders it,
digests of simulations, one life's yearly table, chart rows. What cannot: see "Never
offered" above.

The activity log keeps the tool name, the time, the duration and the outcome of the last
50 calls, in memory, for this page session. It never holds inputs or results.

Names and notes in a plan were typed by a person, and a plan can arrive from a share
link someone else made. Results that can carry such text are marked
`untrustedContentHint`, and the tool descriptions tell the agent to treat it as data,
not instructions. That is advice to the agent, not a guarantee about its behaviour.

## Browser support

| Browser | Status (November 2026) |
|---|---|
| Chrome 150 to 162, desktop and Android | Behind `chrome://flags/#enable-webmcp-testing` or `--enable-features=WebMCP`; or for ordinary visitors of a site that serves an origin-trial token. No ship milestone has been announced. |
| Chrome 146 to 149 | An earlier shape on `navigator.modelContext`. Registration is tolerated on a best-effort basis and is not tested. |
| Edge | Reported by the spec repository's status file as supported from 150; not verified here. |
| Firefox | A prototype of an older draft behind preferences. Not supported. |
| Safari | No implementation. |

Details that vary by Chrome version and are handled: `registerTool` returns nothing
(150) or a promise (151 on); `execute` gets `{ signal }` only from 153; input arrives as
a JSON string before 155. Whether Gemini in Chrome, or any other built-in agent, calls
page tools today is unproven; the confirmed consumers are DevTools, the Chrome DevTools
MCP server, Puppeteer, Playwright, extensions and in-page agents.

### The origin trial

To offer the tools to visitors who have not set a flag, a deployment needs an origin
trial token for its own origin:

1. Register the site's origin for the "WebMCP" trial (id `4163014905550602241`) at
   <https://developer.chrome.com/origintrials/#/register_trial/4163014905550602241>.
   The trial covers Chrome 149 to 162 and ends on 2027-03-30.
2. Set `VITE_WEBMCP_ORIGIN_TRIAL_TOKEN` to the token in the build environment (on
   Vercel: Project Settings, Environment Variables) and rebuild.

The build then writes `<meta http-equiv="origin-trial" content="…">` into the page
(`web/vite.config.ts`). Unset, nothing is added. A token is tied to one origin, so
preview deployments on other hostnames do not get the API from it.

## Limits

- **Experimental API.** The spec is a draft and Chrome's implementation has changed
  shape several times. The tools were exercised end to end on Chrome 157 and 158 and
  Chromium 159; other versions are covered by unit tests against stubbed shapes of the API, not
  by a real browser.
- **A user who does not answer** holds a call open for up to two minutes (consent) or
  five (a change card). The call can be aborted from Chrome 153.
- **One agent surface per page.** The page cannot tell two agents apart, so consent, the
  run registry and "the agent's own edits" are per page session, not per agent.
- **Charts are read as text.** `list_charts` covers charts drawn through `ChartCard` in
  the visible view. An agent that wants a picture has to look at the page.
- **This origin only.** The tools are registered without `exposedTo`, so pages of other
  origins cannot see them.
- **The plan in this browser only.** These tools work on the plan open in the page, and
  `open_plan` lists the plans saved in this browser (which includes local copies of
  synced ones). They do not reach into an account. Working on a plan from outside the
  browser is the hosted MCP server's job: see [mcp.md](mcp.md) and
  [accounts.md](accounts.md).
- **Not seen by a real agent yet.** The checks drive the browser's own `getTools()` and
  `executeTool()`. No built-in agent, extension or DevTools MCP session has been run
  against the tools, and no origin-trial token has been tried on a deployed origin.
