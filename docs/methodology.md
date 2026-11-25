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
Private-company equity (sections 5 and 6.1) was added after the audit; its
measurements were made for this document and are marked where they appear.

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

### Equity compensation and the alternative minimum tax

Stock and options in a private company (section 6.1) reach the tax return as five
kinds of amount. Each is found in the month it arises and added to that year's
return.

**Pay.** The spread on a non-qualified option at exercise (the 409A value of a share
less the strike), the cash an acquirer pays for options and RSUs, and RSUs and
non-qualified options sold after a listing or in a tender offer are wages: income
tax, Social Security up to the wage base, and Medicare. An incentive option that an
acquirer cashes out, or that is exercised more than three months after leaving the
company (IRC 422(a)(2)), is taxed the same way.

**Income without payroll tax.** Incentive-option shares sold before both holding
periods are met (more than twelve months after exercise and more than twenty-four
after the grant) give ordinary income equal to the smaller of the spread at exercise
and the gain actually made, never below zero. It is income on the return but not
wages for payroll tax, and nothing is withheld on it. An incentive option exercised
and sold in one step, in a tender offer or after an IPO, is the same case.

**Gains.** Shares are kept in lots, each with its cost, the month it was acquired
and, for incentive-option shares, the grant month and the spread at exercise. A lot
sold more than twelve months after it was acquired is a long-term gain; sooner, a
short-term one, taxed as ordinary income. (Ordinary taxable accounts are pooled at
average cost and always long-term; these lots are the engine's only short-term
gains.) The cost is the strike for incentive-option shares and the value at exercise
for non-qualified ones, whose spread was already taxed as pay. When a company fails
or is written off, the cost of every lot is a capital loss, netted and carried
forward under the $3,000 rule like any other. If incentive options were exercised
earlier in the same calendar year, their AMT adjustment is undone as well: the
household is taken to sell or abandon worthless shares before the year closes, and
a disposition in the year of exercise limits the AMT income to the gain, which is
nil (IRC 56(b)(3) and 422(c)(2)). Whether worthlessness alone counts as a
disposition is not settled; an advised holder makes sure it does. In a later year
the adjustment stands and only the credit is left. When the owner dies the lots pass to
the survivor at that month's value (the 409A value while the company is private)
and count as long-term.

**The alternative minimum tax.** Exercising an incentive option and keeping the
shares is not income for the regular tax, but the spread is income for the AMT in
the year of exercise (if the shares are sold in the same calendar year the
adjustment is undone). The engine computes Form 6251 in this form:

```text
AMT income = taxable income before the floor at zero
             + the standard deduction, or state and local tax if itemising
             + the incentive-option adjustment + the gain adjustment (never positive)
exemption  = max(0, E - 0.5 x max(0, AMT income - T))
base       = max(0, AMT income - exemption)
flat(v)    = 26% of the first $244,500 of v + 28% of the rest
tentative  = min(flat(base - gains) + tax on the gains at 0/15/20%, flat(base))
AMT        = max(0, tentative - regular tax before credits)
```

The 2026 amounts are `E` = $90,100 single and $140,200 joint, and `T` = $500,000 and
$1,000,000 (Rev. Proc. 2025-32; the 2025 act reset the thresholds and doubled the
phase-out to 50 cents per dollar, so the exemption is gone at $680,200 and
$1,280,400). Inside the phase-out band a dollar of income adds a dollar and a half
to the base, so the marginal rate there is 39% or 42%. Long-term gains and qualified
dividends keep their own rates inside the AMT, stacked on ordinary income as on the
regular return. The amounts are indexed with the chained CPI. The senior and
business-income deductions are allowed; no other preference or adjustment is
modelled. AMT income starts from taxable income as Form 6251 line 1 does, which is
negative when the deductions exceed income: a household that exercises in a year
with no pay adds back only the deduction it used. (Single, $1,429 of interest and
a $400,000 spread: AMT income is 1,429 - 16,100 + 16,100 + 400,000 = $401,429 and
the AMT $82,282, not the $86,390 that starting from zero gives.)

AMT caused by the option adjustment becomes a **minimum tax credit**. The credit
made in a year is the AMT with the adjustment less the AMT without it: under 2026
law a large capital gain can cause AMT by itself through the exemption phase-out,
and that part is not creditable. The credit never expires. In a later year it is
used against regular tax as far as regular tax, after the other non-refundable
credits, exceeds that year's tentative minimum tax. When the shares are sold in a
qualifying sale, the gain for the AMT is measured from the value at exercise and not
from the strike, so the earlier spread comes off the gain taxed at capital-gains
rates inside the AMT. That lowers the tentative tax and releases credit. The amount
taken off is limited to the gain actually made, and to the long-term gain that is
in taxable income that year: where other losses or an exclusion have already
removed the regular gain, nothing comes off income taxed at 26% or 28%. So if the
shares are sold for less than they were worth at exercise, or the company fails in
a later year, the sale releases little or nothing, and the credit comes back only
in small pieces in ordinary years.

Hand-checked against the engine (single filer in Texas, standard deduction $16,100):
wages of $250,000 and a $1,000,000 spread on incentive options held past the year
end. Regular tax on $233,900 is $51,304. AMT income is $1,250,000, the exemption is
gone, and the tentative minimum tax is 0.26 x 244,500 + 0.28 x 1,005,500 = $345,110,
so the AMT is $293,806, all of it credit. What happens next depends on the year
that follows, and the two cases below are alternatives, not a sequence. If it is a
year with the same wages and nothing else, the tentative tax is $41,574 and $9,730
of the credit is used: at that pace the credit takes about thirty years to come
back. If instead it is the year the shares are sold for a $1,000,000 long-term
gain, regular tax is $51,304 + $184,420 = $235,724, the gain for the AMT is nil,
the tentative tax is again $41,574, and $194,150 of the credit is used, leaving
$99,656 of the original $293,806.

The AMT is computed only for plans that list a company under `private_equity`
(counted or not, so that "count it at zero" compares like with like). A household
with moderate wages and a large long-term gain can owe AMT under 2026 law with no
options at all. Computing it for every plan would have changed the results of plans
that have nothing to do with this feature, and keeping those results as they were
took priority. Section 17 lists the omission. There is no state AMT.

**Small-business stock.** A company flagged `qsbs` has its gains excluded from
federal income under section 1202: for stock acquired after July 4, 2025, 50% after
three years, 75% after four and 100% after five; for earlier stock, 100% after more
than five years; up to $15,000,000 of gain per company ($10,000,000 for earlier
stock). The flag is the user's statement that the stock qualifies. The excluded gain
is outside federal income, the investment-income tax and the AMT. Two
simplifications here are large, and they point in opposite directions.

*State tax is too high outside four states.* Every state taxes the excluded gain as
ordinary income, which is right for California, Alabama, Mississippi and
Pennsylvania and too much elsewhere: a state that follows the federal exclusion
does not tax it. The overcharge is the state's and city's rates on the whole
excluded gain. (A New
York City holder with a $2,500,000 gain excluded in full is charged $284,958 of
state and city tax in the year of the sale, against $26,758 the year after.)

*Federal tax is too low on a sale at three or four years.* The part of the gain
that is not excluded at the 50% and 75% tiers is taxed by law at up to 28%, or the
holder's ordinary rate if lower (IRC 1(h)(4) and (7)). The engine taxes it at the
ordinary long-term rates of 0, 15 and 20%. That understates federal tax by 8 points
on the part it taxes at 20% and by up to 13 points on the part it taxes at 15%:
about 4% of the whole gain for a large sale at three years and 2% at four, so
about $400,000 on a $10,000,000 gain sold at three years. (Single filer in Texas, a
$2,500,000 gain at three and a half years, half excluded: the engine charges
$231,768 on the $1,250,000 that is taxed, and at 28% it would be $350,000, about
$118,000 more.) A plan that sells qualifying stock between three and five years
after acquiring it is therefore shown too favourably; a sale after five years is
not affected.

For incentive-option shares the exclusion
applies to each tax's own gain: the AMT gain is measured from the value at exercise,
so with a fraction `f` excluded only `(1 - f)` of the spread comes off the AMT
gain, and nothing when the gain is excluded in full. The AMT paid at exercise is
then mostly not recovered through the sale. (1,000,000 options at $0.50 exercised
at $5 and sold at $30 after three years, half excluded: the AMT gain taxed is half
of $25 million, not $14.75 million less $4.5 million, and the sale releases about
$468,000 of credit where the earlier rule released $919,000.)

**Paying the tax.** At the event, pay-type equity income has FICA taken and federal
income tax withheld at the supplemental rate: 22%, and 37% on the part of a year's
equity pay above $1,000,000 (IRS Publication 15). FICA here includes the 0.9%
Additional Medicare tax on the part that takes the person's wages for the year over
$200,000, as an employer withholds it whatever the filing status (IRC 3102(f)); a
joint return settles the difference. Nothing is withheld for the state.
The month after, the year's tax is planned again from the actual year to date, and
the rest of what the event costs (the federal shortfall, state tax, the
investment-income tax, any AMT) is paid in level instalments over the remaining
months, or in April after a December event. Salary withholding is sized as if the
equity had not happened, so the equity's tax does not hide in the pay cheque. The
cash for the tax is not invested and sold back: the event month's net proceeds, the
estimated AMT on an exercise and the instalments still to come are held out of the
savings sweep, and the funded test does not count them. (Before that rule a traced
life that retired two months after a $6.3 million acquisition counted $0.8 million
of tax still owed as savings.)

**What "affordable" means for an exercise.** Options are exercised for cash only as
far as cash and taxable savings above the emergency fund (the retirement cash
reserve once retired) and above tax already owed can pay for it. The cost counted is
the strike plus the tax the exercise causes. For a non-qualified option that is
everything the spread adds to the year's return, not only what is withheld: the
return as last planned is computed with and without the whole spread at stake
(federal and state income tax, payroll tax, the Additional Medicare tax), and the
average rate is used, never less than the withholding. Withholding is 22% and the
tax is commonly 40 to 50%; the part beyond withholding is held in cash from the
month of the exercise until the instalments take it. (Sized from the withholding
alone, a household with $600,000 and 200,000 options at $1 against a $9 value
exercised 167,000 of them, owed $771,000 in tax and was left in debt in every
life; it now exercises about 111,000 and stays whole.) For an incentive option the
cost is 28% of the spread, the top AMT rate. The 28% is an estimate held in cash;
the actual AMT arrives through the instalments and can be less, or more inside the
phase-out band. When savings cover only part, that part is exercised. The strike is paid by selling
taxable investments. In a random test of the engine one low-strike incentive grant
cost $10,800 of strike and $106,000 of tax and then paid nothing, so "more paid in
than came back" is mostly tax, not strike, for such a grant.

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
are taxed as wages at vest, and are forfeited when the job ends. Stock and options
in a private company have their own model (section 6.1). A private-company grant
entered on a job instead, which is also where a version 1 plan's stock options
land, keeps the older single-event model: one liquidity event taxed as wages, with
44% of grants worthless after 20 years and 43% paying (0.76 times paper value on
average). Validation points such a grant to the newer section.

Past earnings, needed for Social Security, are rebuilt backwards from today's pay
along the same curve the job follows going forward, capped at each year's wage
base.

### 6.1 Private-company equity

Stock, options and RSUs in a company that is not publicly traded are entered under
`private_equity`, up to four companies. What follows was added after the audit and
was not part of it; its measurements were made for this section with the
command-line build.

**Inputs.** For the company: how far along it is (seed, Series A, B, C, late stage,
or valued above a billion dollars), the price per share in its latest funding round
(the *preferred price*, written `P0` below), and optionally the 409A value of a
common share, the money raised as a share of the valuation, a lean on the odds
(pessimistic, typical, optimistic) or a table of the user's own, and tender-offer
terms. For each holding: its kind (incentive options, non-qualified options, RSUs,
or shares already owned), the number of units, the strike or the cost, how many are
vested and how many months of vesting remain. And a policy: when to exercise, what
to do when options would lapse, how much to sell in tender offers, how to sell
after an IPO, and a planned departure. Nobody is asked for a volatility, a hazard
rate or a value for their own stake.

**Paper value is not money.** The position's value at the latest price is reported
(the `equity_paper_value` series, vested holdings only, options at the price less
the strike) and is in no balance, no net worth figure and no test. Results change
only through cash: strike and tax going out, proceeds coming in.

**One fate per company per life.** The outcome table lists mutually exclusive
outcomes for the next ten years: the company fails, is acquired, goes public, or is
still private. Unless the user gives their own table, it comes from the stage:

| | Seed | Series A | Series B | Series C | Late | Above $1B | Basis |
|---|---|---|---|---|---|---|---|
| Fails within ten years | 60% | 48% | 36% | 30% | 18% | 10% | judgement |
| Acquired | 22% | 30% | 34% | 27% | 30% | 25% | judgement |
| Goes public | 1.5% | 3% | 7% | 12% | 22% | 35% | judgement |
| Still private | 16.5% | 19% | 23% | 31% | 30% | 30% | judgement |
| Years to failure: median of the timing curve before the ten-year cut | 2.5 | 2.5 | 3.0 | 3.0 | 3.0 | 3.5 | judgement |
| Years to acquisition: the same | 5.0 | 4.0 | 3.5 | 3.0 | 2.5 | 2.5 | judgement |
| Years to IPO: the same | 9.0 | 7.5 | 6.0 | 4.5 | 3.0 | 2.5 | judgement |
| Spread of the timing (sigma of the log) | 0.6 | 0.6 | 0.6 | 0.55 | 0.5 | 0.5 | judgement |
| Median years to acquisition as drawn, within a ten-year round | 4.6 | 3.8 | 3.4 | 3.0 | 2.5 | 2.5 | derived |
| Median years to IPO as drawn, within a ten-year round | 6.4 | 5.9 | 5.2 | 4.3 | 3.0 | 2.5 | derived |
| Acquisition price of the whole company over its last valuation, median | 0.70 | 0.70 | 0.65 | 0.60 | 0.55 | 0.55 | judgement |
| Its spread (sigma of the log) | 1.2 | 1.2 | 1.1 | 1.0 | 0.9 | 0.8 | judgement |
| IPO price per share over `P0`, median | 40 | 15 | 6 | 3 | 1.6 | 1.25 | judgement (above $1B: derived) |
| Its spread (sigma of the log) | 1.0 | 0.9 | 0.8 | 0.7 | 0.6 | 0.5 | judgement |
| Money raised over valuation (the overhang) | 0.20 | 0.25 | 0.28 | 0.30 | 0.32 | 0.35 | derived, from figures seen in summary |
| 409A value over `P0` | 0.20 | 0.25 | 0.30 | 0.35 | 0.45 | 0.65 | judgement |
| Chance of a tender offer per year, over all futures | 0.5% | 0.5% | 1.5% | 4% | 8% | 15% | judgement |
| The same where the company ends at or above its last round | 3.2% | 2.3% | 5.6% | 13.2% | 22.3% | 36.8% | derived |

"Judgement" means chosen, with an anchor; "derived" means computed by a stated
step, and a row derived from judgements or from figures seen only in summary is no
firmer than they are (the overhang row rests on Carta dilution figures seen in
summary; the two "as drawn" rows and the second tender row follow from the
judgement rows above them). `data/actuarial/params.json` (block `startup_equity`)
gives the anchor or the step beside each row. The main anchors: 38% of the
2010-2015 Series C cohort had exited within a decade (PitchBook), which sets Series
C exits at 39%; among unicorns about seven in ten had not exited, and the exits to
2022 split 54% IPO, 34% acquisition and 4% failure (Strebulaev), moved toward
acquisition for the mix since 2022; 15 to 31% of seed companies raise a Series A
within two years (Carta), and about 65% of venture financings return less than was
invested (Correlation Ventures), behind the early-stage failure rates. The
timing medians are chosen stage by stage with the NVCA Yearbook (5 to 7 years from
first round to IPO, 3.7 to 5.3 to acquisition) and Stout's PitchBook figures (0.9
years from the last private round to IPO) as anchors; no source gives them by
stage. They are medians of a curve that is then cut off at ten years, so the
median the engine actually draws is earlier: 43% of the seed IPO curve lies beyond
the cut, and the median seed IPO within a round comes at 6.4 years, not 9. Failure
timing is moved by less than 0.05 years. A shut exit window then moves IPO dates a
little further. The unicorn IPO multiple is the median step-up from the last
private round to the IPO, 1.25 (0.55 to 1.80 in one quarter); the earlier stages
divide a median IPO valuation of about $1.9 billion by a typical valuation for the
stage and multiply by the share of the company kept through later rounds. Those
five medians are judgements, not derivations: the stage valuations are chosen, and
$1.9 billion is one quarter's figure and the highest on record (see "How much rests
on the IPO valuation" below). Plus or
minus ten points on any failure probability is within what the sources support, and
that is exactly what the pessimistic and optimistic settings do (the other outcomes
are rescaled to keep the total at one).

A fate is drawn once per company at the start of each life from four random words:

```text
which outcome:  the row k that a uniform u1 falls in
when:           preset   T = 12 x median x exp(sigma x Phi_inv(u2 x Phi(ln(10 / median) / sigma)))
                custom   T = uniform between the row's earliest and latest year
at what price:  m = exp(mu_k + sigma_k x z),  z a standard normal held within +/- 2.326
after an IPO:   L = exp(0.45 x zL) / (its mean),  zL held within +/- 2.326
```

The timing is a lognormal cut off at the ten-year horizon, so its shape below ten
years is kept and its median is earlier than the `median` in the formula (the "as
drawn" rows of the table above). A custom row's low and high multiples are read as the 10th and 90th
percentiles of a lognormal.

**Still private is not for ever.** A fate that lands on "still private" is drawn
again from the same table for the following ten years (by rescaling `u1`, so no
extra random number is used), up to three rounds. A company with no event after
thirty years is written off: its shares and options are worth nothing and the cost
of shares already bought is a capital loss, but the job is untouched. Treating the
ten-year probability as "never" would have left every such life holding paper value
until death. The sources describe companies unexited at ten years that mostly
resolve later; none gives odds for them, so reusing the stage's odds is a
judgement. With `q` the still-private probability, outcome `k` ends up happening
with probability `p_k (1 - q^3) / (1 - q)` and the write-off with `q^3`:

| Over all three rounds | Seed | Series A | Series B | Series C | Late | Above $1B |
|---|---|---|---|---|---|---|
| Fails | 71.5% | 58.9% | 46.2% | 42.2% | 25.0% | 13.9% |
| Acquired | 26.2% | 36.8% | 43.6% | 38.0% | 41.7% | 34.8% |
| Goes public | 1.8% | 3.7% | 9.0% | 16.9% | 30.6% | 48.6% |
| Written off at thirty years | 0.4% | 0.7% | 1.2% | 3.0% | 2.7% | 2.7% |
| Acquisitions that pay common stock nothing | 15% | 20% | 22% | 25% | 28% | 29% |
| IPOs priced below the last round | none | none | 1% | 6% | 22% | 33% |
| A share position pays nothing at all | 76% | 67% | 57% | 55% | 39% | 26% |
| Mean price of a common share at the outcome over `P0` | 1.48 | 1.28 | 1.18 | 0.96 | 0.86 | 0.89 |
| The same within the first ten years only | 1.24 | 1.04 | 0.92 | 0.68 | 0.62 | 0.64 |
| Largest of 100 equally likely payoffs, as a share of their sum | 61% | 35% | 20% | 14% | 8% | 6% |

The first four rows follow from the formula. The rest are from the engine's preview
of a holding of plain shares at each stage (`firesight equity-preview`, 50,000
draws; the mean price is by quadrature and does not depend on the draws). The last
two rows show what the second and third rounds add to the average: 19% at seed and
about 40% from Series C on, all of it resting on the judgement that a company still
private after ten years faces its stage's odds again. The rounds also restart the
timing clock, so failures and exits bunch again in years 11 to 14 and 21 to 24:
the "when" curves show a second and a third step that no data describes.

**How much rests on the IPO valuation.** The IPO medians for the five stages below
$1 billion scale in proportion to the median IPO valuation assumed. $1.9 billion
(Stout, fourth quarter of 2025) is at the top of what is on record. The NVCA
medians were $684 million in 2021, $79 million in 2022, $123 million in 2023 and
$569 million in 2024; they include small biotech listings and two trough years, so
they are not a clean replacement either. The honest position is a range:

| Mean price of a common share at the outcome over `P0` | Seed | Series A | Series B | Series C | Late |
|---|---|---|---|---|---|
| With a $1.9 billion median IPO (the engine's figures) | 1.48 | 1.28 | 1.18 | 0.96 | 0.86 |
| With $569 to $684 million | 0.68 to 0.75 | 0.71 to 0.76 | 0.67 to 0.71 | 0.51 to 0.55 | 0.45 to 0.48 |

The upper payouts shown for a position (its 75th and 90th percentiles) move with
this choice as well as the mean. The share of futures that pay nothing does not.

**The mean is carried by a few outcomes.** At the seed stage three futures in four
pay nothing and the average share is still worth 1.48 times the last round's price,
largely because of the 1.8% that go public at a median of 40 times it. The normal
behind every price multiple is held between its 1st and 99th percentiles, which
removes IPO prices above about 410 times the last round at seed, far beyond
anything the sources describe. That trims the far tail and little else: it lowers
the seed mean by about 4% (1.54 to 1.48) and leaves the concentration where it
was. With the limit in force the top 1% of futures still carry about two thirds of
a seed position's expected value (67%; 38% at Series A, 20% at B, 12% at C; an
independent calculation from the parameters, 4 million draws, not a figure the
engine prints). In the engine's own preview the single best of 100 equally likely
payoffs is 61% of their total at seed and 14% at Series C. This is why the engine
reports the mean of equity proceeds only with its standard error, offers no
scenario metric for it, and why the app shows frequencies and never one value for a
position.

**Liquidation preference.** Acquisition multiples describe what the whole company
sells for over its valuation at the latest round; an IPO multiple is per share. For
an acquisition at multiple `m`, with `l` the overhang (money raised over the
valuation, entered or the stage's):

```text
common price / P0 = m                           if m >= 1
                    max(0, (m - l) / (1 - l))   if m < 1
```

This treats the investors as owning the fraction `l` of the company with a 1x
non-participating preference (98% of recent deals carry a 1x preference and 95% are
non-participating, Cooley): below `m = l` they take everything, and between `l` and
1 the common stock gets what is left. It is one number in place of a cap table.
Dilution by later rounds is not an input either: it is inside the per-share
multiples.

**The price between now and the outcome** is a straight line in logs from today's
price to where the fate is heading:

```text
P(t)   = P0 x m_end ^ (t / T)                    for month t up to the event month T
m_end  = the IPO multiple; max(common multiple, 0.25) for an acquisition;
         0.25 for a failure; 1 for a company that is written off
F(t)   = ratio(t) x P(t),   ratio(t) = r0 + (r_end - r0) x t / T
r0     = 409A value / P0 today (entered, or the stage's)
r_end  = 1 for an IPO; common price / P(T) for an acquisition; r0 otherwise
```

`P(t)` values the paper position and prices tender offers. `F(t)` is the value on
which an exercise is taxed: the 409A discount closes as a good outcome nears. A
line is used because a second random process for valuations between rounds would
need parameters nobody has. Its two awkward consequences are handled directly:
companies heading below their last round run no tender offers (nor does one that
will be written off, which sits at its last round's price for thirty years), and
the household's
own exercise decisions never see `P(t)`. They compare the strike with `P0`, the last
price the household has actually seen, so nobody exercises with foresight. Once a
company is public and past its lockup, its price follows the engine's single-stock
return.

**Tender offers and the exit window.** The window is shut while US stocks are more
than 20% below their previous high or a recession is flagged (IPO counts fell 79 to
88% in 2001, 2008 and 2022, Ritter; Series C and later tenders on Carta went from 14
in the first half of 2023 to 44 in the first half of 2026, NewView). Each month one
random word per company decides a tender:

```text
tender in month t  if  the company is private, t < T, m_end >= 1, it will not be written off,
                       the last tender was at least 12 months ago, the window is open,
                       and u < (1 - (1 - p_up)^(1/12)) / 0.70
p_up = min(1, p x (months companies spend private) / (those months on paths with m_end >= 1))
```

with `p` the yearly chance entered or the stage's. `p` is meant over all companies:
the stage rates were estimated as tenders per company-year across everything
venture-backed, and a user who enters a chance states the same kind of number. Only
companies that end at or above their last round run tenders here, so in those
futures the chance is raised by the share of company-months they account for (the
second tender row of the table above; write-offs are left out of the count). The
rate then averages `p` over all futures. Before this correction a 4% yearly chance
at Series C was realised as 1.4% per company-year. A chance near 100% cannot be met
in full: with a tender every year in every future that allows one, a company
valued above $1 billion still has one in about four futures in ten. A chance that
high is flagged in the plan check (`tender_chance_capped`) with the yearly average
the model can deliver, about 30% for the typical Series C odds, and the preview
reports that figure, not the one entered. The division by
0.70, the share of months with the window open, keeps the chance the same whether
or not the window opens and shuts. A tender the company has already announced can
be entered with
its month; it ignores the window and the direction the company is heading. The
price is `P(t)` times a fixed fraction, for an announced tender too: it is the
model's price in that month, which on a path to a good outcome is above the last
round's price, not a figure the user sets
(default 1: the reported median discount to the last preferred price is 0%, a Carta
figure seen only in summary), each
tender takes up to 20% of vested holdings (the median cap, read directly in
NewView's study), former
employees are not eligible by default, and the household sells the entered share of
what it may (default half). An IPO whose drawn month falls in a shut window is moved
at the start of the life to the nearest open month, earlier or later, within five
years. Acquisitions and failures do not respond to the window.

Measured over 2,000 traced lives per market model. The window is shut in month 60 in
29% of lives under the bootstrap, 35% under the regime model and 36% under the
lognormal model (`params.json` records open shares of 0.71, 0.68 and 0.66 over the
first ten years; one figure, 0.70, is used for all three). An IPO dated to month 60
is moved in those lives, by 10, 13 and 10 months on average when it moves (median 5,
8 and 6; six moves in ten are to an earlier month), which is 2.9, 4.7 and 3.5 months
averaged over all lives. For a company that stays private at its last round's price
for ten years and is the only future there is, a 4% yearly chance gives 3.99, 3.74
and 3.84 tenders per hundred company-years in the three models, and a 20% chance
gives 18.0, 17.2 and 16.8: the twelve-month gap bites at high rates.

**After an IPO** nothing is sold or taxed until the lockup ends (default six
months). The price then is the IPO price times `L`, a lognormal factor with a log
standard deviation of 0.45 (a judgement: 24% of IPOs close below the offer price
on their first day, Ritter; a volatility near 60% a year for a recent IPO, which
over six months gives about 0.42, is an assumption with no source) and a mean
of exactly 1: the lockup spreads the price without raising it. At the lockup's end
options in the money and RSUs are settled for cash, and shares already owned follow
the policy: sell everything, sell in equal yearly parts over up to five years (the
unsold shares stay paper value until each sale), or keep them, in which case each
lot moves to the plan's single-stock holding once it is long-term and, for
incentive-option shares, past both holding periods. The single-stock holding has a
value and a basis and nothing else, so it cannot carry the small-business-stock
exclusion: a kept lot that qualifies for it is not moved but waits, as paper value,
until its gain is excluded in full (five years after it was acquired), and is sold
then. Vested RSUs that the plan says are lost on leaving before the event are safe
once the company has listed: leaving during the lockup forfeits only what is
unvested.

**Vesting, exercise and deadlines.** Unvested units vest in equal monthly parts
after any cliff, only while the linked job lasts. Options can be exercised at three
times: only at a sale or listing (the default, which costs no cash), all vested
options in the first month, or as they vest. Exercise is always subject to the
affordability rule of section 5, and never happens for cash while the strike is at
or above `P0`. Options lapse ten years after their grant, or at the end of the
exercise window after leaving (default three months, which 80% of terminated grants
have or less, Carta). When they would lapse, the policy either exercises what is
affordable (the default) or lets them go. What has not vested when the ten-year
term comes lapses with the rest. A grant entered as already ten years old is
refused, and one within two years of its term is flagged with what the plan will
do then. RSUs wait for a sale or listing and are
paid then.

**At each outcome.** Failure and write-off: everything is worth nothing, the cost of
shares held is a capital loss, unvested units are gone. Acquisition: shares are sold
at the common price, options in the money and vested RSUs are paid in cash as
wages, options under water are cancelled, and unvested units keep vesting and are
paid in cash at the deal price while the job lasts. All of it is cash at closing.
IPO: as above. A sale that pays common stock nothing ends the position.

**The job.** A company can be linked to the job that grants the equity. A layoff,
retirement, the end of the job or a planned departure (`leave_after_months`, which
leaves the salary in place as if moving to a similar job) severs the link the same
month or the next: unvested units are forfeited and the exercise window starts. A
re-hire is taken to be at another employer. If the company fails, the person is
laid off that month, with the usual unemployment spell and pay scar (this can be
switched off). An acquisition or a listing does not touch the job. A household that
retires when funded retires the month after a payout meets the test, and forfeits
what is still unvested: the test sees cash, net of tax owed, and nothing else.

**Measured on the fourth demo household.** An engineer of 31 in San Francisco on
$190,000 with $244,000 in accounts and a Series C position worth $1,458,000 on
paper ($845,000 vested): 60,000 incentive options at $1.50, 40,000 non-qualified
options at $4.00, 12,000 RSUs and 10,000 shares exercised in 2025, a last round at
$14.00 and a 409A value of $5.00. She retires when funded at a 3.5% withdrawal
rate. Her policy is to exercise only at a sale or listing, to exercise what is
affordable when options would otherwise lapse, to sell half of what a tender offer
allows, and to sell over two years after a lockup. Over 20,000 lives (seed 1,
bootstrap):

* The company fails in 42.1% of lives, is acquired in 37.8%, goes public in 16.6%,
  and has no outcome within the life in 3.5% (the table above gives 42.2, 38.0, 16.9
  and 3.0; the difference is lives that end first).
* The equity pays nothing at all in 54.7% of lives. Net of exercise cost and the tax
  the equity caused, proceeds are -$194,000 at the 10th percentile, -$73,000 at the
  25th, $1,000 at the median, $603,000 at the 75th and $1,699,000 at the 90th. The
  mean is $509,000 (standard error $9,200), a figure no single life resembles: half
  of all the positive proceeds come from the best 1,278 of the 20,000 lives.
  Among the 45% of lives that are paid anything: $29,000, $715,000 and $2,921,000 at
  the 10th, 50th and 90th percentiles. By outcome the medians are about $800 where
  the company fails (the tax saved by the loss on the shares already held), -$197,000
  with no outcome, $193,000 acquired and $1,567,000 after an IPO.
* Options are exercised for cash in 49.6% of lives, at a median cost of $164,000,
  nearly always because they would otherwise lapse at their ten-year term or after
  she leaves. In 29.2% of lives more goes in through strike and tax than ever comes
  back. Letting options lapse instead gives up more than it saves: per-life net
  proceeds are higher in 30% of lives and lower in 20%, and $123,000 lower on
  average (95% interval $114,000 to $131,000).
* A tender offer pays something in 12.1% of lives (median $202,000).
* The plan holds in 88.3% of lives as modelled and in 88.1% with the equity counted
  at zero: a difference of -0.2 points (95% -0.6 to +0.2) that cannot be told from
  zero. That is not an error. This household retires when funded, so the equity
  changes when she retires and not whether the plan then holds: the median
  retirement age is 45.8 as modelled and 47.0 at zero, and by outcome 48.3 where the
  company fails, 44.5 where it is acquired and 39.6 where it lists, with success
  flat across the three (89.2%, 86.3%, 90.0%). With retirement fixed at 45 the
  equity carries the plan: 69.3% as modelled against 63.0% at zero (-6.3 points, 95%
  -6.8 to -5.8), and by outcome 55.9% where the company fails, 74.6% acquired and
  92.3% public. In the lives where it fails, counting the equity at zero *raises*
  success from 55.9% to 60.7%, because nothing is spent on options that end
  worthless.
* Exercising the vested options now instead of waiting, on the same lives: savings
  above the emergency fund cover about $52,000 of the $107,000 strike once the AMT
  estimate is counted (section 5), and that part is exercised in the first month.
  Per-life net
  proceeds change by -$54,000 at the 10th percentile, -$6,000 at the median and
  +$160,000 at the 90th (mean +$45,000, 95% interval $42,000 to $48,000): cash and
  tax lost where the company fails, tax saved where it succeeds. Final net worth
  moves by a median of -$75,000 in the lives where the company fails, nothing where
  it is acquired and +$438,000 where it lists. Success rises 0.5 points (95% +0.3 to
  +0.7; 268 lives rescued, 162 harmed) under the funded trigger, and falls 1.3
  points with retirement fixed at 45 (-3.0 points where the company fails, +0.9
  where it lists).

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

Private-company equity counts toward none of this until it is cash. Paper value,
vested or not, is in no balance, no net worth figure and not in the funded test, and
the tax still owed on a payout is taken off the wealth the test sees. For a plan
with such equity every headline is therefore also shown with the equity counted at
zero, on the same simulated lives, and split by what happened to the company. There
is no equity cause among the failure labels: a life that fails because the company
paid nothing keeps the label it would have had, and the split by outcome is where
the difference shows (section 6.1).

## 14. Monte Carlo design

**Random streams.** The generator is xoshiro256++, seeded through SplitMix64, with
one independent stream per `(seed, path, subsystem)`: market, inflation, each
person's career, equity compensation, each private company, each person's health,
each person's longevity, expense shocks, each property, and life events. The number
of draws from
a stream never depends on a decision. So two scenarios that differ only in a
decision (rent or buy, claim at 62 or 70) see the same markets, the same layoffs,
the same illnesses and the same lifespans.

A private company's stream is read exactly four times at the start of a life (which
outcome, when, at what price, and the lockup factor) and once a month after that
(whether a tender offer arrives), for every company, whether or not it is counted,
has holdings or its owner is alive. The IPO month is fixed at the start from the
economy's path, which exists before any decision is taken. So a change of policy
(exercise now or later, sell in tenders or not, leave the company, count the equity
at zero) meets the same fate, the same prices and the same tender months on every
life, and its effect is a paired difference like any other. A plan with no company
reads no such stream and computes no AMT, and its results are bit for bit what they
were before the feature existed: the three older demo households' golden digests
did not change. Adding a company, even an empty one, moves no random number but
does switch the AMT on, which can change the tax of a life that realises a large
gain.

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
digests of the four demo households change only deliberately.

**Private-company equity.** In check mode the conservation identities hold with two
more terms (equity income in, exercise cost out), share counts are conserved through
every exercise, sale and lapse, and a fixed-sequence random test runs one to four
random companies on each demo household under every policy, holding kind and market
model (240,000 lives passed when it was widened). The fate drawn in a simulated
life is the one the editor's preview draws; the simulated outcome shares of the demo
match the lifetime odds of section 6.1 within sampling error; the AMT, the credit
and the sale that releases it match returns computed by hand (section 5).

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
* **Private-company equity** (`data/actuarial/params.json`, block `startup_equity`).
  Every number there is tagged sourced, derived or judgement, every source as read
  directly or known only from a summary, and every derived number with what it was
  derived from. Only the 20% tender cap is sourced from a document read directly;
  the tender price and the exercise window are sourced from summaries, and the rest
  is judgement with anchors. Read directly: NewView Capital's 2026
  study of tender offers on Carta (the 20% median cap, eligibility of former
  employees, tender counts by half-year); Stout's venture update for Q4 2025 with
  PitchBook figures (the step-up from the last private round to the IPO and the time
  between them); the NVCA 2025 Yearbook (years from first round to exit, deal
  sizes); Jay Ritter's IPO statistics (IPO counts by year, age at IPO, first-day and
  six-month returns); the PitchBook-NVCA Venture Monitor for Q3 2026 (secondary
  discounts and two large sales far below peak valuation); Collective Liquidity's
  article reporting PitchBook's 2010-2015 Series C cohort (38% exited within a
  decade); OnlyCFO on the 409A discount at a fundraise. Known only from summaries:
  Carta's tender update for the first half of 2026 (median price at the last
  preferred price), Carta's figures on dilution per round, seed-to-A graduation and
  post-termination exercise windows, Cooley's deal terms, Strebulaev on unicorn
  exits, Correlation Ventures on returns per financing. Tax: IRC 53, 422 and 1202 at
  Cornell LII and IRS Publication 15 (2026) were read directly; the AMT amounts are
  from Rev. Proc. 2025-32; the 26% and 28% AMT rates, the treatment of gains inside
  the AMT (IRC 55(b)(3)) and the rule that incentive-option income bears no payroll
  tax (IRC 421(b), 3121(a)(22)) are recalled and agree with the Form 6251
  instructions and practitioner sources, but the sections were not re-read. The
  outcome probabilities, price spreads and per-company tender rates are judgements
  anchored to these sources, not estimates from data.

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
  (1.3% of wages), the alternative minimum tax for households without
  private-company equity, state AMT, the chained-CPI wedge, state itemised
  deductions beyond the basics, most state credits.
* The AMT is computed only for plans that list a private company (section 5). A
  household without one that realises a very large long-term gain would owe some
  AMT under 2026 law, and the engine charges none. In an AMT year the
  Roth-conversion sizer still fills the regular bracket, which is not the one that
  binds.
* The first simulated year uses one inflation factor for 2027 rather than each
  published 2027 figure (tax effect under $20; contribution limits off by up to
  $250 to $1,000 at the cap).
* A tax refund larger than the tax paid that month is recorded as other income,
  which overstates gross income and lifetime tax by the refunds (about 3.5% of tax
  paid on a typical path). Cash is unaffected.
* In the first, partial year a full year's retirement contributions are made in
  the remaining months, which understates that year's tax; it comes back in April.
* A household abroad is taxed at one flat rate for the whole calendar year.

**Private-company equity**

The model answers "what does this position do to the plan across the ways it could
turn out". It does not value the company. In rough order of how much each could
move an answer:

* There is no view of this particular company. The odds are base rates for its
  stage or the user's own table, and the stage rates are judgements anchored to
  cohort data (section 6.1). Results are always shown with the equity counted at
  zero as well.
* No cap table and no preference waterfall: no series by series terms, no
  participating preferred, no multiples above 1x, no ratchets. One overhang number
  and a linear rule stand in for all of it.
* No valuation path with rounds, down rounds or repricing, and dilution is not an
  input. The price moves in a straight line in logs to the drawn outcome, the 409A
  discount closes along it, and dilution is inside the per-share multiples.
* No source gives odds for a company still private after ten years. The stage's
  ten-year odds are applied again, up to thirty years; then the position is written
  off.
* Price multiples beyond the 1st and 99th percentiles of their spread are capped.
  The mean of equity proceeds is still carried by a few lives, is reported with its
  standard error, and is not a scenario metric.
* Tender offers: none in companies heading below their last round or to a write-off
  (the yearly chance entered is an average over all futures, so it is higher than
  entered in the futures that have any), at most one a year, at the path price times
  a fixed fraction with no random discount; an announced tender is priced the same
  way, not at a price the user states. Secondary
  sales outside company tenders, exercise financing, forward contracts and exchange
  funds are not available to the household.
* No state AMT (California's is 7%) and no state rules for incentive options:
  exercising and holding in California is understated by up to 7% of the spread,
  part of it recoverable later.
* Small-business stock: the federal exclusion with the dollar cap only. Every state
  taxes the excluded gain, which overstates tax everywhere but California, Alabama,
  Mississippi and Pennsylvania by the state's rate on the whole gain. The part not
  excluded at three and four years is taxed at 0, 15 and 20%, not at the 28% the law
  sets, which understates federal tax by 8 to 13 points of that part (about 4% of
  the whole gain at three years and 2% at four; section 5 has measured examples).
  The ten-times-basis cap, the gross-assets test at each exercise, section 1045
  rollovers and stacking are not modelled.
* The $100,000 limit on incentive options first exercisable in a year is not applied.
  A warning measures it over the whole grant and the editor offers an estimated
  split into incentive and non-qualified parts; until a holding is split, exercising
  a large incentive grant is taxed too lightly.
* Tender or buyback proceeds above the 409A value can be pay; here all gain on
  shares sold in a tender is capital gain.
* Incentive-option shares kept after a lockup give up their AMT basis when they move
  to the single-stock holding (they move only once long-term and qualifying). Kept
  shares that qualify as small-business stock are not kept for ever: they are sold
  once their gain is fully excluded, five years after they were acquired, because
  the single-stock holding cannot carry the exclusion. Gain after that sale would
  have been excluded too, up to the cap; here it is not.
* Shares kept as single stock enter the equity's proceeds at their value on the day
  they move, and the equity is charged, as an estimate, the tax their gain would
  bear if sold that year. The household pays the actual tax when it sells the
  single stock, and that later tax is not attributed to the equity.
* Equity income is taxed by the state of residence in the year it is paid, with no
  sourcing to the state where it was earned. No state supplemental withholding at
  the event: state tax is paid through the instalments and the return. The 0.9%
  Additional Medicare tax is withheld at the event on equity pay only; on salary
  it is settled with the return, as before. California SDI is not charged.
* An incentive option exercised in the same calendar year in which the company then
  fails is treated as disposed of that year, so its AMT adjustment is undone. The
  law on worthless shares is not settled; the harsher reading leaves AMT on income
  that was never received.
* A shut exit window moves IPO dates and delays tenders. It does not lower
  acquisition prices or raise failure rates, and the company's fate is otherwise
  independent of the stock market and of the household's other investments.
* Equity of a partner who has not yet joined the household is outside it: nothing
  is exercised and anything paid before the join date is not counted. It goes on
  vesting meanwhile, on the linked job's own dates, with no layoff risk until the
  partner joins.
* The editor's preview of a position simulates no household. It applies the option
  deadlines (the exercise window of a former employer or a planned departure, and
  the ten-year term) as the simulation does, and takes a deadline exercise to be
  affordable; it assumes the holder otherwise stays in the job until the outcome,
  leaves tender offers and tax out, and deflates at 2.9% a year, the rate that
  matches the simulation's median price level after ten years. The simulation's
  results are lower where the job ends before the outcome or savings cannot pay
  for an exercise.
* Acquisitions pay cash at closing. No escrow, holdbacks or earn-outs, no
  stock-for-stock deals, no acceleration, no assumption of unvested equity on new
  terms: unvested units keep vesting and pay cash at the deal price while the job
  lasts.
* Only vested options can be exercised: no early exercise, no 83(b) or 83(i)
  election. RSUs wait for the event indefinitely (plans commonly expire them after
  seven years), and single-trigger private RSUs are not modelled.
* After an IPO, options and RSUs still unsettled when the lockup ends are sold for
  cash; only shares already owned follow the sell-down policy. Shares scheduled for
  sale in later years are paper value until each sale and follow the generic
  single-stock return, not one specific to the company.
* No refresh grants and nothing granted by a later employer: enter expected grants
  as holdings.
* The retirement trigger sees cash only, net of tax owed. A household does not stay
  for a vest or retire on paper wealth, and it forfeits what is unvested when the
  trigger fires.
* The exercise decision is a rule: exercise what savings above the emergency fund
  can pay for, strike and tax, whenever the strike is under the last round's price.
  It does not wait for news, exercise part of a grant by choice, or borrow. The AMT
  counted in that test is a flat 28% of the spread; the tax counted on a
  non-qualified option is the average rate on the whole spread at stake, taken from
  the year as last planned, which overstates the rate when only part is exercised.
* Failure causes have no equity label (section 13); the split by outcome shows it
  instead. With several companies the outcome statistics describe the largest
  position, and proceeds and costs are totals.
* A failed company gives a capital loss only: no section 1244 ordinary loss, no wash
  sales, no section 409A failures, no 280G.
* Shares already owned are asked for their purchase date but not their grant date,
  so the two-year incentive-option rule is taken as met once a year has passed since
  exercise.
* Version 1 stock options and private-company grants entered on a job keep the older
  single-event model (section 6); a warning points to the newer section.
* The web app has no "what exit do I need" control and no results by exit-price
  band; the assistant can solve for a certain listing at a multiple of the last
  round's price. There is no scenario metric for equity proceeds: the mean is too
  noisy and the medians are often zero.
* In a first, partial year the withdrawal planner does not see a one-off equity
  amount; the year's return does.

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
