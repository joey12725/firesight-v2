# How FIRESight models a financial life

This document explains what the engine does, relationship by relationship, with the
numbers it was checked against. It is written for a reader who wants to trust the
model or to challenge it. Where a choice is a judgement, it says so. Where the model
is known to be wrong or incomplete, section 17 lists it.

Conventions used throughout: money is in today's dollars (dollars of the plan's
start month) unless a line says nominal; rates are per year; "real" means after
inflation; "geometric" return is the compound rate, "arithmetic" the average of
single-period returns. Measurements quoted with a path count come from the engine
itself (20,000 paths of 720 months per market model, or the stated number), most of
them made by seven independent reviewers who audited the engine in November 2026;
figures marked "after the audit" were re-measured on the corrected code.

## 1. Time

The simulation steps one month at a time from `profile.start` until the last adult
dies or the youngest adult reaches `simulation.end_age` (default 100). Each month, in
order: jobs and other income are paid and payroll deductions taken; the calendar
year turns (in January) and the year is planned; life events due that month fire;
spending is paid; investment returns are applied; the month is settled (cash
shortfalls funded, surpluses saved); the month is recorded.

Things that really happen once a year happen once a year: the tax return is closed
and settled the following April, contribution limits reset, Social Security records
are updated, the retirement spending rule decides the year's spending, required
minimum distributions are sized, the portfolio is rebalanced (if annual), and the
withdrawal plan for the year is made. The first calendar year is usually partial;
its tax return is computed on the simulated months scaled to a full year and the
tax scaled back.

Flows are deflated by the price level at the start of their month and balances by
the level at its end.

## 2. The economy

Every path gets its own monthly history of: US stock returns, international stock
returns, 10-year Treasury bond returns, Treasury bill returns, inflation (CPI-U),
national home prices, gold, the 10-year yield, the 30-year mortgage rate and a
recession flag. Four generators are available. All four are calibrated to the same
dataset: 1,203 months from July 1926 to September 2026 (section 15).

### 2.1 Block bootstrap (the default)

Whole months of history are resampled in blocks: with probability 1/60 each month
the path jumps to a uniformly random month of the record, otherwise it continues to
the next month (wrapping from the last row to the first). Blocks therefore have a
geometric length with mean 60 months. Because a row is one real month, everything
that happened together stays together: crashes come with their recessions, the
1970s come with their inflation and their bond losses.

Verified: every one of 14.4 million sampled months matched a history row bit for
bit; the restart probability was 0.01661 against 0.01667; start rows and row usage
are uniform; per-decade real returns are flat across decades 1 to 6 (no drift).

Two things are deliberately not copied from the sampled row:

* **Interest-rate levels.** A block that starts in 1981 brings 1981's returns but
  not a 17% mortgage rate overnight. The 10-year yield and the mortgage rate start
  from the last observed values (5.29% and 6.86% in September 2026) and move by each
  sampled month's own change, with a pull of 1/120 a month toward their long-run
  means, a floor of 0.5% on the yield and the mortgage spread kept between 0.5 and 4
  points. Before the audit the levels were copied: every path began at a random
  historical rate (5% of them above 12.8%) and 52% of paths offered a mortgage rate
  a full point lower two months into the plan. After: every path starts at 6.86%;
  0.6% of paths see a one-point drop by the first January; one-month moves above a
  point are 0.45% of months (history 0.42%); thirty years out the mortgage rate is
  4.0% / 6.7% / 10.0% at the 5th / 50th / 95th percentile (narrower than the range
  of the record, 3.7% to 12.9%, because of the pull to the mean). Bond *returns* still come from the
  sampled rows, so a path's bond returns and its yield level are not strictly
  consistent with each other.
* **The long-run level of international stocks and gold** (2.5).

#### Bond returns from the yield path (`market.bond_pricing: from_yield`, opt-in)

With the default `sampled`, a month's bond return is the resampled row's, while the
yield level follows today's rate (above): the two need not agree. With
`from_yield` each month's bond return is priced from the path's own 10-year yield,
exactly as `data/history` builds the `bond10` column from Treasury yields and as
the regime model prices its bond: a month of interest at the yield the month began
with, minus the modified duration of a 10-year semi-annual par bond times the
yield change, plus half its convexity times the change squared. The yield's monthly
changes are still the sampled rows' own, so bonds keep history's co-movement with
stocks and inflation; only the carry now comes from the path's level (today's 5.29%
in the first month). No random numbers are drawn and every other series of the path
is unchanged bit for bit.

Measured (1,500 paths of 120 months, the record as it was): the correlation across
paths between a decade's bond return and what its own yield implies (average
carry less duration times the yield change) is 0.34 with sampled returns and 0.99
priced from the yield; monthly bond volatility is within 10% of the sampled one.
Over 60 years bonds return 2.1% a year after inflation (geometric, 800 paths)
against 1.5% sampled and 1.56% in the record: the yield starts above the record's
mean and the record's own bond returns include the decades of financial repression
in which yields sat far below inflation, which a level-based price does not
reproduce. On the dual-income example (4,000 paired paths) success moves from
91.5% to 92.0% (+0.5, 95% interval 0.0 to +1.0) and the median terminal net worth
by 9%; a 30-year retiree in 40/60 at a 4% withdrawal goes from 92.7% to 96.9%
(2,000 paths) and ends 17% richer at the median.

Not modelled: the yield level still does not respond to the path's sampled
inflation (it moves by history's yield changes, not by the inflation of the row),
so a sampled 1970s block raises yields as 1970s yields rose but a long run of
sampled high inflation does not lift the level beyond that; only the block
bootstrap is affected (the regime model already prices from its yield, the replay
uses the record, the lognormal model has a constant yield).

### 2.2 Regime switching

A two-state hidden Markov model (calm and turbulent) fitted to the same data drives
the equity premium and volatilities; inflation and home prices are ARMA(1,1)
processes with persistent expectations; the long yield follows an error-correcting
equation; bonds are priced from the yield path (carry, duration, convexity);
recessions depend on the regime. It starts from today's state (yield 5.3%, bills
3.8%, calm regime).

After the audit, real home prices are pulled back toward their trend, with the pull
sized so that the spread of ten-year real price changes matches history (ten-year
real change at the 5th / 50th / 95th percentile: -40% / -1% / +63% before, -18% /
+10% / +50% after, -10% / +6% / +56% in the record), and the starting momentum is
the average of the last twelve months rather than the last (seasonally low) month,
so paths no longer begin with a real decline (-2.4% in year one before, 0.0% after). Known remaining differences from the record are in 17.

### 2.3 Lognormal

Independent, jointly lognormal monthly returns with the full-sample means,
volatilities and correlations. No crashes clustering, no persistent inflation, a
constant yield. It is the textbook baseline the other models are compared with.
Bill returns are floored at zero (independent draws around a 0.27% monthly mean were
negative in 14% of months) with the centre lowered so that the mean is unchanged.

#### Fat tails and volatility clustering (`market.kind: fat_tailed`, opt-in)

The lognormal model with the same means, volatilities and recession chain, and
three additions. Stocks (US and international), bonds and gold share one Student-t
mixing variable each month (5 degrees of freedom, unit variance: a multivariate t,
so a bad month tends to be bad for all of them). The variance of both stock markets
follows a GARCH(1,1) recursion on the US shock (Bollerslev 1986; with t shocks
Bollerslev 1987), `v' = 0.02 + 0.15 v eta^2 + 0.83 v`, capped at 16 times normal
(the record's highest trailing-year variance, to April 1933, is 15 times its
average) and divided by its long-run mean so that the unconditional volatility is
the lognormal model's. Months whose conditional variance is in its top quarter use
the correlations of the record's turbulent months (trailing 12-month volatility
above its 75th percentile), the others the calm months': US with international
stocks 0.77 against 0.60 (Longin and Solnik 2001 find the same rise in bear
markets), US stocks with 10-year Treasuries 0.00 against 0.13. The record shows the
stock-bond correlation falling in turbulent months, not rising (the flight to
quality), so the model follows the record. The log-return means are exactly the
lognormal model's, and the model draws from the `Market` stream exactly what the
lognormal model draws, so the two see the same normal shocks on a path; the
mixing variables come from a stream of their own (`MarketTails`).

`a`, `b` and the degrees of freedom were fitted by simulated moments: on a grid,
the values whose 1,203-month samples (the record's length) have, at the median, the
record's excess kurtosis of monthly US stock returns (7.4) and autocorrelation of
absolute returns at lags 1 and 12 (0.22 and 0.15). Maximum likelihood on the same
record gives 0.13, 0.83 and 7.3 degrees of freedom, whose samples have a kurtosis of
3.5: too thin-tailed for a model whose point is the tails. Measured
(`realism_tests/fat_tails.rs`, 300 samples of 1,203 months): kurtosis 7.40,
absolute-return autocorrelation 0.229 and 0.131, against 0.03 and 0.000 for
lognormal; the median 100-year worst real fall of US stocks -63% (5th to 95th
percentile -90% to -38%) against -58% for lognormal, and the record's -79% sits
inside that band; 7 months below -15% per century on traced paths against the
record's 11. Volatilities within 2% and log means within 0.05 points of lognormal.

What it does to plans: little to success, more to the bad tail of drawdowns. On a
60/40 retiree spending 4.5% (3,000 paired paths) success is 87.3% lognormal and
88.8% fat-tailed, and the 95th percentile of the portfolio's worst fall is 57%
against 48% (the 5th percentile 15% against 19%). On the early-career example
(`firesight compare`, 4,000 paired paths, lognormal against fat-tailed) success goes
from 82.0% to 84.2%. Volatility is concentrated in fewer, wilder paths: most paths
see calmer markets than lognormal's constant volatility and a few far rougher ones,
and over a 30-year plan that roughly cancels or slightly helps. A trace announces
each month in which US stocks fall more than 15%.

Not modelled: no mean reversion and a constant yield (as in lognormal); recessions
stay independent of the volatility state; no leverage effect (falls do not raise
volatility more than rises); today's volatility is not an input (paths start at the
long-run mean variance); bills, home prices and inflation keep normal shocks.

### 2.4 Historical replay

Path 1 starts in July 1926, path 2 in August 1926, and so on (1,203 cohorts,
wrapping at the end of the record). The replay is exact for every series that has
real data. Use it with `equity_premium_adjustment: 0` (a validation warning says so
otherwise).

### 2.5 Assets without a century of data

* **International stocks** (data from 1975) and **gold** (from 1968) are extended
  backwards by a factor model on US stocks, bonds and inflation plus a residual with
  the fitted volatility. Their own-sample means are not comparable with full-sample
  US returns and inflation: before the audit gold returned 3.4%, 4.3% or 5.3% real
  depending on the model, and international stocks beat US stocks in one of them.
  Now the three resampling models share two stated assumptions
  (`data/actuarial/params.json`, `long_run_assumptions`): international stocks
  return **1 point a year less than US stocks** (geometric) and gold returns **1% a
  year real**. Volatility and co-movement still come from the data. The historical
  replay keeps the record as it was.
* **Listed real estate**: equity beta 0.70, volatility 19%, no alpha (4.4% real
  geometric); conservative against the REIT record.
* **A single stock**: the market return times its beta plus idiosyncratic noise
  with fat tails, mean-preserving, with a ruin hazard (15% of companies gone in 30
  years at the default settings). Volatility 36% by default.
* **Crypto**: 70% volatility, Student-t(4) shocks capped at 8 standard deviations,
  correlation with equities 0.35 rising to 0.6 in sell-offs, and a 1% a year chance
  of total loss. The preset fixes the **expected** (arithmetic) real return,
  collapses included: 0% by default (skeptic -5%, believer +6%). At 70% volatility an
  expected real return of zero means the typical path loses about a fifth of its
  value a year while a few gain enormously. Before the audit the *median* was fixed
  at zero, which made the expected return +26% a year and let a crypto sleeve improve
  the median plan. After: moving 10 points from US stocks to crypto on the
  early-career example lowers success by 9.6 points.

### 2.6 What the models produce

US history as it was (`equity_premium_adjustment: 0`), annualised, in percent.
Columns: arithmetic real / geometric real / volatility.

| Asset | Bootstrap 60 | Regime | Lognormal | History |
|---|---|---|---|---|
| US stocks | 8.62 / 7.16 / 18.4 | 8.96 / 7.54 / 18.2 | 8.64 / 7.16 / 18.5 | 8.62 / 7.16 / 18.4 |
| 10-year bonds | 1.78 / 1.56 / 6.8 | 2.29 / 2.02 / 7.6 | 1.77 / 1.55 / 6.8 | 1.79 / 1.57 / 6.8 |
| Bills | 0.31 / 0.29 / 1.8 | 0.62 / 0.61 / 1.9 | 0.31 / 0.30 / 1.8 | 0.31 / 0.30 / 1.8 |
| Listed real estate | 6.15 / 4.36 / 19.3 | 6.28 / 4.55 / 19.1 | 6.16 / 4.38 / 19.4 | n/a |
| Single stock | 9.44 / 3.91 / 36.1 | 9.78 / 4.27 / 36.0 | 9.48 / 3.94 / 36.2 | n/a |
| Inflation (mean / vol) | 2.96 / 1.81 | 2.96 / 1.81 | 2.96 / 1.81 | 2.96 / 1.81 |

After the audit, in all three resampling models: international stocks geometric
real about 6.2% (US less 1 point, volatility 17 to 18%); gold 1.0% real geometric
(volatility 16 to 17%); crypto 0% real arithmetic (volatility 73%). Real national
home prices grow about 1.0% a year.

Real compound return over 30 years, percentiles 1 / 5 / 25 / 50 / 75 / 95 / 99 (%):

| | US stocks | 60/40 |
|---|---|---|
| History, rolling windows | 4.6 / 4.8 / 6.0 / 7.2 / 8.2 / 9.6 / 10.4 | 3.0 / 3.3 / 4.2 / 5.4 / 6.3 / 7.3 / 7.7 |
| Bootstrap 60 | -0.3 / 1.9 / 5.1 / 7.3 / 9.4 / 12.3 / 14.2 | 0.6 / 2.0 / 4.0 / 5.4 / 6.8 / 8.6 / 10.0 |
| Regime | -3.5 / 0.0 / 4.7 / 7.8 / 10.9 / 15.1 / 17.9 | -1.1 / 1.1 / 4.0 / 5.9 / 7.9 / 10.6 / 12.4 |
| Lognormal | -0.8 / 1.4 / 4.8 / 7.1 / 9.6 / 13.2 / 15.8 | 0.3 / 1.7 / 3.8 / 5.3 / 6.9 / 9.1 / 10.7 |

The simulated tails are wider than the overlapping historical record (which has
only three or four independent 30-year periods). P(30-year real US stock return
below zero): history 0, bootstrap 1.2%, regime 4.9%, lognormal 1.9%.

Worst real drawdown in the first 30 years, median: US stocks -52% in history's
windows, -55% bootstrap, -57% regime, -46% lognormal; 60/40: -32%, -37%, -38%, -30%.

Correlations (monthly | annual): US stocks and bonds 0.06 | 0.07; real bonds and
inflation -0.34 | -0.52; inflation and home prices 0.36 | 0.54; international and US
stocks 0.73; gold and US stocks about 0; crypto and US stocks 0.4.

Recessions: 16.7% of months in the NBER record, 1.6 a decade, mean length 12.6
months; simulated 16.8 to 17.6% of months, 1.7 to 2.1 a decade, 10 to 12 months.

### 2.7 The default is not the US record

US stocks returned 7.2% a year after inflation from 1926, the best large market of
the century. The world outside the US returned about 4.5% over the long run, and
estimates from today's valuations are 3 to 5%. A plan tested only against the US
record is tested against the luckiest case. The schema default is therefore
`equity_premium_adjustment: -0.015`: every month's gross stock return (US and
international; real estate by 0.70 of it, a single stock by its beta) is scaled so
that stocks return 1.5 points a year less than the record. Bonds, bills, inflation,
rates and recessions are untouched. This is a judgement between the record (0) and
the international and valuation views (-0.02 to -0.03); set it to 0 to plan on the
record.

It matters. On the early-career example (10,000 paired paths, after the audit):
success 84.2% at the default, 90.7% at 0, 75.2% at -0.03; the median age of
financial independence moves from 39.3 to 38.7 and 40.0.

#### Valuation-aware expected returns (`market.valuation`, opt-in)

The adjustment above lowers every month's stock return by the same amount for the
whole plan. `valuation: { starting_cape, fair_cape, half_life_years }` instead
starts expected US stock returns from today's valuation and lets it fade. The
state is the log gap between the cyclically adjusted price-earnings ratio (CAPE) and
its normal level, `v = ln(CAPE / fair)`. Each month the model's own US stock return
is multiplied by `exp(-k v)`, with `k` set by the half-life, and the month's real
surprise (its real log return less the model's mean) moves `v` one for one, since a
ten-year earnings average barely moves in a month. So `v` is a stationary AR(1):
an expensive start returns less until the CAPE has come back to normal, a crash
lowers the CAPE and raises later expected returns, and from a fair start the
long-run mean is the model's own. The expected shortfall of the cumulative log
return after `t` months is `v0 (1 - (1 - k)^t)`, which tends to `v0`: the level lost
while valuations normalise is not made up. This is Campbell and Shiller's finding
(1998, 2001) that valuation ratios forecast ten-year stock returns and revert mostly
through prices. Listed real estate, the single stock and the custom asset follow by
their equity beta (as under a stress). International stocks, gold and crypto are
left alone. The overlay applies to the bootstrap, regime and lognormal models, not
to the historical replay. The model's mean is computed exactly for the bootstrap
(the record's mean) and the lognormal model (its parameter), and for the regime
model from 2,000 fixed-seed paths of 60 years, computed once per process. It draws
no random numbers, so the sampled shocks and every other series are unchanged. A
stress overlay (2.8) dictates US stock returns over its window, so no tilt applies
inside it and the gap moves with the stressed prices alone; after the window the
tilt is re-derived from the gap the stress left, so a crash at retirement from a CAPE
of 40 brings valuations down and the returns that follow up, rather than the path
carrying on as if the CAPE were still 40.

Parameters (`data/actuarial/params.json`, `market_realism.valuation`): starting
CAPE 40 (Shiller CAPE of the S&P 500, 38 to 42 through 2026: YCharts, Financer,
multpl.com), normal CAPE 17.3 (its mean since 1881; reverting all the way to it is
the bearish reading, and 25 to 30 assumes part of today's level is permanent),
half-life 15 years (a judgement in the 10 to 20 year range; the papers do not pin
down the speed).

Measured. Over 1,000 paths the first decade's real log return falls short by 0.310
(bootstrap), 0.322 (regime) and 0.303 (lognormal) against the formula's 0.310, about
3.1 points a year. The third decade still loses 0.11 to 0.15. From a fair start the
60-year mean real return moves by less than 0.15 points, and the spread of ten-year
real returns narrows by 18 to 19% (the price feedback is a source of mean reversion
the models did not have). On the early-career example (4,000 paired paths, the
record as it was, `equity_premium_adjustment: 0`): success 90.4% to 88.4% and the
median age of independence 38.7 to 39.7. Combined with the default -1.5 point
adjustment, which partly counts the same high valuations twice (a validation warning
says so), success falls from 83.7% to 80.8% and independence moves from 39.3 to 40.4.
With a 40% crash at retirement as well (default adjustment, 4,000 paired paths) success
is 64.9%, against 58.6% for the crash alone: by retirement much of the gap has
closed, the crash takes the CAPE below normal, and the returns that follow are
tilted up.

Not modelled: earnings growth shocks separate from prices (all reversion goes
through prices), valuations of international markets, and any link from valuation
to dividend yields unless `dynamic_yields` is also on.

### 2.8 Stress scenarios

Applied to every path on top of the random draws.

* **Crash at retirement**: US stocks fall by exactly the drawdown over six months
  from the retirement month; other risky assets move by their equity beta. Over the
  recovery years each month's sampled return is scaled by one constant so that at
  the end the index is back at its **pre-crash nominal level**, and not before; then
  sampled returns resume. What stocks would have earned over the crash and the
  recovery is lost for good. Before the audit the index rejoined the unstressed path
  (a buy-and-hold investor lost nothing): a 40% crash with a five-year recovery cost
  the early-career example 3.6 points of success; now 26 points.
* **Lost decade**: the 120-month real return of US and international stocks is
  exactly zero, with the sampled volatility kept.
* **Stagflation**: inflation at the stated rate for the stated years with the real
  returns of 1973 to 1981 (stocks -2.8%, bonds -5.2%, bills -0.9% a year) and yields
  shifted up with inflation.

A token or company that has collapsed (a return of exactly -100%) stays collapsed
through a stress overlay.

#### Historical episodes and a lost generation (opt-in stresses)

* **Historical episode** (`historical_episode { start, episode }`): from the start
  date the path lives through a real episode again. US stocks, 10-year bonds, bills,
  inflation, home prices and the recession flag are the record's months, as they
  were (no equity-premium or inflation adjustment); international stocks and gold
  too where the record has them (from 1975 and 1968), otherwise they, listed real
  estate, crypto, the single stock and the custom asset move by their equity beta
  times the change imposed on US stocks. The 10-year yield and the mortgage rate
  move by the record's monthly changes from the path's level, so a path does not
  jump to 1929's rates overnight. The windows, computed from `data/history` (CRSP
  total market deflated by CPI-U) as the month after the real peak to the month the
  real index regained it: Great Depression 1929-09 to 1945-02 (186 months, -79% at
  the 1932 trough); dot-com 2000-04 to 2013-02 (155 months, which includes 2008;
  stocks +0.2% after inflation over the whole window, bonds +4.1% a year); 2008
  2007-11 to 2013-01 (63 months, -51% at the trough). The 1966 window, January 1966
  to July 1982, is a judgement: the total market regained its 1966 real level
  briefly in 1967, so it is set by the Dow's 1966 peak and the August 1982 low
  (inflation 7.0% a year, stocks -1.3% and bonds -2.6% a year after inflation, the
  yield from 4.7% to 13.7%). It ends at the low, so none of the 1980s recovery is
  included.
* **Lost generation** (`lost_generation { start, years }`, default 20 years):
  Japan after 1989. Over the window inflation is scaled to average -0.3% a year
  (Japan 1999-2012, World Bank via FRED), US and international stocks to a real
  total return of -2% a year (a judgement: TOPIX fell from 2,881 at the end of 1989
  to 1,086 at the end of 1998, Cabinet Office of Japan 2002, and returned -1.96% a
  year in price from the end of 1990 to 2013, CFA Institute 2013), each month's
  gross return times one constant so volatility is kept. The bill and 10-year
  yields glide in a straight line over eight years from the path's level to 0.1%
  and 1.0% (JGB 10-year yield 7.0% in 1990, 1.5% in 1998, 1.0% in 2003, OECD via
  FRED) with the path's own monthly moves kept, and bonds are priced from that
  yield. Home prices keep their real change.

After either window, the shift imposed on the rate levels fades at 1/120 a month
(the bootstrap's pull to the mean) instead of snapping back, with the carry and
price change of the fading shift added to bond returns. A trace's event log
announces the episode in the month it starts.

Measured (`firesight compare`, early-career example, 4,000 paired paths, each
stress starting at retirement): success 83.7% at baseline, Great Depression 19.1%,
1966 42.4%, dot-com 41.4%, 2008 77.6%, lost generation 29.8%. On a 60/40 retiree
spending 4% from a taxable account with `equity_premium_adjustment: 0` (1,500
paths): 94.5% at baseline, 1966 0.8%, 2008 92.2%, 1929 81.7%, 2000 69.4%, lost
generation 42.1%. The 1966 figure is far below Bengen's 1966 retiree, who survived
30 years: that retiree had the 1982-2000 boom after the window, while here the path
after July 1982 is an ordinary draw, and here bonds are 10-year Treasuries
(-2.6% a year real), not intermediate ones.

Not modelled: the episodes' unemployment and wage paths (layoff risk still follows
the recession flag only), the Japanese land-price collapse in a lost generation,
and episodes outside the US record.

## 3. Inflation and what follows it

Inflation is CPI-U as generated above; `inflation_adjustment` adds a constant.
Budget lines, wages, Social Security, tax brackets and health costs each follow it
in their own way:

* Budget lines rise with CPI plus a real growth rate (defaults: rent +0.5%,
  healthcare lines +1.5%, childcare +1.0%, everything else 0).
* Tax thresholds that the law indexes are indexed with the path's CPI (wage-indexed
  ones with CPI plus 1.1% real wage growth); thresholds the law freezes stay frozen
  (the 3.8% investment-income tax and additional Medicare thresholds, the Social
  Security taxation thresholds, the child-credit phase-out, the $3,000 loss limit).
  Over 30 years at 2.5% inflation with incomes rising with prices, a couple with
  $48k of benefits and $40k of IRA income sees federal tax rise from $1,550 to
  $4,777 in real terms for that reason alone. The chained-CPI wedge (about 0.25
  points a year) is not modelled, so indexed thresholds end about 8% too generous
  after 30 years.
* Medical costs rise 1.5% a year faster than CPI.

## 4. Accounts

Five buckets: cash; taxable (value and cost basis per asset class); and for each
adult traditional, Roth (with contribution basis and five-year conversion tranches)
and HSA. Each month every holding earns its asset class's return less fees
(expense ratio plus advisory fee). The taxable bucket distributes interest and
dividends at each class's yield, which are taxed and reinvested.

New money is saved in this order: employer plan through payroll (with the match),
HSA, IRA (traditional when deductible and today's marginal rate exceeds the
expected retirement rate, otherwise Roth within the income limits), after-tax
401(k) where the plan allows it, then taxable. Contribution limits, catch-ups (50+,
60 to 63, HSA at 55) and the phase-outs are applied.

The target allocation follows the glide path (if any) and is rebalanced annually,
by drift band, or never. With `tax_efficient` location, bonds and other
ordinary-income assets sit in traditional accounts and broad stock funds in
taxable. Employer shares kept with `sell_on_vest: false` are left out of
rebalancing: they stay a concentrated single-stock position, sold only when the
taxable account is drawn on. (Before the audit the January rebalance sold them, so
"hold" behaved like "sell": the 10th-percentile outcome moved 2% where really
holding moves it by half.)

While working, cash above the emergency fund (default six months of spending) is
invested each month.

#### Dividend yields that move with the market (`market.dynamic_yields`, opt-in)

Off, US stocks (and the single stock) distribute 1.5% a year and international
stocks 3%, whatever the market does. On, each yield is a state on the path, after
Lintner's (1956) model of dividend smoothing: a fall in prices raises the yield at
once, dividends then close 30% a year of the gap to their target (a judgement:
Lintner's partial-adjustment form, with published speeds that vary widely), and in
recession months they are cut at a further 10% a year. The 10% is calibrated so that
replaying 2007-11 to 2013-01 (`historical_episode: financial2008`) cuts the US
dividend run rate by 20% from peak to trough, against the 21% fall of S&P 500
dividends per share from 2008 to 2009 (Shiller's data, as tabulated by DQYDJ). Only
the surprise in each month's return moves the yield (against the model's long-run
mean log return), and the target is lifted by the long-run effect of the recession
cuts, so the average yield stays today's: 1.53% and 3.1% over the bootstrap,
regime, lognormal and fat-tailed models (`realism_tests/dividends.rs`). With
`market.valuation` the US target also moves inversely with the CAPE gap: from a
CAPE of 40 the yield rises toward 1.5% x 40 / 17.3 = 3.5% as the market returns to
its normal valuation (3.6% on average after 50 years, against 1.55% without
valuation). Yields are bounded to 0.2% to 15%.

Total returns are never changed: only the split between taxable dividends and price
gains moves, and with it the taxable account's tax timing and the income that ACA
subsidies and IRMAA test. Bond and cash interest already followed the path's
10-year and bill yields before this option (a par bond's coupon and the bill rate of
the month), so the option does not change them; in the lognormal and fat-tailed
models the 10-year yield is constant and so is bond interest. Stress overlays
re-derive the yields from their start month on. A trace announces each fall of
the US dividend run rate by 10% from its peak.

Measured: an 80/20 retiree with $3M in a taxable account spending $120k, with
`valuation` on (1,500 paired paths): median lifetime tax $367,700 -> $400,300
(+8.9%), median terminal wealth -1.2%, success 90.1% -> 89.9%. On the
dual-income example (`firesight compare`, 4,000 paired paths) the change is within
noise: success 91.5% both, median lifetime tax +$500; with `valuation` on, tax
+$4,800 and success 89.7% -> 89.8%. Little of that household's money is in a
taxable account.

Not modelled: listed real estate keeps its constant 4% (REIT payouts follow rents,
not prices); the qualified share of dividends is fixed; bond funds pay the current
yield, not the book yield of bonds bought earlier; special dividends and buybacks.

### Stopping saving in distress (opt-in)

`portfolio.pause_deferrals_in_distress: true` (default false) stops a household
saving for retirement while it is in trouble. A month is one of distress when the
household paid an early-withdrawal penalty within the last three months (a retired
household draws a quarter at a time; the three is a judgement), cut essential
spending, carries a non-mortgage debt at `distress.high_interest_rate` or more
(default 8%: credit cards averaged 21.6% in 2024, Federal Reserve G.19; the line
itself is a judgement), or, while working, holds less cash than its emergency-fund
target. In such a month, cash above the month's cash target first pays down the
high-interest debts, dearest first; then nothing else is invested (no IRA, HSA,
after-tax 401(k) or taxable contribution from the surplus), so the remainder
rebuilds the emergency fund. In the next month's pay the employee deferral drops
to what earns the full employer match (`distress.keep_match`, default true; false
stops it) and the after-tax route closes. Saving resumes the month after a month
without distress. Pauses and resumptions are trace events; the debt paid shows
in the debt-service spending line. No random numbers.

Measured on a test household (35, $100,000, 15% into a 401(k) with a 50% match on
6%, $25,000 on a 22% card, no tax): the card is gone within a year instead of
dragging on at minimum payments, the deferral is $500 a month instead of $1,250
while it lasts, and terminal net worth is higher on paired paths. With the CLI
(3,000 paired paths) the shipped examples barely move, because their debts are
small or cheap: `dual-income-family` median terminal net worth +$30,000 (success
91.8% -> 91.7%, within noise), `early-career-saver` -$21,000, `side-hustle-landlord`
-$4,000; each within noise on success.

Not modelled: solo 401(k) and HSA payroll contributions continue, as do the
employer's own; there are no hardship withdrawals or 401(k) loans; the household
does not cut spending to save more.

### Panic selling (opt-in, a behavioural assumption)

`portfolio.behavior: panic_prone` (the default is `disciplined`) lets the household
sell stocks after a crash. The engine follows the US stock index and its running
peak; a fall of at least `drawdown_trigger` (default 25%) from the peak is one fall,
and in its first month the household decides once, with probability `probability`
(default 8%), whether to panic. If it does, `share` (default half) of its US and
international stock weight moves to cash in its target mix and it trades to that
mix at once through the ordinary rebalance, so taxable sales realise their gain or
loss at average cost; new savings buy the panic mix. It buys back at the plan's own
target when the index is back within `reentry.recovered_within` (5%) of the old
peak, or after `reentry.max_months` (36) in any case. A fall ends at a new high;
only then can the next one trigger a decision. One draw a month from its own stream.

The defaults are calibrated to how US retirement-plan savers behaved in 2008:
Vanguard (How America Saves 2009, as reported by PLANSPONSOR) reports that 16% of
participants traded, 2% abandoned equities entirely, and that of the fall in the
equity share of plan assets from 73% to 61% over 2008, about 4 percentage points came
from participants shifting to fixed income (the rest from prices). Four points of a
73% equity share is about 5.5% of the equities held. An 8% chance of selling half
makes the expected share of stocks sold in a fall 4%, at the low end of that
figure. Morningstar's Mind the Gap (2024) puts the cost of investors' timing at
about 1.1 points a year across all US funds over the ten years to 2023, which
covers much more than panic selling and is an order-of-magnitude check only. The
re-entry rule is a judgement. Plan savers are among the most inert investors; a
retail investor may well be more panic-prone, which is why the probability is the
user's to set.

Measured with the CLI (3,000 paired paths): at the defaults the effect is within
sampling noise for `dual-income-family` (success 91.8% both ways, median terminal
net worth +$10,000) and `early-career-saver` (84.2% -> 84.4%, -$11,000). With
`probability: 1` (panics in every fall) `dual-income-family` loses 0.6 points of
success and $35,000 at the 10th percentile of terminal net worth, and
`early-career-saver` loses $204,000 of median terminal net worth with success
unchanged within noise: selling after a fall misses most of the recovery, but on
some paths it also escapes a deeper fall.

Not modelled: only US and international stocks are sold (not REITs, crypto,
employer stock or the custom asset); a household protecting the marketplace
premium credit trades only inside its sheltered accounts, as its rebalances do; the
chance does not depend on age, wealth, experience or the speed of the fall; a
household never panics twice in one fall.

## 5. Taxes

US federal law for 2026 as enacted, every state and DC, and the larger local
income taxes, computed by one function on a year's inputs. The return includes:
ordinary brackets; long-term gains and qualified dividends stacked on top with
their own 0/15/20% brackets; the standard deduction with the age-65 additions and
the 2025-2028 senior deduction; itemising with the SALT cap schedule and mortgage
interest; the child credit with its refundable part (phased in on *taxable* earned
income, after 401(k) deferrals); taxation of Social Security (the 50%/85% formula
on frozen thresholds); self-employment tax and the QBI deduction; FICA with the
wage base and the additional Medicare tax; the 3.8% investment-income tax; capital
loss netting with the $3,000 limit and carry-forward; early-withdrawal penalties;
the ACA premium credit with the 400% cliff (2026 law) and full repayment of excess
advances; IRMAA Medicare surcharges with the two-year lookback; rental income with
depreciation, the passive-loss allowance and recapture on sale.

Hand-checked returns agree with the engine line by line (thirteen cases: single
$60k in Texas, tax $5,020 and FICA $4,590; joint $400k itemiser in California,
federal $67,146 and state $26,543; early retirees with $110k of gains and
dividends, tax $0; a couple 68/66 with $48k of benefits and $40k of IRA income,
taxable benefits $23,000 and tax $1,550; and others). The ACA credit for a couple
of 60 with a $31,854 benchmark: $30,848 at 138% of the poverty line, $25,535 at
300%, $23,428 at exactly 400%, $0 at 400.01%.

Tax is paid on a cash basis: withholding on wage-like income and level instalments
set from each January's estimate, and the balance of the actual return the next
April. A taxable windfall is paid for through re-planned instalments over the rest
of its year (before the audit it was also "withheld" at up to 60% in the month it
arrived and the excess came back as income the next spring).

State and local rules corrected by the audit:

* A city resolves by any of its names: "New York", "NYC", "Brooklyn" and "New York
  City" are the same place. New York City's tax is its graduated schedule (3.078%
  to 3.876%) on New York taxable income, not the top rate on gross income.
* Local taxes have a base. Wage and earnings taxes (Ohio and Pennsylvania
  municipalities, Kentucky, Kansas City, Wilmington, Detroit, Birmingham) reach
  wages and business profit only; a retiree pays nothing. The Portland-area taxes
  start above $125,000 single / $200,000 joint of Oregon taxable income (2.5%, then
  4% above $250,000 / $400,000); before, 4% was charged from the first dollar.
  Maryland, Indiana and New York City follow all income.
* States that exempt retirement distributions exempt a Roth conversion too
  (Illinois, Pennsylvania, Mississippi, Michigan, Iowa; New York's and others'
  exclusions apply to it). Illinois has no age test; Iowa's is 55; the others use
  59 and a half.
* Pennsylvania taxes wages before 401(k) deferrals. California has its own
  head-of-household schedule.
* The poverty line used for a coverage year is chained from the latest published
  guideline with the path's inflation (it ran about 2% low from 2028 on).

Other tax modes: a flat rate, or none. A household abroad pays its flat foreign
rate instead of US federal, state and payroll tax (a simplification).

### Chained-CPI indexing (`assumptions.taxes.chained_cpi`, off by default)

Since 2018 the income tax brackets, the standard deduction (and the additional
amount at 65), the capital-gain thresholds, the QBI thresholds, the child credit
amounts, the IRA and HSA contribution limits and the Roth and IRA-deduction income
ranges are indexed to the chained CPI-U (IRC 1(f)(3), which sections 219, 223 and
408A cross-reference), which rises more slowly than
the CPI-U that drives the engine's prices: CBO puts the gap at about 0.25 percentage
points a year on average (CBO, "Using the Chained CPI to Index Social Security, Other
Federal Programs, and the Tax Code for Inflation", testimony, 2013; the range in any
single year is +0.1 to -0.5). With the option on, those thresholds are multiplied by
`0.9975^(year - 2027)` on top of the simulated CPI-U factor; the figures published
for 2027 are already on the chained index, so 2026 and 2027 do not change. The
401(k) limits (section 415(d), the Social Security adjustment), the IRMAA tiers, the
poverty line and the wage base keep their own indexes (CPI-U, Medicare premiums,
average wages), and state schedules stay on the CPI-U (states differ and the data
does not say which follow the federal index). With `index_brackets` off nothing is
indexed and the option does nothing (validation warns).
Measured on 3,000 paths of the dual-income example (same seed, option on against
off): median lifetime tax $963k to $1,004k (+4.2%), success rate 91.8% to 91.4%,
median terminal net worth $1.726m to $1.720m. Not modelled: the CPI-U path is the
simulated one, so the wedge is a fixed rate rather than something that varies with
the inflation regime (the historical wedge is larger when inflation is high). The
qualified charitable distribution limit (IRC 408(d)(8)(G), also a section 1(f)(3)
adjustment) takes the same chained factor.

### State detail (`assumptions.taxes.state_detail: full`, `basic` by default)

Three state rules that the base tables leave out, each read from its statute or the
state's own form and stored under a `detail` key of the state in
`data/tax/us-states-2026.json` (sources and what could not be checked are in
`data/tax/SOURCES-states.md`). They change nothing for a household in any other state.

* **California State Disability Insurance**: 1.3% of all wages for 2026, no wage
  ceiling since 2024 (EDD). Charged on wages (RSU vests and bonuses included), not on
  self-employment profit, and counted as state tax, so it is also part of the SALT
  deduction as the IRS allows.
* **New York's tax-benefit recapture** (Tax Law 601(d)-(e)): above $107,650 of New
  York AGI the benefit of the lower brackets is taken back over $50,000 of AGI, tier
  by tier, until taxable income pays the flat rate of its bracket on every dollar
  (and above $25 million of AGI the top rate on everything). The mechanism is the
  state's own worksheet logic: for taxable income in bracket m the recapture is the
  base (flat tax at the rate below, on the bottom of bracket m, less the schedule's tax
  there) plus the increment of bracket m scaled by (AGI minus the bracket's start) over
  $50,000, capped at one. The engine reproduces the bases printed in the 2024 IT-201-I
  (joint 333, 807 and 2,747; head of household 787, 2,289 and 3,076) to the dollar.
  It is applied to the 2026 rates in the data (the first five rates cut by 0.1 point);
  the 2026 worksheet was not read, and head of household uses the single schedule
  because that is all the data holds.
* **Utah's Social Security benefits credit** (Utah Code 59-10-1042): a nonrefundable
  credit of the flat rate (4.45%) times the benefits included in federal AGI, reduced
  by 2.5 cents per dollar of income above $54,000 single or $90,000 joint and head of
  household (not indexed). The rate, the 2.5 cents and the thresholds match the Tax
  Commission's 2025 worksheet for the credit (code AH), which measures income as
  Utah taxable income (TC-40 line 9: federal AGI plus additions less subtractions)
  plus tax-exempt interest. The engine uses federal AGI, which is the same thing for
  a household with no Utah additions or subtractions. The retirement credit it
  excludes is not modelled.

Measured on 3,000 paths of the dual-income example moved to each state (same seed,
option on against off): median lifetime tax in California $1.21m to $1.29m (+6.5%,
the payroll tax on two salaries), New York $1.52m to $1.56m (+2.3%), Utah $1.335m to
$1.315m (-1.5%, the credit once both are on benefits). Not modelled: other states'
recapture or phase-out rules (Connecticut's), New Jersey and other disability or
family-leave payroll taxes, Washington's payroll programmes, Oregon's.

### Policy risk (`assumptions.taxes.policy_risk`, absent by default)

`social_security_haircut` is a switch: every path gets the same cut. `policy_risk`
makes policy a distribution. Once per path, from its own random stream
(`Stream::PolicyRisk`, four uniforms whatever the settings, so scenarios stay paired),
the path draws: whether Social Security benefits are cut (probability
`ss_haircut_probability`), and from which calendar year (uniform over the fifteen years
from `earliest_year`); and the same two things for a rise of `tax_increase_points`
in every federal ordinary-income bracket rate. Capital gains, payroll and state tax do
not change. The household does not foresee the change: planning uses the law of the
year as it stands when the year starts. Once a cut has happened it is known: the
funded test that decides when to retire counts the reduced benefit from then on,
and the spending rule sees the reduced income. The cut multiplies the scheduled
benefit (and stacks on `social_security_haircut` if both are set; validation warns).

Sources: the default factor 0.77 and `earliest_year` 2033 are the OASI trust fund's
depletion year and payable share in the 2025 Social Security Trustees Report (SSA,
2025; the combined funds are 81% in 2034; the 2026 report is reported to move OASI
depletion to late 2032 with 78% payable, which was not read at the source, so the
defaults stay on the 2025 figures). Everything else is a judgement, registered
in `data/actuarial/params.json` under `unverified_from_memory.policy_risk_defaults`:
probability of a cut 0.5, probability of a tax rise 0.25, two points, a 15-year window.

Measured on 3,000 paths of the dual-income example, defaults on against off: success
91.8% to 91.8% (21 paths rescued, 19 harmed), median retirement age 53.4 to 53.7,
median lifetime spending $7.35m to $7.26m, median lifetime tax $963k to $978k, median
terminal net worth $1.726m to $1.845m. This household retires when funded, so it
answers a cut that has happened by working a little longer and spending less, which
protects the success rate and leaves more behind; a household already retired has
no such margin and simply ends with less (the engine's tests check a retiree's
benefit falls to exactly 77% from the drawn year). Not modelled: a cut that spares current retirees, a rise in the
payroll-tax cap or the retirement age instead of a cut, rate changes that differ by
bracket, changes to capital-gains or estate tax, and any link between policy and
the economy on the same path.

### Tax-loss harvesting (`portfolio.tax_loss_harvesting`, off by default)

Once a month after returns, any taxable holding that stands at least 5% below its cost
basis (and by at least $1,000 in today's dollars) is sold and a similar fund of the
same asset class bought, so the portfolio is unchanged and the wash-sale rule is
assumed avoided. Its basis resets to its value and the loss is added to the year's
long-term gains, so the tax return nets it against gains, takes up to $3,000 of the
rest against ordinary income and carries the remainder forward (the existing loss
machinery). Held single stock (employer stock the household is keeping) is not
harvested. A trace event records each harvest.

It defers tax; it does not remove it. The lower basis becomes a larger gain when the
holding is finally sold, and the benefit comes from the delay, from losses used against
ordinary income at a higher rate than the later gain, and from a step-up at death that
erases the deferred gain. The 5% and $1,000 thresholds are judgements (registered under
`unverified_from_memory.tax_loss_harvesting`); no trading cost or tracking difference
is charged.

Measured on 3,000 paths, on against off (medians): dual-income lifetime tax $963.5k to
$962.0k (-0.2%), early-career-saver $851.7k to $850.8k (-0.1%), side-hustle-landlord
$591.7k to $594.0k (+0.4%); median terminal net worth moves +0.4%, -0.7% and +0.8%
respectively, and success rates do not move beyond noise. The effect is small and
mixed in sign: these households hold little in a taxable account that falls below
basis, and the lower basis raises realised gains (and the income that health-insurance
subsidies are tested on) in early retirement. In the year of a harvest the mechanism
is exact: a worker who sells nothing sees $3,000 come off income and the rest of the
loss carried forward (tested). Not modelled: trading
costs, tracking difference against the replaced fund, wash sales through the household's
other accounts or a spouse, harvesting individual lots rather than whole asset classes.

### Qualified charitable distributions (`retirement.qcd`, off by default)

From age 70 and a half, the budget's Giving lines are paid straight from the person's
traditional account (the engine keeps one traditional balance per person, so a
401(k) is treated as already rolled into an IRA) up to the annual limit per person: $111,000 for 2026 (IRS Notice
2025-67, the SECURE 2.0 Act indexed amount, $108,000 in 2025), then indexed with prices
(the chained index when `chained_cpi` is on) and rounded to $1,000 (the rounding is an
assumption). The money never enters taxable
income, so it lowers the taxable share of Social Security, the Medicare income-related
surcharge (IRMAA) and state tax, and it counts toward the required distribution, which
the annual plan reduces by what it expects to give. The giving itself, its amounts and
timing, is the budget line unchanged; what changes is where the money comes from. With
the spending options of section 7 on, the month's QCD follows the gift the month
actually pays (giving's seasonal weight, its price wedge, and the unemployment cut on
discretionary lines); the annual plan expects the unseasoned level. With
no Giving line or no traditional account nothing happens (validation warns).

The plan nets the year's expected gifts off the requirement at January prices, so
in a year of high inflation slightly more than the requirement leaves the account.

Measured on 3,000 paths, on against off: dual-income lifetime tax (median) $963.5k to
$948.0k (-1.6%), median terminal net worth $1.726m to $1.747m, success rate unchanged
within noise; the other two examples have no giving and do not change. Not modelled: the one-time $55,000 split-interest
gift allowance, QCDs to donor-advised funds (not allowed), giving above the Giving
lines, and a spouse's separate limit pooled across two IRAs (each person gives from their own).

## 6. Income and careers

A job pays its entered salary (today's dollars) and follows either a flat real
raise or a lifecycle curve fitted to earnings data (Cocco, Gomes and Maenhout's age
polynomials: "typical" is the average of their high-school and college profiles,
"steep" the college profile). Real pay on the typical curve rises 66% from 25 to
its peak at 45 and is 15% below the peak at 65 (22% on the steep curve). A job or
self-employment income that starts in the future starts at the amount entered and
grows from its own start (before the audit the curve ran from the plan's start: a
$50,000 job starting eight years out paid $79,600 in its first full year).

With career risk on: persistent and transitory wage shocks each January
(mean-preserving); a monthly layoff hazard that depends on age, job security and
the recession flag (0.5% a month in expansions and 0.8% in recessions for a typical
job); unemployment spells with a lognormal length (mean 4.8 months, 24% longer than
six months), unemployment insurance (half of pay up to $600 a week for 26 weeks,
39 after a recession), and a pay scar on re-employment. Measured over 30 years from
age 30 at typical risk: lifetime wages average 90% of the no-risk curve (5th
percentile 56%, 95th 135%), nine months unemployed in total, 86% of careers with
at least one layoff.

Bonuses vary with the stock market (correlation 0.5 with the trailing year);
commissions and self-employment profit are lognormal around their entered means
with the entered volatility. RSUs vest monthly after a cliff with refresh grants,
are taxed as wages at vest, and are forfeited when the job ends. Private-company
equity pays off through liquidity events: 44% of grants are worthless after 20
years, 43% pay (0.76 times paper value on average).

Past earnings, needed for Social Security, are rebuilt backwards from today's pay
along the same curve the job follows going forward, capped at each year's wage
base.

### 6.1 Voluntary job changes (opt-in: `risk.job_mobility`)

Off by default. With `typical` or `high` (and the job's risk level not `none`, and
`assumptions.career_risk` on), an employed person changes employer in a month with a
probability that falls with age: 30% a year under 25, 20% at 25 to 34, 12% at 35 to
44, 8% at 45 to 54, 5% at 55 to 64 (`high` is 1.6 times the yearly rate; moves are
40% rarer in recession months). A move multiplies base pay by `exp(J)`, `J` normal
with mean 8% under 35, 5% at 35 to 54, 2% from 55 (4 points less in a recession) and
standard deviation 0.12, and forfeits unvested equity (a sign-on grant at the new
employer is not modelled; the jump is meant to cover it). Employer match vesting is
not modelled by the engine, so there is nothing to reset there.

*No double counting.* The lifecycle curve is fitted to earnings panels in which people
change jobs, so its average growth already includes the average gain from moves.
Every month an employed person does not move, pay is therefore multiplied by
`(1 - h E[e^J]) / (1 - h)` (`h` the month's move probability), which makes the expected
growth of pay equal to the curve exactly: `h E[e^J] + (1 - h) d = 1`. Forfeited equity
is not compensated, so total pay of a job with large grants is lower on average. Movers gain more than stayers, stayers fall behind the curve, and the mean
career is the curve; what changes is the spread between careers. The persistent wage
shock of the existing model is calibrated to the same data and carries some of the
same dispersion, so the two overlap in variance (not in the mean).

*Sources.* The yearly move probabilities are a judgement shaped by Fallick and
Fleischman (Federal Reserve Board FEDS 2004-34: 2.6% of employed people change
employers each month, falling with age and in recessions), FRBSF Economic Letter
2016-30 (SIPP: 1.7 to 1.9% a month at 40 to 44) and the BLS NLSY79 job counts; the
jump size is a judgement shaped by Topel and Ward (1992) and the Atlanta Fed Wage
Growth Tracker (switchers out-earn stayers in expansions, not in recessions). The
constants are listed in `params.json` under `unverified_from_memory.career_realism`.

*Measured* (dual-income-family example, 4,000 paths, seed 1, both jobs on `typical`):
success rate 91.5% either way; median terminal net worth $1.73m off, $1.76m on; 10th
percentile $104k off, $117k on (the Monte Carlo noise on the 10th percentile is of
that order); median retirement age 53.4 off, 53.5 on (90th percentile 61.2 off, 61.8
on); share of paths with a retired year below 90% of planned spending 44.2% off, 42.6%
on. The headline barely moves
because the mean is preserved; the unit tests show the spread of pay at 45 to 50
rising by more than 10%, and workers with five or more moves ending 15 points ahead
of workers with two or fewer.

*Not modelled:* a link between moves and layoffs, moves into self-employment,
relocation, employer-specific benefits (plans, health cover) differing across
employers, moves by people on a career break or in an unemployment spell.

### 6.2 Recession pay freezes and hours cuts (opt-in: `risk.recession_pay`)

Off by default; `typical` needs `assumptions.career_risk` on. The career model already
makes layoffs likelier and longer in recessions; this adds what happens to people who
keep their job, using the path's recession flag. In each recession month an employed
worker may (independently) get a *pay freeze* (25% a year: the raise the job would have
had, the lifecycle curve's real growth at that age plus the path's last-12-month
inflation, is lost for good, at most one a year) or start a *temporary cut* (15% a
year: pay down 5 to 20% for nine months, then restored). Bonuses are 30% smaller in a
recession and for twelve months after it, and carry the freeze, being a share of pay.
Commission follows the pay factor. The nine months of a cut and the year between
freezes run on calendar time, so a cut does not resume in a new job after a layoff.
The whole forgone raise is taken off pay in the month the freeze starts (in reality it
opens up over the frozen year), which overstates the first year's loss by about half a
raise.

*Sources.* The direction is documented (the share of workers with an unchanged annual
wage reached a record in and after the Great Recession: Daly, Hobijn and Lucking, FRBSF
Economic Letter 2012-10; an FRBSF letter of July 2013; an administrative study of
Washington State hourly stayers found 15 to 20% with no change in a year even in normal
times). The sizes are judgements: no source was found for per-recession probabilities
of a freeze or cut, or their depth, for salaried workers, and the percentages in those
papers were not retrieved. All are named constants in `career_realism/recession_pay.rs`
and listed in `params.json` under `unverified_from_memory.career_realism`.

The lifecycle curve is an average over business cycles and so already contains some
recession effect of unknown size; this feature is not mean-preserving and slightly
overstates the loss for that reason. Freezes are permanent level losses (no catch-up,
a judgement); cuts recover fully.

*Measured* (dual-income-family, 4,000 paths, seed 1, both jobs on `typical`): success
rate 91.5% off, 91.0% on; median retirement age 53.4 off, 53.8 on (90th percentile
61.2 off, 62.1 on); median terminal net worth $1.73m off, $1.72m on; 10th percentile
$104k off, $75k on. In the integration test (a 35-year-old on $90,000 with a bonus,
300 paths) pay is identical to the baseline until the first recession month on every
path, never higher afterwards, and lifetime pay is 2.7% lower over all paths (the test
accepts 0.2% to 6%). The same kind of check for 6.1 (a 30-year-old to 50, 1,000 paths,
`typical`) gives lifetime pay 0.6% below the baseline, within the noise of the jumps.

*Not modelled:* a freeze's effect on the nominal-versus-real distinction beyond the
inflation term, work sharing and furlough programmes, sector differences (a freeze is
equally likely for every job), cuts and freezes for the self-employed (see 6.5).

### 6.3 Long-term disability (opt-in: `people[].disability_risk`, `ltd_insurance`)

Off by default. Each month a person who has not retired and is below full retirement
age faces an onset hazard that rises steeply with age (yearly rates per 1,000 working
people: 1.0 at 20, 1.6 at 30, 2.2 at 35, 3.6 at 40, 5.5 at 45, 8 at 50, 12 at 55, 14 at
60). Onset ends all of the person's wage and self-employment income and forfeits
unvested equity. The household then receives:

* **SSDI** after SSA's five-month wait: the person's disability PIA, constant in real
  terms (benefits rise with prices) until full retirement age. The PIA follows SSA's
  disability rule: computation years are the years after the 21st birthday up to the
  year before onset, less one dropout year per five elapsed years (at most five), never
  fewer than two; AIME is the best of those years' indexed earnings over their months
  (SSA POMS RS 00605.016). This is why a 30-year-old with eight years of earnings gets a
  full-earner-sized benefit rather than one divided by 35 years. The claimant's record
  is frozen at that PIA while disabled (later years without earnings do not lower it),
  and at full retirement age the claim age is set to full retirement age so the existing
  retirement code pays exactly that PIA (the SSDI-to-retirement conversion). Not
  insured (basis `none`, under five years of earnings on record, already claiming
  retirement) means no SSDI. Paid through the Social Security income series and taxed
  with it.
* **LTD insurance**, if set: after a three-month elimination period,
  `replacement x base salary at onset`, fixed in nominal dollars (as most policies are),
  less the SSDI received (the usual offset), until `to_age`. Taxable (employer-paid
  premiums assumed; no premium is charged). Only job salary is insured: a
self-employed person's profit earns no LTD. Paid through the other-income series.
* **Recovery** with 3% a year, below full retirement age only (after it the benefit has
  converted for good): jobs resume with a 10% pay scar and the record is released; a
  planned claim age that passed during the disability becomes the next month. A disabled person's jobs sit in an open-ended idle state, so layoffs,
  unemployment insurance and mobility draws cannot occur; the idle months are not
  counted as unemployment in the summary.

*Sources.* Verified: the Trustees' age-sex-adjusted ultimate disabled-worker incidence
of 4.6 awards per 1,000 exposed a year (2025 Trustees Report, Alternative II; 4.8 in 2023
and 4.5 in 2024); in the SSA Annual Statistical Report on the SSDI Program 2024, 16.2%
of disabled-worker awards went to ages 50 to 54, 28.0% to 55 to 59 and 27.4% to 60 to
full retirement age (read through search summaries); the five-month wait; the
computation-year rule. The age-specific incidence tables (OACT) could not be retrieved
(the SSA site refused automated access), so the age profile above is a judgement shaped
to those facts (weighted by the age mix of employed people it averages about 5.2 onsets
per 1,000 a year, some 13% above the Trustees' rate, and puts about 22% of onsets at 55
to 59 and 29% from 60: too few at 55 to 59, too many after 62), as are the recovery
rate, scar, LTD wait and the insured-history rule
(5 recorded years standing in for 20 quarters in 40). All are named constants in
`career_realism/disability.rs` and in `params.json` (`unverified_from_memory`). The
hazard is an SSDI award hazard, so it covers the severe end of work limitation only.

*Measured* (dual-income-family, 3,000 paths, seed 1, both people on `typical`): success
rate 91.8% off, 91.4% on, 91.8% with 60% LTD for both; financially independent on 96.4%
of paths off, 94.6% on; retirement age p90 60.9 off, 62.4 on; terminal net worth p10
$125k off, $111k on, $131k with LTD. The median terminal net worth *rises* ($1.73m to
$1.75m): the household usually reaches independence in its early fifties anyway, and a
person disabled before that loses a few years of wages but draws a full-size SSDI
benefit for up to two decades before the age at which the baseline would claim, which
helps more than the lost wages hurt in the middle of the distribution. The tails move
the intuitive way for the low end (p10, success rate).

*Not modelled:* medical costs and care needs of the disabled, higher mortality of
beneficiaries, Medicare entry after 24 months of SSDI, the denial of claims (the hazard
is awards), partial or short-term disability, the SSDI family benefits for children,
the SSDI earnings-record effect on a spouse's benefits beyond the existing spousal
logic, the planner's knowledge of a disability (the annual plan sees SSDI and LTD
only as they arrive), a cap on LTD, and disability after retirement.

### 6.4 Older workers and involuntary retirement (opt-in: `risk.late_career`)

Off by default; `realistic` needs `assumptions.career_risk` on and a layoff `level`
other than none. The career model already lengthens spells (median 1.4 times from 55)
and deepens re-employment pay losses (1.5 times from 50). For a layoff from 55 on this
adds, on top: a spell 1.3 times longer at 55 to 59, 1.6 times at 60 to 64, 1.8 times from
65 (capped at 48 months), a pay loss 1.3 times larger (only a loss is deepened; the
random part of the career model's scar can be a gain, which is left as drawn), and a
probability of never working again of 15% at 55, rising 4 points a year to at most 60%
(35% at 60, 51% at 64). Such a worker is retired on the spot with an "Involuntary
retirement" event and all their jobs end (their own self-employment, if any, carries on
under its own settings); if nobody else in the household still has a job or
self-employment (a disabled partner, 6.3, does not count) the household retires the next month on what it has, otherwise the
partner keeps working under the plan. The spell multiplier applies to the whole spell
the career model drew. How often layoffs happen is still set by `level`; this changes what follows one.
It uses one extra uniform per job per month from the feature's own stream, whatever
happens.

*Sources.* All the numbers are judgements (named constants in
`career_realism/late_career.rs`, listed in `params.json` under
`unverified_from_memory.career_realism.late_career`). Direction and scale: Johnson and
Gosselin (Urban Institute 2018) as reported by ProPublica ("Forced Out"): in the Health
and Retirement Study 56% of workers over 50 in long-term full-time jobs were pushed out
before they chose to retire and about one in ten of them ever again earned as much as
before. Those figures include exits for poor health and use a six-month or 50%-drop
threshold, so they do not give a per-layoff probability.

*Measured* (dual-income-family with the two adults born in 1969 and 1970, 3,000 paths,
seed 1, both jobs on `late_career: realistic`): success rate 80.9% off, 74.1% on; median
terminal net worth $658k off, $520k on; 10th percentile $0 either way; share of retired
paths with a year below 90% of planned spending 38.2% off, 45.5% on. (This couple is
under-saved for its age; it works to 67 in both runs, so the effect is the lost pay and
the unemployment spells, and a pushed-out person's contributions and wages stop.)

*Not modelled:* unemployment insurance for a worker pushed into retirement (their job
ends in the layoff month, so they collect at most that month's benefit; a real claimant
would draw up to 26 weeks first), health as the cause of the exit (disability is 6.3), a rise in the layoff
hazard itself with age beyond the career model's, partial retirement or bridge jobs
after a layoff, employer retirement incentives and buyouts, and the age at which UI
runs out differing by state.

### 6.5 Self-employment cycles and business failure (opt-in: `business_risk`)

Off by default; needs `assumptions.career_risk` on. The base self-employment model
already redraws a unit-mean log-normal profit factor each January, half persistent and
correlated with the last year's stock return, which gives some co-movement with the
economy. `typical` and `high` add, on top of that, three things:

* **A persistent regime**: a standardised state redrawn each January,
  `z <- 0.7 z + sqrt(0.51) e`, which scales profit by `exp(sd z - sd^2 Var[z] / 2)`
  (unit mean; sd 0.20 typical, 0.35 high), so good and bad years come in runs.
* **Recession months** cut profit by 10% (typical) or 20% (high), using the path's
  recession flag. This is not mean-preserving, on purpose: the base model has no
  recession effect.
* **Failure**: a monthly hazard from a yearly rate by business age: 20.4%, 15%, 12%,
  10%, 9% in years 1 to 5 and 7% after. It is 1.3 times higher in a recession month,
  multiplied by `exp(-0.25 z)` (bad regimes fail more) and, for `high`, by 1.5 (capped at
  95% a year). A business started in the future has age 0 at its start; one already
  running has `years_operating` plus the time since the plan began (so set it, or a
  decades-old practice is treated as a start-up). A failed business pays nothing for
  good and counts as ended for the retirement planner and trigger. With `fallback_wage`
  the person, after `search_months`, earns that wage (today's dollars) as ordinary wages
  with payroll tax until they retire or reach the end date of the self-employment
income, whichever is first, and not while disabled (6.3); no retirement-plan
contributions. A "Business
  failed" event is logged.

*Sources.* Verified: BLS Business Employment Dynamics: 79.6% of the March 1994 cohort of
establishments open after one year (TED 2018), 20.4 points of the 2013 cohort closed in
its first year and 34.7% was open after ten (TED 2024). Hazards for years 2 to 5 and 6
onward are judgements fitted to that ten-year figure (the BLS two- and five-year rates
could not be retrieved); regime, recession and hazard sensitivities are judgements.
Constants are in `career_realism/business.rs` and `params.json`
(`unverified_from_memory.career_realism.business`). BED counts establishments, most with
employees: sole proprietors fail more, which is what `high` is for.

*Measured* (side-hustle-landlord, the consulting income only, `typical`, fallback wage
$50,000 after four months, 3,000 paths, seed 1): success rate 92.2% off, 91.9% on; median
terminal net worth $988k off, $983k on; 10th percentile $158k off, $157k on. The effect is
small because the consulting income starts in 2031 and the household retires at about 47
in most paths, so the business runs only a few years. The test for a 20-year business
shows 66% less lifetime profit than with the feature off (61% from the hazard table
alone, worked out by hand), and a ten-year-old business loses over ten points less.

*Not modelled:* firm size or sector, a partner or employees, selling the business or
winding it down with proceeds, debt or a personal guarantee, restarting a business, a
fallback that is itself self-employment, a retirement plan on the fallback job, and the
retirement planner counting on the fallback wage before a failure.

## 7. Spending

The budget is a list of lines, each essential or discretionary, with its own
frequency, real growth, start and end, and a treatment in retirement (same, stops,
a factor, an amount). On top of it:

* **Lifestyle creep**: a share (default 25%) of each real rise in income above its
  previous high becomes discretionary spending while working.
* **The retirement spending smile** (Blanchett): from 60, real spending drifts down
  by `0.00008 age^2 - 0.0125 age - 0.0066 ln(spending) + 0.546` a year when that is
  negative.
* **Irregular expenses**: about one surprise a year averaging 3% of annual spending
  at the typical level.
* **A survivor** spends 70% of what the couple did (shared lines; default).
* **Relocation** rescales each category by the destination's cost-of-living factor.

"Planned" spending is what spending would be with no spending-rule adjustment and
no shortfall: the budget at a multiple of 1 plus every non-discretionary cost
actually incurred (housing, health net of the premium credit, care, children, debt
service, one-off costs). It has exactly the scope of actual spending, so delivered
and planned can be compared year by year.

### Vehicle replacement cycles

Opt-in through `spending.vehicles`. Without it, car costs are whatever the user put in
a budget line: smooth. Each listed vehicle instead has a price in today's dollars, a
replacement cycle, an age, a trade-in share and optional financing. The first
replacement falls `replace_every_years - age_years` from now (at once if the vehicle is
already older), later ones one cycle after the last, each shifted by a random jitter
(normal, standard deviation six months, clipped at two standard deviations: a
judgement, `REPLACEMENT_JITTER_SD_MONTHS`) so that paths do not all replace in the same
month. The jitter does not accumulate: the schedule is fixed, each date is shifted
alone. A replacement costs `price x (1 - trade_in)` in today's dollars, raised with
the price level; vehicle prices are assumed to track CPI. Paid in cash it is one-off
spending in that month. Financed, the down payment is one-off spending and the rest
becomes a debt slot (the same mechanism as a financed `expense` event, an auto loan
repaid by the ordinary debt payments, with interest; a later loan on the same vehicle
takes back the payment the earlier one released to the extra debt payment). The jitter
draws come from one stream per vehicle, one word per scheduled date whether or not the
vehicle is still being replaced (after its `end` date or its owner's death the schedule
runs on and nothing is bought), so a scenario that changes anything else sees the same
replacement dates.

The funded test counts the vehicles: each one still being replaced adds its net price
spread over its cycle to the annual need (two cars at $38,000 every ten years and
$32,000 every nine with a 25% trade-in: $5,517 a year, $138,000 of savings at a 4%
rule), as a home's running costs do. Without that a household that retires when funded
would stop work with the next cars unpaid for. Loan interest is not counted there.

What the user must do: take the purchase and depreciation of these vehicles out of the
Transport budget lines (keep fuel, insurance, maintenance). The validator warns when
the average annual cost of the listed vehicles is more than half of the Transport lines.

Measured (shipped `dual-income-family`, 3,000 paired paths, two cars: $38,000 every ten
years, 4 years old; $32,000 every nine years, 6 years old, financed 90% at 7% over 60
months; 25% trade-in; the existing transport line left as it is, so this is extra
cost): the household retires when funded, so it pays for the cars by working longer,
not by failing more often. Median retirement age 53.4 to 55.2 (+1.75 years), success
91.8% to 91.9% (+0.1 points, 95% interval -0.5 to +0.7, within noise; 43 paths rescued,
39 harmed), median terminal net worth $1.73 million to $1.79 million, median lifetime
spending +$324,000 (today's dollars: the cars, and nearly two more working years at the
working budget). With the retirement age fixed at 54 instead, the same cars cost 9.7
points of success (77.1% to 67.4%, 95% interval -10.8 to -8.6; no path rescued, 291
harmed) and $524,000 of median terminal net worth.

Not modelled: depreciation and resale value beyond the trade-in share, vehicle prices
moving differently from CPI, repair costs rising with vehicle age (the detailed
shocks scale car repairs with the vehicles' cost but not their age), buying a car out
of a loan balance still owed on the old one (a second loan simply adds to the first
and the payment is recomputed), leasing.

### Seasonal spending

Opt-in through `spending.seasonality = "typical"`. Each category has twelve monthly
weights, rescaled in code to average exactly 1, so a line's annual amount is unchanged
and only its timing moves (utilities peak in winter and summer, dining and travel in
summer, gifts and giving in December, flat categories such as rent stay even). Four
categories are fitted to monthly, not seasonally adjusted national series, each month
averaged over its calendar year's mean for 2010 to 2019 by
`tools/calibrate_seasonality.py`: utilities to the Federal Reserve's G.17 output index
of electric and gas utilities (January 1.16, April 0.87, July 1.10), groceries, dining
and transport to US Census Bureau Monthly Retail Trade sales of grocery stores
(December 1.06, February 0.91), food services and drinking places (January 0.91, May
1.05) and gasoline stations (February 0.86, May 1.08), all retrieved from FRED. Each is
a proxy for a household's line (energy delivered to all customers, fuel as a part of
transport). Travel, education, entertainment, personal and giving are judgements, not
fitted to data: the BLS Consumer Expenditure Survey publishes no monthly table (see
`spending_realism.seasonality` in `params.json`). Seasonality acts on the monthly
budget, so it matters for the month a cash shortfall or a withdrawal appears, not for
the year's spending.

Measured (shipped `dual-income-family`, 3,000 paired paths): success 91.8% to 91.8%
(6 paths rescued, 5 harmed), median retirement age unchanged, median terminal net worth
+$2,200 (0.1%), median lifetime spending -$4,700 (0.06%). As expected, it is a timing
feature: the differences are the noise of which month a withdrawal falls in.

Not modelled: seasonal income, regional or household-specific patterns, weights that
vary with age or children.

### Personal inflation by category

Opt-in through `spending.category_inflation = "stochastic"`. Lines in the food, energy
(utilities and transport), shelter (rent), medical and education groups carry a
persistent price wedge against CPI: the log of the category's relative price level is an
AR(1), `x_k = phi x_(k-1) + sigma z_k`, stepped each January from zero today, and the
multiplier is `exp(x_k - Var(x_k)/2)`, so its expectation is exactly 1 in every year
(it adds risk, not drift). `phi` and `sigma` are fitted to annual averages 1980 to 2024
of the BLS CPI-U sub-indexes relative to all items (retrieved from FRED) by
`tools/calibrate_category_inflation.py`; `phi` is capped at 0.95, a judgement, because
the medical and education ratios are unit roots in this sample. Each group uses its own
standard normal draws from a new stream (`CategoryPrices`) with a fixed number of draws
per year, so paired comparisons stay paired. The category's long-run real growth stays
separate. The draws are not truncated (the limit of 8.5 sd in the code is beyond what the
generator can produce and only guards its last value). The wedge acts on the budget
lines; the modelled healthcare costs, children's costs and owned-home costs keep their
own price paths, and so do rent lines that follow the market rent index
(`rent_growth: market`, below), which already moves against CPI.

Measured (shipped `dual-income-family`, 3,000 paired paths): success 91.8% to 91.5%
(-0.3 points, 95% interval -0.6 to +0.1, within noise; 11 paths rescued, 19 harmed),
median terminal net worth -$2,600, 10th percentile -$10,400, median lifetime spending
-$2,400. The medians barely move because the wedge is mean-preserving; what it adds is
risk in the paths where the household's own prices run ahead of CPI for years.

Not modelled: correlation of the wedges with each other or with CPI and market returns,
and a wedge that differs by region.

### Richer irregular expenses

Opt-in through `spending.shocks = "detailed"`. Four independent compound-Poisson shock
types fire monthly: home repair (scaled with the value of owned primary homes: 0.4 a
year, median 0.5% of value), car repair (scaled with Transport spending plus the
vehicles' annual cost: 0.4 a year, median 8%), medical-adjacent (0.16 a year, median 2%
of annual spending) and family emergency (0.12 a year, median 3%). Sizes are lognormal
(sigma 0.9 to 1.2, capped at 3.5 sd), so the tail is heavy. A shared excitement term
rises with every shock and halves every 12 months, so a bad year raises the hazard of
another (Hawkes-style clustering), scaled so the long-run frequency is unchanged.
Medical frequency and size are anchored on the JP Morgan Chase Institute (2017) figures
used for the other levels (16% of families a year, median $1,143); the car repair size on
the same source ($953 median); home repair, family emergency and the clustering are
judgements (see `spending_realism.detailed_shocks`).

Like the other levels, the detailed level adds variance, not cost. The expected annual
cost of the car, medical and family types at the start is netted out of the budget.
Home repairs cannot be netted there, because owned-home costs are computed from the
property: their expected cost (0.4 x 0.5% x exp(1.1^2 / 2) = 0.366% of the home's value
a year) is taken out of the `maintenance_rate` paid on an owner-occupied home, not
below zero (the validator warns when the rate is too low to absorb it). The household
still plans with the full maintenance rate: the funded test and the annual budget count
the repairs as an expected cost of the home, as they did before the repairs became
random. Whether a type fires, and the excitement it adds, depend on the random draws
alone: a household with no home still has its "home repair" months, they cost nothing,
so every other shock keeps its date and its frequency whatever the household owns.
The excitement starts at its long-run mean, so the frequency is the calibrated one
from the first month.

Measured (shipped `dual-income-family`, which buys a home, 3,000 paired paths, against
the `typical` level it ships with): success 91.8% to 91.4% (-0.3 points, 95% interval
-0.7 to +0.1, within noise; 14 paths rescued, 24 harmed), median retirement age
unchanged, median terminal net worth +$29,000, 10th percentile +$12,000, median
lifetime spending -$8,100. Against shocks off, in a test with the retirement age fixed
(1,500 paths), mean lifetime spending on the budget and housing falls by $194,000 and
the shocks drawn cost $178,000: mean-neutral to within 8%, the shocks costing slightly
less than what is netted (the 3.5 sd cap, the medical and family types being sized from
the budget after netting, and the netted share being fixed at the start). The typical level nets 2.1% of the
budget lines; the detailed one nets about as much for this household ($52,000 over the
plan in both cases) plus the home repairs out of maintenance, so the two differ mostly
in shape: a heavier tail, repairs that scale with the home, and bad years that come
together. The medical-adjacent type sits beside the healthcare module's own
out-of-pocket costs; being netted from the budget it adds variance there, not level.

Not modelled: shocks that depend on the age of the home or car, insurance
reimbursement, and shocks to rental property (landlord repairs use the property's own
maintenance rate).

### Belt-tightening when unemployed

Opt-in through `spending.unemployment_cut` (0 by default). While an earner is in an
unemployment spell, discretionary spending is multiplied by `1 - cut`, phasing in
linearly over three months and unwinding over three months after re-employment (the
phase length is a judgement, `UNEMPLOYMENT_CUT_PHASE_MONTHS`). Essential spending is
untouched. Suggested value 0.15: Ganong and Noel (2019, AER) find nondurable spending
falls about 6% at the onset of unemployment and further when benefits end; with
discretionary lines about half the budget, 6% to 12% of total spending is 0.1 to 0.25 of
them. The trace logs when the cut starts and when it lifts.

Measured (shipped `dual-income-family`, cut 0.15, 3,000 paired paths): success 91.8% to
91.5% (-0.2 points, 95% interval -0.4 to 0.0; 2 paths rescued, 9 harmed), median
terminal net worth +$5,800, 10th percentile +$9,000, median lifetime spending -$7,700.
Spells are short with the typical career risk, so the effect is small; it grows with a
high layoff risk and a thin cushion. The cut does not raise success here: this
household retires when funded and spends by guardrails, so the money saved in a spell
changes later decisions (a retirement date, a guardrail raise) and a few marginal paths
end on the other side. With the retirement age fixed at 54 the same comparison gives 7
rescued and 7 harmed; in a test household with a fixed budget and retirement age no path
ends poorer.

Not modelled: a cut that deepens when benefits run out, durable goods deferred during
a spell, and a cut for a partner's spell that differs from the earner's own.

### Children

USDA expenditure estimates by age band and income tier (2015 dollars brought to
today), with the only-child and three-plus adjustments, childcare for ages 0 to 4
(or the amount entered) and college at the chosen type and share. The tier is
chosen from earned income at the start or, when that is lower, from the budget
divided by 0.8 (a retired household used to get the lowest tier whatever it spent).
In the funded test, child costs still to come are a sum set aside, not a permanent
cost.

### Uncertain arrival of planned children (opt-in)

A child whose birth month is in the future can carry
`arrival: {"type": "uncertain", "probability", "delay_mean_months", "twins_probability"}`
(default `certain`). On each path three uniforms from the child's own stream
(`ChildArrival(index)`, drawn whatever the values) decide whether the child arrives
(probability, default 0.85), how many whole months after the stated month (an
exponential with the given mean, default 4) and whether it is twins (default 3%).
Costs, the child tax credit and dependants, family size in the USDA adjustment, college,
the age at which a child leaves the health plan and the events in the trace all follow
the realised birth; a twin birth incurs every per-child cost twice (parental leave is
taken once).

Sources: twin births 30.1 per 1,000 births in 2024 (CDC NCHS, Health E-Stat "A Decade of
Decline in Twin Childbearing in the United States, 2014-2024"). Time to pregnancy:
reviews of time-to-pregnancy studies find 78 to 92% of couples conceive within a year
(monthly fecundability about 0.2 below 35, falling toward 0.05 at 40). The default
arrival probability and mean delay are judgements near those figures, named constants in
`model/family.rs` and listed under `unverified_from_memory.family` in `params.json`
(a unit test in `sim/family.rs` keeps the two equal).

The funded test (the retirement trigger) plans on the stated birth of one child until
the stated month; from then on it sets money aside for what happened on the path:
nothing for a child who did not arrive, the later birth, twice for twins.

Measured on `dual-income-family` with the second (planned) child uncertain at the
defaults (3,000 paired paths, seed 7): success 92.2% to 92.5% (within noise), median
terminal net worth up $60,000, median lifetime spending down $53,000, median retirement
five months earlier (the 15% of paths with one child are funded sooner).

Not modelled: a household that is still waiting for a late child is treated as knowing
the child will come (the funded test does not allow for the chance that it will not);
births are independent
of the parents' ages and health, and a late arrival does not change leave or career
plans; no adoption, fertility-treatment costs or pregnancy medical costs; higher-order
multiples.

### College cost uncertainty (opt-in)

`college_cost: {"type": "stochastic", "attend_probability", "real_tuition_growth",
"tuition_volatility", "aid_scale"}` on a child with a college plan (default `fixed`:
four years growing at CPI plus one point). The parents' annual bill for each year enrolled
is `college cost x college_share x tuition index x (1 - aid discount)`.

* **Tuition index.** The log of real growth above CPI is its mean (default 1%, the
  engine's fixed default, between +3% a year from the 1980s to the mid-2010s and a
  real fall of 0.7% a year at public four-year colleges and a rise of 0.2% at private
  nonprofits over 2015-16 to 2025-26: College Board, Ma, Pender and Hu, Trends in
  College Pricing and Student Aid 2025) plus a stationary AR(1) deviation with
  coefficient 0.8 and standard deviation 1.2 points. Both persistence and spread are
  judgements chosen so that decade averages range over the recent and the long-run
  record. One path of 60 standard normals (stream `CollegePrices`, scalar 1200) is
  shared by all children, so siblings face the same prices.
* **Aid.** A discount on the published price that falls with household income, linearly
  between 57% at $50,000 or less, 31% at $125,000 and 21% at $200,000 or more, plus a
  per-child normal shock with standard deviation 10 points (judgement), clamped to
  0..90%. The schedule is the average of the two sectors in Levine, "Ignore the sticker
  price" (Brookings 2024, NPSAS 2019-20, 2023 dollars): public four-year in-state,
  published cost of attendance $35,000, net price $18,000 below $50,000 of income (49%
  off) and at most $29,000, reached at about $125,000 (17% off); private nonprofit,
  published $72,000, net $25,000 below $50,000 (65% off) and at most $53,000, reached at
  about $200,000 (26% off). Averaging the sectors and reading the 2023-dollar incomes as
  today's dollars are judgements. Income is the household's MAGI two tax years earlier
  (the FAFSA's prior-prior year), in today's dollars, plus 12% of its cash and taxable
  investments today: the asset conversion rate for parents in the federal Student Aid
  Index formula (Higher Education Act section 475; the Department of Education's 2026-27
  SAI guide, with an asset protection allowance of zero), which leaves out retirement
  accounts and the family home. So a household that retires early on a taxable portfolio
  is not given the aid of a low-income family. `aid_scale` multiplies the whole
  discount. The discount is applied after `college_share`: a share below 1 that already
  stands for aid counts aid twice, and validation warns (set `aid_scale` to 0).
* **Duration and attendance.** The child attends with probability 0.95 (judgement) and
  takes 4, 5 or 6 years with probability 0.68, 0.25 and 0.07, the shares among graduates
  in NCES data (first-time, full-time bachelor's students who started in fall 2010: 41%
  graduated within four years, 56% within five, 60% within six; Status and Trends in the
  Education of Racial and Ethnic Groups, indicator 23). The mean is 4.39 years, 10% more
  than four. Draws come from the child's `ChildCollege` stream (three per path).
  Dependants, the last month of child costs and the trace follow the realised years.

The mean of the tuition index is not exactly the fixed path: the random part is in logs,
so the expected price 15 years out is about 0.7% above `(1 + growth)^15`.

Measured on `dual-income-family` with both children stochastic at the defaults (3,000
paired paths, seed 7): success 92.2% to 93.1%, median terminal net worth up $119,000.
The direction comes from aid, not from uncertainty: the fixed plan charges the published
price, and the schedule takes at least 21% off it even at high incomes (the discounts
on a dozen traced paths ranged from 3% to 51% and averaged about 25%), on top of the
example's 75% parental share, and the saving compounds over the rest of the plan. With
`aid_scale: 0` the same run gives success unchanged at 92.2% and median terminal net
worth down $8,000, the cost of the longer average stay and of price risk.

Not modelled: the asset test is the federal formula's conversion rate only (no CSS
Profile, which also counts home equity; rental property and business equity are left
out; the conversion is added to income rather than run through the formula's
allowances and assessment brackets), one schedule serves public and private colleges,
merit aid is only in the average, students who leave without a degree, loans and 529
plans are not modelled, the funded test sets aside the fixed four-year cost, and tuition
shocks are independent of inflation and markets.

### Adult children who need support (opt-in)

`adult_support: {"probability", "annual", "mean_years"}` on a child (default absent: none).
With probability 0.3 (judgement; 18% of 25 to 34 year olds lived in a parent's home in
2023, Pew Research Center 2025 from Census data, and more do at some point) the child
needs support for one spell. The spell starts at a uniformly random month in the six years
after school ends (18 plus the realised college years under stochastic college costs,
otherwise 22, also for a child with no college plan) and
lasts an exponential number of months with the given mean (default 2 years, a judgement),
at least one month. Support is `annual` (default $12,000 in today's dollars, a judgement)
for each child of the birth (twins need it together) and is added to the child spending
of those months, so it flows into the existing child-cost ledger category and year
series. The probability, amount, mean length and six-year window are listed under
`unverified_from_memory.family` in `params.json`. Three draws per child from stream
`ChildSupport` (0x1280_0000 + 2i + 1, sharing the base with `ChildCollege` on the
even ids), always taken. The trace shows when a spell starts and
ends.

Measured on `dual-income-family` with both children at the defaults (3,000 paired paths,
seed 7): success unchanged at 92.2%, median terminal net worth down $14,000, the tenth
percentile down $16,000. The effect is small because the household's wealth is large
relative to about $14,000 of expected extra spending (2 children x 0.3 x $12,000 x 2
years).

Not modelled: support does not depend on the household's wealth or the child's income,
only one spell per child, the funded test does not set money aside for it, and no
grandchildren.

### Parental leave (opt-in)

`parental_leave: {"person", "months", "pay_fraction"}` on a child (default absent: none),
and `second_parental_leave` with the same fields for the other parent's leave at the
same birth (validation rejects the same person twice). There is no household-level
setting: leave is entered per child.
When the child is born during the plan, in the month of the realised birth (so a late,
absent or twin birth under uncertain arrival follows the draw; twins are one leave), the
named person's employed jobs go onto the engine's existing career break for `months`
(default 3, the length of the 12 weeks of unpaid job-protected leave of the FMLA, US
Department of Labor) at `pay_fraction` of pay (default 0.6, a judgement between unpaid
FMLA leave and the 60% to 100% many employer and disability plans replace). Wages, bonus
and commission fall to that fraction; payroll deferrals, the employer match, HSA payroll
contributions, FICA, the wage tax base and the Social Security earnings record are all
computed from the pay actually received. Equity keeps vesting and the job cannot be lost
during the leave (the career-break rules). A person who is not employed in the birth
month (unemployed, retired, not yet started) has nothing to take leave from. The trace
shows "Parental leave for ...". No new random draws are taken.

Measured on `dual-income-family` with unpaid three-month leave for the first person at
the second child's birth (3,000 paired paths, seed 7): about $32,500 of wages lost per
path; median terminal net worth down $14,000 (1,740,552 to 1,726,222), success 92.2% to
92.4%, which is inside the 95% interval and is a change of who retires when, not a gain
(the leave lowers early savings, so some paths retire a little later). With both parents on
three months at 60% of pay (`parental_leave` and `second_parental_leave`): median
terminal net worth down $8,000, success 92.2% to 92.3%, again within noise.

Not modelled: state paid family leave programmes (enter their effect in `pay_fraction`),
leave for adoption or for children already born, leave taken in separate blocks or
starting after the birth month (both parents' leaves start in the birth month; use a
career-break event for a later block), loss of promotion or wage growth after leave,
and leave before the job begins.

### Healthcare

With `modeled` healthcare each adult is, month by month, on an employer plan, COBRA
(18 months), the ACA marketplace, Medicare (from 65) or a custom amount. Premiums
and out-of-pocket costs by phase, per adult per year before excess inflation:
employer $2,565 to $3,465; COBRA $10,800 to $11,700; ACA silver unsubsidised $8,292
at 30, $12,989 at 50, $19,615 at 60, $21,503 at 64 (the CMS age curve on a $703 a
month benchmark at 40); Medicare with Part B, Part D, Medigap G and out-of-pocket
$7,318 at 65 rising 1.5% a year with age to $10,618 at 90. Out-of-pocket costs are
random around their means with a persistent component.

Lifetime Medicare-phase costs from 65 for today's average 65-year-old: mean $189k
for a man and $223k for a woman (Fidelity's comparable estimate: $185,500). With
the default good-health setting about 18% more (longer lives); for a 35-year-old
about twice as much in today's dollars, because 1.5% excess inflation compounds for
30 more years.

The marketplace credit is part of the plan, not an afterthought. The year's
withdrawal plan and the credit it earns are solved together (plan, estimate income,
compute the credit, plan again if it changed), so the budget is health cost net of
the credit; the HSA pays only what an HSA may pay (out-of-pocket costs, Medicare
premiums, care; not marketplace premiums). Before the audit the plan budgeted
insurance at full price: households withdrew from retirement accounts and re-saved
a median $210,000 in taxable accounts before 65, and the HSA paid "medical" bills
of $177,000 more than was spent on health.

### Children on the health plan (opt-in)

`spending.healthcare.cover_children` (default false, with
`children_coverage_end_age`, default 26, the ACA dependent-coverage age) charges
each child the household's coverage phase. It applies only to `modeled` healthcare.

* **Employer plan** (a parent has employer cover): the household pays the family tier
  in place of a single tier for each adult, $7,471 against $1,765 a year (KFF
  Employer Health Benefits Survey 2026). The adults are already charged a single tier
  each, so the increment is the family tier less those: $475 a month for one adult on
  the plan, $328 a month for a couple, charged once however many children are covered.
  A single parent is charged the family tier too (KFF does not publish the cheaper
  employee-plus-children tier), which overstates their cost.
* **COBRA**: the same with the full price of the plans, $28,303 against $9,804, with
  the 2% load.
* **ACA marketplace** (a parent is on it): the benchmark premium at the child's age on
  the CMS age curve (0.765 of the age-21 price to 14, 1.0 at 21), times the metal-tier
  factor, for at most the three oldest children under 21 (the rating rule, 45 CFR
  147.102(c)(1)); older dependants up to the end age are charged in full. These
  premiums join the advance credit, and their benchmark joins the benchmark premium
  of the credit calculation and of the January plan, so a covered child raises the
  credit and the credit cliff the planner steers under.
* Adults on Medicare: the children are charged an unsubsidised marketplace plan and are
  not counted in the credit. Households on a custom early-retirement amount or abroad
  are not charged (the custom amount is the household's own figure).
* **Out-of-pocket**: the mean of the age band for the plan (MEPS: $400 a year under 18,
  $800 from 18; bronze and gold scaling as for adults), as an expected value: children
  draw no random numbers.
* The health-care column of the USDA child cost (mean $1,645 a year for the
  high-income tier in 2015 dollars) is removed from the modelled core child cost when
  the switch is on, so health is not counted twice.

Measured on `dual-income-family` (3,000 paths, seed 7, paired): success 92.2% to
91.4%, median terminal net worth down $110,000 (family-tier premiums, out-of-pocket
costs and marketplace premiums over the children's years to 26, less the USDA health
share removed from the child costs to 18).

Not modelled: the funded test (the retirement trigger) does not include the children's
premiums in its health projection; the tax value of pre-tax employer premiums is
ignored for adults and children alike; state children's programs (CHIP, Medicaid) are
not modelled; a dependant over 18 who is covered is not a tax dependant after 23.

### Long-term care

Each person may spend the end of their life in paid care. The probability depends
on sex and age at death; the episode is scheduled backwards from the death month,
with a type (home care, assisted living, nursing home) and a lognormal length.
Over 200,000 simulated lives on the 2023 period table: 38% of men and 55% of women
use paid care (HHS/ASPE: 38 and 55); 15% and 30% for more than two years; mean
lifetime cost $74k for men and $150k for women in 2025 dollars. With default
health and excess inflation, from 65: mean $114k for men, $227k for women, 95th
percentile $618k and $997k.

Care is the dominant cause of failure for well-funded retirees: on the family
example, switching it off halves depletion risk. Medicaid is not modelled unless
switched on (below); "health" as a failure cause means private-pay care exhausted
the portfolio. A care episode
under way is in the year's plan, and the spending rules do not raise spending
during one.

### Health states (opt-in)

By default medical costs, care and the age at death are drawn independently.
With `assumptions.health_model: states` each adult moves once a year between
three states: healthy, chronic (two or more chronic conditions) and frail (the
Fried phenotype). From age 50 the yearly chance of becoming chronic is
`logistic(-4.155 + 0.050 (age - 50))` (1.5% at 50, 4.1% at 70); chronic to frail
is `logistic(-2.743 + 0.061 (age - 80))` (3.4% at 70, 6.0% at 80, 10.6% at 90);
healthy to frail directly a quarter of that; a frail person recovers to chronic
with 10% a year; chronic is permanent. The four coefficients were fitted so that
the chain, run from 18 on the 2023 period table, reproduces US prevalence: two or
more chronic conditions in 6.9% / 30.5% / 67.1% of adults at 18-44 / 45-64 / 65+
(NHIS 2018, Boersma et al. 2020: 6.7 / 33.0 / 63.7) and frailty in 8.3% at 65-69,
17.0% at 65+ and 34.7% at 90+ (NHATS 2011, Bandeen-Roche et al. 2015: 9 / 15 /
38). The direct healthy-to-frail share and the recovery rate are judgements.

The state multiplies three things, each normalised so that the population
average is unchanged:

* **Mortality**: hazard ratios 1.73 for chronic (Nunes et al. 2016 meta-analysis,
  two or more morbidities) and 2.79 for frail (Crow et al. 2018, NHANES
  1999-2004, frail against robust),
  relative to healthy at the same age, scaled every year so that the hazard
  averaged over the survivors' states equals the person's own life-table rate
  exactly. Deaths are therefore not redrawn: the age at death comes from the
  person's longevity draw as before, and the health path is drawn *given* it
  (forward filtering, backward sampling; exact for the joint model). Life
  expectancy is unchanged to the bit and a frail year is far more likely just
  before death.
* **Out-of-pocket costs**: 1.91 times for chronic (AHRQ MEPS Statistical Brief
  498, elderly with supplemental coverage: $1,685 against $883) and 2.79 for
  frail (that ratio times the severe-disability ratio of Park and Stimpson 2024,
  a judgement), divided by their average among the living of that age.
* **Paid long-term care**: the episode probability is multiplied by 0.5 / 1 / 2
  for the state at death (a judgement), divided by its average over the states
  of the people dying at that age, so the ASPE incidence targets still hold.

Measured (unit and simulation tests): every death age is identical with the
feature on; the share of 70-year-old women using paid care and its mean cost move
by less than sampling noise; for a 70-year-old man, health spending in the last
two years of life rises by about $26 a month (premiums included) and by $1 a month
before. On the dual-income example (4,000 paired paths): success 91.5% against
91.4%, depletion 8.5% against 8.6%, both within noise; median terminal net worth
+$16k. The feature redistributes cost toward the end of life rather than adding
it.

Not modelled: states before 18 or a state reported by the user (the starting
state is drawn from the prevalence at the person's age, whatever the
self-assessed `health`, which still scales mortality as before); health affecting
work, earnings or the spending budget; a correlation of the two partners' states;
the year's planned spending uses the average out-of-pocket cost, not the state's.

### Medicaid for long-term care (opt-in)

With `spending.healthcare.medicaid: true` Medicaid is the payer of last resort
for the care episodes above, with the national default rules of 2026 (CMS SSI
and spousal impoverishment standards; 42 U.S.C. 1396a(q), 1396r-5 and 1396p):

* **Spend-down.** Care is paid privately while countable assets (every account,
  plus the equity in property other than the home) are above the $2,000 asset
  limit, a dollar figure fixed since 1989 and not indexed. In the month they no
  longer cover the cost above the limit, Medicaid starts paying (the month's
  income counts toward the liability below, not also as an asset). A single person
  with home equity above $752,000 is not eligible; the home is otherwise exempt.
* **Patient liability.** The person then pays all of their own income (Social
  Security and the pensions, annuities and other streams they own) less a
  personal needs allowance of $60 a month in a nursing home (a judgement: the
  federal minimum is $30 and most states set more) or the special income level
  ($2,982, 300% of the SSI benefit) for care at home or in assisted living, less
  their own premiums and out-of-pocket costs. Medicaid pays the rest of the cost.
* **Spousal impoverishment.** With a spouse at home, half of the countable
  assets in the first month of care (the snapshot) is protected, at least
  $32,532 and at most $162,660 (the 2026 community spouse resource allowance),
  and the spouse keeps enough of the patient's income to bring their own up to
  $2,643.75 a month (the minimum maintenance needs allowance of July 2025 to June
  2026). Indexed figures rise with the path's prices.
* **Estate recovery.** When the path ends with the deaths of the recipient and
  any spouse, the state recovers what Medicaid paid, up to the estate, which
  lowers the estate tested against the bequest target. A path that ends with
  someone alive at the horizon has nothing recovered yet.

A month in which Medicaid pays for a nursing-home resident with no spouse at
home is covered by the safety net and is not counted as depletion: the nursing
home provides room and board and the person lives on the personal needs
allowance, so the household budget the engine still charges is not a real need.
Medicaid paying for care at home or in assisted living does not cover rent and
food, so a shortfall there is still depletion. Such paths are reported on their own (`medicaid_rate` in the
headline, `medicaid_age` per path, and "medicaid" trace events for the start of
coverage and for the estate recovery), as `home_sale_rate` is for a home sold.
A spouse at home who runs out is still depleted. Care that Medicaid pays is not
in `long_term_care_cost`, which is what the household paid.

Measured (simulation tests): for a widow of 78 with $150,000 and Social Security
covering her budget (3,000 paths), depletion falls from 34% to 15% of paths and
Medicaid pays on 32%; paths on which it never pays are identical bit for bit;
every Medicaid path that ends with her death has its cost recovered from her
home. Most of the remaining depletion is care at home: Medicaid pays the care
but her Social Security, less her Medicare premiums and out-of-pocket costs
(which Medicaid would partly cover in reality, not modelled), falls short of her
budget. For a couple, Medicaid begins with at least the protected allowance
still held. On the dual-income example (4,000 paired paths) success rises from
91.5% to 97.5% (95% interval of the change +5.3 to +6.8 points) and depletion
falls from 8.5% to 2.5%; Medicaid pays on 14.4% of paths, more than were
depleted without it, because a couple qualifies while the spouse at home keeps
up to $162,660.

Not modelled: state rules (asset limits, income caps, medically needy
spend-down, more generous allowances, which settings a state's waivers pay for:
here every care type is covered), the five-year look-back on gifts and Medicaid
planning (trusts, annuities, spending on exempt assets), the higher maximum
maintenance allowance by court order or excess shelter costs, Medicaid paying
Medicare premiums and cost-sharing (the person's own health costs are deducted
from the liability instead), the spouse's wages in the income test, recovery
liens during life, and waiting lists for home and community-based services.

### Long-term-care insurance (opt-in)

`ltc_insurance` on a person adds a policy with the usual terms: a level annual
premium (fixed in dollars of the plan's start), a daily benefit, a benefit pool of
daily benefit x 365 x benefit years, an elimination period in days of paid care
(rounded up to whole months) and an optional compound inflation rider that grows
the daily benefit and the unused pool each year. Premiums are paid monthly as
healthcare spending until the person's care episode begins, then waived. From the
month after the elimination period the policy pays the cost of care up to the
monthly benefit (daily x 365 / 12) until the pool is used up; the benefit pays the
provider, so the household's long-term-care spending falls by it (benefits of a
tax-qualified policy are not income, IRC 7702B; premiums are not deducted).
Medicaid, when on, pays only what the policy does not.

**Premium increases.** Premiums of a policy class are not guaranteed. With
`premium_increase_risk` on, each policy year the class (one for the household)
gets an approved increase with probability 8%, uniform between 10% and 30%,
drawn from the new `Stream::LtcPremiumRates` (two uniforms a year, always). These
are judgements calibrated to the record: an average cumulative increase of 31%
since 2001 on policies sold under the NAIC rate-stability rules and 55% on older
ones (LTC Shop data reported by InvestmentNews, 2018); the model gives a geometric-mean
cumulative factor of about 1.28 over 17 years (a unit test).

**Lapse.** At each policy anniversary, before a claim, a policy lapses (no more
premiums, no benefits, no non-forfeiture value) if the household was depleted in
any month of the past year, or if the year's premium is above 10% of its
resources: the past year's wages, self-employment income, Social Security and
taxable pensions plus 4% of its investable assets. Both thresholds are
judgements.

Measured (simulation tests): for a woman of 70 with a $3,000 policy paying $200 a
day for three years after 90 days (3,000 paired paths), the deaths are identical,
the care the household pays never rises on any path and its mean falls by between
20% and 80%, premiums add roughly $3,000 a year of remaining life, most paths see
a premium increase, and a widow with $20,000 and $1,500 a month whose $5,000
premium is unaffordable lapses at the first anniversary on every path without an
earlier claim. On the dual-income example with a $1,500 policy each ($200 a day,
three years, 90 days, 3% compound rider) bought at the start (4,000 paired
paths): success rises from 91.5% to 93.0% (95% interval of the change +1.1 to
+2.1 points), depletion falls from 8.5% to 7.0%, median retirement age is 0.3
years later (the premiums are budgeted), and the 10th percentile of terminal net
worth rises by $142k; with Medicaid also on, success is 98.1%.

Not modelled: premiums that depend on the age and health at purchase (the user
enters the premium), underwriting and buying a policy later in the plan,
non-forfeiture and reduced-benefit options offered at a rate increase, hybrid
life/LTC policies, partnership policies' Medicaid asset protection, shared pools
for couples, benefits that depend on the care setting, a restoration of benefits,
and the tax deduction of premiums.

## 8. Housing and debt

A property has a value that follows the national index times a local random walk
(4% a year by default at the default idiosyncratic setting) plus any stated
relative appreciation; a mortgage amortised monthly at a fixed rate; property tax
(the state's average effective rate unless given), insurance, maintenance, HOA and
PMI below 20% equity. A purchase waits until cash and taxable savings cover the
down payment and closing costs (retrying monthly, abandoned after five years). A
sale pays selling costs, repays the mortgage, and puts the gain on the return (the
$250,000 / $500,000 exclusion for a home; depreciation recapture and released
passive losses for a rental). An event dated in January lands on that year's
return (it used to land on the previous year's).

**Refinancing.** Each January, if `debt_policy.refinance_mortgages` is on (the
default), a mortgage is refinanced at the market rate over its remaining term when
the market is at least one point below the loan's rate *and* four years of interest
saved exceed the closing costs (2% of the balance plus $2,000), which are paid.
With rates now starting from today's level, a household with a mortgage at today's
rate refinances in the first January on about 1% of paths (54% before the audit)
and within five years on about a quarter.

**Home equity.** A household does not starve in a paid-for house. With
`retirement.home_equity: last_resort` (the default), when savings are gone and an
essential would have to be cut, property is sold: vacation homes first, then
rentals, then the home, at 6% selling costs with the gain taxed as usual. After
selling its home the household rents at 4% of the sale price a year (a judgement:
gross yields are 5 to 7% and a household in this position moves somewhere cheaper).
Such a path is not "depleted"; it is reported on its own as `home_sale_rate`.
Before the audit, 16.7% of all paths of the family example were scored as failed
while holding more than $100,000 of home equity (median $926,000), and owning cut
the success rate by nine points against renting. After: owning 91.2%, renting
86.9% on the same paths, with a home sold as a last resort on 24% of paths. Read
the success rate together with that number: `home_equity: never` restores the old
rule (76.3%).

Other debts are amortised with their payments; extra payments follow the avalanche
or snowball order.

### 8.1 Housing realism options (all opt-in)

Every option below defaults to the earlier behaviour; a profile that does not
mention them simulates bit for bit as before and serialises without the new keys.
Their numbers are in `data/actuarial/params.json` under `housing_realism`, each
marked sourced or a judgement (most are judgements). Each option draws from its own
random stream, a fixed number of words a month whatever the household decides, so
switching one on leaves markets, careers, health and lifespans path for path the
same. Effects were measured on the side-hustle landlord example (a renter with a
mortgaged rental duplex), one option at a time, 4,000 paired paths, seed 7;
baseline success 91.7%, median terminal net worth $987,000, 10th percentile
$146,000.

**Lumpy maintenance** (`maintenance: lumpy`). Half of `maintenance_rate` stays a
smooth routine charge. The other half arrives as major repairs of five components
(roof, heating and cooling, water heater, exterior, appliances and flooring) whose
hazard rises with years since replacement (Weibull shape 3; mean lives of 11 to 25
years recalled from secondary reports of the NAHB and Bank of America 2007
component-life study; the split and the budget shares are judgements). A repair
costs a fixed share of the home's value of the day (a roof is 3.75% of value at
the default 1% rate), so the expected cost equals the smooth rate: over 3,000
paths of 30 years the lumpy total was 1.001 times the smooth one (1.02 in the first
ten years, 0.993 in the last twenty, because the random starting ages are not
quite the stationary mix). Measured: success 91.7% (unchanged; 22 paths rescued,
22 harmed), 10th-percentile terminal net worth $12,000 lower. On a rental the
reported lifetime spending rises (+$128,000 at the median) without any more money
being spent: a month with a repair bigger than the rent is booked as housing
spending and the other months as higher rental income. Not modelled: the actual age
of the house, catastrophes, repairs that hold their price when home values fall,
and the tax rule that a rental's roof is depreciated, not expensed.

**Assessment cap and insurance drift** (`assessment: {type: capped, annual_cap}`,
`insurance_real_growth: stochastic`). A capped base starts from the basis (or a
stated `assessed_value`), resets to the price on a purchase and rises by the cap
each year. Tax is charged on the base, or on the market value in a year when
that is lower; when prices recover the assessment returns to the base, which
kept rising meanwhile (California's Proposition 8 reductions work this way).
Measured at 2%: success 91.5% (-0.2 points, within noise), financial independence
1.2 years earlier at the median (the duplex is assessed at its $310,000 basis,
not its $385,000 value, and the gap widens), which the household spends by
retiring sooner. Not modelled: caps tied to inflation (California's is the lesser
of 2% and CPI; here the full cap applies every year), Proposition 19 transfers,
supplemental assessments, other states' homestead rules.

The insurance premium of a property that opts in is multiplied by one index per
path, shared by all of its properties: a persistent real growth rate (mean 0.5% a
year, stationary sd about 1.6 points) plus repricing jumps (8% of years, 11% at
the median). The only evidence is the Urban Institute figure in `params.json`
(premiums on new agency mortgages up 46% from 2018 to 2024, about 2.6 points a
year above CPI); every process parameter is a judgement set so that the long-run
real drift is about 1.5% a year and that episode is about half a standard
deviation above the mean. Over 3,000 paths the index after 30 years was 1.09 at
the 10th percentile, 1.51 at the median and 2.22 at the 90th. Measured on the
example (a Florida duplex insured at 1.1% of value): success 90.6% (-1.1 points,
58 paths harmed, 13 rescued), median terminal net worth $147,000 lower. Not
modelled: insurers withdrawing from a market, non-renewal, regional differences
(one national index).

**Market rents** (`rent_growth: market` on a rent line). One real rent index per
path: it grows 0.5% a year (the default real growth of a rent line) plus 15% of
the gap between lagged real home-price growth and 1% a year, with local noise
(stationary sd 5%, half-life about four years). The household pays the rent set
at its last yearly renewal, which closes 75% of the gap to market; in 6% of years
the lease is not renewed and a new one starts 4% above market. All of these are
judgements. Over 3,000 paths the rent paid was 1.003 times the fixed-growth rent
at year 10 and 1.006 at year 29, with a spread of 5 to 6% between paths. Measured:
success 91.7% (unchanged; 24 rescued, 25 harmed), 10th-percentile terminal net
worth $7,000 lower. Not modelled: metro-level indices, rent control, moving
costs, and the rent taken on after a home sale (`then_rent_monthly`), which keeps
the fixed growth. A market-rent line is not also given the shelter price wedge of
`spending.category_inflation`, which would count the same risk twice; it does take
the rent category's seasonal factor.

**Slow home sales** (`assumptions.housing_liquidity: realistic`). A planned or
last-resort sale is listed and closes two months later plus the whole months of an
exponential wait whose mean is 0.5 months in a normal market and 3.5 at full stress
(a recession counts for half, a 10% fall of the national index over twelve months
for the other half): a little over 2 months on average in a normal market, about 5
in a 2009-style one, at most 24. It closes at the home's value of the closing
month less a discount of 6% of the price on average at full stress and none in a
normal market. The discount is a judgement sized from Campbell, Giglio and Pathak
(2011), who measure 3 to 7% for sales forced by a death or bankruptcy; they
measure the seller's circumstances, not the state of the market. Carrying costs
(and a rental's rent) run until closing, and a household that listed as a last
resort borrows against the expected proceeds meanwhile, at no interest. Measured:
success 91.8% (+0.1 points, within noise; few paths of this example sell), 10th
percentile terminal net worth $3,000 lower. Not modelled: a base discount for a
distressed seller in a normal market, sales that fall through, interest on the
bridge, and a purchase that depends on the proceeds of a slow sale (it waits up to
its usual retry period for the cash).

**Adjustable-rate mortgages** (`mortgage.kind` or `buy_home.mortgage_kind`:
`arm`). After the fixed months the rate resets every period to the simulated
Treasury bill yield plus the margin, moving at most the periodic cap at a time,
never above the starting rate plus the lifetime cap and never below the margin
(Fannie Mae Selling Guide B2-1.4-02 for the floor and the requirement of periodic
and lifetime caps); the payment is re-amortised over the months left. For a
mortgage that already exists the fixed months and the lifetime cap count from the
start of the plan. Without a stated rate a purchased ARM starts 0.75 points below
the simulated 30-year rate (a judgement). The yearly refinancing test compares the
market fixed rate with the loan's current rate; a refinanced ARM becomes a fixed
loan over the months it had left. Measured on the duplex loan (4.25%, 26 years
left) as a 1/1 ARM with a 2.75-point margin and 2/5 caps: the first reset takes
the rate to about 6%, financial independence comes 1.0 year later at the median,
and success is 92.3% (+0.6 points): the household works longer to carry the
dearer loan, and that extra year protects it for the rest of its life. With five
fixed years: 92.2%, 0.7 years later. Not modelled: interest-only and
negative-amortisation loans, a first-reset cap different from the periodic cap,
the SOFR and CMT indices themselves (the bill yield stands in for them).

### 8.2 Home equity and housing decisions (all opt-in)

Each option below is off by default; with all of them off a path is bit for bit
the same as without them. Their numbers are in `data/actuarial/params.json` under
`housing_decisions`.

**Home-equity line of credit** (opt-in, `retirement.home_equity_borrowing: {type:
heloc}`; code in `home_equity.rs` and `sim/housing_decisions.rs`). At any age, working or
retired, when liquid savings cannot pay the month and an essential would otherwise be
cut or property sold, the household opens a line on its main home (once, the first
time it is needed) and draws what is missing. The credit limit is
`max_combined_ltv x value - mortgage` (default 0.80; CFPB's HELOC brochure and lender
practice: 80 to 85%), cut further if the home later loses value. The rate is the
simulated Treasury bill yield plus `spread` (default 3.5 points: prime is the federal
funds target plus 3, and Bankrate's national average HELOC rate was 7.81% in early
December 2025 against a 6.75% prime). Interest only for `draw_years` (10), then a
level payment over `repay_years` (20). When a recession begins, or national home
prices fall 10% below their running peak (a judgement), lenders freeze the unused
part of the line and new lines with probability `freeze_probability` (0.3, a
judgement: in the Fed's April 2008 Senior Loan Officer Survey about half of the banks
had tightened terms on existing lines, nearly all citing falling collateral values;
no borrower-level freeze rate is published); lines open again when the downturn
ends. The freeze draw comes from its own stream (`HomeEquityCredit`), one uniform a
month whatever the household does, so paths stay paired. Draws are booked as cash
from property (`property_sale`, a transfer, not income), interest and repayments as
housing spending, and the balance in the `mortgage` series; the FI test counts it as
a debt to clear. Surpluses repay the line before they are invested, and a sale repays
it out of the proceeds. Measured on the family example (4,000 paired paths, the profile's seed):
with `home_equity: last_resort` the success rate is unchanged (91.5%, 33 paths rescued
and 32 harmed: the line's interest can cost a path that would have sold the house
early anyway) while the 10th percentile of terminal net worth rises from $104,000 to
$144,000; FI falls 0.4 points because a working household that borrowed must clear the
line first. With `home_equity: never` the line lifts success from 76.3% to 85.9%. Not
modelled: closing and annual fees (commonly waived), the lender's income test,
interest deductibility (only for home improvements since 2018), lines on second homes
or rentals, and fixed-rate conversion options.

**Reverse mortgage** (opt-in, `retirement.home_equity_borrowing: {type:
reverse_mortgage}`; same files). An FHA Home Equity Conversion Mortgage line of credit
on the main home, opened once the youngest adult present is 62 (24 CFR 206.33):
either as a last resort, when savings cannot pay the month (`open_when: last_resort`,
the default), or as a standby line as soon as liquid savings fall below `years` of
planned spending (`open_when: {type: savings_below, years}`). The maximum claim amount
is the lesser of the home's value and the FHA limit ($1,249,125 for 2026, HUD
Mortgagee Letter 2025-22, indexed on the path with national home prices). The principal
limit is the principal limit factor at the borrower's age and the expected rate (the
simulated 10-year Treasury yield plus `lender_margin`, at least 3%) times that amount;
the factors are HUD's published excerpt of the October 2017 table (ages 62, 72, 82, 92;
rates 3% to 8%; ML 2017-12), interpolated linearly. At closing the 2% upfront mortgage
insurance premium, the origination fee (the greater of $2,500 or 2% of the first
$200,000 plus 1% above, at most $6,000; 24 CFR 206.31), `other_closing_costs`
($3,000, a judgement) and any existing mortgage are financed out of the principal
limit; if they do not fit, no loan is made. In the first 12 months at most 60% of the
principal limit (or the mandatory obligations plus 10%) can be disbursed (ML 2013-27).
Each month the balance and the unused line both grow at the bill yield plus the margin
(default 2 points, a judgement) plus the 0.5% annual insurance premium. Nothing is
repaid while the household lives in the home; property tax, insurance and upkeep go on
as before. The loan is non-recourse: the liability booked in the `mortgage` series is
never more than the home is worth, and a sale repays at most the proceeds. A draw is
cash from property (`property_sale`), the financed costs and accrued interest raise the
balance (and lower net worth) rather than spending. Measured on the family example
(4,000 paired paths): with `home_equity: last_resort` success is unchanged (91.5% to
91.3%, within noise; 35 paths rescued, 41 harmed) and the 10th percentile of terminal
net worth rises from $104,000 to $132,000; a standby line opened below five years of
spending gives 91.4% and $105,000. With `home_equity: never` the reverse mortgage lifts
success from 76.3% to 87.6%. Not modelled: non-borrowing spouses (the borrower is
the youngest adult present, and the loan continues for the survivor), the HECM-for-
purchase, tenure or term payment plans, the eighth-of-a-point rounding and full
whole-age table of the principal limit factors, the financial assessment and
set-asides for property charges, the deduction of interest when it is finally paid,
and default when property taxes go unpaid.

**Planned downsizing** (opt-in, `retirement.downsize`; code in `sim/downsize.rs`).
Once, when its condition is met, the household sells its main home and moves to one
worth `to_value_fraction` of the old home's market value (default 0.6, a judgement).
Conditions: `at_age` (a person reaches an age), `when_children_leave` (the youngest
child turns 18, or 22 if a college is planned), `when_widowed` (a year after the first
death in a couple, a judgement), and `when_funded_ratio_below` (in a January of
retirement, investable savings less debts fall below `ratio` times the funded test's
requirement, recomputed then). The sale is an ordinary sale: `selling_cost_fraction`
(default 6%, the bottom of the 6 to 10% seller's range in `params.json`
`housing.closing_costs`), the mortgage and any home-equity line repaid, the gain above
the section 121 exclusion taxed. The new home sits in the same property slot, so its
property tax, insurance and maintenance follow its lower value; `buying_cost_fraction`
(3%, the buyer's default there) is one-off spending. The proceeds pay the closing costs
and as much of the price as they can; the rest is a 30-year mortgage at the month's
simulated rate, as long as the proceeds make a 20% down payment (a judgement at the
conventional no-PMI line); otherwise, or with `rent_instead`, the household rents such
a home at 4% of its price a year, like a household that sold as a last resort. The
move draws nothing at random. Measured on the family example (4,000 paired paths,
guardrails spending, `home_equity: last_resort`): downsizing to 0.6 when the children
leave lowers success from 91.5% to 90.8% (37 paths rescued, 62 harmed), the median
terminal net worth by $35,000 and its 10th percentile from $104,000 to $63,000, while
FI rises from 96.4% to 98.3% (the freed equity is investable): 9% of the home's value goes in transaction costs, the
guardrails spend part of the freed equity, and a household that later runs short has a
smaller home to sell. Downsizing when savings fall below 80% of the requirement gives
91.8% (within noise) and raises the 10th percentile of terminal net worth from $104,000
to $137,000; with `home_equity: never` it lifts success from 76.3% to 84.0%. Not
modelled: separate moving costs, rescaling of budget lines such as utilities, a new
home with different tax or insurance rates, HOA or appreciation than the old one, the
lender's income test, moving to another state, and the choice of when to move by
anything but these rules.

**Cash-out refinancing** (opt-in, `debt_policy.cash_out_refinance`; code in
`sim/cash_out.rs`). When money would otherwise be withdrawn with the 10% early-withdrawal
penalty (the year's withdrawal plan contains penalised withdrawals, or a mid-year
shortfall would be covered from penalised sources; both found by running the
withdrawal on a copy of the accounts), the household considers refinancing its main
home: a new loan up to `max_ltv` of the value (default 0.80, the Fannie Mae April 2026
eligibility limit for a one-unit main home), repaying the mortgage and any home-equity
line, at the simulated 30-year rate plus `rate_premium` (0.25 points: Fannie Mae's
loan-level price adjustments charge a cash-out loan 1.25 points of price more than a
rate refinance at 75 to 80% LTV and a 740-759 score, which at the usual 4 to 6 points
per point of rate is 0.2 to 0.3 points, a conversion that is a judgement) over
`term_years` (30), with closing costs of 2% plus $2,000 (the judgement the rate
refinance already uses), at most once in twelve months and only for at least $10,000
(judgements). A retired household's annual plan raises the year's penalised
withdrawals times the years until the youngest adult is 59 and a half (at least a
year of spending), so that one refinance bridges the gap; a mid-year shortfall raises
only itself. It goes ahead only if the penalty avoided (10% of the cash, less the
rise in loan payments while withdrawals would be penalised) exceeds the closing
costs, plus the higher rate on the old balance over that loan's remaining life, plus
the carry of the new borrowing over the 10-year Treasury yield until 59 and a half
(the safe-asset comparison the prepayment rule also uses). Without that test the
first version refinanced a working household's whole mortgage to cover a month's
shortfall and invested what was left over, which lowered success on the family
example from 91.5% to 90.7% and its 10th percentile of terminal net worth from
$104,000 to $42,000. The cash is booked as cash from property, the costs as one-off
spending, the loan in `mortgage`. Measured (4,000 paired paths): on the early-career
saver, who retires at about 39, success moves from 83.7% to 83.9% (within noise; 29
rescued, 20 harmed) and the median terminal net worth rises by $60,000; on the family
example success moves from 91.5% to 91.7% (9 rescued, 1 harmed) and the 10th
percentile of terminal net worth from $104,000 to $119,000. The test is strict: with
mortgage rates well above Treasury yields a refinance rarely pays for itself. Not
modelled: the lender's income and credit tests, freezes of refinancing in downturns,
the rule that interest on the cash-out part is deductible only if spent on the home
(it is deducted like the rest), repaying the loan from penalty-free withdrawals after
59 and a half (it runs to term), and the choice between a cash-out and a home-equity
line by their costs.

**Rate-aware mortgage prepayment** (opt-in, `debt_policy.mortgage_prepayment: {type:
when_rate_above}`; same file). When surplus cash is about to be invested, the part the
savings waterfall would put in the taxable account (found by running the waterfall on a
copy) pays down the highest-rate mortgage on a home or second home instead, while its
rate is above the 10-year Treasury yield after tax at the household's marginal rate
(the expected inflation on both sides cancels), or, with `real_rate`, while its rate
less last year's inflation is above that real rate. Contributions to tax-advantaged
accounts come first, as before; the prepayment is booked as debt spending and the
scheduled payment is unchanged, so the loan ends sooner. Measured (4,000 paired paths):
on the early-career saver success rises from 83.7% to 90.3% (296 rescued, 29 harmed),
the FI age moves 0.3 years later, median lifetime tax falls by $49,000 (less taxable
income from the brokerage account) and median terminal net worth rises by $192,000: a
household that retires at 39 without a mortgage needs smaller withdrawals, many of
them otherwise from penalised or fully taxed accounts, and is less exposed to a bad
sequence of returns; on the family example success is unchanged within
noise (91.5% to 91.2%). Not modelled: the mortgage interest deduction in the
comparison, rental mortgages, the liquidity lost by prepaying, and expectations of
future rates.

**Rentals that behave like rentals** (opt-in per rental, `use: {type: rental, ...,
dynamics: {type: realistic, annual_turnover}}`; code in `rental_market.rs`, applied by
`PropertyModel::step_realistic`). By default a rental is empty in short random spells
at the entered vacancy and its rent follows its trend. With realistic dynamics a
tenancy ends each month with probability `annual_turnover / 12` (default 0.25:
Invitation Homes reported 22.6% annual turnover and stays of about 38 months in 2024,
and 24.3% in 2023); the unit then stands empty for a geometric number
of months whose mean, `v / (1 - v)` tenancies-worth divided by the move-out rate, keeps
the long-run vacant share at the entered `vacancy` (when that mean is under a month it
is re-let within the month and loses that fraction of the rent). Spells are 1.5 times
longer in a recession and slightly shorter otherwise so that the average holds at 13%
of months in recession (judgements). The market rent is the trend times
`exp(x - S^2/2)`, with `x` an AR(1) of persistence 0.8 a year and long-run sd 5%
(judgements informed by the Zillow Observed Rent Index, whose yearly growth peaked at
17.2% in February 2022); new leases and yearly renewals reset the rent to the market.
A share of move-outs (2% of tenancy-years, a judgement informed by the Eviction Lab's
filing rate of about 8% of renter homes) are evictions costing three months of rent.
Half of the maintenance rate comes as large repairs, one every five years on average,
sized to keep the expected maintenance unchanged (judgements). The rental draws five
values a month from its own stream (`RentalMarket(slot)`, base 0x1600_0000) whether
owned or not; the property's own stream is drawn exactly as before. Costs go through
the rental's operating costs, so they are deductible and reported as before; trace
events mark move-outs, evictions and large repairs. Measured: a $2,000-a-month rental
kept its average income less costs within 1% of the simple model's over 20 years, while
a quarter of its years were $5,000 or more away from it; on the side-hustle landlord
(4,000 paired paths) success moves from 92.1% to 91.8% (within noise) and the 10th
percentile of terminal net worth from $143,000 to $126,000. Not modelled: rents that
move with the local house price or the economy (the index is the rental's own),
correlation between rentals, rent control and lagged renewals, security deposits,
turnover make-ready costs beyond maintenance, and capitalisation (rather than
expensing) of large repairs.

**With the housing realism options of 8.1.** A rental with realistic dynamics on a
property with lumpy maintenance keeps the lumpy repairs and draws no second set of
large repairs (its tenant, rent and eviction draws are unchanged). A planned
downsizing lapses if the home is already listed under slow sales; a home listed as a
last resort counts toward the bridge to closing net of any home-equity line, which
is repaid from the proceeds. A cash-out refinance or a downsizing mortgage is a
fixed-rate loan, whatever the old loan was.

## 9. Social Security

Each person's benefit comes from their own earnings record: the rebuilt past plus
the simulated future, wage-indexed to age 60, best 35 years, the bend-point formula
of their eligibility year, cost-of-living adjustments from 62, and the reduction or
delayed credits for the claim age (70% at 62, 100% at 67, 124% at 70 for full
retirement age 67). The engine reproduces SSA's published 2026 maximum benefits to
the dollar ($2,969 at 62, $4,152 at full retirement age, $5,181 at 70). Spousal
top-ups, survivor benefits (the higher of the two) and the earnings test (with the
withheld months given back at full retirement age) are modelled. Benefits rise with
the path's CPI.

Future bend points grow with wages, so a 36-year-old's projected benefit is about
38% higher in today's dollars than a 62-year-old's for the same relative earnings.
That is current law under the Trustees' assumptions; SSA's own "today's dollars"
statements do not include it, so the engine will read higher than a statement. A
trust-fund haircut (benefits at 79% from 2033) is available and off by default.

### Adaptive claiming (opt-in)

`social_security.claiming: adaptive { earliest (62), latest (70) }` (the default is
the fixed `claim_age`) plans the claim at `latest`, which is also the claim age the
funded test and the present value of the benefit start from. Each month from
`earliest`, once the person has stopped working, the household compares its
investable savings with what delaying still costs:

```
bridge  = months to latest x the benefit available by claiming now (today's $)
reserve = 12 months of the household's budget
claim now if savings < reserve + bridge (+ the higher earner's bridge, for the
                                         lower earner of a living couple)
```

and claims at once, working or not, in a month after one in which essentials were
cut. The bridge is what the portfolio has to pay in place of the benefit; the
reserve of a year's budget is a judgement. **Survivor benefit:** the higher
earner's benefit is what the survivor keeps, so while both are alive the lower
earner's test also counts the higher earner's remaining bridge: when money is short
the lower earner claims first and the higher earner keeps delaying. No random
numbers are used. The claim shows in a trace as a life event naming the reason,
followed by the usual "starts Social Security" event, and in the per-person
claim-age column.

Measured on a test household (single, 61, $2,000 a month at full retirement age,
$300,000 in a 60/40 account, $36,000 budget, no tax, 400 paths): fixed at 70,
paths spend 10.3 years on average with savings exhausted; adaptive, 7.4 years, with
lifetime spending $1.029 million against $1.002 million for claiming at 62. With
the CLI (3,000 paired paths, both partners adaptive in `dual-income-family`, the
single person in the others): `dual-income-family` 8.2% -> 7.8% depleted (within
noise), 10th-percentile terminal net worth +$85,000; `side-hustle-landlord` 7.8%
-> 7.1% depleted, median retirement age +0.25 years (planning on a claim at 70
raises the funded-test bridge); `early-career-saver` within noise (15.8% ->
16.1%).

Not modelled: the decision does not weigh life expectancy, health, tax or the
marketplace premium credit; a claim is never withdrawn or suspended; a working
person claims early only when essentials are being cut; the cache of benefit
thresholds follows the claim age as decided (it uses the claimant's claim age, which
only adaptive claiming moves).

## 10. Longevity

Each adult's age at death is drawn from the SSA 2023 period life table, improved
along the cohort diagonal with the Trustees' rates (life expectancy at 65: 19.2
years for men, 21.7 for women, matching the published cohort tables), with a
mortality multiplier for health (default "good" = 0.85, which adds about 1.4 years
at 65). Deaths of partners are independent unless linked lifespans are switched on (below). For a couple of 65 at the default
setting, the chance that at least one is alive is 66% at 90, 38% at 95 and 14% at
100. A fixed lifespan can be set instead.

When a partner dies: their jobs end, accounts pass to the survivor with a step-up
of taxable basis, the return is joint for that year and single after, shared
spending falls by the survivor factor, and the survivor keeps the higher Social
Security benefit.

### Linked lifespans (opt-in)

With `longevity.couple_dependence: linked` the two partners' ages at death are
drawn together. Two parts, both leaving each person's own survival curve exactly
as above:

* **Shared frailty.** A Frank copula on the two longevity uniforms with
  association 7.065, the estimate of Dufresne, Hashorva, Ratovomirija and
  Toukourou (2018, Table 4.1, IFM, no age-difference term) on the joint-and-last-
  survivor annuity data of a large Canadian insurer analysed by Frees, Carriere
  and Valdez (1996). Frees, Carriere and Valdez's own Frank fit to the same data
  was weaker (association about 3.4, Spearman's rho about 0.5), so the strength
  of the link is uncertain by a factor of about two. Kendall's tau 0.56; the Pearson correlation of the two
  simulated death ages of a couple of 66 and 64 is about 0.75.
* **Bereavement.** The survivor's risk of dying in the six months after the
  partner's death is raised by the relative risk 1.41 of the Moon, Kondo, Glymour
  and Subramanian (2011) meta-analysis of 15 cohorts. It is a mixture component
  whose joint mass is the independent chance of each such pair times 0.41, and
  the copula's residual curves have that mass taken out, so the margins stay
  exact (a unit test checks them to 1e-12, including a 28-year age gap). The
  lasting excess after six months (RR 1.14) is taken to be part of the shared
  frailty and is not added. A death drawn this way is logged ("bereavement").

Draws: the first uniform of each person's longevity stream (the one the
independent draw uses) and two uniforms of the new `Stream::CoupleLongevity`,
whatever the household does.

Measured: for a couple of 66 and 64 (4,000 paths) mean death ages are unchanged
within noise, the average gap between the two deaths falls from 10.9 to 6.0
years and the last survivor's expected death age from 92.1 to 89.4. On the
dual-income example (4,000 paired paths) success moves 91.5% to 91.1% (within
noise), depletion 8.5% to 8.8% (within noise), and reaching retirement 99.5% to
98.6% (both partners dying young together is more likely); median lifetime tax
falls by $36k.

Not modelled: the dependence of the association on the age difference (the
paper's alpha(d)); dependence for unmarried partners or a partner who joins
later (it applies to the first two adults from the start); bereavement effects on
spending, work or health costs; the copula was fitted on insured annuitants,
whose dependence may differ from the population's, and combining it with the
bereavement term may overstate short-term dependence a little.

### Longevity and income (opt-in)

With `longevity.income_gradient: true` each person's mortality is multiplied by a
factor set by the household's rank in the national income distribution. The data
are Chetty, Stepner, Abraham et al. (2016, JAMA), online table 1: race-adjusted
period life expectancy at 40 by percentile of household income, by sex, US
2001-2014 (men 72.7 / 81.7 / 87.3 years at the 1st / 50th / 100th percentile,
women 78.8 / 85.0 / 88.9). The factor is the constant multiplier on the SSA 2023
period table that moves its life expectancy at 40 by the percentile's gap from
the average over the percentiles (found by bisection), applied on top of the
`health` multiplier through the same mortality curve, so every later draw, the
health states and the linked lifespans see it. It is an association: higher income
goes with longer lives for many reasons (health, education, work, place), and the
model does not claim income causes them.

The rank comes from `income_percentile` (1 to 100) when given; otherwise from the
gross pay of the jobs and the net self-employment income running at the start,
converted to 2012 dollars with CPI-U (1.4676, the project's own CPI series) and
placed between the percentile means of the table. With no earnings at the start
(a retired household) the 50th percentile is used and validation warns: the paper
ranks people past 61 by their income at 61 to 63, which a profile of retirees does
not hold, so `income_percentile` should be set. With the option off the
multiplier is exactly 1 and nothing changes.

Measured: for a woman of 70 (3,000 paths) the 95th percentile moves the mean age
at death up by between one and four years and the 5th down by a similar amount
(a simulation test); each paired death moves only later at the 95th percentile.
On the dual-income example (4,000 paired paths) the earnings rank the household
near the 90th percentile (the result is within noise of `income_percentile: 90`):
success falls from 91.5% to 89.8% (95% interval of the change -2.3 to -1.0
points) and depletion rises from 8.5% to 10.2%, because longer lives must be
paid for; median terminal net worth falls by $166k. At the 10th percentile success
rises to 93.7%.

Not modelled: ranking by lifetime rather than current earnings (a high earner
early in a career or a low earner by choice is misranked; set the percentile),
the paper's differences by place and over time (life expectancy at the top rose
faster in 2001-2014), the gradient's narrowing at old ages beyond what a
proportional shift gives, and any link between income and the health states'
prevalence.

## 11. When retirement happens

Triggers: at an age, at a date, already retired, when a fixed number is reached, or
**when funded** (the default). The funded test, evaluated every month:

```
need        = retirement budget + home running costs + modelled healthcare
              - income already being received that continues
              + tax on the withdrawals that pay for it
gross       = need / withdrawal_rate
              + mortgage balances + child costs still due        (sums, not for ever)
requirement = gross                                               (count_future_income off)
            = max(gross - PV(income not yet started),
                  min(gross, bridge))                             (count_future_income on)
funded when wealth >= requirement
```

* Tax is the engine's own return on a year of retirement withdrawals, split across
  account types as the household holds them, grossed up at the marginal rate (the
  tax itself has to be withdrawn and taxed).
* Healthcare before 65 is averaged with the Medicare cost over the years to 65,
  net of the premium credit the retirement income would earn.
* Income not yet started (Social Security, pensions) is valued with the person's
  survival curve, discounted at the rule's own withdrawal rate.
* **The bridge.** Income that starts in 25 years cannot be spent today. `bridge` is
  what savings must do on their own: pay the whole need until each income starts
  and the remaining need afterwards, discounted at the real return at which the
  withdrawal rate exhausts a portfolio in 30 years (1.2% for a 4% rule). For a
  household near its claim age the present-value form binds; for one decades away
  the bridge binds and future income counts for little.

Before the audit the present value of a benefit 25 years away lowered the liquid
target one for one: a "4%" household retired at 42 on a 4.55% first-year draw and
9.3% of paths ran out, mostly before the benefit began. After (single 40-year-old,
$60k budget, constant spending, 3,000 paths, default equity adjustment), columns:
rule's rate | median retirement age | median first-year draw / investable | paths
depleted:

| | future income counted | not counted |
|---|---|---|
| 3.0% | 46.4 / 2.93% / 2.1% | 46.5 / 2.90% / 1.8% |
| 3.5% | 44.8 / 3.41% / 5.4% | 44.8 / 3.38% / 4.7% |
| 4.0% | 43.4 / 3.88% / 10.4% | 43.4 / 3.87% / 9.8% |
| 4.5% | 42.2 / 4.35% / 17.2% | 42.2 / 4.34% / 16.7% |
| 5.0% | 41.3 / 4.83% / 25.7% | 41.3 / 4.82% / 25.4% |

Two things to read from it. The realised first-year rate
(`headline.initial_withdrawal_rate`) is now reported next to the age, and it is at
or under the rule's. And a 4% rule over a 55-year retirement with stocks returning
1.5 points less than the US record fails about one time in ten: 4% is a 30-year
number (section 14).

A partner can stop work later (`partner_offset_years`). Coast-FI (the age at which
today's savings, with no more saving, would meet the test at the planned
retirement age) is reported too.

### Not retiring into a crash (opt-in)

Two settings on `when_funded` change the month a funded household retires; the FI
age (the first month the test is met) does not move.

* `confirmation: { months (6), max_equity_drawdown (0.2) }`: the test, with
  `min_age`, must hold for `months` months in a row, and US stocks must not be more
  than `max_equity_drawdown` below their peak. A household funded at the top of a
  boom that loses it in the crash keeps working. A postponement for the market is a
  trace event.
* `one_more_year: { probability (0.25) }`: each time the household would retire it
  decides, with this chance, to work another year first, then decides again (the
  "one more year" habit). One draw a month from its own stream.

`max_age` still forces retirement. Both defaults are judgements: there is no agreed
estimate of how long households wait for confirmation or how often they add a
year, so they are the user's to set.

Measured with the CLI (`firesight run`, 3,000 paths each; retirement age p10 / p50
/ p90, then paths depleted):

| | early-career-saver | dual-income-family |
|---|---|---|
| plain rule | 36.1 / 39.3 / 45.1, 15.8% | 50.0 / 53.4 / 60.9, 8.2% |
| confirmation | 36.7 / 40.2 / 47.8, 9.2% | 50.4 / 54.7 / 63.8, 6.1% |
| one more year | 36.3 / 39.7 / 45.6, 13.4% | 50.0 / 53.9 / 61.6, 7.4% |
| both | 37.0 / 40.6 / 48.6, 8.0% | 50.5 / 54.9 / 64.6, 5.5% |

For `early-career-saver`, confirmation moves the median by 0.9 years and cuts
depletion by 6.6 points; one more year moves the median by 0.4 years and cuts it
by 2.4 points. Confirmation acts on the paths where retiring would have happened
during a fall of more than 20%, and stretches the upper tail (the 90th percentile
moves by 2.7 to 2.8 years) as those households wait the fall out; how much of its
larger effect comes from that timing rather than the longer delay was not
separated. One more year at 25% adds about a third of a year on
average, as the geometric expectation 0.25 / 0.75 says it should.

Not modelled: the decision does not weigh the margin over the target, age or how
the job is going; a delay is always a whole year; a retired household does not
reconsider (see going back to work below).

### Going back to work (opt-in)

`retirement.return_to_work` (off by default) lets a retired household look for
paid work again when its savings fall behind. Each January the funded test above
is run on the retired household as it now stands; its funded ratio is wealth over
the requirement (never below zero). Below `funded_ratio_below` (default 0.8),
every adult who has stopped working and is younger than `until_age` (70) starts a
search. Its length is drawn from the engine's own unemployment-duration model
(lognormal, median 2.2 months, 4 in a recession or the year after one, sigma 1,
times 1.4 from 55; section 6), times a further 1.1 from 65: the ratio of mean
unemployment durations at 65+ and 55 to 64 in the BLS Current Population Survey
(2024 annual averages, table 31: 28.9 against 26.2 weeks). That multiplier is a
judgement as much as a measurement, because the medians go the other way (10.9
against 12.0 weeks). The search can also be fixed at a number of months. The job
then pays `pay_fraction` (0.5: half time, or full time at half the pay) of the
person's real pay when they retired, or of `former_annual_pay` for a household
retired from the start. The wages are taxed, pay employee FICA, count toward the
Social Security earnings record and the earnings test, and appear as wages in the
year series and as a "back at work" event in a trace. Work stops in the first
January the ratio is 1 or more again, or at `until_age`. Random numbers come from
their own stream, one draw per person per month whatever happens.

Measured with the CLI (`firesight compare`, 3,000 paired paths, default settings):
`early-career-saver` (retires at a median 39 on a 3.5% rule) goes from 15.8% to
0.9% of paths depleted, with median lifetime tax up $332,000 as the wages are
taxed; `dual-income-family` from 8.2% to 6.3%; `side-hustle-landlord` from 7.8%
to 6.2%. The young retiree gains most because it has thirty years in which it can
go back.

Not modelled: the job has no wage growth, shocks or layoffs, brings no employer
health cover, workplace plan or HSA; the search does not depend on how long the
person has been retired; the household's spending does not change while it
works; there is no age discrimination beyond the longer search.

## 12. Spending in retirement

Six rules decide each January how much to spend:

* **Constant real** (Bengen): the budget, whatever the portfolio does.
* **Percent of portfolio**: a fixed rate of the current portfolio plus other
  income.
* **Guardrails** (Guyton and Klinger): cut spending 10% when the withdrawal rate
  rises 20% above its reference, raise it 10% when it falls 20% below, skip the
  inflation raise after a losing year, and stop cutting in the last 15 years.
* **Vanguard dynamic**: move toward a target rate, by at most +5% / -2.5% a year.
* **Floor and ceiling**: the same target, kept between 85% and 120% of the budget.
* **VPW**: the annuity payment for the remaining years at the portfolio's expected
  return, capped at 10%.

Whatever a rule says, spending is not set below the essential part of the budget.

**What the withdrawal rate is measured on.** In the published rules the rate is
this year's withdrawal over the portfolio, which is right for a retiree with a
constant budget and no other income. In a real household the first year's cash
withdrawal is a bridge rate: it includes a mortgage that ends, children who leave,
health insurance before Medicare, and excludes Social Security not yet claimed or a
partner still earning. Anchored on it, the rules cut households that were exactly
on plan (college raised the draw as planned: cut; the partner retired as planned:
cut every year down to essentials) and then overspent late in life. The three
rate-following rules now measure the rate on the funded test's own quantities:

```
A = investable assets - mortgage balances - child costs still due + PV(income not yet started)
N = retirement need + its tax - income already in payment          (at a budget multiple of 1)
rate = (N + (m - 1) x budget) / A          m = last year's multiple of the budget
```

and the adjustment applies to total spending. The guardrail cut is measured against
the larger of the household's starting rate and the funded test's rate, and
spending is raised *above* the plan only when the rate is 20% under the lower of
the two. With no other income and a constant budget this is the published rule
(the six-year hand-worked Guyton-Klinger sequence and the Vanguard sequence in the
tests reproduce exactly).

Before and after, on the same paths: the family example (guardrails) succeeded on
83.2% of paths against 87.8% for constant spending before, and 91.1% against 91.8%
after (4,000 paired paths, a difference within noise); the landlord example
(Vanguard dynamic) 90.3% against 91.0% before, 92.0% against 92.3% after. With a partner working four more years, guardrails delivered 68% of
plan before (158% with no offset); after, 98% (92% with no offset).

A flexible rule spends more in good times, so it is not guaranteed to succeed more
often than constant spending: it trades some safety for consumption. On a retired
couple with heavy late-life care risk, guardrails still succeed three points less
often than constant spending (95.5% against 98.4%) and leave a median estate of
$1.9M against $3.2M: the difference was spent.

**How delivery is reported.** Three numbers instead of one ratio that could read
128%: `delivered_spending_ratio`, the share of planned spending delivered
(`sum of min(delivered, planned) / sum of planned`, never above 1);
`plan_delivered_share`, the average over retired years of `min(delivered / planned,
1)`; and `extra_spending_ratio`, spending above the plan as a fraction of the plan.
Alongside: the share of paths with a year below 90% and below 75% of plan, and by
year the share of households depleted and how deep the shortfall is.

### Where the money comes from

Each January a withdrawal plan is made for the year and carried out monthly.

* **Conventional**: cash, taxable, traditional, Roth.
* **Proportional**: from each account type in proportion to its size.
* **Tax-aware** (the default): required distributions first; then fill the
  ordinary bracket up to `fill_to_bracket` (default 12%) with traditional
  withdrawals; taxable money and Roth for the rest; Roth conversions into unused
  bracket room; gains harvested in the 0% bracket when that is really free.

The plan is grossed up for the tax its own withdrawals cause (a fixed point found
by secant steps on the real tax function). Before 59 and a half the order is:
penalty-free money first (taxable, Roth contributions, seasoned conversions), then a
72(t) schedule (the amortisation payment on the balance at up to 5%, fixed once
started for five years or until 59 and a half), then penalised withdrawals.
Required minimum distributions follow the Uniform Lifetime Table from 73 or 75.

Corrections made by the audit:

* **A conversion is judged by its real cost.** The planner sized conversions on
  the ordinary bracket alone. The top of the 12% bracket is also the top of the 0%
  gains bracket, so each converted dollar pushed a dollar of that year's realised
  gains from 0% to 15%: a true cost of about 27% on the margin. Now a conversion,
  and bracket-filling on the marketplace, is kept only as far as the federal tax
  and lost premium credit it causes per dollar stay within the bracket rate plus
  two points. On a household with appreciated taxable assets the default order
  succeeded less often than the conventional one (90.7% against 91.6%, 0 paths
  rescued, 44 harmed); after, 89.3% against 88.9% (23 harmed by switching to
  conventional, none rescued), with a median lifetime tax of $338k against $439k.
* **The premium credit is protected in practice.** Planning aimed 2% under the
  400% cliff on January estimates that were off by more; the January rebalance
  realised gains; and once forced income was over the cliff the 12% bracket was
  filled. A couple retiring at 60 lost the credit in 88% of pre-Medicare years
  (57% under the conventional order). Now: the plan aims 8% under the cliff;
  bracket-filling counts the credit it loses; a household that guards the credit
  rebalances inside its tax-advantaged accounts and leaves the taxable account
  alone; and in October the year is re-planned on actual income when the cliff is
  within reach. After: the credit is received in 119 of 120 household-years (99%),
  and the tax-aware order beats the conventional one (95.5% against 94.6% success).
* **A 72(t) schedule is one schedule.** A fresh penalty-free tranche was granted
  every year on top of the running one, so penalties nearly vanished after the
  first year ($51 in year two on $67,000 withdrawn). Now only the running payment
  is exempt: about $2,000 to $6,000 of penalty a year for a 50-year-old living on a
  $700,000 traditional account.

## 13. What success means, and why there is more than one number

A path **succeeds** if the household retired (or died before the plan's latest
retirement age), was never *depleted*, and ends with at least the bequest target.
Depleted means: nothing liquid left, every discretionary cost already cut, any
property sold (under the default home-equity rule), and still an essential cannot
be paid. A depleted path stays failed even if income later covers spending again.

A success rate alone hides most of what matters, so the engine also reports:

* how much of the plan was delivered and how often and how deeply spending was cut;
* the share of paths that sold a home to get there;
* when the money runs out on the paths where it does, and how long the shortfall
  lasts;
* the whole distribution of net worth, spending, income and tax by year;
* the first-year withdrawal rate the household really starts on;
* one first cause per failed path, by the first rule that holds: long-term care
  above a quarter of the savings held at retirement; running out while still
  working (job loss if six months or more of unemployment came first, otherwise
  spending more than was earned); 18 months or more of unemployment; inflation above
  4.5% a year; poor returns in the first ten retirement years; running out at 90 or
  later; a first-year withdrawal rate above 5%. It is a first-match label, not a
  decomposition: the long-term care flag is also true on 23% of successful paths.

## 14. Monte Carlo design

**Random streams.** The generator is xoshiro256++, seeded through SplitMix64, with
one independent stream per `(seed, path, subsystem)`: market, inflation, each
person's career, equity compensation, each person's health, each person's
longevity, expense shocks, each property, and life events. The number of draws from
a stream never depends on a decision. So two scenarios that differ only in a
decision (rent or buy, claim at 62 or 70) see the same markets, the same layoffs,
the same illnesses and the same lifespans.

**Common random numbers.** That makes every comparison a paired experiment. The
difference in success between two scenarios is estimated from the paths that flip
(rescued and harmed), with the paired standard error
`sqrt((d - delta^2) / n)` where `d` is the share that flip and `delta` the net
change. A difference of half a point that would need a million independent paths
to see is visible in a few thousand paired ones.

**Path counts and standard errors.** A success rate `p` over `n` paths has a
standard error of `sqrt(p (1 - p) / n)`: at 10,000 paths and 85%, 0.36 points (a
95% interval of about +/- 0.7). Every probability is reported with a Wilson
interval, every median with a distribution-free interval. These are sampling
errors only. The spread between market models (several points) and the equity
assumption (section 2.7) is larger than the sampling error of any run above a few
thousand paths: run the alternatives.

**Determinism.** A path is a pure function of the profile, the seed and its index.
All transcendental functions come from one portable library, so native and
WebAssembly builds produce identical output (the audit compared the accumulator
bytes of 30 runs across the three examples, four market models and a stress; after
the fixes the summary and a traced path of each example under each market model
were compared again, native on three threads against WebAssembly in two merged
chunks, and are identical); results do not depend on the
number of threads, the chunking or the order of merging (100,000 paths, 66
irregular chunks merged forwards, backwards, shuffled and as a tree). One exception
found by the audit and fixed: a household retired in the first month ran the funded
test before the first year's tax law existed and read whatever the previous path in
that worker had left, so its funded ratio depended on path order (1.38 to 1.63 for
identical balance sheets; now 1.5636 on every path).

**Robustness.** About 35,000 randomly mutated valid profiles ran 50 paths each
with the conservation checks, overflow checks and debug assertions on: no panic in
the simulation, no conservation failure, no non-finite value. 61,524 hostile calls
through the WebAssembly module produced no trap.

**Speed.** One Apple M4 core runs about 4,400 to 6,800 paths a second on the three
examples (a path is 700 to 900 months); ten cores about 27,000 to 40,000.

## 15. Validation

**The 4% rule.** Bengen and the Trinity study: a 4% initial withdrawal, kept
constant in real terms for 30 years from a 60/40 portfolio, survived almost every
start date in US history. The engine in historical mode, annual rebalancing, no tax
or fees: **96.0% of 843 start months succeed; the failing start years are 1965 to
1969.** An independent Python recurrence on the same data: 96.4%, the same years.

| Horizon | 3.5% | 4.0% | 4.5% |
|---|---|---|---|
| 30 years, engine / independent | 100.0 / 100.0 | 96.0 / 96.4 | 87.2 / 87.6 |
| 40 years | 99.9 / 100.0 | 90.3 / 89.9 | 74.8 / 75.0 |
| 50 years | 95.9 / 96.5 | 79.1 / 80.3 | 58.2 / 58.3 |

Thirty-year survival of 3.5% / 4% / 5% withdrawals by model, 60/40, on the record
as it was: historical 100 / 97.5 / 83.5; bootstrap 60: 97.5 / 94.0 / 81.1; regime
94.2 / 90.5 / 79.3; lognormal 97.1 / 93.4 / 79.1. With stocks 2 points lower:
bootstrap 92.8 / 86.0 / 66.5.

**A riskless saver** (salary, spending, Treasury bills, no tax) matches the
spreadsheet recurrence month by month to rounding.

**Conservation.** In check mode every month satisfies: cash in equals cash out;
each account's change equals contributions less withdrawals plus return; property,
mortgage and debt balances reconcile. An independent reconciliation of traced
ledgers found residuals only at whole-dollar rounding. Tax paid over a 69-year
path was $1,774,154 against $1,774,165 for the sum of the annual returns.

**Mechanisms against their sources.** Life expectancy against the SSA table
(47.49 / 29.89 / 18.11 / 6.03 years for men at 30 / 50 / 65 / 85 against 47.50 /
29.90 / 18.12 / 6.04); Social Security maximum benefits to the dollar; required
distributions against the Uniform Lifetime Table; the 72(t) payment against the
Single Life Table; long-term care incidence against ASPE; wage-curve coefficients
against Cocco, Gomes and Maenhout; federal parameters against Rev. Proc. 2025-32.

**Regression tests.** Each audit finding that was fixed has a test that states the
defect (`crates/firesight-core/tests/sim_audit.rs` and the module tests). Golden
digests of the three demo households change only deliberately.

## 16. Data sources

* **Market history** (`data/history`): US stocks and bills from the Fama-French
  research factors (CRSP); 10-year bond returns computed from Treasury yields
  (FRED); CPI-U (BLS via FRED); home prices from Shiller and Case-Shiller; mortgage
  rates from the Freddie Mac survey (synthetic before 1971); NBER recessions;
  developed ex-US stocks from French's international data (1975 on); gold from 1968.
  `data/history/README.md` lists every file and URL.
* **Tax law** (`data/tax`): the 2026 federal parameters (Rev. Proc. 2025-32, SSA,
  CMS, HHS poverty guidelines) and each state's 2026 schedule, with sources in
  `SOURCES-federal.md` and `SOURCES-states.md`.
* **Actuarial and behavioural parameters** (`data/actuarial/params.json`, each with
  its source and a confidence note): SSA 2023 period life table and Trustees'
  improvement rates; KFF employer premiums; CMS marketplace age curve and Medicare
  premiums; Genworth/CareScout care costs; HHS/ASPE long-term care incidence; USDA
  child costs; Cocco, Gomes and Maenhout wage profiles; BLS unemployment duration.
* **Cost of living** (`data/col`): city indices for relocation.

Constants that are judgements, not data, are named constants with a comment and
are listed by a test (`tests/unverified_constants.rs`).

## 17. Known limitations and deliberate omissions

Stated plainly, roughly in order of how much they could move an answer.

**Markets**

* The history is that of one country. The default 1.5-point reduction of stock
  returns is a judgement; so are the long-run levels of international stocks, gold
  and crypto. There is no "lost generation" scenario beyond the stress overlays,
  and the left tail of 30-year stock returns is thinner than the international
  record suggests.
* Bootstrap: bond returns and the yield level on a path are not mutually
  consistent; the pull of rates to their mean is a modelling device.
* Regime model: real returns run above the calibration data (US stocks 7.5%
  against 7.2%, bills 0.6% against 0.3%) and real equity returns trend instead of
  mean-reverting (variance ratio 1.6 at ten years against 0.75 in history), so its
  long-horizon tails are too wide in both directions; home prices follow inflation
  more closely than in history (annual correlation 0.73 against 0.55).
* Lognormal model: understates inflation and rate risk by construction.
* Crypto parameters are assumptions, not estimates.

**Taxes**

* Not modelled: New York's recapture of lower brackets (understates state tax by
  $500 to $2,600 for high earners), Utah's Social Security credit, California SDI
  (1.3% of wages), the alternative minimum tax, the chained-CPI wedge, state
  itemised deductions beyond the basics, most state credits.
* The first simulated year uses one inflation factor for 2027 rather than each
  published 2027 figure (tax effect under $20; contribution limits off by up to
  $250 to $1,000 at the cap).
* A tax refund larger than the tax paid that month is recorded as other income,
  which overstates gross income and lifetime tax by the refunds (about 3.5% of tax
  paid on a typical path). Cash is unaffected.
* In the first, partial year a full year's retirement contributions are made in
  the remaining months, which understates that year's tax; it comes back in April.
* A household abroad is taxed at one flat rate for the whole calendar year.

**Spending, health and care**

* No Medicaid: a household that exhausts its savings on care is a failure, where in
  reality Medicaid would pay for the care.
* Children are not charged health-insurance premiums (about $5,000 a child a year
  for an unsubsidised early retiree), and employed parents pay single-coverage
  contributions.
* The 1.5% a year age gradient of Medicare-phase costs is applied to Part B and D
  premiums too, which do not vary with age (overstates costs at 90 by about 12%).
* Payroll deferrals continue while a household is paying penalties to withdraw or
  cutting essentials.
* After depletion a path keeps running with essentials cut and an interest-free
  overdraft; statistics of failed paths after their depletion date describe that
  state and should not be read as a forecast.

**People**

* Paths stop when the youngest adult reaches `end_age` (100): 14 to 20% of couples
  still have someone alive, their death ages are reported as 100, and care that
  falls after 100 is not charged (about 8% of mean lifetime care cost).
* Survivor benefits are understated in three cases (a widow of an early claimer; a
  widow who works past 60; a worker who dies before 62).
* Deaths of partners are independent; health and wealth do not affect each other.
* The lifecycle wage curve falls 15 to 22% in real terms from 45 to 65, which
  reflects the data (including shorter hours) but will understate pay for someone
  who stays full-time in a stable job; use the flat profile.

**Housing**

* A last-resort home sale is always possible, immediate and at the path's value
  less 6%; there is no reverse mortgage, no home-equity line and no downsizing
  before savings run out.
* Refinancing is a rule of thumb, cash-out refinancing is not modelled, and
  mortgages are fixed-rate only.

**Decisions**

* The household follows its rules mechanically. It does not go back to work,
  move, or change its allocation in response to a bad decade, except through the
  spending rule and the last-resort sale.
* Flexible spending rules spend more in good times and can succeed slightly less
  often than constant spending (section 12).
* The funded test does not include the tax cost of planned Roth conversions.

**Statistics**

* Intervals are sampling error only.
* Failure causes are first-match labels (section 13).
