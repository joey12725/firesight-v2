# Accounts and synced plans

FIRESight works without an account, and that stays the default: a plan is made in a
browser, saved in that browser, and sent nowhere. An account is an optional extra for
two things the browser alone cannot do:

- keep chosen plans on the site, so they open on another device;
- let an AI tool outside the browser (Claude, Claude Code, ChatGPT, Cursor, VS Code)
  read, build and change those plans through the [hosted MCP server](mcp.md), with the
  result showing up on the website.

A deployment does not have to offer accounts at all. Without a database behind it the
site is the static, local-first app it always was, with no sign-in button anywhere.

Code: `api/auth.ts`, `api/plans.ts`, `api/wellknown.ts`, `api/_lib/` (the server);
`web/src/auth/`, `web/src/state/cloud*.ts`, `web/src/shell/cloud-ui.tsx` (the app);
`db/` (migrations and the embedded development database). Tests: `api/_tests/`,
`web/src/state/cloud.test.ts`, `web/src/state/households.test.ts`,
`web/src/auth/account.test.ts`, `web/src/auth/delete-account.test.ts`. End-to-end checks:
`npm run e2e:api` (the API over HTTP) and `npm run accounts` in `web/` (a browser
through sign-up, an agent writing a plan, sync, conflicts and account deletion).

**Status.** Everything described here runs and is tested locally, against an embedded
Postgres and a real Postgres 16. None of it has been deployed yet. The OAuth sign-in
that hosted assistants use has been driven by test clients only, not by claude.ai or
ChatGPT. Treat the first deployment as a trial and follow the checklist at the end.

## What reaches the server, and what does not

| | Signed out, or no accounts | Signed in |
|---|---|---|
| A plan you have not synced | Stays in the browser | Stays in the browser |
| A plan with "Sync to account" on | (not possible) | Stored on the server, updated on every save |
| A plan an AI tool you connected creates | (not possible) | Stored on the server, then copied into your browser when you open it |
| Assistant conversations, attached files | Stay in the browser | Stay in the browser |
| Settings, your Anthropic API key | Stay in the browser | Stay in the browser |
| Share links | The plan is in the link's fragment; no server sees it | The same |

A plan reaches the server in exactly two ways: you switch on **Sync to account** for it
(in the household switcher; off by default for every plan made in the browser), or an
AI tool you authorised writes it to your account. Signed out, the app makes no request
about plans at all, and the sync code is not even downloaded.

On a site that has accounts, every page load asks the server two things: whether
accounts exist (`GET /api/config`) and who is signed in (`GET /api/auth/get-session`).
Neither carries anything about a plan.

**The server can read synced plans.** They are encrypted where they are stored, but with
a key the server holds, because the server has to read a plan to validate it and to run
simulations for the AI tools you connect. This is encryption at rest, not end-to-end
encryption, and nothing in the app claims otherwise. If that is not acceptable for a
plan, do not sync it: the unsynced plan loses nothing but the features above.

## Signing in

Email and password (at least 12 characters), and GitHub or Google where the operator
has set them up. The session is an HttpOnly cookie; the app keeps no token in
`localStorage`.

Limits worth knowing before relying on it:

- **There is no password reset and no email verification.** The site sends no email. A
  forgotten password means the account cannot be recovered; plans already in a browser
  stay there.
- **Sign-up says when an address already has an account** (it answers 422). With no
  email to send, the server cannot give every address the same "check your inbox".
  Sign-in does not say: a wrong password and an unknown address get the same answer.
  Sign-ups are limited to 10 per five minutes per network address.
- **Signing out leaves local copies.** Plans that were synced stay in the browser they
  were opened in, with a "sync paused" mark. They are not uploaded again until the same
  account signs back in.
- **Two tabs of one browser share what is stored, not what is open.** Whether a plan is
  synced is part of its stored record, so turning sync on or off in one tab holds in
  every tab: the others are told at once, and each also checks the stored record before
  it saves or uploads, so a tab that missed the message still sends nothing once sync is
  off. The contents of a plan open in two tabs are not merged: each tab saves what it
  has open, and a synced plan catches up with the other tab's edits through the
  account, within about ten seconds.
- **One account per browser at a time.** The session cookie is shared by every tab. If
  another tab signs out and a different account signs in, a tab still showing the first
  account has its next plan request refused (it names the account it is acting for, and
  the server compares that with the cookie). The tab then stops syncing, before it
  unlinks or uploads anything, asks who is signed in, and carries on as that account.

Pages: `/login` (sign in or create an account; continues a pending request from an AI
tool), `/consent` (an AI tool asking for access), `/account`, and `/p/<plan id>` (the
link an AI tool gives after writing a plan: it signs you in if needed, opens the plan,
and then becomes the ordinary address of the view).

## Sync

A synced plan in the browser carries a link to its copy in the account: the plan's id
there, the revision (`rev`) the browser last agreed with, and who wrote it.

- **Saving** uploads the plan with `If-Match: <rev>`. The server writes only if the
  account is still at that revision, and the revision goes up by one.
- **Someone else wrote first** (an AI tool, or another device): the upload is refused
  with 409 and the app asks: **Load the newer version**, or **Keep mine as a copy**
  (your version becomes a new local plan; nothing is lost either way).
- **While the tab is visible** the app asks for the list of plans about every ten
  seconds and when the window regains focus. If the open plan has a newer revision and
  no unsaved changes, it is loaded and a notice says who updated it. This is what makes
  an agent's edit appear on the page while you watch.
- **A plan an agent creates** raises a notice with an **Open** button, and appears under
  "In your account" in the switcher.
- **Offline**, a save stays in the browser, marked as not yet uploaded, and is retried
  with a growing delay and on every poll. The mark survives a reload.
- **Turning sync off** asks whether to delete the account's copy or keep it. **Deleting
  a synced plan** asks whether to delete it from this browser only or everywhere.
- A plan synced by one account is left alone when a different account signs in on the
  same browser, in this tab or in another one: it keeps its link, is not uploaded, and
  is not reported as deleted. Turning its switch on as the other account uploads it
  there as a new plan, and only then.

Limits: 50 plans per account, 256 KB per plan.

## AI tools: two ways in

Both are on the account page under **Connect an AI**.

**OAuth sign-in**, for clients that sign the user in (Claude's custom connectors,
ChatGPT, Claude Code and Cursor when no key is given). The client is sent to this
site's `/login` and `/consent`. The consent page shows what the app calls itself, the
identifier it is registered under, the host the browser will be sent back to, and what
it is asking for in plain words. A name is chosen by the app and proves nothing; the
identifier and the return address are what to check. **Connected apps** on the account
page lists them, and disconnecting one cuts it off on its next call.

**API keys**, for clients that can send a header. A key is read-only or read-write,
has a name and an optional expiry, and is shown once. The list shows the first few
characters, when it was created and when it was last used. Revoking a key takes effect
on its next call. A key is as good as a password for the plans in the account.

Neither credential works on the website's own API, and the session cookie does not
work on the MCP endpoint: the three are kept apart on purpose, so a leaked credential
of one kind is useless against the other surface.

What an AI tool reads goes to that tool and to the AI provider behind it. FIRESight
neither runs nor controls them.

## Data model

Better Auth owns the account tables (`user`, `session`, `account`, `verification`,
`apikey`, `jwks`, the `oauth*` tables, `rateLimit`). FIRESight adds three
(`db/migrations/001_plans.sql`):

| Table | What it holds |
|---|---|
| `user_key` | One row per user: that user's data key, wrapped by the server key. Deleted with the user. |
| `plan` | One row per synced plan: `id` (`pl_` and 22 random letters and digits), owner, the encrypted name and document, size, `rev`, who last wrote it (`web`, `apikey:<key name>` or `oauth:<app name>`), timestamps. Deleted with the user. |
| `firesight_rate` | Request counters for rate limits. A user's counters are removed with the account. |

There is no row-level security in the database. `api/_lib/plans.ts` is the only module
that touches `plan` and `user_key`, and every statement in it names the owner as well as
the id. The owner always comes from a verified credential, never from a request body.
A plan that belongs to someone else answers exactly as one that does not exist (404).

The server checks a plan's envelope (`{ firesight: 2, profile, scenarios, run? }`) and
size. The profile inside is validated by the engine: in the browser before upload, and
in the MCP server before anything is stored or simulated.

When the app loads a plan from the account it checks the profile with the engine again,
and gives a new id to any scenario whose id is missing, is `base` (the app's name for
the plan itself) or repeats another scenario's, whoever wrote it there.

## Encryption

```
PLAN_KEK  (a 32-byte key in the server's environment)
  └─ wraps one random 256-bit data key per user            table user_key
       └─ encrypts each plan's name and document            table plan
```

AES-256-GCM throughout, a fresh nonce per write. Each ciphertext is bound to where it
belongs (the user for a data key; the user, the plan and the field for a name or a
document), so a row copied onto another user or plan fails to decrypt.

What this protects against: a database dump, a backup, a leaked connection string, the
database provider's console. All of those see ciphertext for names and documents. What
they still see: that a user has plans, how many, their sizes, revisions, timestamps and
the name of the key or app that last wrote each.

What it does not protect against: the server itself, or anyone who has both the database
and `PLAN_KEK`.

**Losing `PLAN_KEK` makes every synced plan unreadable.** Keep a copy somewhere safe.

### Rotating the server key

1. Move the old key to `PLAN_KEK_V<old version>`, put the new one in `PLAN_KEK`, raise
   `PLAN_KEK_VERSION`, and deploy. Old data stays readable through the old key. Each
   user's data key is rewrapped under the new one the next time their plans are used,
   reads included.
2. Run `npm run rewrap` with the same database and key variables as the deployment. It
   rewraps the data keys of everyone who has not been back, and prints how many data
   keys are on each version. `npm run rewrap -- --check` only prints the counts. It
   touches the `user_key` table only, decrypts no plan, and is safe to run while the site
   is serving and to run again.
3. Remove `PLAN_KEK_V<old version>` only after `npm run rewrap` reports every data key on
   the current version (it exits 0). Until then some users' plans can be read only with
   the old key. Keep a backup of the old key afterwards too: database backups taken
   before the rotation need it.

A rotation cannot be rolled back by redeploying the previous build. From the first
request after step 1, users' data keys move to the new version, and a deployment that
does not hold the new key answers 500 `unreadable` for those users (and for anyone who
signed up since) until you roll forward again. Nothing is lost while both keys exist.

## Export and deletion

- **Export** (account page) downloads every plan in the account as one JSON file.
- **Deleting a plan** removes its row. There is no recycle bin.
- **Deleting the account** takes the password, and the server refuses without it (400
  `PASSWORD_REQUIRED`). An account that has only ever signed in with GitHub or Google
  has no password; it can be deleted without one from a session less than a day old, so
  an older session has to sign in again first. The account page asks for whichever
  applies (the password, or the account's email address typed out) and says so when the
  session is too old. Deletion removes the user, their
  sessions, API keys, OAuth consents and tokens, plans, data key and rate counters.
  With the data key gone, a copy of the ciphertext in an old backup can no longer be
  read. Plans already in a browser stay in that browser.
- AI tools cannot delete plans. There is no such tool.

## What the server refuses

- **Cross-site requests.** Every write to `/api/plans` must carry this site's exact
  `Origin` and a JSON body; reads refuse a request that names another site. Better Auth
  applies the same check to its own endpoints.
- **Bearer credentials on the website's API**, and cookies on the MCP endpoint.
- **Rate limits**, counted in the database so they hold across function instances: 10
  sign-ins a minute and 10 sign-ups per five minutes per address; 600 plan reads and 240
  writes a minute per user; 120 calls a minute per API key. The MCP endpoint's own limits
  are in [mcp.md](mcp.md).
- **Logging.** The functions contain no logging call, so neither a credential nor a plan
  can end up in a log. (The local development server prints request lines: method, path
  and status.)

OAuth details: the authorisation server is Better Auth's (`/api/auth`, discovery at
`/.well-known/oauth-authorization-server`), authorisation code with PKCE (S256) only,
access tokens are JWTs bound to `<site>/api/mcp` as their audience and last an hour.
Clients identify themselves with a Client ID Metadata Document; Dynamic Client
Registration is off unless `FIRESIGHT_OAUTH_DCR=1`, because it lets anyone create
client rows. The library's endpoints that let a signed-in user create, edit, delete or
re-key a client are switched off in both cases (404): sign-up is open, so they would be
registration by another door, and nothing here uses them.

## Running it locally

No cloud resource is needed.

```bash
npm install && npm install --prefix web
npm run wasm --prefix web      # once: builds the engine (needs Rust)
npm run dev:api                # the functions on http://localhost:8787, embedded Postgres under .local/
npm run dev --prefix web       # the app on http://localhost:5173, sending /api and /.well-known to 8787
```

`dev:api` migrates its database on start and generates development secrets beside it in
`.local/` (gitignored). `FIRESIGHT_DEV_DATA=memory` keeps nothing. If Vite runs on
another port, start the API with `BETTER_AUTH_URL` set to Vite's origin, or sign-in is
refused as cross-site. Plain `npm run dev` with no API running behaves as before: no
account UI and no errors.

```bash
npm test                 # the server's unit tests, in-process on the embedded database
TEST_DATABASE_URL=postgres://… npm test    # the same through the `pg` driver on a real Postgres (CI runs both)
npm run e2e:api          # the API over HTTP
npm run mcp:smoke        # an MCP client through the hosted endpoint
BASE_URL=http://localhost:5173 npm run accounts --prefix web    # the browser journey (needs dev:api and vite)
```

## Self-hosting on Vercel

The steps below have been written from the code and from a local `vercel build`, not
from a real deployment. Do them on a preview first.

1. **Database.** In the Vercel project, Storage, add Neon (Marketplace). It sets
   `DATABASE_URL` (pooled) and `DATABASE_URL_UNPOOLED` (direct). Any Postgres 14 or later
   works; only `DATABASE_URL` is required.
2. **Secrets.** Add these as environment variables, marked Sensitive:

   | Variable | Value |
   |---|---|
   | `BETTER_AUTH_SECRET` | `openssl rand -base64 32` |
   | `PLAN_KEK` | `openssl rand -base64 32` (exactly 32 bytes). Back it up. |
   | `BETTER_AUTH_URL` | The site's public origin, e.g. `https://example.com`. Optional in Production, where the project's production domain is used. |

   Optional: `GITHUB_CLIENT_ID`/`GITHUB_CLIENT_SECRET`, `GOOGLE_CLIENT_ID`/
   `GOOGLE_CLIENT_SECRET` (callback `<origin>/api/auth/callback/<provider>`),
   `FIRESIGHT_OAUTH_DCR=1`, `FIRESIGHT_ALLOWED_ORIGINS`,
   `FIRESIGHT_MCP_ALLOWED_ORIGINS`. All are described in `.env.example`.
3. **Migrate.** With the database URLs in your shell (`vercel env pull .env.local`, then
   export them):

   ```bash
   npm install
   npm run migrate        # creates Better Auth's tables and FIRESight's; safe to run again
   ```

4. **Deploy.** `vercel.json` already installs the root package, builds the engine for the
   MCP function and routes `/api/auth/*`, `/api/plans/*`, `/api/config` and the
   `/.well-known/*` documents.
5. **Check.** `GET /api/config` should answer `{"accounts":true,…}`. If it says `false`,
   one of the three required values is missing; if another endpoint answers 503
   `accounts_misconfigured`, its message names the variable that is malformed.

A preview deployment has no accounts unless it is given a `BETTER_AUTH_URL` of its own,
because cookies and tokens are bound to one origin.

For the in-page WebMCP tools, which need no account, the optional origin-trial token is
described in [webmcp.md](webmcp.md#the-origin-trial).

## Limits

- Not deployed and not tested against a real hosted AI client yet (see Status).
- No password reset, no email verification, no passkeys, no two-factor sign-in.
- Sync is whole-plan, last agreed revision wins or the user chooses: there is no merge
  of two edits to the same plan.
- A plan from the account needs a local copy, so it cannot be opened in a browser that
  blocks IndexedDB.
- An AI tool simulates 2,000 lives unless it asks for more (5,000 at most); the website
  runs the plan's own setting, usually 10,000, on the same seed. Their figures agree
  within the sampling interval, not to the digit.
- The server runs simulations on one thread. A burst of agent calls is rate-limited, not
  queued.
