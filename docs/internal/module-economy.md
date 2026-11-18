# economy module: integration report

ECONOMY MODULE: all four MarketModel kinds, derived assets, adjustments and stress overlays are implemented. Gates pass: cargo fmt, cargo clippy -p firesight-core --all-targets -- -D warnings, cargo test -p firesight-core (123 passed, 3 ignored reports, about 2 s in debug); cargo check --workspace is clean. STUB allow removed.

FILES (economy.rs became a directory; nothing outside it was touched, lib.rs's module table still calls economy a stub):
- crates/firesight-core/src/economy/mod.rs: public types, EconomyModel, generate, derived assets, adjustments
- economy/regime.rs: regime and lognormal calibration and step
- economy/stress.rs: ResolvedStress and apply_stress
- economy/sampler.rs: one-draw quantile-table samplers
- economy/fit.rs: OLS, Cholesky, ARMA(1,1) by moments, Baum-Welch HMM
- economy/tests.rs

PUBLIC INTERFACE
EconomyPath layout is unchanged from the stub (months, inflation, price_level[months+1], returns[N_ASSETS], tbill, long_yield, mortgage_rate, home_national, recession). Semantics now fixed:
- tbill[t] is the bill yield annualised with monthly compounding: math::annual_to_monthly(tbill[t]) == returns[Cash][t]. For cash accounts, use returns[Cash][t] directly and convert only the spread.
- long_yield[t] and mortgage_rate[t] are end-of-month levels.
- price_level[t] is the level at the START of month t. generate fills it; do not call rebuild_price_level after generate.
- returns are >= -1.0; exactly -1.0 marks a crypto collapse or single-stock ruin month.
Unchanged signatures: EconomyStreams, EconomyModel::new(&MarketModel, &Data), with_assets(self, &PortfolioPolicy), cohort_count(), generate(&self, &mut EconomyStreams, path_index: u64, months: usize, &mut EconomyPath), apply_stress(&self, &ResolvedStress, &mut EconomyPath).
Additions:
- EconomyPath::income_yields_at(t) -> [f64; N_ASSETS]
- pub const DIVIDEND_YIELD, QUALIFIED_FRACTION: [f64; N_ASSETS]
- EconomyModel::stress_betas() -> [f64; N_ASSETS]
- pub const REIT_EQUITY_BETA, REIT_HOME_BETA, REIT_IDIOSYNCRATIC_VOL
- economy::stress is a private module; only ResolvedStress is re-exported (path economy::ResolvedStress as before). CRASH_MONTHS (6) and LOST_DECADE_MONTHS (120) are pub inside it but not reachable from outside the economy module; re-export them from economy/mod.rs if sim needs them.
SEMANTIC CHANGES to ResolvedStress (variants and fields identical):
- Crash unfolds over 6 months, not the stub's 12 (the task said "a few months").
- Stagflation depresses real returns to 1973-81 levels instead of the stub's "flat in real terms".

CALL SEQUENCE
1. Once per Plan: let econ = EconomyModel::new(&profile.assumptions.market, data).with_assets(&profile.portfolio).
   - new() already applies SingleStock::default() and CustomAsset::default().
   - with_assets only rescales those two columns; draws and every other series are bit-identical.
   - Cost: new 80-190 us, with_assets 12-23 us. The regime calibration is 1.2 ms the first time and then cached per process for Data::embedded().
2. Per path: build Rng::stream(seed, path, Stream::Market) and Stream::Inflation, then econ.generate(&mut EconomyStreams { market, inflation }, path_index, months, &mut ws.economy).
   - No allocation once capacity suffices (tested by buffer pointers).
   - generate does not zero the buffers; it overwrites all `months` elements and sets the lengths.
3. Stress: econ.apply_stress(&ResolvedStress, &mut path), any time, in order. Deterministic, no draws, never touches months before the start, truncated at the horizon; no-op for a start past the end or non-finite parameters. Resolving When to month indices is sim's job.
4. Historical: path i replays from row (i mod cohort_count()), wrapping to row 0. cohort_count() is 1203 (0 for other kinds).

DRAWS (fixed per month, independent of the portfolio; tested by stream position)
- Market stream: bootstrap 8, historical 7, regime 17, lognormal 12 plus 1 at path start.
- Inflation stream: 1 per month in regime and lognormal (the CPI innovation), 0 otherwise.
- All 9 asset classes are always generated.

YIELDS AND DIVIDENDS FOR TAX
path.income_yields_at(t) gives the annualised cash distribution yield per asset class as a fraction of market value. A holding worth v distributes about v * y / 12 that month; this is part of the total return, not an addition.
- Bonds: long_yield[t] (prevailing par coupon). Cash: tbill[t].
- DIVIDEND_YIELD: US 1.5%, Intl 3.0%, RealEstate 4.0%, SingleStock 1.5%, Gold/Crypto/Custom 0.
- QUALIFIED_FRACTION: US 0.95, Intl 0.75, SingleStock 1.0, everything else 0 (REIT distributions and interest are ordinary).
These dividend numbers are round mid-2020s figures from memory, not sourced from data/. They should be verified or moved into data/.

MODELS
Bootstrap:
- Stationary bootstrap over row indices. One u64 per month: the high 32 bits decide a restart with probability 1/mean_block (0 is treated as 1), the low 32 bits pick the new row. Otherwise the next row, with circular wrap.
- Every series of a month comes from the same row. Levels jump at block seams.
- from_year selects rows from January of that year, clamped so at least 120 rows remain. Any i32 is safe.
- The one blank home value (2026-09) is forward-filled.
- With no adjustments, values are bit-identical to the CSV.
Short columns (intl before 1975, gold before 1968-04):
- OLS on [1, equity_us, bond10, cpi] over each column's own sample.
- Intl: US-equity beta 0.717, residual sd 3.53% a month. Gold: CPI beta 1.85, residual sd 4.83% a month.
- Missing months get the fit plus residual sd times a unit-variance Student-t(6). Real months use the data exactly. The two residual draws are taken every month.
Regime (calibrated from the history at construction):
- 2-state Gaussian HMM by Baum-Welch on ln(1+eq) - ln(1+tbill), started from the README threshold classification. It reproduces calibration.json: mu 0.010773 / -0.020470, sigma 0.035989 / 0.097091, P(calm to turbulent) 0.02505, P(turbulent to calm) 0.12367, loglik 2009.5348, 1024 calm months.
- Per regime: means, sds and pairwise-complete correlations (Cholesky, shrunk to PSD if needed) of six series: US and intl log excess returns, gold log real return, and the yield, home and CPI surprises.
- Innovations are multivariate Student-t with 8 dof (4 uniforms and one ln for the chi-square), mixing scale capped at 8. I chose 8 because 6 gave excess kurtosis near 15 against 7.4 in the data; 8 gives about 9-10.
- Inflation: cpi = m + v, m' = mu + phi (m - mu) + K v. This is an AR(1) expected-inflation state in innovations form (ARMA(1,1)), fitted to the lag-1 and lag-12 autocorrelations: phi 0.958, K 0.215.
- Long yield: dy = b (y - 4.78%) + c * 12 (m - mu) + shock, by OLS with clamps: b = -0.0069 a month, c = 0.0053, y kept in [0.25%, 25%].
- Bond return = y/12 - D(y) dy + 0.5 C(y) dy^2 for the 10-year par bond (tables). It matches the bond10 column within 0.12% in every month since 1962.
- Bills = max(0, y - spread), spread AR(1). Mortgage = y + 1.76 points.
- Home = m + real mean + momentum + surprise; momentum is ARMA(1,1) fitted from 1953 (phi 0.971, K 0.296).
- CPI and home surprise variances are rescaled so the unconditional monthly variance equals the sample's.
- Recession: P(rec | previous rec, regime) = [[0.011, 0.092], [0.872, 0.958]].
- Each path starts from the latest state in the data: calm, y 5.29%, expected inflation 3.95%, spread 1.57%.
Lognormal:
- i.i.d. joint normal on ln(1+r) of [eq, intl, bond, tbill, home, gold, cpi] with full-sample moments (pairwise-complete).
- Long yield (4.78%) and mortgage rate are constant.
- Recession is an independent 2-state chain with NBER transition frequencies.

DERIVED ASSETS (noise on the Market stream)
- RealEstate = bill + 0.70 (eq - bill) + 1.0 (home - cpi - mean) + 14% annual Student-t(6) noise. No alpha. Simulated vol 19%, equity correlation 0.67, mean about 9.1%. The betas are from memory and judgement.
- Crypto, per params.json: gross = (1+cpi) * exp(mu/12) * F(eq) * exp(sigma_i t4).
  - mu is the neutral preset (0 real median).
  - Equity link: beta 1.34 (correlation 0.35), plus 0.96 extra beta for the part of an equity month more than 1 sd below its mean (correlation 0.6).
  - Total log vol is 70%. The t4 shock is capped at 8 sd.
  - Collapse hazard 1% a year (bitcoin_or_ether), applied monthly.
  - Simulated: log vol 0.70, equity correlation 0.39, arithmetic mean about +26% a year with median real growth 0; 26% of 30-year paths collapse.
- SingleStock = (1 + bill + beta (eq - bill)) * L - 1, with L lognormal, mean exactly 1, idiosyncratic vol = sqrt(vol^2 - beta^2 * 18.3%^2).
  - Annual ruin hazard is interpolated on idiosyncratic vol through the params.json presets (25% gives 0.3%, 35% gives 1%, 50% gives 3%; linear to 0 at 0, flat above).
  - Survivors are boosted by 1/(1-h), so the mean is exactly market-implied (tested).
  - The configured volatility is matched in non-ruin months; ruin adds on top.
- Custom: monthly mean from annual_to_monthly(mean); beta = corr * vol / equity vol on the equity deviation; normal residual.
- Collapse and ruin semantics: the month returns exactly -1.0 and the process then continues, so money rebalanced in later buys a successor. Sim can detect == -1.0 for event logs.

ADJUSTMENTS
- equity_premium_adjustment: US and intl gross returns times (1+adj)^(1/12). RealEstate and SingleStock inherit it through beta. Crypto and Custom do not.
- inflation_adjustment: (1+cpi), every asset's gross return and home prices times (1+adj)^(1/12); long_yield and mortgage_rate plus adj; tbill follows cash. All real returns and rates are preserved, including Custom's.

STRESS
- Crash { at_month, drawdown, recovery_months }:
  - US equity returns for 6 months are replaced by (1-d)^(1/6) - 1, so the fall from the at_month level is exactly d (d clamped to [0, 0.99]).
  - The next recovery_months get sampled return times a constant drift, sized so the index rejoins the unstressed path exactly.
  - Other assets: r += beta * (change in the US return), floored at -1. Betas: intl 0.717, RealEstate 0.70, gold -0.03, crypto 2.29, SingleStock its beta, Custom its beta, bonds and cash 0.
  - Recession is set to 1 in the 6 crash months. This is my addition; drop the line in stress.rs if you do not want it.
- LostDecade { from_month }: US and intl gross returns are each scaled by one constant so the 120-month real return is exactly 0 and monthly vol is preserved. Derived assets move by beta.
- Stagflation { from_month, months, inflation }:
  - (1+cpi) is scaled so geometric inflation over the window equals the target.
  - Real geometric returns over the window are set to the 1973-81 values computed from the history: equities and intl -2.83% a year, bonds -5.24%, bills -0.90%.
  - Home and gold keep their real returns; derived assets move by beta.
  - long_yield and mortgage_rate shift by (target - the path's own window inflation). price_level is rebuilt from from_month.

30-YEAR REAL US EQUITY CAGR (5,000 paths; historical = all 1203 cohorts with wrap)
| model | p1 | p5 | p25 | p50 | p75 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| bootstrap 60 | -0.53% | 1.82% | 5.03% | 7.26% | 9.46% | 12.41% | 14.48% |
| bootstrap 12 | -1.73% | 0.97% | 4.79% | 7.41% | 9.89% | 13.34% | 15.58% |
| bootstrap 120 | 0.64% | 2.52% | 5.32% | 7.31% | 9.13% | 11.88% | 13.55% |
| bootstrap 1 | -0.92% | 1.46% | 4.79% | 7.14% | 9.58% | 13.23% | 15.73% |
| regime | -3.51% | -0.06% | 4.88% | 7.91% | 10.89% | 14.98% | 18.28% |
| lognormal | -0.97% | 1.38% | 4.79% | 7.15% | 9.59% | 13.26% | 15.84% |
| historical | 4.66% | 4.96% | 6.21% | 7.23% | 8.04% | 9.34% | 10.20% |
| actual windows, no wrap | 4.59% | 4.79% | 5.99% | 7.21% | 8.16% | 9.58% | 10.36% |
Reading: longer blocks narrow the tails. The regime model is the widest and its median is about 0.7 points higher: it has no long-horizon mean reversion, calendar-year equity sd is 21.4% against 19.8% in history, and equities are bills plus a premium while simulated real bills run 0.6% against 0.3% in history, largely from the zero floor on bills and the 5.3% starting yield.

SIMULATED MOMENTS (1,000 paths x 720 months; history in brackets)
- Bootstrap 60: equity 11.6% mean / 18.3% vol (11.57 / 18.3); bond 4.70 / 6.5 (4.71 / 6.5); bills 3.26; CPI 2.97 / 1.81. Annual real-bond vs inflation correlation -0.515 (-0.50); home vs CPI annual 0.55 (0.56); CPI lag-1 autocorrelation 0.46 (0.48); recession share 0.168 (0.167); equity excess kurtosis 7.7 (7.4).
- Regime: equity 12.0 / 18.2; bond 5.2 / 7.2; bills 3.6; CPI 2.98 / 1.81, lag-1 0.455, lag-12 0.275. Home annual sd 7.3% (5.6); CPI annual sd 4.4% (3.9); real-bond vs inflation -0.61; recession share 0.176; kurtosis about 9.
- Lognormal: means and vols match; no autocorrelation; home and CPI annual sd 2.1% / 1.85%; real-bond vs inflation -0.34.
Test tolerances are stated in tests.rs against data/history/calibration.json.

SPEED (release, M-series, 720-month paths, all asset classes)
- bootstrap 11.8 ns/month (8.5 us per 60-year path), against the 60 ns target
- historical 10.0 ns/month
- lognormal 32 ns/month
- regime 44 ns/month
- apply_stress: crash 116 ns, lost decade 263 ns, stagflation 881 ns per call
The speed comes from sampler.rs: one u64 per variate by inverse transform from a 2-level, 1024-cell quantile table (normal, Student-t 4 and 6, and per-model lognormal and crypto shock tables), truncated at about 2.4e-7 tail probability per side and rescaled to exact unit variance or mean. rng.normal and rng.student_t are not used. sampler.rs calls libm::erfc directly at table-build time; it is deterministic, but math.rs has no erf wrapper.

REPORTS
cargo test --release -p firesight-core --lib economy::tests::report -- --ignored --nocapture --test-threads=1 prints the CAGR table, the moments and the speed. A golden-value test freezes seed 2026, path 3 for all four models.

KNOWN SIMPLIFICATIONS
- Bootstrap and historical paths start at a random or cohort state, not today's.
- The regime model has a constant mortgage spread and no equity mean reversion.
- The lognormal model has constant yields.
- Stress betas are linear in simple returns.
- No unemployment series is output; recession is the only labour signal.

## Shared type requests

- lib.rs: the module table still lists economy as an interface stub; move it to the implemented row.
- model::CustomAsset: add an `income_yield: f64` field (annual distributions as a fraction of value) and optionally `qualified_fraction`. The economy currently assumes the custom asset distributes nothing (DIVIDEND_YIELD[Custom] = 0), which is wrong for something like private credit.
- model::SingleStock has only volatility and beta. The ruin hazard is interpolated from idiosyncratic volatility through the params.json presets; add an optional `ruin_hazard: Option<f64>` (and a dividend yield) if users should set it directly.
- model (PortfolioPolicy or MarketModel): there is no crypto knob. params.json defines outlook presets (skeptic -10%, neutral 0, believer +5% median real growth) and asset type (bitcoin_or_ether 1%/yr vs other_tokens 8%/yr collapse hazard); the economy hard-wires neutral and bitcoin_or_ether. Add e.g. `crypto: CryptoAssumption { outlook, kind }`, to be passed through with_assets.
- data/: the dividend yields and qualified fractions (economy::DIVIDEND_YIELD, QUALIFIED_FRACTION) and the REIT betas (REIT_EQUITY_BETA 0.70, REIT_HOME_BETA 1.0, REIT_IDIOSYNCRATIC_VOL 0.14) are named constants quoted from memory, not sourced. They should move into a sourced data file such as data/actuarial/params.json and be verified.
- math.rs: add an `erfc` wrapper (libm::erfc). economy/sampler.rs calls libm::erfc directly at table-construction time because math has none.
- model::Stress::CrashAtRetirement doc says stocks crash 'in the year' of retirement, and the old ResolvedStress doc said 12 months. The overlay now falls over 6 months (economy::stress::CRASH_MONTHS, not currently re-exported from economy). Align the wording or tell me to change the constant.
- career / EconomyPath: the history has an unemployment column the economy does not output; only the recession flag is exposed. If career wants an unemployment-rate path, or the 'employer stock idiosyncratic return below -40%' signal from params.json, EconomyPath needs extra columns (unemployment: Vec<f64>, single_stock_idio: Vec<f64>).
