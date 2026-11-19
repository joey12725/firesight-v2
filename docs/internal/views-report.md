# outcomes

Outcomes and Assumptions are both complete, type-clean, lint-clean and tested (21 vitest tests pass). The previous engineer's work was nearly finished and good, so I kept it, verified it in the browser and fixed four problems I found. All three demo households were re-shot at desktop, laptop and phone in light and dark with a clean console and no horizontal overflow, and the interaction script passes. The final single-shot re-run after the last 1-line label-threshold tweak in `Cohorts.tsx` was not looked at.

## What I changed this session
- **Sequence-of-returns takeaway** (`outcomes/text.ts`): it used to read "18 of 100 run short… among the rest, 17 of 100 do", leaving the reader to work out that there is no effect. It now tests the gap against its sampling error and, when it is inside the noise, says "A poor first decade does not make this plan likelier to run short…", plus what it does change (wealth left). Plans with a real effect keep the original sentence.
- **Cohort takeaway**: it was headlining starting years whose lives outrun the record and replay it from 1926 (e.g. "2020 to 2024 fare worst"). It now headlines only starting years the record covers in full, and reports a worse wrapped year as a caveat. Unit test added.
- **Cohort strip label** (`Cohorts.tsx`): "From 1960, lives outrun the record" collided with the axis title on phone. The title now has its own row and the label flips to the left of the rule when it would run off the plot.
- **Longevity figures** (`assumptions/data.ts`): "lives past N in half of lives" was read off the yearly survival curve at December ages, which overstated by about a year. It now uses the engine's own `death_age_0` / `death_age_1` quantiles, null when censored by the plan end. Unit test added.
- **Life expectancy wording** (`assumptions/index.tsx`): "life expectancy 83" sat next to "lives past 88 in half" with no explanation. The bullet now says it is the average age at death and that half of lives run longer. The headline also merges "job loss and health shocks are simulated" when both switches agree.

## Outcomes: "how wide is the range of lives, and what separates the good from the bad?"
- **Headline sentence** computed from the data: median, p10 and p90 terminal net worth, the failed share, and the top separating circumstance.
- **Quantile dot plots**, 100 dots each (one per life under 100 lives), with never/censored groups drawn apart:
  - age work becomes optional against age actually retired, collapsing to one plot when the two coincide, with hollow "never" dots;
  - net worth at the end, log axis;
  - deepest spending cut, with "On plan" and "Never retired" groups;
  - years short of essentials, with a "Never short" group;
  - lifetime tax, log axis.
- **Survival and money-lasting curves** with the engine's joint "alive and short of essentials" strip beneath (not a product of the two curves), on the shared time range with drag-to-zoom.
- **Sequence scatter**: about 5,000 lives on canvas, failed lives encoded distinctly, a median trend through 12 equal-count bins, and a bar strip of the share that ran short per bin. A toggle switches between wealth at the end and age the money ran short. Dragging or the Unluckiest / Middling / Luckiest buttons list lives, which open through `navigateToLife`; clicking a dot does the same; it is keyboard readable.
- **Failed against held**: nine small multiples (all-lives histogram with both medians), ordered by separation, each with its own sentence and an association-not-cause caption.
- **Historical cohorts** (only under the Historical model): one traced life per starting year coloured by fate, with wrapped years in grey, and a pass/fail strip by start year; hover links the two and click opens the life.
- **Four-model triangulation**: on a button, with Stop and a stale state. It shows three dot-and-interval strips (plan holds, median age work becomes optional, median terminal net worth) and a sentence comparing model spread with sampling error.

## Assumptions: "what is this model assuming, and is it reasonable?"
- **Headline sentence** naming the assumptions in force. Each block has a statement, facts, an editor deep link, and charts.
- **Markets**: 30-year real stock return ranges for the four models against the actual record, and the record's annual ranges per asset. Both come from the static `history.json` and are labelled as the historical record 1926–2026, not simulation output. The deepest drawdown per life is drawn from this run.
- **Inflation**: per-life average, as a dot plot from this run.
- **Lifespans**: per-person and either-of-them survival curves.
- **Social Security**: mean benefit by age, against planned retirement spending.
- **Health and care**: stacked mean cost by age.
- **Tax**: the bracket ladder for the household's filing status, with markers for the median life's first full year and its fifth retired year. Both come from the traced tax returns, placed in the bracket the engine reports; a year abroad is handled in words.
- **What this model does not know**: six groups of omissions taken from the module docs.

Other states: skeleton before the first result, a tiny-sample warning tag, dimming when stale, and worded takeaways for no failures and no retirees. No assistant chart is registered, because no registry kind fits these views (`fi_age` is already the Overview's), so `assistant-charts.ts` is untouched.

## Kit gaps filled in ext/
- `outcomes-QuantileDots.tsx`: quantile dot plot with side groups for censored values, an interval whisker, keyboard reading and an exported `layoutDots`.
- `outcomes-XAxis.tsx`: a horizontal value axis for non-age x (money, percent, log), which the kit lacks.

## Engine output I wished existed
- `PathScalar` `years_funded` (years retired minus years depleted, per life): "years of retirement funded" cannot be built from the marginals, so the view shows its complement, years short of essentials.
- `Summary.failure.contrasts[]` with per-group quartiles (`failed_p25`, `failed_p75`, `succeeded_p25`, `succeeded_p75`) or per-group histograms, so the panels could show paired distributions and not just two medians over the pooled histogram.
- `Summary.scatter` columns `years_depleted`, `lifetime_tax`, `worst_drawdown`, `inflation`, and under the historical model an explicit `start_month` per path. The view currently hard-codes path index → start month (1926-07, 1,203 months) in `outcomes/data.ts`.
- `PlanPreview` or `Summary` tax block: `filing_status`, `standard_deduction`, `ordinary_brackets[]` in today's dollars, plus median `taxable_income` and `marginal_ordinary_rate` in year one and a typical retired year. The view currently uses a static copy of the published 2026 table (`tax-2026.json`), infers filing status in TypeScript (`filingStatus()`), and spends one trace on the median life.
- Per-path realised market scalars (`equity_real_return`, `equity_real_return_30y`), so the model-range strip could come from this run and not the calibration file.
- A stated convention for `survival`: it is "alive at some point in the year" plotted at December ages, about 11 months late.

## Performance
- Every chart draws from the Summary; nothing is per-path except the scatter sample (about 5,000 dots, one canvas draw, hover in SVG).
- Extra engine work:
  - Assumptions: one `pool.trace` of the median life, debounced and cancelled on change.
  - Historical cohorts: about 100 traces, only under the Historical model and only when not stale.
  - Triangulation: 4 × 2,000 lives through `pool.runMany` on the run's seed, on demand only. It measured 1.1 s, and Stop leaves 0 active jobs (asserted in the e2e script).

## Representative screenshots (`/Users/joeyhersh/.t3/projects/firsight-v2/web/e2e/shots/`)
- `outcomes-dual-income-family-desktop-light.png`
- `outcomes-early-career-saver-desktop-dark.png`
- `outcomes-side-hustle-landlord-phone-light.png`
- `outcomes-dual-income-family-desktop-light-historical.png`
- `outcomes-early-career-saver-desktop-dark-triangulate.png`
- `outcomes-state-scatter-selection.png`, `outcomes-state-scatter-age.png`, `outcomes-state-models-done.png`, `outcomes-state-60-lives.png`
- `assumptions-dual-income-family-desktop-light.png`
- `assumptions-side-hustle-landlord-laptop-dark.png`
- `assumptions-early-career-saver-phone-light.png`

## Engine requests
- PathScalar `years_funded` (years retired minus years depleted, per life) with its ScalarDistribution, so 'years of retirement funded' can be drawn directly; today only its complement `years_depleted` is available.
- Summary.failure.contrasts[]: add per-group quartiles (failed_p25, failed_p75, succeeded_p25, succeeded_p75) or per-group histogram counts on the scalar's existing `edges`, so failed-vs-held panels can show two distributions instead of two medians over the pooled histogram.
- Summary.scatter: add columns `years_depleted`, `lifetime_tax`, `worst_drawdown`, `inflation`; and under the historical model an explicit per-path `start_month` ("YYYY-MM") column, or Summary.market.history { first_month, months }, so the view need not hard-code path index -> start month (currently 1926-07, 1,203 months, in outcomes/data.ts).
- PlanPreview (or Summary) tax block: `filing_status`, `standard_deduction`, `ordinary_brackets[] { rate, from, to }` in today's dollars, plus median-life `taxable_income` and `marginal_ordinary_rate` for year one and a typical retired year. Would replace the static tax-2026.json copy, the TypeScript filing-status inference and the one trace the Assumptions view runs.
- Per-path realised market scalars, e.g. `equity_real_return` (annualised over the life) and `equity_real_return_30y`, so the market-model range strip can be drawn from this plan's run instead of the calibration file.
- Document or change the convention for Summary.survival: values are 'alive at some point in the year' but are plotted at `axis.ages[y]` (December age), about 11 months late. Either state it as 'alive at the start of the year' or provide start-of-year ages.

## Open issues
- The final re-run after the 1-line label-flip threshold change in Cohorts.tsx (190 to 172 px) was not looked at; it type-checks and lints. The earlier phone crop showed the label fully legible but hard against the left edge.
- Every desktop 1440 screenshot has the editor and assistant panels open, so the results column is about 660 px and the wide two- and three-column layouts (@4xl / @3xl container breakpoints) were not captured or reviewed this session.
- Phone scatter: the x axis is clipped to a rounded range, so some lives lie beyond it (19 for the landlord). They are counted in the end groups and the count is stated on the page, but they are not drawn.
- 'Years of retirement funded' is shown as its complement (years short of essentials) because no per-life scalar exists; see engine requests.
- tax-2026.json is a static copy of the published brackets from data/tax/us-federal-2026.json and filingStatus() infers the filing status in TypeScript. No tax is computed in TypeScript (markers come from the engine's traced returns), but both should be replaced by engine-provided values when available.
- history.json and the model-range strip are calibration figures, labelled as the historical record and not simulation output; they will not move if the Rust engineer changes the market models, so the file needs re-deriving after engine changes.
- Cohort path-index to start-month mapping (1926-07, 1,203 months) is hard-coded in outcomes/data.ts from docs/internal/module-economy.md; it breaks silently if the engine changes the record length or the mapping.
- Mean life expectancy from PlanPreview sits 3 to 5 years below the simulated median age at death (e.g. Sam: mean 82.9, median 86.7, 8.5% still alive at 100). The mean matches the engine's own death_age_0 mean, so it is consistent, but the long upper tail may deserve a look from the Rust side.
- The e2e scripts write shots into the shared web/e2e/shots directory; the scratchpad is also shared with other engineers (my temporary files there are prefixed oc-).


# cashflow

Both views are finished and working on the kit: type-check, eslint and 54 vitest tests pass, the browser console is clean, and the interaction script passes all 13 checks. The previous engineer's partial work was sound and nearly complete, so I kept it, reviewed screenshots for the three demo households, fixed what was wrong and added what was missing. Not individually inspected: laptop 1024 shots (captured in both themes, console clean, never opened) and dark phone; dark was read at desktop only, and the early-career-saver life-stories page was not opened.

**Cash flow** ("where does the money come from and go, year by year?")
- **Lead sentence**: computed from the data (who pays for this life and when savings take over), with a paragraph stating that every figure is a mean over households with someone alive that year. Charts stop while at least 1 household in 4 is alive, and never draw the final part-year.
- **Money in against money out**: a diverging stacked area on one scale (in above zero, out below), justified in the caption. Clicking or pressing Enter on a year opens it in the flow diagram.
- **One year of money**: a hand-built SVG Sankey with a single trunk, because the engine records sources and uses but not which dollar paid for what. It has a scrubber, previous/next buttons and jump chips, labels that are spread so they never collide, and values that glide between years.
- **Balance sheet**: assets by tax treatment plus property above zero, debts below, with the median net worth line over the mean stack.
- **Paid into and drawn from each account**: saving by destination above, withdrawals by source below, in the balance-sheet colours.
- **Saving rate and withdrawal rate**: one percent axis with the plan's moments marked and a 4% reference line.
- **Tax**: tax by kind over time, a strip for tax as a share of money in, and a lifetime-tax panel (p10, p50, p90 across lives).
- **Assistant chart**: `cash_flow` compact variant was already registered; it now also labels foreign tax correctly.

**Life stories** ("what does one simulated life actually look like?")
- **Picker**: seven lives (unlucky, middling, lucky, typical path to independence, ran short, earliest and latest to independence), each with its stated rule and a one-line description, plus previous/next, a path-number input and a random life. The selection lives in the URL as `?path=N&why=...`.
- **Headline and narrative**: deterministic sentences from events and scalars; each sentence links to that year's ledger. "Ask Claude about this life" sends a compact context.
- **Timeline**: monthly net worth and investable assets over the crowd's 5–95 and 25–75 bands, event markers with a five-row lane layout (lowest-priority labels dropped, still in the tooltip), and a life-stage ribbon.
- **Lanes on the same axis and crosshair**: income, spending, planned spending and tax; the tax bracket by year; account balances; real growth of a dollar against prices on a log axis with the worst drawdown shaded.
- **Year inspector**: full ledger (money in equals money out), year-end balances, memo lines, and the tax return as a 1040-style table in filed and today's dollars.
- **Traces**: cached by (plan hash, path), 48 entries, with the previous life dimmed while the next loads. Error, out-of-range path and tiny-sample states are designed.
- **Assistant chart**: `life_story` compact variant (timeline with markers) was already registered.

**What I changed this session**
- **Foreign tax mislabelled**: the landlord household moves to Lisbon, and the engine reports foreign income tax in the state series, so the chart showed a large "State income tax" for a Florida resident. `livesAbroad(profile)` now relabels it "State and foreign income tax" in the chart, flow diagram and compact variant.
- **Balance-sheet takeaway**: no longer repeats the same figure when nothing is owed ("owns $3.45M … and owes nothing").
- **Flow diagram on phone**: the column headings overprinted the trunk label; they are now dropped when they would collide.
- **Picker descriptions**: picks outside the 5,000-path scatter sample had a blank description; they now describe themselves from the pick's own value.
- **Repeated shortfalls**: a life that alternated depleted/recovered month by month produced repeated sentences and three stacked "Savings run out" labels. The first is now the event and the rest are unlabelled repeats summarised in one sentence.
- **Market lane**: the "Worst fall" label sat on top of the line; it is now above the plot.
- **New tax-bracket lane**: the federal marginal bracket as steps and all tax over AGI, per year from the trace's returns. Years abroad are detected and described as the plan's flat foreign rate, not a bracket.
- **Tests and scripts**: six new tests (tax years, foreign detection, shortfall folding, pick fallback, abroad labels) and `web/e2e/cashflow-interact.mjs`.

**Kit gaps filled in `ext/`**
- `cashflow-StackedAreas.tsx`: a diverging stacked area with in-band labels, overlay lines, an optional strip, a selected-year marker and a legend.
- `cashflow-FlowDiagram.tsx`: the Sankey, with a pure `layoutFlow` function.
- `cashflow-lanes.ts`: `layoutLanes`, a priority lane layout for labels along a time axis.
- A log scale and a step-line option live locally in `life-stories/Lanes.tsx`; both are candidates for the kit.

**Performance**
- Cash flow triggers no extra engine runs; everything comes from the Summary's composition series and is SVG.
- Life stories runs one `pool.trace` per selected life, cached. Nothing needs cancelling: a stale response is ignored.
- Only the timeline uses canvas (about 750 monthly points); hover never redraws it.

## Engine requests
- Summary: taxable income against bracket tops, for the bracket-fill picture across all lives. Suggested shape: `tax_brackets: { rates: number[], by_year: { tops: number[][] (indexed bracket tops in today's dollars, for the household's filing status), taxable_ordinary: { levels: number[], values: (number|null)[][] } (quantiles of federal ordinary taxable income among living households), share_in_bracket: number[][] (share of living households whose marginal bracket is rates[k]) } }`.
- Summary.quantiles: add HistSeries for `taxable_income` and `agi` (or at least mean `agi` and `taxable_income` as memo YearSeries in composition), so an effective rate on income can be drawn; today only tax over all money in is possible.
- Summary: per-year quantiles of the saving rate (saving / gross income among working households) and the withdrawal rate (net withdrawals / prior-year investable among retired households). Today both are ratios of means over a mix of working and retired lives.
- Summary.composition: split means by status (working vs retired), or add `composition_by_status`, so cash-flow averages around retirement do not blend earners with retirees.
- Summary.composition: a separate `tax_foreign` YearSeries. Foreign income tax is currently folded into `tax_state`, and the view has to infer it from the profile.
- PathTrace.tax_returns[].result: add `bracket_tops: number[]`, `bracket_rates: number[]` and `ordinary_taxable_income` (nominal), so a single life's bracket fill can be drawn without parsing the note text; also a boolean `foreign_resident` per return.
- PathTrace.events: a `died` event for the last death (the log currently stops without one, so the narrative infers it from the horizon), and a structured `person` index on events instead of names inside the label.
- PathTrace.events: structured fields in place of label text for `laid_off`/`rehired` (job id, new salary), `bought_home` (mortgage rate), `spending_cut`/`spending_raise` (percent change) and refinancing (rate). The view currently reads these with regular expressions on the label.
- Summary.picks: add `earliest_fi`, `latest_fi` and scalars (fi_age, depletion_age, terminal_net_worth) for every pick, so the picker need not depend on the 5,000-path scatter sample.
- Summary.quantiles for net_worth by month, or documentation that year-end quantiles align with December trace months, for the one-life-against-the-crowd overlay.

## Open issues
- The aggregate tax-bracket-fill chart is not built: the Summary carries no taxable income or bracket tops. The Cash flow tax caption says so and points to the per-life bracket lane in Life stories.
- The Cash flow tax strip is tax over all money in (withdrawals included), not an effective rate on income, because the Summary has no AGI or taxable-income series.
- Cash-flow averages late in life are dominated by the luckiest lives (the early-career saver's mean net worth reaches about $20M against a median near $7.5M). This is stated in the lead, captions and takeaway, but a median-based composition would be more honest; it needs the engine requests above.
- Timeline: the y scale is capped at 1.6 times the life's own peak, so the crowd's 5–95 band is clipped flat at the top of the plot for below-median lives.
- Flow diagram on phone: a node's name is dropped when name plus amount does not fit the label column (for example '$5.9k' without 'Debt payments'). The name is still in the hover and the table.
- Event curation parses engine label strings with regular expressions (layoff job, mortgage rate, refinance rate, cut percentages). A change in label wording degrades to generic titles, not errors.
- A path index beyond the run (for example ?path=999999) still traces and renders, with a warning tag; arguably it should be refused.
- The log scale and step-line option in life-stories/Lanes.tsx are view-local and could move into the kit.
- Screenshots not individually reviewed: laptop 1024 (both views, both themes), dark phone, and early-career-saver life stories. The capture run reported no console errors for any of them.


# levers

What matters view (key: levers): the previous engineer's partial work was nearly complete and sound, so I kept it, fixed what the screenshots showed, and finished the gaps. One brief item is not built: a separate "savings rate" solver variable (see open issues).

Verification: my files type-check and lint clean; 22 vitest tests pass; `e2e/levers-check.mjs` passes all 13 behaviour checks; the 18-shot capture (3 households x desktop/laptop/phone x light/dark) ran with a clean console. The two dual-income and early-career full-page sets predate my last wording change (the "on the line" map takeaway); only side-hustle desktop was recaptured after it. My dev server on 5183 is stopped (it hit its background time limit once and I restarted it for the last capture).

**What the view does**
- **Question, stated at the top:** "what does my result depend on most, and what should I change?" A lead sentence names the controllable lever with the largest paired improvement. One measure switch (plan holds / runs short / work optional / wealth at the end / spending delivered) drives everything below; it becomes a select on phone.
- **Biggest wins:** up to three controllable changes, one per lever, only those distinguishable from zero, each with size, 95% paired interval and "Try this change". Below them: the worst world-side risk in a sentence and the "association within this model, not advice" caveat.
- **Tornado ("What moves it"):** SVG bars from the plan's level to each variant's level, grouped "What you control" / "What the world decides", sorted by largest effect with quiet levers sunk. Whiskers are the 95% interval of the paired difference. Within-noise settings are hollow and labelled "within noise"; no-change settings are a ring at the baseline. Rows are focusable buttons; click or Enter opens a paired detail table (all five measures, lives that flip each way) with "Try this change", which adds a scenario through the profile store and then links to Scenarios.
- **Trade-off map:** 9x9 canvas heat map of the chosen measure over two variables (default trigger withdrawal rate or retirement age x annual spending; also salary, stock share, claiming age). Cells print their value; labelled contours (75/90/95 for holds, 5/10/25 for runs short, round values otherwise); plan marked; hover and arrow keys read a cell with its interval; click selects a cell and shows all five measures plus "Adopt as a scenario". Coarse 5x5 pass first, then fine, then sharper.
- **Find the number:** sentence-form goal (holds in at least N of 100; runs short in at most N of 100; work optional by age X in half of lives) and variable (spending, trigger rate, retirement age, salary scale). Bisection of at most 10 runs, drawn as tested values vs measure with intervals, order numbers, goal line, sampling band and plan marker; answer sentence and "Apply as a scenario".
- **Assistant charts:** `tornado` and `heatmap` were already registered; the compact variants render from `ranking` / `grid` at 320 px, including the single-row grid.

**What I changed this session**
- Wins now show the other side of each change ("The price: work becomes optional 0.8 years later" / "Also: ..."), from the same paired run. When a plan retires when funded, a sentence explains why "spending 10% higher" can appear as a win (it also means working longer). Without this the list read as advice to spend more.
- Solver answer now carries its tolerance: the search resolution, or the sampling range when that is wider ("at most $46,100 a year; on a different draw of lives the boundary could fall anywhere from $41,900 to $50,000"). The detail gives both sides of the boundary as counts of lives (3,600 vs 3,598 of 4,000); before, both read "90.0 of 100".
- Solver chart: the answer label no longer collides with the axis title.
- Map contour labels sit on the seams between rows, so they no longer cover cell values (lines still cross some numbers).
- Map takeaway reworded, with a new case for a plan sitting on the 90-of-100 line within one cell's sampling error.
- Win buttons bottom-aligned; shorter unit captions.
- `levers-check.mjs` had two wrong expectations (a cache check against a plan whose sweep never finished; a table read before the map had cells). Both corrected.
- New harness `e2e/levers-compact.tsx` + `.mjs` renders the compact charts from real sweeps converted to the stored tool shapes.

**Kit gap filled in ext/**
`levers-heatmap.tsx`: `contourLines` (marching squares with saddle handling), `joinSegments`, `polylineAt`, `drawHeatCells`, and `GridCursor`, a two-dimensional pointer/touch/keyboard reader, since the kit `Cursor` reads one axis only. Nothing in it knows about levers.

**Performance**
- Tornado: about 18 settings plus the baseline in one `runMany` at 250, then 1,000, then 4,000 lives (99,750 lives per plan). First picture measured at 830-950 ms in dev mode with load average about 28; the capture script, which includes the tab click under heavier contention, saw 0.8-2.7 s.
- Map: 25 cells at 300 lives, then 81 at 400 and 1,000 (120,900 lives), in batches of 20; starts only when scrolled into view.
- Solver: at most 10 runs of 4,000 lives, only on request, probes cached.
- All sweeps settled in 11-35 s depending on machine load. Sweeps run at pool priority 0 so the live run goes first, abort on edit (250 ms settle, old picture held dimmed) and on unmount, and are cached by compiled-plan key + seed + lever set or axes. Progress (lives, lives per second) is in each caption.

**Engine output I wished existed**
- `ScalarComparison.median_delta { value, lo, hi }`: I derive the median paired difference and its interval from the 100 `dots` by order statistics, which is coarse.
- A batch call returning only `Headline` plus a trimmed paired comparison for N variants, so 19 full Summaries and Comparisons are not serialised to be mostly discarded.
- `Lever.group: 'decision' | 'assumption'`: the control/world split is a hard-coded id set in `model.ts`.
- Engine levers for retirement age, allocation and claiming age (currently built in the view), and an `at_age` alternative to the withdrawal-rate lever.
- `Headline.success_count` and `depletion_count` as integers, so natural frequencies need no rounding of `value * paths`.

Files are in /Users/joeyhersh/.t3/projects/firsight-v2/web:
- src/views/what-matters/index.tsx
- src/views/what-matters/text.ts
- src/views/what-matters/TradeoffMap.tsx
- src/views/what-matters/Solver.tsx
- src/views/what-matters/model.test.ts
- e2e/levers.mjs
- e2e/levers-check.mjs
- e2e/levers-compact.mjs
- e2e/levers-compact.tsx
- e2e/shots/levers-dual-income-family-desktop-light-full.png

## Engine requests
- ScalarComparison.median_delta { value, lo, hi }: median of the per-life paired differences with a 95% interval computed in core (the view currently derives it from the 100 `dots` by order statistics, which is coarse).
- A batch entry point (e.g. runMany/compare variant) that returns only Headline plus a trimmed Comparison (outcomes success/depleted; scalars fi_age, terminal_net_worth, delivered_spending_ratio) for N variants against one baseline, to avoid serialising 19 full Summaries and Comparisons per tornado pass.
- Lever.group: 'decision' | 'assumption' on each lever returned by levers(profile); the control/world split is currently a hard-coded id set in the view.
- Additional engine levers: retirement_age (-/+2 years, or an at_age alternative when the trigger is when_funded), allocation (-/+10 points of growth assets) and Social Security claim_age (62 / 70); these are currently constructed in the view's model.ts.
- Headline.success_count and Headline.depletion_count as integers alongside the rates, so natural-frequency wording does not round value * paths.

## Open issues
- Savings rate is not offered as a solver or map variable. The profile has no savings-rate field (saving is what is left of income after spending and tax; the 401(k) share only changes tax treatment), so it would duplicate the spending and salary variables. If the lead wants it as a label, it needs a definition, e.g. scaling pre-retirement spending only.
- The grid is 9x9 (81 cells), not 10x10, so the plan's own values land on grid nodes.
- The first tornado pass (250 lives) can name a different top lever than the final pass (dual-income: claiming age at 250 lives, salaries at 4,000). It is tagged 'Early picture' and the wins only list effects distinguishable from zero, but the lead sentence can change as it sharpens.
- The tornado takeaway ranks levers by largest swing in either direction, while the lead sentence and wins rank by best improvement, so they can name different levers (dual-income: claiming age vs salaries). Both are correct by their definition; the lead may want one rule.
- Contour lines still pass over some printed cell values; only the contour labels were moved clear. The 'Plan as entered' label can sit over a neighbouring cell's edge.
- Solver order numbers are dropped where probes crowd near the boundary (within 14 px), and at laptop width two can still touch.
- The solver assumes the measure is monotone in the variable; this is stated in the caption but not tested. A non-monotone case (e.g. spending under a when-funded trigger, where more spending can raise the hold rate slightly) would return a misleading bracket or 'never'.
- The plan's figures in this view come from the first 4,000 lives, so they can differ from the Overview's 10,000-life headline by about a point; the page footnote says so.
- Screenshots for dual-income and early-career were taken before the final 'on the line' map-takeaway case was added; only side-hustle desktop light and dark were recaptured after it.
- One earlier run of the compact-chart harness hung for about ten minutes while the full capture was running alongside it; it completed normally when run alone. The script has no timeout on its page.evaluate call.


# scenarios

The Scenarios view is finished. The previous engineer's work was nearly complete and of good quality, so I kept it, fixed six defects found in the browser, and added an interaction test. Type check and lint are clean for my files, 20 unit tests pass, all 12 interaction checks pass, and the 18-shot matrix (three demo households, desktop 1440 / laptop 1024 / phone 390, light and dark) ran with a clean console. The dev server on 5184 is stopped.

## What the view contains

**Bench**
- One row per scenario: inline rename, enable switch, a plain-language description generated from the patch ops (label, struck-through before, after), and per-change remove.
- Row actions: "Edit in the plan editor" (toggles the edit target; opens the editor sheet on phone), and a menu with Duplicate, Make this the plan (toast with Undo that restores both the baseline and the scenario) and Delete (with Undo).
- Each row carries a paired "Plan holds: +4 of 100 · within noise" tag.
- Quick-create buttons, grouped, offered only when the plan has something for them to change:
  - retire 2 years earlier / later (a "when funded" plan becomes a fixed age two years either side of the median retirement age, and the hint says so)
  - spend 10% less / more in retirement, save 5 points more
  - part-time for 5 years after retiring (new job income with retirement-relative start and end)
  - delay Social Security to 70
  - do not buy the house (when a BuyHome event exists)
  - 100% stocks, 60/40
  - a 40% crash at retirement, a Social Security cut of 21% from 2033
- Pressing one creates the scenario and runs it immediately; pressing it again selects the existing one.

**Many scenarios at once** (two or more enabled)
- A dot-and-95%-interval ranking against a "The plan" zero line. The measure is chosen from plan holds, work optional, wealth at the end and lifetime spending.
- Scenarios whose interval includes zero are drawn hollow and labelled "within noise"; the caption says their order means nothing.
- Rows are buttons (keyboard-selectable) that open the detail.
- A small-multiples board in the same order: three figures with their change from the plan, and a median net-worth sparkline of scenario against plan on one shared scale.

**Paired comparison** for the selected scenario
- Headline sentence from the flips table ("In 2 of 100 lives this change is the difference between the plan falling short and holding; in 1 it is the other way round… In 97 it changes nothing"), with the net effect, 95% interval, standard error and a noise verdict underneath.
- A rescued / harmed / unchanged bar with raw counts.
- A dot plot of the engine's 100 per-life differences centred on zero, switchable between wealth at the end, work optional, spending and tax. Unchanged lives are hollow, the tails are gathered into open end columns, and a median and middle-80 bar sits underneath.
- An "Every measure, paired" table: 11 rows with plan, changed, difference, interval, a small interval drawing and a verdict.
- Net worth and spending as overlaid medians with middle-half bands in the baseline and scenario colours, directly labelled, with a "scenario median minus plan median" strip beneath on its own scale. The caption explains why the bands stop at the middle half and that the strip is a difference of medians.
- Life stages as a difference chart (lives per 100 by age, per stage) rather than a pair of stacks; the caption says why.

**Same life, two decisions**
- Picks a life from the joined scatter samples (one it rescues, one it harms, a typical change, the biggest gain, the biggest loss) and replays it under both profiles on the same path index.
- A two-line net-worth chart with the divergence year annotated and retire / runs-short markers, plus a "Read this life year by year" link through `navigateToLife`.
- I did not reuse the Life stories timeline component; the compact two-line chart was already built and reads well.

**Assistant**: `compare_delta` is registered (`web/src/views/assistant-charts.ts` line 9). The compact variant shows the flip bar and the dot plot, with a provisional sentence before the paired statistics arrive.

**States**: empty bench, queued / simulating / pairing, a tiny-sample warning ("Only 60 lives paired"), stale dimming, a scenario that no longer applies, trace error, and no life to pair.

## Fixes I made this session

- **Stale names after rename.** The run store snapshots scenario names at run start, so the board, ranking and detail kept the old name. The view now reads names live from the profile store.
- **App shell pushed off screen.** Choosing a ranking row called `scrollIntoView`, which also scrolled the document by 328 px and hid the toolbar. It now scrolls only the results pane.
- **Dot plot disagreed with its own takeaway.** The plot counted 11 unchanged lives where the sentence said 10. `binDeltas` now takes the engine's exact `equal` share, so both count the same lives.
- **Phone layout.** The paired table no longer scrolls sideways with the Difference column hidden: it drops to four columns with the verdict under the difference. The four-way measure control becomes a menu, the scenario name gets its own row, and the divergence label no longer clips.
- **No-op scenarios.** After a promote, a leftover copy read "$68k → $68k (+0%)". It now reads "…which the plan already has".
- **Type-check error.** `e2e/scenarios-compact.tsx` broke `tsc -b`; I moved it to `web/src/views/scenarios/compact/probe.tsx`.
- The ranking's label column is wider, so long names are no longer truncated at desktop width.

## Kit gap filled

`web/src/ui/charts/ext/scenarios-marks.tsx` holds `ValueAxisX` (a horizontal value axis), `IntervalMark` (dot with interval, hollow variant), `ZeroRule` and `zeroTicks`. The kit core has no horizontal value axis or interval mark.

## Performance

- The view triggers no extra aggregate runs; everything draws from the run store's summaries and comparisons.
- The only extra engine work is two `pool.trace` calls per chosen life, and only once the run has settled. Results are discarded if the selection changes, but an in-flight trace is not aborted.
- Baseline plus four scenarios at 10,000 lives each (50,000 total) took 2.6 to 4.5 s on a quiet machine, and 6 to 13 s while other work was running on it.
- Rename, select, measure change and hover never re-run anything.

## Verification

- Unit tests: `npx vitest run src/views/scenarios` (20 passing, covering the transforms, patch descriptions, quick patches and sentences on real engine output).
- Interaction: `node e2e/scenarios-check.mjs` — empty bench, 60 lives, rename, duplicate, keyboard ranking, table view, switch off, edit target, delete with undo, promote with undo, same-life replay and the link out.
- Screenshots: `node e2e/scenarios.mjs` (`COMPACT=1` adds the assistant variant; `DSF=1` for tall phone captures).
- The last full-page look was before the final scroll and no-op-wording fixes; those two were checked by a targeted run and the interaction test.

## Engine requests
- Comparison.scalars[]: add `dot_paths[100]`, the path index of each quantile dot, so a dot in the per-life difference plot can open that exact life (navigateToLife and the same-life replay). Today the dots are anonymous.
- Comparison: add flipped path indices, e.g. `outcomes[success].b_only_paths[]` and `a_only_paths[]` (capped at about 50 each). The view currently finds rescued and harmed lives by joining the two Summaries' scatter samples on path index, which covers only the first 5,000 lives and comes up empty for rare flips.
- Comparison: add a per-year paired series, `by_year: { net_worth: { delta_quantiles[year][9] }, spending: { ... } }` (quantiles of scenario minus baseline per life at each age). The strip under the paired fans is the difference of two medians, not the median per-life difference; the caption says so, but the paired fan would be the honest chart.
- Comparison: add a per-year status transition count, `status_flows[year][from][to]`, so the life-stage chart can show which stage lives moved from and to rather than net share differences.
- Comparison.scalars[]: add `mean_delta` over all lives for censored scalars (fi_age, retirement_age), or a paired restricted-mean. The current mean covers only lives with a value under both plans, which biases the 'work optional' ranking when a scenario changes who reaches it.
- Comparison.headline[]: add a paired interval for the median deltas (the delta of medians has no interval today), or state the method. The board shows median deltas without intervals for this reason.
- Comparison.scalars[]: add an `equal_tolerance` share (for example within $500, or within a month), so 'unchanged' can mean materially unchanged; `equal` is exact equality only.
- Check the `crash_at_retirement` stress with a `when_funded` retirement trigger: on the dual-income demo a 40% crash at retirement raises the share of lives in which the plan holds by 0.9 of 100 and average end wealth by $109k. The view reports this faithfully, but it looks like the forced five-year recovery overshoots trend, or that the crash delays the funded test.

## Open issues
- Phone full-page captures at device scale 2 (the -phone-*-full.png files) repeat the page partway down: a Chrome capture limit, not a layout bug. Use DSF=1 for a faithful tall phone capture; the phone fold shots are fine.
- Promote's Undo calls the store's undo twice (scenario removal, then the baseline edit). If the user makes another edit before pressing Undo on the toast, it will undo the wrong steps. A store-level transaction or `promoteScenario` action would make this atomic (store change, outside my scope).
- The run store keeps each scenario's name from run start. I worked around it in the Scenarios view only; other consumers of `comparison[].name` (assistant charts, other views) will show the old name after a rename until the next run.
- On phone the results header (title, four stats, tabs) is fixed above the scrolling pane and takes about half the screen, so every view gets roughly 400 px of scroll area. This is in the app shell, not my view.
- The dot plot shrinks its dots to about 4 px when one column holds 25 or more of the 100 lives (for example the early-career saver's crash scenario). It stays legible, but in the 150 px assistant variant the dots are small.
- Same-life candidates come only from lives present in both runs' scatter samples (the first 5,000), so with very rare flips the 'A life it rescues / harms' options can be missing even though flips exist. The caption states the sample; the flipped-path-indices engine request would fix it.
- In-flight `pool.trace` calls are ignored rather than aborted when the selection changes (the pool exposes no cancel for traces); the cost is two single-path replays.
- The paired fans overlay two translucent middle-half bands, which read as grey where they overlap. This is intentional and explained in the caption, but a per-year paired delta fan from the engine would be the better chart.
- `web/src/views/scenarios/compact/probe.tsx` is a dev-only mount helper for the screenshot script. Nothing imports it, so it stays out of the bundle, but it lives under src because e2e/ is type-checked without JSX or DOM.
