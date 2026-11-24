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
example, switching it off halves depletion risk. Medicaid is not modelled; "health"
as a failure cause means private-pay care exhausted the portfolio. A care episode
under way is in the year's plan, and the spending rules do not raise spending
during one.

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

## 10. Longevity

Each adult's age at death is drawn from the SSA 2023 period life table, improved
along the cohort diagonal with the Trustees' rates (life expectancy at 65: 19.2
years for men, 21.7 for women, matching the published cohort tables), with a
mortality multiplier for health (default "good" = 0.85, which adds about 1.4 years
at 65). Deaths of partners are independent. For a couple of 65 at the default
setting, the chance that at least one is alive is 66% at 90, 38% at 95 and 14% at
100. A fixed lifespan can be set instead.

When a partner dies: their jobs end, accounts pass to the survivor with a step-up
of taxable basis, the return is joint for that year and single after, shared
spending falls by the survivor factor, and the survivor keeps the higher Social
Security benefit.

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
