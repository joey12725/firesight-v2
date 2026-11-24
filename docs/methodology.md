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

### Children

USDA expenditure estimates by age band and income tier (2015 dollars brought to
today), with the only-child and three-plus adjustments, childcare for ages 0 to 4
(or the amount entered) and college at the chosen type and share. The tier is
chosen from earned income at the start or, when that is lower, from the budget
divided by 0.8 (a retired household used to get the lowest tier whatever it spent).
In the funded test, child costs still to come are a sum set aside, not a permanent
cost.

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
