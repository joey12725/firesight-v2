# Landscape: retirement planners, modelling practice, visualisation and AI

Research date: 2026-11-17. Purpose: decide where FIRESight v2 should be deeper than the state of the art (model, AI, visualisation) and where it should deliberately do less.

Evidence labels used throughout:

- **[doc]** confirmed from the tool's own page or methodology document, fetched for this survey.
- **[3p]** from a third-party review, competitor comparison or forum snippet (lower authority; competitor pages are biased).
- **[mem]** from the author's prior knowledge, not re-verified in this pass. Treat as a lead to check before quoting publicly.

Limits of this survey: Bogleheads forum and wiki pages returned HTTP 402 to the fetcher, and Reddit threads were not reachable through search. User complaints therefore come from one large Hacker News thread, reviewer write-ups, forum snippets surfaced by search, and [mem]. No tool was run hands-on.

---

## Summary of findings

1. **Nobody combines a serious market model with a serious household model.** The tools split into two camps. Camp A has thoughtful return modelling and almost no household detail (TPAW, ERN, cFIREsim, FIRECalc, FI Calc, Portfolio Visualizer, Honest Math, Rich Broke or Dead). Camp B has detailed taxes and accounts on top of a weak stochastic model (Boldin: normal draws, one blended return per account, deterministic inflation; Fidelity: fixed 2.5% inflation; MaxiFi: fixed max age). ProjectionLab and Pralana sit closest to the middle but still randomise only market returns (and at most inflation), over a fixed lifespan.
2. **No surveyed consumer tool simulates the things that actually end early retirements**: job loss correlated with recessions, health and long-term-care shocks, stochastic death, and home prices that fall together with equities. Fidelity adds a deterministic 4-year long-term-care block [doc]; Rich Broke or Dead overlays a mortality table but does not sample lifetimes per path [doc].
3. **Path counts are small.** Fidelity has used 250 [doc, older methodology PDFs], NewRetirement help said 500 and Boldin now says 1,000 [doc], ProjectionLab caps at 2,000 [doc], Empower says 5,000 [doc], Honest Math 10,000 [3p]. At 1,000 paths a 5% failure probability has a 95% interval of roughly 3.6% to 6.4%. A 100k+ engine in the browser is a real differentiator for tail metrics and for scenario differences.
4. **"Probability of success" is the universal headline number and the most criticised one** (Kitces, Income Lab, Pfau): it hides magnitude, treats an adjustment as ruin, and is badly calibrated when produced by a single-CMA i.i.d. model. Only TPAW and MaxiFi have abandoned it, and both then struggle to tell a lay user what to do.
5. **AI in planners arrived in 2026 and is converging on one pattern**: the engine calculates, the LLM explains, edits are proposed as diffs and applied on confirmation (Boldin AI, Income Lab Penny, RightCapital Iris). Nobody yet uses the LLM to drive paired scenario experiments, sensitivity search, or assumption audits with stated statistical confidence. That is the open ground.
6. **Visualisation is dominated by the percentile fan chart and the Sankey.** The research literature (Kay, Hullman, Padilla) says frequency-framed displays (quantile dot plots, hypothetical outcome plots) are read more accurately by lay users than bands and densities. Almost no planner uses them.

---

## Part 1. Tools

### 1.1 Comparison table

| Tool | Return model | Inflation | Taxes / accounts | Soc. Sec. | Healthcare | Longevity | Flexible spending | Paths |
|---|---|---|---|---|---|---|---|---|
| ProjectionLab | Historical sequences (random start, random restart, block length input, or "historical, no looping") or user-defined distributions [doc] | Historical sequence travels with returns in historical modes [doc: failed-trial view lists returns, dividends, inflation]; else fixed | Federal, state, local estimates; taxable/deferred/Roth/HSA etc.; cost basis; Roth conversion, gain harvesting, IRMAA charts [doc][3p] | User enters PIA and claim age [3p] | Manual premium inputs; Medicare estimated [3p] | Fixed end age | Some flex-spending controls [mem] | up to 2,000 [doc] |
| Boldin | Normal draws around one blended mean and SD per account; accounts made co-moving in 2025-26 update; arithmetic mean used [doc] | Deterministic, user optimistic/pessimistic rates [mem]; not randomised (not mentioned in MC doc) | Federal and state; Roth Conversion Explorer; IRMAA targeting [3p, vendor] | Integrated, spousal strategies, benefit-cut modelling [3p] | Medicare Part B/D and IRMAA engine [3p, vendor] | Fixed longevity age | "Spending Guardrails" around ~80% success with 95%/70% thresholds [3p, vendor] | 1,000 [doc] |
| cFIREsim | Historical cohorts from Shiller data (every start year), or constant growth, or one cycle [doc] | Historical CPI or flat [doc] | None; fees only [doc] | Two income streams with start/end years [doc] | None | Fixed horizon | Rich: % of portfolio, VPW, Guyton-Klinger, Hebeler autopilot, variable CAPE, floors and ceilings [doc] | ~100-150 cohorts |
| FIRECalc | Historical cohorts since 1871; also constant growth and a random mean/SD option [doc] | Historical CPI (or fixed with constant growth) [doc] | None stated [doc] | Amount and start year [doc] | None | Fixed horizon | Bernicke declining spend, % of remaining portfolio, 95% rule [doc] | ~125 cohorts |
| FI Calc | Historical backtest in the Bengen tradition [doc]; Shiller-style annual data from 1871 [mem] | Historical CPI [mem] | None [mem] | Extra income streams [doc: links to Income guide] | None | Fixed horizon | Its reason to exist: about a dozen strategies (constant dollar, % of portfolio, VPW, Guyton-Klinger, 95% rule, CAPE-based, Vanguard dynamic, endowment, 1/N, Hebeler) [mem] | cohorts |
| Pralana (Online / Gold) | Four modes: deterministic, conventional Monte Carlo, bootstrap Monte Carlo, historical [doc] | Deterministic; stochastic option in MC [mem] | The deepest of the consumer tools: federal and state, IRS form mock-ups, IRMAA, LTCG brackets, ACA subsidy used as a Roth-conversion constraint [doc] | Claim-age optimisation inside the full plan [doc] | ACA subsidy, healthcare inputs [doc] | Fixed life expectancy | Consumption smoothing plus conventional strategies [doc] | not stated |
| MaxiFi | Deterministic consumption smoothing first; Premium adds "Living Standard Monte Carlo" where spending floats with returns; assets need 10+ years of history [doc] | Fixed assumption [mem] | Detailed federal and state taxes and Social Security rules (Kotlikoff's speciality) [doc, vendor] | Full claiming optimisation ("Maximized Plan") [3p] | Medicare Part B premiums, IRMAA [3p] | Fixed maximum age (often 100), not probabilities [3p] | Spending is the output, not the input; "as-if" safe rate controls caution [doc] | 500 trajectories [doc, vendor] |
| Empower | Asset-class Monte Carlo on linked holdings; CMAs not published [doc] | Fixed [mem] | Effective-rate style, not bracket-level [mem] | Estimate or user value [mem] | Not modelled in detail [mem] | Fixed age | None | 5,000 [doc] |
| Fidelity (Planning & Guidance Center) | Monte Carlo from historical asset-class return, volatility and correlation since 1926 (Morningstar data); 4 asset classes [doc] | Fixed 2.5%; healthcare 4.9% declining [doc] | Federal/state/local, indexed brackets; excludes AMT, NIIT; "does not calculate actual tax liabilities" [doc] | Estimated from current income, claim age 62-70 [doc] | 15% of expenses default; COBRA 18 months, then marketplace, then Medicare at 65; 4 years of nursing-home LTC at Genworth median [doc] | Planning age = age with 25% survival chance (RP-2014) [doc] | None | 250 minimum in older methodology PDFs [doc]; current page silent |
| Portfolio Visualizer | The widest menu: historical bootstrap (single month, single year, block of years with min/max block, circular), statistical (mean/vol/correlation), forecasted, parameterised; normal or fat-tailed (Student-t, df 5-50); GARCH option [doc] | Historical or parameterised (mean, volatility) [doc] | Flat rate inputs: federal, capital gains, dividend, ACA, state [doc] | None | None | Fixed horizon, or life-expectancy-based withdrawal [doc] | Fixed, % of balance with smoothing, RMD-style [doc] | 10,000 [mem] |
| Rich, Broke or Dead (Engaging Data) | Historical cohorts, Shiller 1871-2023 [doc] | Historical CPI [doc] | One average tax rate [doc] | As an income stream [doc] | None | SSA table (average) or SOA tables (low/high) overlaid as population probability [doc] | One rule: cut X% when portfolio falls below a threshold [doc] | ~100 cohorts |
| TPAW Planner | Block bootstrap of monthly US stock and 10-year Treasury returns from 1871, default 5-year blocks, history shifted so its mean matches the chosen expected return (options: regression prediction, 1/CAPE, historical, fixed) [doc homepage][3p] | Works in real terms; real bond return from TIPS yields [3p] | None: user enters after-tax amounts; no account types [mem] | As an income stream valued in the "total portfolio" [mem] | None | Fixed max age, with spending tilt and legacy [mem] | The whole design: amortisation-based withdrawal recomputed yearly, risk tolerance slider mapped to RRA and Merton allocation [3p] | 500 default [mem] |
| Honest Math | Parametric, monthly, "fat-tailed" stochastic process (unnamed); user CMAs [3p] | Not documented | "Tax considerations", no method given [3p] | Income sources [3p] | None | Fixed | Black-swan event injection, glide path stages [3p] | 10,000 [3p] |
| ERN SWR toolkit (Google Sheet) | Historical cohorts, monthly; Fama-French from 1926 plus Shiller; assumed returns to extend recent cohorts [doc] | Historical CPI [doc] | None | "Cash Flow Assist" tab converts SS and pensions to % of portfolio [doc] | None | Horizon in months | CAPE-based rule (intercept + slope x earnings yield); conditional failure rates by CAPE bucket and drawdown [doc] | cohorts |

### 1.2 Tool notes: signature visualisations and where users say they fall short

**ProjectionLab** (https://projectionlab.com, https://projectionlab.com/monte-carlo)
- Signature visuals: year-by-year Sankey cash-flow diagram with a year scrubber; Chance of Success fan chart with milestone-timing view; per-trial table where a failed trial can be opened to see its return, dividend and inflation sequence; tax analytics (effective and marginal rate over time); Compare Mode [doc][3p].
- Useful details: RNG can be seeded for reproducibility; user-defined outcome categories ("how far did the trial get") instead of a binary success flag [doc, changelog and HN developer replies].
- Complaints (HN launch thread, 766 points, 288 comments, https://news.ycombinator.com/item?id=36849502): success probability ignores shortfall size (commenter asked for expected shortfall); users want variable lifespan and market-responsive spending instead of a fixed end age; drawdown order surprising across spouses; onboarding "makes it seem simple but forecasting assumes expertise"; defaults that silently produce nonsense (0% mortgage APR); users want guidance on return assumptions; pensions hard to find; trial auto-charge called a dark pattern; no account linking. Reviewers: "numbers-intensive", "a bit of a learning curve" (quoted on Boldin's comparison page, biased). A ChooseFI forum user reported ProjectionLab and Boldin giving much lower success rates than Portfolio Visualizer for the same inputs [3p], which is the inter-tool inconsistency users find most unsettling.

**Boldin** (https://www.boldin.com/retirement/understanding-boldins-monte-carlo-simulation-what-it-is-why-it-matters-and-whats-new/)
- The model is the weakest of the "serious" planners: normally distributed returns around a single blended mean and SD per account, no asset-class structure, no randomised inflation. The 2025-26 update fixed two outright errors (geometric mean used as arithmetic mean, and accounts drawn independently) [doc]. Score changes after that update are a plausible source of user confusion.
- Signature visuals: single "Chance of Retirement Success" gauge; lifetime income/expense/tax bar charts; Roth Conversion Explorer; Insights library [3p].
- Strengths to respect: breadth of household detail (Medicare, IRMAA, state taxes, Social Security cuts), account linking, coaching, and Boldin AI.
- Complaints: opaque assumptions and score swings [3p inference]; no historical backtest [3p, vendor page concedes]; optimistic/pessimistic toggles stand in for real uncertainty [mem].

**cFIREsim** (https://www.cfiresim.com) and **FIRECalc** (https://firecalc.com)
- Signature visual: the spaghetti plot of every historical start year. Honest, memorable, and unreadable as a decision aid.
- Complaints [mem, long-standing forum consensus]: US-only history with heavily overlapping cohorts (about five independent 30-year periods since 1871); recent cohorts are incomplete; no taxes or account types; binary success; FIRECalc's UI is frozen in 2005. FIRECalc's own page states its philosophy as "if it survived the worst of the past it will probably survive the future" [doc], which is an assumption, not a result.

**FI Calc** (https://ficalc.app)
- Signature visuals: clean per-cohort drill-down (click a start year, see that life), withdrawal-over-time charts that show spending volatility of each strategy, not just survival. Praised as "visually friendly" (https://www.mymoneyblog.com/fi-calc-visually-friendly-retirement-withdrawal-calculator.html).
- Falls short: same historical-cohort limits; no taxes; one portfolio.

**Pralana** (https://pralanaretirementcalculator.com)
- Signature features: "Metric MRI" (click any number to see how it was derived) and IRS-form mock-ups [doc]. Both are trust features worth copying in spirit. Three simultaneous scenarios.
- Complaints: spreadsheet-style density, long manuals, "requires more technical and financial planning competence" [3p]; early-retirement.org users report results "not as expected until inputs are tuned" (https://www.early-retirement.org/threads/pralana-gold-or-pralana-online.123819/).

**MaxiFi** (https://maxifi.com/how-it-works, review at https://thefinancebuff.com/maxifi-review-financial-planning.html)
- Signature idea: the output is a sustainable living standard (annual discretionary spending), not a success percentage. Monte Carlo produces 500 living-standard trajectories; a lifetime expected-utility index compares strategies within a risk-tolerance column [doc].
- Complaints (The Finance Buff): false precision to the dollar; claiming and annuity advice driven by a fixed max age rather than mortality probabilities; "discretionary spending" jargon; the Monte Carlo range is so wide the reviewer could not choose a spending level and found the deterministic reports more useful; no account linking; needs help to get started. Others note retirees do not all want flat consumption (https://larrykotlikoff.substack.com/p/how-to-invest-let-economics-not-wall).

**Empower** (https://www.empower.com/the-currency/money/how-much-do-i-need-to-retire) and **Fidelity** (https://www.fidelity.com/go/guidance/retirement-methodology)
- Signature visuals: one projected-balance curve with a shaded band; Fidelity reports the plan under a "significantly below average market" (90% of simulations did better), "below average" (75%) or "average" (50%) [doc].
- Fidelity's methodology document is the most transparent of the incumbents and a good checklist: asset-class proxies since 1926, fixed 2.5% inflation, planning age at 25% survival, COBRA then marketplace then Medicare, 4-year LTC event, taxes modelled but "should not be used for tax planning" [doc].
- Complaints [mem]: Empower's planner is a lead-generation funnel (advisor sales calls); assumptions cannot be inspected; Fidelity's default view at the 90% confidence level reads as pessimistic and gives no sense of the distribution.

**Portfolio Visualizer** (https://www.portfoliovisualizer.com/monte-carlo-simulation)
- The best menu of return generators available to consumers and the reference for "what options should exist": block bootstrap with variable block size, Student-t tails, GARCH volatility [doc].
- Signature visuals: percentile table and percentile fan (10/25/50/75/90), survival-probability-by-year curve, plus safe and perpetual withdrawal rate tables.
- Falls short: it is a portfolio tool. One pot, flat tax rates, no household, no lifespan. Free tier restricted since 2023 [mem].

**Rich, Broke or Dead** (https://engaging-data.com/will-money-last-retire-early/)
- The single best chart in the field: a stacked area over age showing the share of outcomes that are broke (red), alive with less than start, more than start, 2x, 5x (greens), and dead (grey) [doc]. It makes the point that for most ages "dead" dwarfs "broke".
- Falls short: mortality is a population overlay, independent of the financial path, not a sampled lifetime; a single average tax rate; historical cohorts only; "black swans outside scope" [doc].

**TPAW Planner** (https://tpawplanner.com)
- The intellectually strongest spending model: wealth is the total portfolio (savings plus present value of future income), spending is re-amortised every year, and risk tolerance jointly sets asset allocation (Merton) and how fast spending responds. Expected returns are tied to current valuations (regression prediction or 1/CAPE; TIPS yields for bonds) and the historical distribution is shifted to that mean before 5-year block sampling [doc homepage][3p]. Open source.
- Signature visual: the spending fan (percentile bands of monthly real spending by age) as the primary output, with the portfolio fan secondary. This is the right primary chart for any flexible-spending model.
- Falls short [3p][mem]: no taxes and no account types (user must think in after-tax terms); output is a wide fan with no decision rule, which lay users find hard to act on; at default settings median spending rises through retirement, which surprises people (Citywire thread); US-only history.

**Honest Math** (https://kevinbracker.substack.com/p/honest-math-a-portfolio-simulation)
- Claim to fame is fat-tailed monthly sampling with 10,000 trials and "black swan" injection. The process is not documented, nor are correlation or inflation treatment [3p]. Good marketing of a narrow improvement; household model thin.

**Early Retirement Now SWR toolkit** (https://earlyretirementnow.com/2018/08/29/google-sheet-updates-swr-series-part-28/)
- The most rigorous historical-cohort work for long (40-60 year) horizons. Key ideas to import: fail-safe withdrawal rate conditional on starting CAPE bucket (<20, 20-30, 30+) and on drawdown from the peak; supplemental cash flows as first-class inputs; six explicit criteria for grading withdrawal rules (https://earlyretirementnow.com/2017/03/15/the-ultimate-guide-to-safe-withdrawal-rates-part-11-criteria/).
- Signature visuals: fail-safe SWR by cohort start date as a time series; failure-rate tables by withdrawal rate x CAPE regime.
- Falls short: a Google Sheet; no taxes; no stochastic model by design (ERN argues i.i.d. Monte Carlo destroys mean reversion); recent cohorts padded with assumed returns [doc].

### 1.3 What this implies for v2

| Gap in the field | v2 answer |
|---|---|
| Returns randomised, everything else fixed | Joint scenario generator: inflation, equities, bonds, bills, home prices, plus unemployment regime driving job loss |
| Fixed death age | Sampled lifetimes per person per path; success defined as never short while alive |
| Tax depth and model depth never in the same tool | Real brackets, IRMAA, ACA cliff/subsidy, NIIT, LTCG stacking, indexed to simulated inflation, inside every path |
| Binary success | Report probability of shortfall, depth and duration of shortfall, spending-cut frequency, and a funded-ratio style margin |
| 250 to 2,000 paths | 100k+ with common random numbers, so differences between scenarios carry confidence intervals |
| Opaque assumptions, inter-tool disagreement | Every assumption visible with provenance; "Metric MRI"-style drill-down from any number to the ledger of one path |
| Wide fans with no guidance (TPAW, MaxiFi) | Solver and sensitivity tools that turn the distribution into "what to change" |
| Starting state ignored | Condition on today's valuations and yields (CAPE, TIPS curve, current inflation) |

Things to deliberately not build: account linking and aggregation, budgeting, a strategy zoo of a dozen withdrawal rules (ship three or four that are distinct: constant real, risk-based guardrails, amortisation-based, floor-and-ceiling percentage), estate planning, international tax systems, advisor/client management.

---

## Part 2. Modelling practice

### 2.1 Fat tails and volatility clustering

- Stylised facts (Cont 2001, https://doi.org/10.1080/713665670): heavy tails, volatility clustering, leverage effect, near-zero linear autocorrelation of returns but strong autocorrelation of squared returns; "aggregational Gaussianity", meaning tails thin out as the horizon lengthens.
- The practitioner corollary (Tharp at Kitces, https://www.kitces.com/blog/monte-carlo-analysis-risk-fat-tails-vs-safe-withdrawal-rates-rolling-historical-returns/): annual 60/40 real returns since 1871 do not reject normality; the fat-tail problem lives at daily and monthly frequency. So "use a Student-t" is a small fix. What matters over 40-60 years is the dependence structure (clustering, regimes, mean reversion) and uncertainty about the mean.
- Recommendation: simulate monthly, with clustering generated by a regime or stochastic-volatility structure, and heavy-tailed innovations (Student-t with roughly 5-8 degrees of freedom, as Portfolio Visualizer exposes). Hardy's two-regime lognormal (https://doi.org/10.1080/10920277.2001.10595984) is the standard actuarial baseline and reproduces monthly skew, kurtosis and clustering with six parameters. Validate against the empirical distribution of 1, 12, 60, 120 and 360 month real returns and of maximum drawdowns, not against monthly moments alone.

### 2.2 Mean reversion: why i.i.d. is wrong in both directions

- Evidence for long-horizon mean reversion in equities: Poterba and Summers 1988 (https://doi.org/10.1016/0304-405X(88)90021-9); Campbell and Viceira 2005, "The Term Structure of the Risk-Return Trade-Off" (https://doi.org/10.2469/faj.v61.n1.2682), where annualised equity volatility falls from about 17% at one year to about 8% at 25+ years in a VAR with the dividend yield as predictor.
- Kitces/Tharp's test: 10,000 i.i.d. normal paths calibrated to history put about 6.5% of trials below the worst actual 30-year sequence; a 93.5% i.i.d. success rate matched 100% historical success. With a 2% real mean, about half of i.i.d. trials were worse than the historical worst. The mechanism: i.i.d. draws chain bear markets together without the rebounds history shows. So **i.i.d. overstates the dispersion of long-horizon compound returns**.
- But i.i.d. also **understates sequence risk in the first decade**. Valuation-driven low-return regimes are persistent (1929, 1966, 2000 cohorts): a decade of poor returns is more likely than independence implies when starting valuations are high, and withdrawals during that decade do damage that later mean reversion cannot repair. Momentum at 1-12 months plus clustering also makes deep multi-year drawdowns more frequent than i.i.d. normal. A model that conditions on starting state gets a wider near-term distribution and a narrower far-term one than i.i.d.
- The counterweight is parameter uncertainty. Pastor and Stambaugh 2012, "Are Stocks Really Less Volatile in the Long Run?" (https://doi.org/10.1111/j.1540-6261.2012.01722.x): once uncertainty about the expected return and about the predictive relation is included, predictive variance per year rises with horizon. The US sample is also a survivor: Anarkulova, Cederburg and O'Doherty use 38 developed markets and find a 12% chance of real loss over 30 years in equities ("Stocks for the long run? Evidence from a broad sample of developed markets", https://doi.org/10.1016/j.jfineco.2021.06.040) and a safe withdrawal rate nearer 2.3% than 4% for a 65-year-old couple (https://papers.ssrn.com/abstract=4227132).
- Calibration evidence: Income Lab's backtest of forecast quality for 1951-2002 retirements (https://www.kitces.com/blog/monte-carlo-models-simulation-forecast-error-brier-score-retirement-planning/). Historical and regime-based models had Brier scores about 25% lower than single-CMA Monte Carlo. Traditional Monte Carlo was overconfident above 50%: at a stated 95%, about 20 in 100 plans overspent, not 5. Regime-based (near-term CMAs conditioned on CAPE, long-term CMAs normalised) was conservatively biased.
- Recommendation for v2: (a) model mean reversion structurally through a valuation state variable, not by a fudge; (b) draw the long-run real equity premium per path from a prior (for example centred near 4-5% real with a standard deviation of 1-1.5 points) so that mean reversion does not produce false long-run certainty; (c) report a calibration check against historical cohorts in the methodology page; (d) offer a pessimistic "developed-markets" calibration as a named stress, since US history is one draw.

### 2.3 Bootstrap: fixed block vs stationary

- i.i.d. bootstrap (Portfolio Visualizer "single month/year") keeps the marginal distribution and cross-asset correlation within a month but destroys all serial dependence. Do not offer it.
- Fixed-length block bootstrap (v1 used 12-month blocks; TPAW defaults to 60 months) keeps dependence inside blocks but creates artificial seams at fixed multiples of the block length and makes the series non-stationary.
- Stationary bootstrap (Politis and Romano 1994, https://doi.org/10.1080/01621459.1994.10476870): block lengths are geometric with mean 1/p, with wrap-around, so the resampled series is stationary. Automatic block-length selection: Politis and White 2004 with the Patton, Politis and White 2009 correction (https://doi.org/10.1080/07474930802459016).
- Block lengths used in the retirement literature vary by an order of magnitude, depending on what dependence is being preserved:
  - Forsyth and co-authors (decumulation control papers, e.g. https://arxiv.org/abs/2101.02760): paired stock/bond stationary bootstrap with expected block sizes from about 3 months to 2 years, tested across 1-12 months and longer for robustness.
  - TPAW: 5-year fixed blocks "to preserve autocorrelations within five years" [doc].
  - Anarkulova, Cederburg and O'Doherty: stationary block bootstrap with a mean block of 120 months, drawing whole-country blocks, specifically to preserve long-horizon mean reversion and stock/bond/inflation co-movement [mem; confirm in https://papers.ssrn.com/abstract=4590406].
- Known bias: Cogneau and Zakamouline 2013 (https://doi.org/10.1080/14697688.2012.713115) show block bootstrap estimates of long-horizon risk are biased; naive use underestimates the risk of independent series and overestimates the risk of mean-reverting ones, and long blocks relative to sample length throw away the cross-block dependence entirely.
- Two structural limits of any bootstrap: it cannot condition on today's starting state (TPAW's mean shift is a patch), and 155 years of US data contain roughly five non-overlapping 30-year periods, so 100k bootstrap paths are 100k recombinations of very little independent long-horizon information.
- Recommendation: make the primary generator parametric and state-conditioned (Part 2.4), and ship a **joint stationary block bootstrap** (all series resampled with the same indices; mean block 60-120 months; real-return mean optionally re-centred) as a named alternative "History reshuffled" model plus exact historical cohorts as "History replayed". Showing the headline number under all three is the single most honest thing the product can do, and follows Income Lab's advice to triangulate.

### 2.4 Valuation-dependent expected returns

- Campbell and Shiller 1998 (https://doi.org/10.3905/jpm.24.2.11); Vanguard, "Forecasting stock returns: what signals matter, and what do they say now?" (Davis, Aliaga-Diaz, Thomas 2012): CAPE explains roughly 40% of the variance of subsequent 10-year real returns, more than any other tested signal, and still leaves most of it unexplained.
- ERN conditions fail-safe withdrawal rates on CAPE buckets [doc]; TPAW sets the equity expected return from a regression or 1/CAPE and the bond return from TIPS yields [doc]; Income Lab's regime-based model uses near-term CMAs from historically similar CAPE periods [doc].
- Recommendation: carry a valuation state (log CAPE or earnings yield) in the scenario generator with slow mean reversion (half-life on the order of 5-10 years), initialise from current market data bundled at build time and shown to the user with its date, and let expected equity returns depend on it. Initialise the yield curve and inflation state the same way. Expose a "valuations don't matter" toggle (state fixed at its long-run mean) so users can see how much of the answer comes from this assumption. This is the regime-based approach Tharp in 2017 said no planning software supported.

### 2.5 The critique of "probability of success"

Sources: Kitces on magnitude (https://www.kitces.com/blog/multidimensional-abstraction-to-communicate-monte-carlo-simulation-probability-and-magnitude-of-success/), on renaming outcomes as "probability of adjustment" and "probability of excess" (https://www.kitces.com/blog/renaming-the-outcomes-of-a-monte-carlo-retirement-projection/), on one-time vs ongoing plans (https://www.kitces.com/blog/monte-carlo-one-time-vs-ongoing-retirement-planning-probability-of-success-guardrails/), on communicating fear (https://www.kitces.com/blog/calming-client-fears-communicating-monte-carlo-outcomes-probability-of-failure-adjustment-based-planning/); Income Lab on guardrails (https://incomelaboratory.com/risk-based-vs-guyton-klinger-guardrails/, https://www.kitces.com/blog/guyton-klinger-guardrails-retirement-income-rules-risk-based/); Pfau and Young on assumption sensitivity (https://www.advisorperspectives.com/articles/2023/01/10/the-dangers-of-monte-carlo-simulations).

- **Magnitude is missing.** Two plans with 20% failure can differ between one year and fifteen years of lost income.
- **Failure is really adjustment.** Nobody spends on autopilot into a wall. A plan that fails in a fixed-spending simulation is one that would have needed a cut.
- **It is a slider, not a risk measure, in an ongoing plan.** Kitces: holding a constant 95%, 70%, 50% or 20% target and re-planning yearly gives similar ranges of realised spending; the target mostly trades early income against legacy.
- **It is asymmetric.** Calling underspending a "success" hides the cost of working years longer than needed (Income Lab).
- **It is fragile to inputs.** Pfau and Young: the 4% rule scores from above 95% to 62% across published CMA sets.
- **Guardrails: risk-based beat withdrawal-rate-based.** Guyton-Klinger triggers on the withdrawal rate drifting 20% and cuts 10%; in history this produced cumulative cuts of 28% (2008 cohort), 36% (1999) and 45% (1936) per Income Lab's examples (vendor-selected). Risk-based guardrails re-run the plan each year and adjust only when total plan risk leaves a band, closing part of the gap (about 10% in their example), giving a 3% cut in the 2008 case. Communicate them as dollars: "spending stays at $X unless the portfolio falls below $Y, in which case cut $Z a month".

Recommendation for v2 headline metrics (all in today's dollars, all "while alive"):
1. Probability of never falling short of essential spending while alive (the honest version of success).
2. Probability of having to cut discretionary spending at least once, plus the typical size and duration of cuts (adjustment framing).
3. Depth and duration of shortfall given shortfall: median years short, median fraction of essential spending unmet, age at first shortfall. This is the expected-shortfall request from the HN thread.
4. Excess: median and 10th/90th percentile bequest, and "years you could have retired earlier" at the chosen confidence.
5. A guardrail statement in dollars for the current year when a guardrail spending rule is active.
Never show a bare percentage without its Monte Carlo interval and without the model-choice range from 2.3.

### 2.6 Amortisation-based withdrawal and TPAW

- Amortisation-based withdrawal: each year spend the payment that would exhaust the total portfolio over the remaining horizon at the expected return (PMT), then recompute. VPW is the simplest version (https://www.bogleheads.org/wiki/Variable_percentage_withdrawal); see also https://www.whitecoatinvestor.com/amortization-based-withdrawal-vs-safe-withdrawal-rates/.
- TPAW (Ben Mathew): three additions. The asset base is the total portfolio, including the present value of future earnings, Social Security and pensions net of committed expenses. Asset allocation is set on that total, using the Merton share (risk premium divided by relative risk aversion times variance), so a young earner with large bond-like human capital holds more stock in savings. A spending tilt and a legacy target shape the schedule. The risk-tolerance slider maps to relative risk aversion (a forum reader found slider value 15 corresponds to RRA of about 1.9 [3p]).
- Why it matters: with CRRA utility and i.i.d. returns, Merton 1969 shows the optimal policy is exactly this shape, a constant risky share of total wealth and consumption as an annuity-like fraction of total wealth. It never depletes, because spending scales down with wealth. Failure becomes "spending fell below what I need", which requires a floor to be meaningful.
- Weaknesses to design around: spending volatility passes straight through (a 30% equity fall cuts spending by the equity share of total wealth times 30%); results depend heavily on the expected-return input; taxes complicate "wealth" (pre-tax and Roth dollars are not equal).
- Recommendation: ship amortisation-based spending as one of the spending rules, computed on after-tax total wealth, with an essential-spending floor and optional smoothing. Measure shortfall against the floor. Do not make it the only model: most users think in terms of a spending target.

### 2.7 Lifecycle utility as an objective

- Yaari 1965 and Merton 1969 set the framework; Milevsky and Huang, "Spending Retirement on Planet Vulcan" (https://doi.org/10.2469/faj.v67.n2.2) derive optimal spending with mortality, showing spending should decline with age in proportion to survival probability and risk aversion; Irlam's dynamic-programming work (https://www.aacalc.com) and Haghani and White's "The Missing Billionaires" popularise expected-utility sizing; MaxiFi ships a lifetime expected utility index by risk-tolerance column [doc].
- Value: one number that ranks whole strategies while penalising both running short and dying with unused wealth, which success probability cannot do. The certainty-equivalent consumption (the constant real annual spending giving the same expected utility) is in dollars and interpretable.
- Problems: users cannot state their risk aversion; CRRA with a floor is very sensitive near the floor; bequest utility needs another parameter; a utility number means nothing to a lay user.
- Recommendation: compute certainty-equivalent spending (CRRA, with gamma at 2, 4 and 8, survival-weighted, floor at essential spending, optional bequest weight) as a secondary diagnostic for ranking strategies in solver and comparison tools. Show it only in an "advanced" panel as "steady-income equivalent" in dollars per year. Never use it as the headline.

### 2.8 Funded ratio

- Definition: present value of assets (portfolio plus future income) divided by present value of liabilities (planned spending), discounted at a low-risk real rate (TIPS curve), survival-weighted or to a planning age. Pension-fund practice brought to households by Pfau and McLean's Retirement Researcher tools and by the safety-first school (https://retirementresearcher.com/two-philosophies-for-retirement-income-planning-part-one-probability-based/).
- Strengths: deterministic, no return assumptions beyond a discount rate, stable from year to year, easy to explain ("you hold 118% of what your plan costs if invested safely"). Weaknesses: ignores risk premium and sequence risk; one number.
- Relation to Monte Carlo: a ratio of 100% at the safe rate corresponds to very high success for an all-TIPS portfolio and is a conservative bar for a risky one. The humbledollar thread asking whether 100% funded equals 50% success (https://humbledollar.com/forum/funded-ratio-vs-monte-carlo-different-routes-to-get-to-the-same-destination-or-not/) shows users want the two reconciled.
- Recommendation: compute it per path-independent baseline (current TIPS curve, survival-weighted) and show it next to the simulated probability as the "no-market-luck" view. Also use funded ratio along each simulated path as the state variable for risk-based guardrails: it is far cheaper than nested simulation and gives the same ordering.

### 2.9 Variance reduction and common random numbers

- **Common random numbers (CRN)** are mandatory. For two scenarios A and B run on the same paths, Var(A - B) = Var(A) + Var(B) - 2Cov(A, B), and the covariance is large because most paths end the same way under both. For a failure indicator, the paired difference has variance (d - delta^2)/N where d is the share of paths whose outcome flips and delta the net change. If retiring a year earlier flips 1% of paths net, the standard error at N = 100,000 is about 0.03 points, against 0.10 points for independent runs at 5% failure, a tenfold variance saving. Glasserman, "Monte Carlo Methods in Financial Engineering" (2003), chapter 4, is the reference.
- CRN only works if a decision change does not shift which random number is consumed where. Implementation rules for the Rust core:
  1. Use a counter-based generator (Philox or ChaCha) keyed by (seed, path, stream, period), not a sequential stream.
  2. Generate the exogenous world (markets, inflation, unemployment regime, mortality, health shocks) independent of all user decisions.
  3. Decision-dependent events take a dedicated uniform per period and compare it to a hazard (job loss occurs if u < hazard), so the same u is reused under both policies.
  4. Path identifiers are stable, so a path can be named, bookmarked and replayed.
- **Antithetic variates**: pair each innovation vector z with -z. Gains are real for near-linear outputs (mean and median wealth) and modest for failure indicators (typically 1.2x to 2x). They do not apply cleanly to bootstrap index sampling, and they break the i.i.d. assumption used for simple standard errors (pairs must be treated as one unit). Low value; implement only if it stays simple.
- **Quasi-random (scrambled Sobol)**: converges near O(1/N) for smooth integrands of low effective dimension (Caflisch 1998, https://doi.org/10.1017/S0962492900002804; Joe and Kuo direction numbers https://web.maths.unsw.edu.au/~fkuo/sobol/). A lifetime simulation has thousands of dimensions (720 months x 5+ factors) and discontinuous outputs (ruin indicator, death, job loss), where the advantage collapses toward plain Monte Carlo unless variance is concentrated in early dimensions with a Brownian-bridge or PCA construction. Recommendation: skip Sobol for the core. If wanted later, use it only for the few dimensions that dominate variance (stratify paths on lifespan quantile and on first-decade cumulative real equity return), which is cheap stratified sampling with most of the benefit.
- **Importance sampling** toward bad early sequences would sharpen 1% tail estimates but complicates every downstream statistic with weights. Skip; 100k+ plain paths are enough.

### 2.10 How many paths

Standard error of an estimated probability p from N independent paths is sqrt(p(1 - p)/N). For p = 5%:

| N | Standard error | 95% interval half-width | Who runs this |
|---|---|---|---|
| 250 | 1.38 pts | +/- 2.7 pts | Fidelity (older docs) |
| 500 | 0.97 pts | +/- 1.9 pts | NewRetirement help, MaxiFi, TPAW default |
| 1,000 | 0.69 pts | +/- 1.35 pts | Boldin |
| 2,000 | 0.49 pts | +/- 0.96 pts | ProjectionLab max |
| 5,000 | 0.31 pts | +/- 0.60 pts | Empower |
| 10,000 | 0.22 pts | +/- 0.43 pts | Portfolio Visualizer, Honest Math |
| 100,000 | 0.069 pts | +/- 0.135 pts | v2 target |
| 1,000,000 | 0.022 pts | +/- 0.043 pts | |

Notes:
- Worst case is p = 50%: 1.58 pts at N = 1,000 and 0.16 pts at N = 100,000.
- Tail statistics use only the tail. Expected shortfall in the worst 5% at N = 100,000 rests on 5,000 paths; "probability of shortfall given you live past 95" rests on the 10-20% of paths that live that long.
- Quantile standard error is sqrt(q(1 - q)/N)/f(x_q), which is large where the density f is thin, so the 1st percentile wealth line is noisy even at 10,000 paths.
- A decision heatmap of 12 x 12 cells needs CRN across cells far more than raw N: 10,000 shared paths per cell gives smooth contours; independent paths per cell give speckle.
- More paths do not fix model error. The spread between the three generators in 2.3 will be several points, an order of magnitude larger than Monte Carlo error at 100k. Report both, labelled separately: "sampling precision +/- 0.1 pt; model range 88-94%".
- Progressive refinement: show results at 2,000 paths within a few hundred milliseconds, refine to 100k in the background, and state the interval so users see it tighten.

---

## Part 3. Visualisation

Foundational evidence:
- Kay, Kola, Hullman, Munson 2016, "When (ish) is My Bus?" (https://doi.org/10.1145/2858036.2858558): quantile dot plots give more accurate and less variable probability estimates than densities for lay users.
- Fernandes, Walls, Munson, Hullman, Kay 2018 (https://doi.org/10.1145/3173574.3173718): quantile dot plots and CDFs produce better decisions over time than intervals or text.
- Hullman, Resnick, Adar 2015 (https://doi.org/10.1371/journal.pone.0142444): hypothetical outcome plots beat error bars and violins for judging comparisons.
- Kale, Kay, Hullman 2020 (https://arxiv.org/abs/2007.14516): the display that gives the best magnitude estimates is not always the best decision aid; adding a mean makes people ignore the uncertainty.
- Padilla, Kay, Hullman, "Uncertainty Visualization" (https://doi.org/10.1002/9781118445112.stat08296) and Wilke's chapter (https://clauswilke.com/dataviz/visualizing-uncertainty.html): frequency framing ("12 of 100 lives") beats probability framing; bands invite deterministic-boundary and containment errors.

### 3.1 Catalogue

**1. Quantile bands (percentile fan)**
- Answers: what range of wealth or spending is plausible at each age.
- Pitfalls: a percentile line is not a life; no single path follows the 10th percentile, so users read the lower band as "a bad scenario" and underestimate how paths cross. The outer band edge reads as a hard limit. Nominal dollars on a linear axis make the top band dominate. Bands of wealth after death are meaningless unless conditioned on being alive.
- v2: **keep**, as the backbone, with fixes: today's dollars; bands are computed over lives still alive at that age, with the count alive shown as a thin strip beneath; 5/25/50/75/95 only; three real sampled paths drawn on top (see 3); a toggle between portfolio and spending as the plotted quantity.

**2. Density fan (continuous gradient, Bank of England style)**
- Answers: the same as 1, with relative likelihood by colour intensity.
- Pitfalls: lay users cannot read probability from colour intensity; the gradient implies more precision than the model has; expensive to render legibly on small screens.
- v2: **skip** as a separate chart. Use two or three opacity steps on the band chart and stop there.

**3. Spaghetti with highlighted paths**
- Answers: what individual futures look like; shows that paths zig-zag and cross.
- Pitfalls: 100 lines are noise; 100,000 are a blob; which paths get highlighted is an editorial choice that can mislead.
- v2: **keep as an overlay**, not a chart of its own. Draw about 30 faint sampled paths behind the bands and highlight three chosen by a stated rule: the path whose outcome ranks at the 10th, 50th and 90th percentile on the headline metric. Clicking a path opens its life story (11).

**4. Hypothetical outcome plots (animated draws)**
- Answers: how variable is this, felt rather than read; good for "A vs B, how often does A win".
- Pitfalls: needs time and attention; inaccessible for some users (motion sensitivity, screen readers); cannot be screenshotted; users may fixate on one memorable frame.
- v2: **keep, narrowly**: a "shuffle" control that draws another random life onto the main chart, one at a time, user-paced rather than auto-playing. Respect reduced-motion settings. No autoplaying animation anywhere.

**5. Quantile dot plots**
- Answers: "how sure is that?" for a single quantity, as countable frequencies: each of 50 or 100 dots is an equally likely outcome.
- Pitfalls: bin choice changes the picture; only shows one variable; dots must be few enough to count (20-100).
- v2: **keep, high priority.** Use for the two most important single-number distributions: age at which work becomes optional (100 dots by age), and outcome at end of life (dots coloured by short / tight / comfortable / large surplus). This replaces the gauge. Phrase the caption as counts: "In 7 of 100 simulated lives you run short before dying."

**6. Survival-style stacked probability over age ("rich, broke or dead")**
- Answers: at each age, what share of lives are dead, alive and short, alive and tight, alive and fine, alive and wealthy. Puts financial risk and mortality on one scale.
- Pitfalls: stacked areas hide transitions (a life can leave "short" and return); middle layers are hard to read against a moving baseline; for couples "dead" is ambiguous; category thresholds are arbitrary.
- v2: **keep, high priority, and do it properly**: lifetimes are sampled per path, so death is correlated with nothing spurious and joint survival for couples is exact. Categories: both dead / one alive / alive and short of essentials / alive with spending cut / alive on plan / alive with more than 2x starting real wealth. Put the "short" layer at the baseline so its height is readable. Offer a per-person toggle.

**7. Success heatmap over two decision variables**
- Answers: "when can I stop and how much can I spend" as a trade-off surface; shows where the cliff is.
- Pitfalls: rainbow or red-green scales are unreadable for colour-blind users and exaggerate boundaries; a discrete grid without CRN is speckled; a probability scale implies the probability is the objective; expensive.
- v2: **keep.** Retirement age (x) by annual spending in today's dollars (y), about 12 x 12 cells, common random numbers across all cells, sequential single-hue scale with labelled contour lines at 75/90/95%, the current plan marked, cell tooltips showing the shortfall depth as well as the probability. Compute progressively at reduced N then refine. A second preset: retirement age by Social Security claim age.

**8. Tornado sensitivity chart**
- Answers: what does the result depend on most.
- Pitfalls: one-at-a-time changes ignore interactions; bar length depends entirely on the chosen ranges, so arbitrary plus or minus 10% ranges mislead (a 10% change in spending is not as likely as a 10% change in equity returns); mixes things the user controls with things they do not.
- v2: **keep**, split into two panels: "what you control" (retirement age +/- 1 year, spending +/- 5%, savings rate, claim age, housing choice, Roth conversion on/off) and "what you don't" (equity premium -1 pt, inflation +1 pt, longevity +3 years, healthcare inflation, an earner losing a job at 50, long-term-care event). State each range in the label. Compute by CRN paired runs, and show the interval of each difference so small bars are visibly indistinguishable from zero.

**9. Sankey / alluvial cash flow**
- Answers: where does the money come from and go in a given year, including taxes.
- Pitfalls: it is one year of one path, and every Monte Carlo number disappears; many small flows become clutter; across-year alluvial versions are unreadable.
- v2: **keep, scoped**: one year of the selected path (default the median path), with a year scrubber, at most about 12 nodes: sources (wages per earner, Social Security, withdrawals by account type, conversions) to uses (federal tax, state tax, FICA, healthcare, housing, essentials, discretionary, saving by account type). Label it with the path it belongs to. Do not attempt an alluvial across years.

**10. Small multiples for scenario comparison**
- Answers: how do several plans compare at a glance.
- Pitfalls: users compare across panels poorly unless axes are shared; more than about six panels loses people; differences smaller than the bands are invisible (which is why 11 exists).
- v2: **keep** for up to four scenarios: the same band chart with shared axes, plus one row of the same three numbers under each. For A vs B questions, lead with the paired difference chart instead.

**11. Paired-difference distributions under common random numbers**
- Answers: "what does this change actually do, path by path?" It removes market luck from the comparison.
- Pitfalls: abstract for lay users; differences in a probability need a different display from differences in dollars; must be explicit that both plans lived through the same futures.
- v2: **keep, high priority; this is the feature nobody else has.** Display as a 100-dot quantile dot plot of the per-life difference in a dollar metric (lifetime spending, or wealth at 85) with zero marked, and a three-way count for the binary outcome: "of 1,000 lives: 31 rescued by the change, 4 harmed, 965 unchanged". Always shown with its interval.

**12. Lifetime tax-bracket-fill chart**
- Answers: where is my income taxed each year, where is there unused low-bracket room for Roth conversions, where do IRMAA and ACA cliffs bite.
- Pitfalls: on one path it looks deterministic; brackets move with simulated inflation, so real dollars are essential; stacking ordinary income and capital gains is easy to get wrong visually.
- v2: **keep.** Per year of the selected path: stacked bar of taxable income by source (wages, pension, taxable Social Security, traditional withdrawals, Roth conversions, gains), over bracket threshold lines in today's dollars, with markers for the ACA subsidy limit, IRMAA tiers and the 0% long-term gains ceiling. Add a companion line of effective and marginal rate, with a band across paths. Pralana and ProjectionLab show pieces of this; none combine brackets, cliffs and conversion fill.

**13. Annotated single-path life story**
- Answers: "what does a typical / unlucky life actually look like year by year?" Makes the simulation legible and auditable.
- Pitfalls: any one path is an anecdote; users over-weight a vivid bad story; a "typical" path does not exist, so the selection rule must be stated; risk of reading it as a forecast.
- v2: **keep, high priority.** A timeline of one sampled life with event annotations generated from the ledger (laid off at 47 during a recession, eight months out; market down 38% in year two of retirement; spending cut 10% for four years by guardrail; ACA to Medicare at 65; long-term-care event at 86; died at 91 with $410k). Offer three named picks by rule (median, 10th-percentile, a failing life if any) plus shuffle. Every figure links to the year's ledger: this is v2's version of Pralana's Metric MRI.

Additional forms considered:
- **Single gauge or dial with a percentage**: skip. It is the field's worst habit: no magnitude, no interval, false precision.
- **CDF of retirement age**: keep as the companion to the dot plot for users who want to read a threshold ("by 58 in 80% of lives"). Fernandes et al. found CDFs perform as well as dot plots for decisions.
- **Spending fan** (TPAW style): keep whenever a flexible spending rule is active; it is then the primary chart, with the portfolio fan secondary.
- **Failure attribution bar**: keep. Among lives that ran short, which factor was present (early bear market, job loss, long-term care, long life, high inflation), from the engine's diagnostics. Present as overlapping tags with counts, not as an additive decomposition.
- **Model triangulation strip**: keep. Three dots with intervals for the headline metric under the three return generators.
- **3D surfaces, radar charts, pie charts of allocation over time**: skip.

### 3.2 Prioritised list (14)

| # | Visualisation | Question it answers |
|---|---|---|
| 1 | Quantile dot plot of the age work becomes optional (with CDF toggle) | When can I stop working? How sure is that? |
| 2 | Headline outcome strip: count-framed sentence, interval, and three-generator triangulation dots | How sure is that, and how much does the model matter? |
| 3 | Life-stage stacked chart over age (short / cut / on plan / surplus / dead), from sampled lifetimes | What could go wrong, and when? |
| 4 | Real-dollar band chart of net worth among the living, with sampled paths and three highlighted lives | What does the range look like? |
| 5 | Spending band chart (planned vs delivered spending, cuts visible) | What will I actually be able to spend? |
| 6 | Annotated life story for median / unlucky / failing lives, with shuffle | What does a typical or unlucky life look like year by year? |
| 7 | Year ledger drill-down with single-year Sankey | Where does the money go, and can I trust the number? |
| 8 | Heatmap: retirement age x spending, CRN, contour lines | When can I stop, at what lifestyle? |
| 9 | Two-panel tornado (controllable / uncontrollable) with intervals | What does it depend on most? |
| 10 | Paired-difference view: rescued / harmed / unchanged counts and dot plot of dollar differences | What should I change, and does it really help? |
| 11 | Failure attribution tags among short lives, with depth and duration of shortfall | What goes wrong in the bad cases, and how bad is it? |
| 12 | Lifetime tax-bracket-fill chart with ACA, IRMAA and gains thresholds | Where is tax being wasted; when to convert? |
| 13 | Account-type composition over age (taxable, traditional, Roth, HSA, cash, home equity) on the selected path with median band | Is the withdrawal order and conversion ladder doing what I think? |
| 14 | Small multiples of chart 4 or 5 for up to four saved scenarios | How do my options compare overall? |

Cross-cutting rules: today's dollars everywhere, stated once and never mixed; frequency language in captions; every probability carries its sampling interval; no autoplaying motion; colour-blind-safe palettes with direct labels, never colour alone for good/bad; every aggregate chart has a route to a single life and every single-life chart states which life it is and why it was picked; the same path identifier means the same world in every chart and every scenario.

---

## Part 4. AI

### 4.1 How planners use LLMs today

- **Boldin AI** (beta announced 25 Feb 2026; https://www.boldin.com/retirement/boldins-ai-planner-assistant-financial-planning-accessible-affordable-understandable/): "the model calculates; the AI explains". Questions route to the planning engine first, then the LLM explains. What-ifs in chat. Since August 2026 PlannerPlus users can have it propose field-level edits from chat or from uploaded Document Vault files, shown as current vs new values and applied only after "Review and Apply". Several models screen responses; CFP reviewers audit explanations; transcripts kept 90 days; LLM vendor not named. Self-reported 15,000 users and 190,000 questions in beta.
- **Income Lab** (advisor-facing): AI Scribe turns client-meeting conversations into plan inputs (Jan 2026, https://incomelaboratory.com/income-lab-launches-ai-scribe-to-turn-client-conversations-into-retirement-plans/); "Penny", an AI paraplanner pairing natural language with deterministic engines (Apr 2026, https://incomelaboratory.com/penny-ai-paraplanner-launch/).
- **RightCapital Iris** (June 2026, https://www.investmentnews.com/fintech/rightcapital-claims-industry-first-with-ai-agent-for-financial-planning/267144): an agent that runs retirement simulations inside the advisor workflow.
- **ProjectionLab**: no in-app assistant according to Boldin's comparison page (https://www.boldin.com/retirement/boldin-vs-projectionlab/) [3p, competitor; unverified].
- **Pralana, MaxiFi, TPAW, cFIREsim, FIRECalc, FI Calc, Portfolio Visualizer**: no LLM features found.
- **FIRESight v1** (`assistant.py`): Claude with two tools, `run_simulation` and `scenario_analysis` (same-seed comparison), over a profile passed in the system prompt.

Pattern across all of them: engine computes, LLM explains, human confirms edits. What none advertises: stated confidence on extracted inputs, paired experiments with statistical intervals, assumption audits, or model-initiated search for the highest-leverage change.

### 4.2 Proposal for Claude in v2

Design principles:
1. **The engine is the only source of numbers.** Claude never does arithmetic on money and never recalls tax parameters from memory.
2. **Claude proposes, the user disposes.** Every profile change is a schema-valid patch shown as a diff and applied on confirmation.
3. **Every comparison is paired.** Scenario requests run on common random numbers and report the difference with its interval, or say the difference is not distinguishable from zero.
4. **The profile stays in the browser.** Tool calls execute client-side against the WASM engine; a thin Vercel function proxies the model API and stores nothing.
5. **No advice theatre.** Claude describes what the model shows and what it assumes. It does not recommend securities or claim certainty the model lacks.

Capabilities:

**A. Conversational intake.** The user describes their life in plain language; Claude emits JSON Patch operations (RFC 6902) against the profile schema. Each operation carries `source` (user_stated, document, inferred, default), `confidence` (high, medium, low), and for anything not stated by the user an `assumption` sentence ("assumed both earners retire the same year"). Claude asks only questions whose answer would move the result, decided by calling the sensitivity tool on the draft profile rather than by walking a fixed questionnaire. Low-confidence and defaulted fields remain visibly flagged in the UI until confirmed.

**B. Document extraction.** Pay stubs, W-2s, brokerage and 401(k) statements, Social Security statements (earnings history for the benefit calculation), mortgage statements, last year's Form 1040. Claude reads the image or PDF directly and emits the same patch format with `source: document` and an `evidence` field (page and quoted text) per value, so the UI can show the snippet beside the field. Rules: never extract or store account numbers or SSNs; annualise only with the stated method ("biweekly gross $4,210 x 26"); when year-to-date and per-period figures disagree, report both and ask. Cost basis and contribution type (traditional vs Roth) are the fields that most change tax results and most often get skipped, so target them.

**C. Plain-language scenarios.** "What if I go part-time at 50 and we move to Colorado?" becomes one or more overlay patches, run through `compare` on the same paths as the baseline. Claude restates the interpretation before or alongside the result ("part-time modelled as 50% of salary from age 50 to 55, no employer match, ACA coverage"), since the interpretation is where errors hide.

**D. Grounded explanation.** Claude explains results from structured diagnostics the engine returns: headline metrics with intervals, failure attribution counts, percentile-ranked path identifiers, per-year ledgers. Enforcement, strongest first:
   - Numbers in the reply are written as references to tool-result fields (for example `{{run_7.p_short}}`), and the client substitutes and formats the value. A reply containing a monetary or percentage literal not backed by a reference is rejected and regenerated.
   - Charts are produced by reference, not by data: Claude calls `show` with a run id and chart kind; it cannot supply its own series.
   - Tax and benefit parameters come from `lookup`, which returns the value, the year, and the citation the engine was built with.

**E. Assumption audit.** On request, and once automatically after intake, Claude reviews the profile against engine-computed flags and its own reading: spending that does not grow with a planned child; no healthcare line before Medicare; retirement before 59.5 with all assets in traditional accounts and no bridge; a Social Security estimate inconsistent with the earnings record; return assumptions overridden to optimistic values; home equity counted as spendable with no sale planned; one earner's job-loss risk switched off. Output is a ranked list: issue, why it matters, the measured effect on the headline metric if corrected (from a paired run), and a proposed patch.

**F. "What would move the needle" search.** Claude calls `sensitivity` to rank levers, then `solve` for the interesting ones (earliest retirement age meeting the target, maximum sustainable spending, the Roth conversion bracket ceiling that maximises after-tax bequest, the claim age pair that maximises certainty-equivalent spending), then `compare` to confirm the top three as paired differences. It reports them as trade-offs in the user's units ("working 14 more months does about as much as cutting $6,000 a year"), including levers that turned out not to matter. It runs when the user asks and is offered, not pushed, after a large profile change.

Guardrails: system prompt, schema and tool definitions cached; tool outputs capped in size (summaries and identifiers, never 100k paths); a hard cap on engine calls per turn with the budget visible to the model; an "explain what you ran" trace available to the user for every answer; refusal to produce a number when the engine errors; an evaluation set of profiles with known engine answers, checked for number fidelity and patch validity on every prompt change.

### 4.3 Minimal tool set

Eight tools. All take and return JSON; all run in the browser against the WASM engine except where noted.

| Tool | Input | Returns |
|---|---|---|
| `get_profile` | optional JSON pointer | Profile subtree, schema version, and per-field provenance (source, confidence, last confirmed), plus the list of fields still on defaults |
| `propose_patch` | JSON Patch operations, each with rationale, source, confidence, optional evidence; optional scenario name | Validation result (schema errors, cross-field warnings from the engine), a human-readable diff, and a `patch_id`. Applies to the saved profile only after user confirmation; usable immediately as a scenario overlay |
| `run` | profile reference plus optional `patch_id` overlays, path count, seed, generator (structural, history reshuffled, history replayed) | `run_id`; headline metrics each with standard error; retirement-age distribution quantiles; shortfall depth and duration; spending-cut frequency; funded ratio; failure attribution counts; identifiers of representative paths (median, 10th percentile, a failing life) |
| `compare` | baseline reference and up to four variants (patch ids), same seed and paths enforced | Per variant: paired difference in each headline metric with interval, counts of lives rescued / harmed / unchanged, quantiles of per-life dollar differences, and a flag when the difference is within noise |
| `solve` | objective or constraint (for example earliest retirement age with shortfall probability at most X; maximum spending; conversion ceiling maximising a metric), decision variable and bounds | Solution value with interval, the bracketing evaluations, binding constraint, convergence status, and the `run_id` at the solution |
| `sensitivity` | list of parameters with ranges, or a preset (controllable, uncontrollable, intake) | Ranked table: parameter, range tested, paired change in headline metric with interval, direction; plus pairwise interaction flag for the top few |
| `inspect` | `run_id` and a query: path ledger (path id, year range), percentile slice, tax detail for a path-year, attribution detail, assumption flags | Structured slices: per-year ledger rows (income by source, tax by type, withdrawals by account, balances, events), or aggregate tables; never raw path arrays |
| `lookup` | key (for example standard deduction, bracket thresholds, IRMAA tiers, ACA rules, contribution limits, bend points) and tax year | The value the engine uses, its legal year, whether it is published or projected, and the citation |

A ninth, UI-only tool is worth having: `show` (run id or compare id, chart kind, optional path id), which renders one of the Part 3 charts in the conversation and returns nothing but an acknowledgement. Document reading needs no tool: it is model input, and its output goes through `propose_patch`.

What is deliberately absent: a general calculator, a code-execution tool, web search, any tool that writes the profile without confirmation, and any tool returning unbounded data.

---

## Sources

Tools
- ProjectionLab: https://projectionlab.com , https://projectionlab.com/monte-carlo , https://projectionlab.com/help/increase-trials-in-chance-of-success , https://projectionlab.com/help/roth-conversion , https://projectionlab.com/changelog , HN launch thread https://news.ycombinator.com/item?id=36849502 , review https://www.financialsamurai.com/projectionlab-review/
- Boldin: https://www.boldin.com/retirement/understanding-boldins-monte-carlo-simulation-what-it-is-why-it-matters-and-whats-new/ , https://help.newretirement.com/en/articles/5805671-monte-carlo-simulation , https://www.boldin.com/retirement/boldin-vs-projectionlab/ , https://www.boldin.com/retirement/boldins-ai-planner-assistant-financial-planning-accessible-affordable-understandable/
- cFIREsim: https://www.cfiresim.com ; FIRECalc: https://firecalc.com ; FI Calc: https://ficalc.app , https://ficalc.app/about
- Pralana: https://pralanaretirementcalculator.com , https://boglecenter.net/bogleheads-chapter-series-pralana-retirement-calculator/ , https://www.early-retirement.org/threads/pralana-gold-or-pralana-online.123819/
- MaxiFi: https://maxifi.com/how-it-works , https://support.maxifi.com/support/solutions/articles/35000161688-setting-up-monte-carlo , https://thefinancebuff.com/maxifi-review-financial-planning.html , https://robberger.com/tools/maxifi/
- Fidelity: https://www.fidelity.com/go/guidance/retirement-methodology , https://www.fidelity.com/planning/retirement/pdf/rqc_methodology.pdf ; Empower: https://www.empower.com/the-currency/money/how-much-do-i-need-to-retire
- Portfolio Visualizer: https://www.portfoliovisualizer.com/monte-carlo-simulation
- Engaging Data: https://engaging-data.com/will-money-last-retire-early/
- TPAW: https://tpawplanner.com , https://www.bogleheads.org/forum/viewtopic.php?t=331368 , https://moneyforums.citywire.com/yaf_postst18317p5_TPAW-Planner---an-interesting-online-drawdown-simulator.aspx , https://github.com/chriskelly/LifeFinances/pull/232
- Honest Math: https://kevinbracker.substack.com/p/honest-math-a-portfolio-simulation
- Early Retirement Now: https://earlyretirementnow.com/2018/08/29/google-sheet-updates-swr-series-part-28/ , https://earlyretirementnow.com/2017/03/15/the-ultimate-guide-to-safe-withdrawal-rates-part-11-criteria/

Modelling
- Kitces/Tharp, fat tails vs historical: https://www.kitces.com/blog/monte-carlo-analysis-risk-fat-tails-vs-safe-withdrawal-rates-rolling-historical-returns/
- Kitces/Income Lab, forecast error and Brier scores: https://www.kitces.com/blog/monte-carlo-models-simulation-forecast-error-brier-score-retirement-planning/
- Kitces, probability of success critiques: https://www.kitces.com/blog/monte-carlo-one-time-vs-ongoing-retirement-planning-probability-of-success-guardrails/ , https://www.kitces.com/blog/renaming-the-outcomes-of-a-monte-carlo-retirement-projection/ , https://www.kitces.com/blog/multidimensional-abstraction-to-communicate-monte-carlo-simulation-probability-and-magnitude-of-success/ , https://www.kitces.com/blog/calming-client-fears-communicating-monte-carlo-outcomes-probability-of-failure-adjustment-based-planning/ , https://www.kitces.com/blog/monte-carlo-retirement-projection-probability-success-adjustment-minimum-odds/
- Guardrails: https://www.kitces.com/blog/guyton-klinger-guardrails-retirement-income-rules-risk-based/ , https://incomelaboratory.com/risk-based-vs-guyton-klinger-guardrails/ , https://incomelaboratory.com/why-guyton-klinger-guardrails-are-too-risky-for-most-retirees/
- Pfau and Young: https://www.advisorperspectives.com/articles/2023/01/10/the-dangers-of-monte-carlo-simulations ; Retirement Researcher: https://retirementresearcher.com/two-philosophies-for-retirement-income-planning-part-one-probability-based/
- Cont 2001: https://doi.org/10.1080/713665670 ; Hardy 2001: https://doi.org/10.1080/10920277.2001.10595984
- Poterba and Summers 1988: https://doi.org/10.1016/0304-405X(88)90021-9 ; Campbell and Viceira 2005: https://doi.org/10.2469/faj.v61.n1.2682 ; Pastor and Stambaugh 2012: https://doi.org/10.1111/j.1540-6261.2012.01722.x
- Anarkulova, Cederburg, O'Doherty: https://doi.org/10.1016/j.jfineco.2021.06.040 , https://papers.ssrn.com/abstract=4227132 , https://papers.ssrn.com/abstract=4590406
- Politis and Romano 1994: https://doi.org/10.1080/01621459.1994.10476870 ; Patton, Politis, White 2009: https://doi.org/10.1080/07474930802459016 ; Cogneau and Zakamouline 2013: https://doi.org/10.1080/14697688.2012.713115 ; Forsyth: https://arxiv.org/abs/2101.02760
- Campbell and Shiller 1998: https://doi.org/10.3905/jpm.24.2.11
- Milevsky and Huang 2011: https://doi.org/10.2469/faj.v67.n2.2 ; amortisation-based withdrawal: https://www.whitecoatinvestor.com/amortization-based-withdrawal-vs-safe-withdrawal-rates/
- Caflisch 1998: https://doi.org/10.1017/S0962492900002804 ; Sobol direction numbers: https://web.maths.unsw.edu.au/~fkuo/sobol/

Visualisation
- Kay et al. 2016: https://doi.org/10.1145/2858036.2858558 ; Fernandes et al. 2018: https://doi.org/10.1145/3173574.3173718 ; Hullman et al. 2015: https://doi.org/10.1371/journal.pone.0142444 ; Kale et al. 2020: https://arxiv.org/abs/2007.14516 ; Padilla, Kay, Hullman: https://doi.org/10.1002/9781118445112.stat08296 ; Wilke: https://clauswilke.com/dataviz/visualizing-uncertainty.html

AI
- Boldin AI: https://www.boldin.com/retirement/boldins-ai-planner-assistant-financial-planning-accessible-affordable-understandable/ , https://www.newsfilecorp.com/release/285258/Boldin-Launches-AI-Planner-Assistant-Signaling-New-Era-of-TechnologyEnabled-DIY-Financial-Planning
- Income Lab: https://incomelaboratory.com/income-lab-launches-ai-scribe-to-turn-client-conversations-into-retirement-plans/ , https://incomelaboratory.com/penny-ai-paraplanner-launch/
- RightCapital Iris: https://www.investmentnews.com/fintech/rightcapital-claims-industry-first-with-ai-agent-for-financial-planning/267144
