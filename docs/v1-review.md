# FIRESight v1: relationship-by-relationship audit

Scope: every file in the v1 repository was read in full (`FIRE.py` 1,508 lines, `scenarios.py`, `assistant.py`, `storage.py`, `Home.py`, the five files in `pages/`, `docs/methodology.md`, the three `examples/*.json`, the three test files, and the bundled data files). File and line references are to the v1 tree.

How claims were checked. Nothing from the v1 tree was executed. Every bug below was confirmed by tracing the code path line by line. Where the claim is arithmetic (calendar drift, bonus cadence, loan pay-off, Markov chain means, tax amounts, data statistics) the formula was re-implemented in a standalone script and run; those are marked **replicated**. Everything else is marked **traced**.

Verdict vocabulary: **sound** (keep the idea, maybe the code), **simplistic** (defensible toy, wrong magnitude or missing dependency), **wrong** (produces an incorrect number or does something other than what the UI and docs say), **dead** (parsed or collected, never used).

---

## 0. What v1 was trying to build

The intent is stated in `docs/methodology.md:3`: "given my income, spending, debts, portfolio and plans, at what age am I financially independent, and how sure is that?" The approach is to live out one household's finances a month at a time, many times, under different simulated markets, and report a distribution rather than a date.

Ideas worth keeping in v2:

1. **A month-by-month household ledger** (`User.step_month`, `FIRE.py:1416-1472`). Income, bills, debt service, cash policy, market returns, then a decision check, in a fixed order. This is the right skeleton. It lets planned events and market luck interact, which closed-form calculators cannot do.
2. **Block bootstrap of history** for equities and home prices (`FIRE.py:144-159`). Sampling contiguous blocks keeps crashes and recoveries in their real shape. The mistake was doing it per asset instead of sampling whole rows of a joint history.
3. **Pre-generated return paths per run** (`FIRE.py:92`, `833-838`). Generating the whole path up front is what makes common random numbers possible.
4. **Common-random-number scenario comparison** (`scenarios.compare`, `scenarios.py:166-179`). Running baseline and what-if on one seed so the difference is the decision and not luck. The implementation only half delivers this (entry 84) but the idea is the most valuable thing in the scenario engine.
5. **The LLM calls the simulator as a tool** (`assistant.py:57-74`, `114-128`) and is told never to quote a number it did not get from a run (`assistant.py:84`). Right division of labour.
6. **A flat "changes" vocabulary** that both a form and an LLM can produce (`scenarios.apply_changes`, `scenarios.py:39-98`).
7. **The nested "reduced income" stress test** (`FIRE.py:1326-1341`): decide whether you can stop working by simulating forward from the current state without the paycheck. This is the correct definition of readiness (forward-looking, state-dependent, conditional on today's portfolio and obligations). v1's version is noisy, nominal and too short, but the concept should become v2's primary retirement test.
8. **Life milestones on dates** (`FIRE.py:806-816`, `1055-1067`): house purchase, financed purchase, windfall, move, family change.
9. **Spending that changes when you stop working** (`BudgetItem.value_if_not_working`, `FIRE.py:714-724`), which is how v1 lets users express the health-insurance cliff.
10. **Heterogeneous income types** with their own mechanics (overtime, bonus cadence, commission noise, vesting, rentals) instead of one "salary" number.
11. **Demo households** (`examples/`) so the tool is usable without typing in a life.
12. **A monthly extension hook** (`monthly_hook`, `FIRE.py:1467-1472`).

What v1 actually answers is narrower than the intent: "in what share of N nominal, independent-asset market paths does net worth (including home equity and paper option value) touch a target within the horizon." It never asks whether the money then lasts.

---

## 1. Time and calendar

### 1. Simulation clock

- **v1.** Start date is the wall clock: `self.time = datetime.datetime.now().date()` (`FIRE.py:854`). Each step does `self.month+=1; self.time+= timedelta(days=30); self.age+= 1/12` (`FIRE.py:1417-1419`).
- **Verdict: wrong.** Twelve steps are 360 days, so the calendar loses 5.25 days a year against age and against the step counter. **Replicated:** a 30-year run (360 steps) ends 29.57 calendar years after it starts, a 50-year run 49.28. Anything keyed on the date (milestones, income growth, IPO date) uses one clock; anything keyed on `month` or `age` (taxes every 12 steps, Social Security at 67, horizon) uses another. Because 30-day steps do not align with months, a 31-day month can be visited twice and February can be skipped: starting 2026-11-17, a 30-year run visits December 2029, March 2030, October 2035, July 2041, January 2047, May 2047 and October 2052 twice and never visits February 2030 or February 2047. Averaged over all start days, a 30-year run has 6.9 double-visited months and 1.7 skipped months. Results also depend on the day you press the button, so a seed does not reproduce across days.
- **Rigorous.** Integer month index `t = 0..T-1` from a fixed start (year, month). Calendar year and month derived by integer arithmetic. Ages derived from birth year and month. No date arithmetic in the hot loop.
- **Not exposed.** Start date; birth month.

### 2. Age and horizon

- **v1.** `age` is a float incremented by 1/12 (`FIRE.py:1419`). Horizon is `simulate_years` from today (`FIRE.py:878`, `912`, `1475`), slider 1 to 50, default 10 (`pages/Edit Profile.py:712`). `birth_year` is computed and never used (`FIRE.py:872`). `spouse_age` is stored and never used (`FIRE.py:875`, `1073`).
- **Verdict: simplistic.** The horizon is a UI convenience ("a few years longer than your expected retirement age", `pages/Edit Profile.py:713`), not a lifetime. The early-career example stops at age 57. No one dies; nothing is asked about ages 57 to 95.
- **Rigorous.** Simulate each person to a stochastic death age drawn from cohort mortality tables (SSA period tables with improvement, optional health adjustment), joint for couples. The run ends at the second death. Horizon is not a user input.
- **Not exposed.** Sex, health status, mortality table choice, planning-age override.

### 3. Order of operations inside a month, and the second income call

- **v1.** Order (`FIRE.py:1416-1472`): advance clock, milestones, grow income, grow budget, leases, `inc = calculate_monthly_income()`, expenses, `cash += inc - exp`, debt service, home payment, `manage_cash()`, market returns, net worth, record, retirement check, taxes every 12th step, hook. `manage_cash` computes its target as `self.emergency_fund_target + 3*self.calculate_monthly_income()` (`FIRE.py:1257`).
- **Verdict: wrong.** `calculate_monthly_income()` is not a pure getter. `IncomeSource.get_monthly_income` (`FIRE.py:574-652`) decrements the bonus counter, increments `months_at_job`, draws commission sales and calls `self.home.make_payment()` for rentals. It therefore runs **twice per simulated month** (lines 1431 and 1257). **Traced and replicated** consequences: bonuses are paid at the wrong cadence or never (entry 26); rental mortgages amortise and rental property appreciates twice per month (entry 51); option tenure and cliffs run at double speed (entry 29); the per-source `income_<name>` column recorded at `FIRE.py:1307` holds the second call's value, not the one that was paid. Separately, surplus cash is invested before the month's return is applied (`FIRE.py:1452-1453`), so each contribution earns a full month it was not invested for.
- **Rigorous.** State transitions happen exactly once per step in one function; reads are pure. Fixed, documented timing convention: start-of-month balances earn the month's return, flows happen at month end.
- **Not exposed.** n/a.

---

## 2. Market returns and the economy

### 4. Joint behaviour of asset classes

- **v1.** Each asset class with a non-zero allocation gets its own `InvestmentVehicle`, which builds its own `ReturnGenerator` and its own path (`FIRE.py:914-923`, `833-838`). The owned home builds another (`FIRE.py:412`), each rental property another (`FIRE.py:530`).
- **Verdict: wrong.** There is no economy. Stocks, bonds, home prices and the two "stock" sleeves (entry 6) are statistically independent of each other and of inflation, wages and employment. Independence manufactures diversification that does not exist: the 2008 pattern (equities down 50%, home prices down, job loss up) cannot occur except by coincidence. Left-tail outcomes, which are what a retirement plan is tested against, are understated.
- **Rigorous.** One joint economic scenario generator per path producing, for every month, inflation, equity total return, bond return (from a yield curve), bill rate, home-price growth, wage growth and a recession state. Either bootstrap whole rows of a joint monthly history (stationary bootstrap with random block lengths) or fit a regime-switching VAR; both preserve cross-correlations and serial dependence.
- **Not exposed.** Any correlation, any regime assumption, any choice of historical window.

### 5. Equities ("Index Funds"): block bootstrap of `stocks.csv`

- **v1.** `block_bootstrap` (`FIRE.py:144-159`) concatenates random 12-month blocks: `start_idx = random.randint(0, length - self.block_size)`. Source is `stocks.csv`, 1,115 monthly values from 1928-01 to 2020-11 (`FIRE.py:42`).
- **Verdict: simplistic, with a data error.** The method is the good idea. Problems: (1) **Replicated** statistics of the file: monthly mean 0.619%, annualised volatility 18.6%, geometric return **5.84% a year**. That matches the S&P price index over the period, not total return (nominal total return over the same span is roughly 9.7%). The file and the docs call it "total-market returns" (`FIRE.py:42`, `docs/methodology.md:75`). Dividends are missing, so equity growth is understated by about 4 points a year. (2) Fixed block length with non-circular starts undersamples the first and last 11 months and puts a seam every 12 months exactly. (3) Returns are nominal with their historical inflation embedded while the model's inflation is a separate constant. (4) Data ends in 2020. (5) No fees.
- **Rigorous.** Real total returns (dividends reinvested), sampled jointly with the other series (entry 4), stationary bootstrap with geometric block lengths, optional valuation-aware mean adjustment, explicit expense ratio.
- **Not exposed.** Block length, expected-return haircut, fee, international or small-cap split, history window.

### 6. "Retirement Accounts" as an asset class

- **v1.** `'Retirement Accounts'` is in `INVESTMENT_TYPES` (`FIRE.py:46`) with its own allocation slider, and `generate_returns` gives it the same stock bootstrap as index funds (`FIRE.py:97-98`) on an independent draw.
- **Verdict: wrong.** A retirement account is a tax wrapper, not an asset. In v1 it is a second, uncorrelated stock market. A 55/25 split between "Index Funds" and "Retirement Accounts" (the dual-income example) has lower volatility than 80% equities because the two sleeves diversify each other. There is no contribution limit, no employer match (entry 32), no tax deferral, no penalty before 59 and a half, no RMD.
- **Rigorous.** Accounts (taxable, traditional, Roth, HSA, cash) are containers; each holds an asset allocation over the same economy-wide asset returns. Contribution limits, match formulas, tax treatment and access rules live on the account.
- **Not exposed.** Account type of every balance, contribution rates, match, Roth versus traditional split.

### 7. Bonds

- **v1.** Three-state Markov chain (`FIRE.py:165-200`): normal `N(0.002, 0.001)`, rising rates `N(-0.001, 0.001)`, falling rates `N(0.004, 0.001)` per month; transition rows `[.6,.2,.2]`, `[.5,.3,.2]`, `[.5,.2,.3]`; initial state uniform.
- **Verdict: simplistic to wrong.** **Replicated:** stationary distribution (0.556, 0.222, 0.222), mean 2.15% a year, annualised volatility about 0.7%. That is a savings account, not a bond fund (aggregate US bonds: roughly 4 to 6% volatility, a 13% loss in 2022). At the default 3% inflation the real return is about -0.8% a year forever. No link to inflation or to equities, so the main reasons to hold bonds (deflation hedge, crash ballast) and the main risk (inflation shock) are both absent. Docstring calls the transition probabilities "somewhat arbitrary" (`FIRE.py:169`).
- **Rigorous.** Simulate a yield (level, optionally slope) jointly with inflation; bond fund return = carry + duration times yield change + convexity. TIPS as a separate real-yield asset.
- **Not exposed.** Duration, starting yield, credit quality, nominal versus inflation-linked.

### 8. High-yield savings

- **v1.** Two-state chain (`FIRE.py:202-228`): stable `N(0.0016, 0.0004)`, bonus `N(0.002, 0.0004)`.
- **Verdict: simplistic.** **Replicated:** 1.99% a year, unrelated to inflation or policy rates, so cash loses 1% real a year at default inflation regardless of regime. The rate the user typed for the account is discarded (entry 19).
- **Rigorous.** Cash earns the simulated bill rate minus a spread.
- **Not exposed.** Spread to the bill rate.

### 9. Peer-to-peer lending

- **v1.** Two-state chain (`FIRE.py:230-257`): normal `N(0.004, 0.002)`, meltdown `N(-0.02, 0.01)`, P(normal to meltdown) 0.05.
- **Verdict: simplistic.** **Replicated:** 2.9% a year (docstring says 5 to 6%). Defaults are not tied to recessions.
- **Rigorous.** Drop as a first-class asset. If kept, model as credit: yield minus default losses that rise in the recession state.
- **Not exposed.** n/a.

### 10. Individual stocks

- **v1.** Three-state chain (`FIRE.py:259-286`): bull `N(0.015, 0.02)`, bear `N(-0.015, 0.02)`, sideways `N(0, 0.005)`.
- **Verdict: wrong.** **Replicated:** stationary (0.457, 0.257, 0.286), mean **3.7% a year** with about 7% volatility. The docstring says "~10% annual". A single-stock sleeve has lower expected return and lower volatility than the index, the opposite of reality (same expected return as the market at best, with two to three times the volatility). Independent of the index.
- **Rigorous.** Market return times beta plus idiosyncratic noise (lognormal, 25 to 40% annual idiosyncratic volatility), with a concentration parameter.
- **Not exposed.** Beta, idiosyncratic volatility, concentration.

### 11. Cryptocurrency

- **v1.** Three-state chain (`FIRE.py:288-315`): mania `N(0.05, 0.08)`, crash `N(-0.08, 0.08)`, normal `N(0, 0.03)`.
- **Verdict: wrong.** **Replicated:** stationary (0.324, 0.382, 0.294), mean **-1.44% a month, about -16% a year arithmetic and -20% geometric**, 31% volatility. Any crypto allocation is a guaranteed long-run loss; contributions are routed into it every month by `add_investments` (`FIRE.py:1270-1277`). The side-hustle example holds 5%.
- **Rigorous.** Treat as a high-volatility satellite with a user-stated expected real return and volatility, correlated with equities in crashes; default expected real return zero and say so.
- **Not exposed.** Expected return, volatility.

### 12. "Other"

- **v1.** Three-state chain (`FIRE.py:317-344`): stable `N(0.005, 0.003)`, mild bull `N(0.01, 0.005)`, mild bear `N(-0.005, 0.005)`.
- **Verdict: simplistic.** **Replicated:** 4.1% a year (docstring: "~7% annual"), 2.4% volatility. An undefined asset with a made-up distribution.
- **Rigorous.** Remove. Offer a "custom asset" with explicit real return, volatility and equity correlation.
- **Not exposed.** All of it.

### 13. "Rental Real Estate" as a portfolio sleeve

- **v1.** Bootstrap of `realestate.csv` (`FIRE.py:100-119`), then by `real_estate_investment_type`: `"long term rentals"` and default `"REIT"` use the raw series; `"short term rentals"` adds `N(0, |r|*0.1)`; `"house flipping"` uses `N(r*0.5, 0.1)` per month.
- **Verdict: wrong.** **Replicated** statistics of `realestate.csv`: 400 undated months, 3.9% a year, annualised volatility 2.9%, lag-1 autocorrelation 0.54, lag-12 autocorrelation 0.67. It is a smoothed home-price index. Using it as a REIT gives a low-return, near-riskless asset with no rent yield (listed REITs: equity-like volatility around 20%, high equity correlation, 4%-ish dividend yield). "House flipping" halves the mean and adds a 10% monthly standard deviation (35% annualised). `real_estate_investment_type` has no field in the profile editor, so UI users always get "REIT". Bootstrapping a series with 0.67 autocorrelation at lag 12 in 12-month blocks destroys the multi-year housing cycle.
- **Rigorous.** REITs are an equity sub-asset with their own total-return history. Direct property is a separate object (section 9) with value, loan, rent and costs.
- **Not exposed.** The type selector itself.

---

## 3. Inflation

### 14. Inflation level

- **v1.** A constant: `self.inflation = data.get('inflation', 0.03)` (`FIRE.py:880`), slider 0 to 10% in 1-point steps (`pages/Edit Profile.py:718`).
- **Verdict: simplistic to wrong.** No inflation uncertainty, no persistence, no 1970s or 2021-23 path, and no relationship to asset returns. Inflation risk is one of the two or three largest risks in a 50-year retirement and v1 has none.
- **Rigorous.** Stochastic, persistent inflation from the scenario generator, jointly with bill rates, bond yields, equity returns, wages and home prices. Separate medical inflation.
- **Not exposed.** Anything about volatility or persistence; category-specific inflation.

### 15. What inflation touches, and the unit of account

- **v1.** Inflation is used in exactly two places: budget growth (`FIRE.py:1426-1427`) and the safe-withdrawal target (`FIRE.py:1322-1323`). It does not touch wages, tax brackets, the standard deduction, the FICA wage base, the child credit, Social Security, the emergency fund, rents, home prices, insurance, `average_post_retirement_income` or the fixed-value goal. All outputs are nominal dollars; charts are labelled "Net Worth" with no unit (`pages/Run Simulation.py:129`).
- **Verdict: wrong.** A 30-year nominal net worth figure is not interpretable (at 3%, $1 then is 41 cents now). Mixed indexing creates artefacts: brackets creep (entry 70), the retirement-income offset shrinks in real terms (entry 72), rent received never rises while rent paid does (entry 51).
- **Rigorous.** Simulate in nominal terms with a cumulative price index per path, apply statutory indexing rules to every dollar threshold, and report everything deflated to today's dollars.
- **Not exposed.** Real versus nominal display.

---

## 4. Portfolio, allocation and cash

### 16. Starting balances and the allocation

- **v1.** All balances whose type is in `PORTFOLIO_ACCOUNT_TYPES` are summed into one number (`FIRE.py:894`) and re-split by the nine allocation sliders: `self.vehicle_balances = {v.name: v.ratio*self.investment_balance ...}` (`FIRE.py:926`). Allocations must sum to 100 within 0.5 or the constructor raises (`FIRE.py:905-906`).
- **Verdict: wrong.** What the user says they hold is discarded and replaced by the sliders. The dual-income example's $245k 401(k) and $68k brokerage become $313k split 55/15/25/5; the landlord's $9k crypto wallet is redistributed. The asset list and the allocation are two unrelated inputs that look related. A fresh profile defaults to an equal 11.1% in each of nine classes, including crypto and P2P (`pages/Edit Profile.py:826-829`). A profile with no allocation keys cannot be simulated at all.
- **Rigorous.** Each account has a balance, a cost basis where relevant and its own allocation (or a household target allocation with asset location rules).
- **Not exposed.** Per-account holdings, cost basis.

### 17. Contributions, withdrawals, rebalancing, fees

- **v1.** Surplus cash is split by target ratio (`add_investments`, `FIRE.py:1270-1277`). Withdrawals are pro rata to current balances (`liquidate_investments`, `FIRE.py:1214-1228`). Returns compound per sleeve (`FIRE.py:1192-1197`). Nothing rebalances.
- **Verdict: simplistic.** With no rebalancing the mix drifts toward whatever grew most, so the stated allocation describes only month one. No glide path. No fees. No dividends or interest as cash flows. No tax on sales. Savings is a residual, not a decision: there is no 401(k) deferral, no IRA, no HSA.
- **Rigorous.** Contribution waterfall (match, HSA, tax-advantaged limits, taxable), periodic or band rebalancing done inside tax-advantaged accounts first, optional glide path, expense ratios, tax-lot or average-cost basis on sales.
- **Not exposed.** Rebalancing rule, glide path, fees, savings waterfall.

### 18. Cash balance and interest

- **v1.** `self.cash = sum(a.value ... if a.type in CASH_ACCOUNT_TYPES)` (`FIRE.py:893`), and those `AssetOrDebt` objects are then filtered out (`FIRE.py:895-896`).
- **Verdict: wrong.** **Traced:** cash earns nothing, ever. `AssetOrDebt.update` would apply interest (`FIRE.py:781-784`) but is never called for cash accounts because they are no longer in `self.assets_and_debts`. The interest rate the user enters for a savings account (the examples enter 4.0 and 4.2) is dropped. `docs/methodology.md:24` says "Savings and checking accounts earn their interest"; they do not. `tests/test_fire.py:115-119` tests interest on a checking object that no simulation ever updates. The cash buffer is large (entry 19), so this is a material drag.
- **Rigorous.** Cash earns the simulated bill rate less a spread, taxed as ordinary income.
- **Not exposed.** n/a (the input exists and is ignored).

### 19. Emergency fund and cash policy

- **v1.** `self.emergency_fund_target = 6*self.calculate_monthly_expenses()` in the constructor (`FIRE.py:947`). Each month `desired = self.emergency_fund_target + 3*self.calculate_monthly_income()`; excess is invested, a deficit is refilled from investments (`FIRE.py:1255-1268`).
- **Verdict: wrong.** **Traced:** the six-month figure is computed once from month-zero expenses and never updated (the only other assignment copies it into the stress-test clone, `FIRE.py:1401`). It erodes with inflation and ignores a new mortgage or child. The "three months of income" term means higher earners hold more idle cash and the target collapses at retirement, when a cash bucket matters most. A bonus month inflates the target. Total buffer for a 50%-saver is about a year of spending earning 0%. The target is not adjustable.
- **Rigorous.** Target expressed in months of current essential spending, user-set, recomputed monthly; optionally a retirement cash bucket (one to three years of spending) as part of the withdrawal strategy.
- **Not exposed.** Months of expenses; whether income counts; retirement bucket.

### 20. Shortfalls and negative cash

- **v1.** Three bill payers share one pattern (`update_assets_and_debts` `FIRE.py:1204-1212`, home payment `1442-1450`, `pay_annual_taxes` `1242-1250`):

  ```python
  if self.cash>= total_pmt:
      self.cash-= total_pmt
  else:
      needed= total_pmt- self.cash
      self.cash=0
      if self.investment_balance>= needed:
          self.liquidate_investments(needed)
      else:
          self.investment_balance=0
  ```

  and `liquidate_investments` ends with `self.cash+= amount` (`FIRE.py:1228`).
- **Verdict: wrong.** Four defects, all **traced**:
  1. **The shortfall is never paid.** After liquidation the proceeds are credited to cash and nothing debits them. The household pays only the cash it had; `needed` is forgiven.
  2. **Negative cash is erased every month.** Milestones subtract from cash without a check (`FIRE.py:1090`, `1145`, `1151`). `update_assets_and_debts` runs every step even with no debts; with `cash < 0` and `total_pmt = 0` the else branch sets `cash=0` and then moves `|cash|` from investments into cash, leaving cash at `+|cash|`. A down payment or cash purchase larger than the cash buffer is therefore partly free: the overdraft is forgiven. With the early-career example (down payment $84k against a cash buffer of roughly $50k by the purchase date) on the order of $30k is created in the purchase month.
  3. **`self.investment_balance=0` is a no-op.** It does not touch `self.vehicle_balances`, and `calculate_investment_return` recomputes `self.investment_balance = sum(self.vehicle_balances.values())` in the same step (`FIRE.py:1197`). The bill is unpaid, cash is zero and the investments are intact.
  4. **`manage_cash` duplicates money near depletion.** In its else branch (`FIRE.py:1266-1268`) `self.cash+= self.investment_balance; self.investment_balance=0` credits cash with the whole portfolio and, for the same reason as (3), the portfolio is still there next month. A nearly broke household receives its residual portfolio as free cash every month.

  Net effect: cash is floored at zero at every record point, spending the household cannot fund is silently dropped, and ruin is invisible in the output.
- **Rigorous.** One funding waterfall with conservation of money asserted in tests: cash, then taxable (realising gains), then tax-advantaged in the configured order (with tax and penalties), then optional borrowing (HELOC or card at a simulated rate), then an explicit **failure state** recorded with its date and unfunded amount.
- **Not exposed.** Withdrawal order, borrowing, what happens at ruin.

### 21. Other assets

- **v1.** Anything not cash, portfolio or debt stays in `self.assets_and_debts` and grows at `interest_rate/12` per month (`FIRE.py:781-784`) with defaults by type (`FIRE.py:744-757`); types missing from the table grow at 0.
- **Verdict: simplistic to wrong.** The editor offers "Home (Paid Off)", "Rental Real Estate" (as an asset) and "Other Assets" (`pages/Edit Profile.py:772-778`); none is in the default table and the editor hides the rate field for them, so they are constant nominal forever (declining in real terms). "Home (Being Paid Off)" is handled worse (entry 45). All of them count toward the retirement target (entry 23).
- **Rigorous.** Typed assets: property (section 8), vehicles (depreciating), business equity, collectibles; each flagged liquid or illiquid and included in or excluded from spendable wealth.
- **Not exposed.** Liquidity flag, growth assumption.

### 22. Net worth definition

- **v1.** `calculate_net_worth` (`FIRE.py:1230-1238`): portfolio + cash + other assets − debts + option value from every income source + (home value − loan) + lease asset values.
- **Verdict: simplistic.** A reasonable balance sheet figure but used as "money available to retire on" (entries 72, 74). Missing: rental property equity (entry 52), accrued tax for the current year (up to 12 months of unpaid tax, entry 69), embedded tax in pre-tax accounts and unrealised gains. Calling it has side effects: it vests stock options (entry 29).
- **Rigorous.** Report three numbers: balance-sheet net worth, liquid investable assets, and after-tax spendable assets. Decisions use the last.
- **Not exposed.** n/a.

---

## 5. Income

### 23. Salary and hourly pay, overtime

- **v1.** `get_monthly_income` (`FIRE.py:576-597`): annual/12; monthly; weekly × 4.33; biweekly × 2.17; hourly × 40 × 4.33. Overtime adds `monthly_overtime_hours * hourly_rate * overtime_rate` with `hourly_rate = amount/(40*52)`.
- **Verdict: sound arithmetic, simplistic model.** 4.33 versus 52/12 = 4.333 understates by 0.08%. Pay is deterministic and gross: no pre-tax deductions, no benefits, no unemployment, no disability, no hours variation, no end date. Every salary stops in the same month (entry 75). The editor tells couples to "use either average or combined numbers" (`pages/Edit Profile.py:704`), so two earners are one earner.
- **Rigorous.** Per-person earnings with an age-earnings profile, persistent and transitory shocks, and an employment state (entry 32). Paystub mechanics: 401(k), HSA and premium deductions before tax.
- **Not exposed.** Who earns it, end date, benefits, deferral rate.

### 24. Raises (income growth)

- **v1.** `IncomeSource.update` (`FIRE.py:566-572`): if `has_growth` and `growth_type == 'percentage'` and the source has an `amount`, `amount *= (1+g)**(months_elapsed/12)` with `months_elapsed` taken from calendar month differences. `growth_percentage = data.get('growth_percentage', 0.03) / 100` (`FIRE.py:473`).
- **Verdict: wrong in three places.** **Traced:** (1) The editor offers `growth_type` "inflation" (`pages/Edit Profile.py:333-342`); no code handles it, so "grows with inflation" means frozen nominal pay for the whole run. (2) Growth applies only to sources with an `amount` attribute, so rent, gig pay and commission never grow although the editor shows the growth checkbox for them. (3) The fallback default is 0.03 percent (0.0003), not 3 percent; the editor default is 3.0 so it bites only hand-written or LLM-written profiles. Also: growth is nominal, constant, uncorrelated with inflation, and continues to compound at 3 to 4% for 30 years with no career plateau. The docs say raises "compound annually" (`docs/methodology.md:13`); they compound monthly.
- **Rigorous.** Wage growth = simulated inflation + economy-wide real wage growth + an age and tenure profile that flattens in the 40s and 50s + individual shocks.
- **Not exposed.** Real versus nominal, career curve, promotion jumps.

### 25. Bonus

- **v1.** Counter logic (`FIRE.py:599-604`): `months_until_next_bonus` starts at 1, 3, 6 or 12; each call pays when it is 0 and resets it, otherwise decrements.
- **Verdict: wrong.** **Replicated.** The counter alone is off by one (period 13 instead of 12, 4 instead of 3, 7 instead of 6, 2 instead of 1). Combined with the double call per month (entry 3), over 120 months: an annual bonus is paid 9 times (first in month 7, then every 13 months); a biannual bonus 17 times instead of 20; **quarterly and monthly bonuses are never paid at all**, because their payment always lands in the `manage_cash` call, where it only inflates that month's cash target. Bonuses are deterministic and not tied to salary, performance or the economy.
- **Rigorous.** Bonus as a target percentage of salary paid in a given calendar month, with a payout factor that is random and falls in recessions.
- **Not exposed.** Payment month, variability, percent-of-salary form.

### 26. Commission

- **v1.** `sales = self.random_state.normal(self.sales_target, self.sales_standard_deviation)`; `commission = sales*commission_percentage` or the fixed amount; optional cap; `income = commission/12` (`FIRE.py:629-637`).
- **Verdict: simplistic to wrong.** An annual sales figure is redrawn every month and divided by 12, so annual commission has a standard deviation of the stated figure divided by √12 (the user's volatility is cut by 71%). Draws are independent month to month and of the economy. Sales, and therefore income, can be negative. The "fixed" type ignores sales entirely. The cap is applied to the annualised draw. Never grows.
- **Rigorous.** Annual sales with persistence and recession sensitivity, quota and accelerator structure, floor at zero.
- **Not exposed.** Units (annual versus monthly) are not stated in the editor.

### 27. Gig work

- **v1.** `gigs_per_month * average_take_home_per_gig * (1 - gig_expenses_percentage)` (`FIRE.py:624-627`). `gig_work_hours_per_month` is collected and unused (`FIRE.py:537`).
- **Verdict: simplistic.** Deterministic, never grows, and taxed as wages with employee-side FICA only (7.65%) instead of self-employment tax (15.3% on 92.35% of net, half deductible) and no QBI deduction.
- **Rigorous.** Self-employment income with its own tax treatment, variability and an end age.
- **Not exposed.** Variability, end date.

### 28. Stock options

- **v1.** `get_monthly_income` returns 0 and advances `months_at_job`; at a random `job_duration ~ N(3.5, 1.5)` years the vested count resets to zero (`FIRE.py:606-615`, `513`). Vesting happens inside `get_net_worth_contribution` (`FIRE.py:654-705`): after the cliff, add `number_of_options / total_vesting_months` per call. Value per option is `max(0, noisy_price - strike)` where price is the entered current price ±5% i.i.d. noise (post-IPO) or the entered IPO price ±10% noise after the IPO date and zero before (pre-IPO).
- **Verdict: wrong.** **Traced:**
  1. Vested value is added to net worth (`FIRE.py:1234`) and so counts toward the retirement target, but is never exercised, sold, taxed or turned into cash. `exercised_options` is always 0.
  2. `exercise_strategy`, `custom_exercise_percentage`, `option_type` (ISO/NSO) and `eighty_three_b` are parsed and have no effect; the two branches at `FIRE.py:696-703` are identical.
  3. Vesting is a side effect of reading net worth, so it continues after retirement (`calculate_net_worth` iterates every income source regardless of `working`).
  4. `vesting_frequency` "quarterly" vests three times too fast and "annually" twelve times too fast, because the per-period amount is added every month (`FIRE.py:670-673`).
  5. Nothing vests at the cliff (a real one-year cliff vests 25% at once).
  6. `months_at_job` advances twice per month (entry 3), halving the cliff and tenure.
  7. The share price never moves: it is the entered constant plus fresh noise each month.
  8. Leaving the job forfeits all vested options and silently starts an identical new grant; salary is unaffected by the "job change".
- **Rigorous.** Equity compensation as grants (RSU, NSO, ISO) with a vesting schedule, a share price following the equity market with company-specific volatility, tax at vest or exercise (ordinary income, AMT preference for ISOs), sale into the taxable account with basis, and a liquidity event distribution for private companies including the zero outcome.
- **Not exposed.** RSUs, price dynamics, failure probability, sale policy.

### 29. 401(k) match

- **v1.** The editor collects `401k_match_eligible` and `401k_match_percentage` (`pages/Edit Profile.py:176-193`).
- **Verdict: dead.** `FIRE.py` never reads either key.
- **Rigorous.** Employee deferral rate and employer match formula per job, flowing into the traditional or Roth 401(k), subject to the annual limits.
- **Not exposed.** Deferral rate, match formula, vesting.

### 30. Government assistance and "other" income

- **v1.** Paid monthly, or annual/12 (`FIRE.py:639-649`).
- **Verdict: wrong at the edges.** **Traced:** the editor offers hourly, weekly and biweekly frequencies for these types (`pages/Edit Profile.py:29-37`); the engine handles only monthly and annual, so the other three silently produce zero. Both types are taxed as wages including FICA, and both stop at retirement (entry 34), so a pension or annuity entered as "other" disappears the month you retire.
- **Rigorous.** Typed non-wage income (pension with COLA and survivor option, annuity, alimony, benefits) each with start and end ages and its own tax treatment.
- **Not exposed.** Start and end dates, taxability, inflation indexing.

### 31. Rental income

See section 9 (entries 50 to 52).

### 32. Job loss and career risk

- **v1.** None. The only tenure randomness is the option reset in entry 28.
- **Verdict: missing.** Earnings are certain until the model retires you. Job loss in a recession, when the portfolio is also down, is the main reason early-retirement plans slip.
- **Rigorous.** Per-earner employment state machine: separation hazard that rises in the simulated recession state, unemployment duration, unemployment insurance, re-employment wage scarring, optional disability.
- **Not exposed.** Everything.

### 33. Social Security

- **v1.** `if self.age>= self.social_security_age and not self.is_retired: total+= 1500` (`FIRE.py:971-972`), with `social_security_age = 67` (`FIRE.py:867`).
- **Verdict: wrong.** **Traced:** a flat $1,500 a month, the same for a single person and a couple, never inflated, unrelated to earnings, taxed as wages including FICA, and paid only while the household has **not** retired. Anyone who reaches financial independence before 67 (the purpose of the tool) never receives it. `docs/methodology.md:21` says "after 67, a simple Social Security amount" without the condition.
- **Rigorous.** Per person: indexed earnings history (entered or imputed) plus simulated future earnings, AIME over the top 35 years, PIA from the bend-point formula indexed to simulated wage growth, claiming age 62 to 70 with actuarial adjustment, COLA from simulated inflation, spousal and survivor benefits, taxation of benefits by provisional income, optional trust-fund haircut.
- **Not exposed.** Claiming age, earnings history, benefit estimate, haircut.

### 34. Income after retirement

- **v1.** `sum(i.get_monthly_income(...) for i in self.income_sources if self.working or i.income_type=='rental')` (`FIRE.py:969-970`).
- **Verdict: wrong.** Once `working` is false every non-rental source stops in the same month: both salaries, bonus, gig, commission, assistance and "other". `average_post_retirement_income` reduces the target (`FIRE.py:1317`) but is never paid as cash afterwards. So the plan is declared feasible assuming an income that the simulation then does not deliver.
- **Rigorous.** Each income stream has its own end condition. Retirement is per person. Post-retirement income (part-time work, pension, Social Security) is a set of real cash flows with start and end ages.
- **Not exposed.** Per-person retirement, phased retirement, part-time income as a cash flow.

---

## 6. Spending and budget

### 35. Budget items and frequency

- **v1.** `BudgetItem.get_monthly_expense` (`FIRE.py:723-732`): monthly as is, weekly × 4.33, annual/12. Eleven fixed categories plus "other" (`pages/Edit Profile.py:741`).
- **Verdict: sound for what it is.** Annual items are smoothed. Items have no start or end date, so childcare runs for 50 years and college never arrives. No distinction between essential and discretionary spending, so no spending rule can cut discretionary first. Recorded columns are keyed by item name (`FIRE.py:1300`), so two items with the same name overwrite each other in the output.
- **Rigorous.** Spending lines with category, essential/discretionary flag, start and end (by date or by a person's age), and an inflation index.
- **Not exposed.** Dates, essential flag, per-category inflation.

### 36. Budget growth: inflation plus lifestyle creep

- **v1.** Every month every item is multiplied by `1 + (inflation + expenditure_growth_beyond_inflation)/12` (`FIRE.py:1426-1427`, `718-721`). Default creep is 0.03 (`FIRE.py:887`, `pages/Edit Profile.py:759`).
- **Verdict: wrong default, wrong shape.** **Replicated:** defaults give 6.17% effective annual growth: spending is 6.0 times nominal and **2.45 times real** after 30 years, and keeps compounding through retirement to the end of the horizon. Meanwhile the retirement target grows at inflation only (entry 72), so the target and the spending it is meant to fund diverge by the same factor. Creep applies to fixed nominal obligations such as a mortgage line. Empirically real spending rises with income during a career and falls about 1 to 2% a year in real terms through retirement until late-life medical costs.
- **Rigorous.** Spending in real terms by category; lifestyle creep tied to real income growth while working and defaulting to zero; an optional retirement spending curve; fixed nominal obligations excluded from indexing.
- **Not exposed.** When creep stops; a retirement spending shape.

### 37. Spending that changes when not working

- **v1.** `value_if_not_working` replaces `value` when `working` is false (`FIRE.py:714-724`).
- **Verdict: sound idea, crude.** It is the only way to express the health-insurance jump, and it works. But it is a single switch for the household, the editor defaults the replacement value to 0 (`pages/Edit Profile.py:747-749`), and the retirement target (`post_retirement_expenses`) is a separate number unconnected to it (entry 72).
- **Rigorous.** Derive retirement spending from the budget itself (items flagged work-related drop out), and model healthcare explicitly (entry 39) instead of by hand.
- **Not exposed.** n/a.

### 38. Unplanned costs

- **v1.** `self.average_life_event_cost = 1000; self.life_events_per_year = 1; self.life_events_std_dev = 1000` (`FIRE.py:948-950`).
- **Verdict: dead.** Three attributes, never read.
- **Rigorous.** Spending shocks: a mean-zero noise term on monthly spending plus rare large shocks (home repair as a percent of home value, vehicle replacement on a cycle, medical out-of-pocket up to the plan maximum).
- **Not exposed.** n/a.

### 39. Healthcare and long-term care

- **v1.** A budget category named "healthcare" with an optional not-working value.
- **Verdict: missing as a model.** No employer versus ACA versus Medicare phases, no premium subsidy that depends on income (which interacts with Roth conversions and withdrawals), no Medicare Part B/D premiums or IRMAA, no medical inflation, no long-term-care risk. For an early retiree this is typically the second or third largest line.
- **Rigorous.** Per person: employer coverage while employed, COBRA option, ACA benchmark premium by age and region less the premium tax credit computed from simulated MAGI, Medicare at 65 with IRMAA, out-of-pocket distribution, and a long-term-care state process with age-dependent incidence, duration and cost.
- **Not exposed.** Everything.

---

## 7. Debts and leases

### 40. Amortising debts

- **v1.** `AssetOrDebt` (`FIRE.py:735-803`): payment `P·r(1+r)^n / ((1+r)^n − 1)` from `remaining_term` in years; each month interest `value*rate/12`, principal the remainder, `remaining_term -= 1/12`; when `value <= 0`: `leftover = -self.value ...; cash_flow += leftover` (`FIRE.py:796-800`).
- **Verdict: mostly sound, two bugs.** (1) **Replicated:** `remaining_term` is a float decremented by 1/12, so after the last scheduled payment it can be a hair above zero, which triggers one more payment; that payment overshoots, and the overshoot is **added** to the cash flow instead of subtracted. A $30,000, 7.5%, 60-month loan (the dual-income example's kitchen remodel) pays 61 cash flows, the last one $1,202.28 (two payments), $1,202 more than scheduled. A $100,000, 4%, 30-year loan pays an extra $954.83. Other terms (4, 8, 10 years) happen to land exactly. (2) Debt payments are not included in `expenses` (`FIRE.py:1432-1438`), so reported spending and the savings rate (`scenarios.py:138`) exclude them. No extra payments, no payoff strategy, no refinancing, no variable rates, no interest deduction, fixed-rate only.
- **Rigorous.** Integer months remaining, final payment equal to the remaining balance plus interest, debt service reported as spending, optional avalanche or snowball prepayment, variable rates tied to the simulated bill rate, student-loan specifics (income-driven plans) if in scope.
- **Not exposed.** Extra payments, payoff priority, rate type.

### 41. Credit cards and debts with no term

- **v1.** A debt is serviced only `elif self.is_debt and self.remaining_term>0` (`FIRE.py:785`); otherwise `update` returns 0.
- **Verdict: wrong.** The editor does not show a term field for "Credit Card Debt" (`pages/Edit Profile.py:795-804`). A card balance entered there has `remaining_term = 0`: it accrues no interest and is never paid. It is a constant negative number on the balance sheet for the whole run. New borrowing never happens: a shortfall is forgiven (entry 20) instead of going on a card.
- **Rigorous.** Revolving debt with a minimum payment rule, interest at its APR, paid down by the cash policy before investing; shortfalls can draw on it.
- **Not exposed.** Minimum payment, payoff policy.

### 42. Financed major purchase

- **v1.** `handle_major_expense` with `purchase_type == 'finance'` (`FIRE.py:1146-1159`): subtract the down payment from cash, add a "Personal Loan" with `loan_amt = data.get('loan_amount', cost-dp)`, rate `data.get('interest_rate',0.05)/100`, term `loan_term_months/12`.
- **Verdict: sound core, fragile inputs.** The loan then amortises correctly (subject to entry 40). The editor's `loan_amount` field defaults to 0.0 (`pages/Edit Profile.py:600-607`) and is present whenever "finance" is chosen, so the `cost-dp` fallback never applies; a user who leaves it blank gets a zero-balance loan and the purchase costs only the down payment. The fallback rate is 0.05 percent. Rates here are percent while the house milestone's mortgage rate is a fraction (entry 46).
- **Rigorous.** Derive the loan from price and down payment; one unit convention for rates across the schema.
- **Not exposed.** n/a.

### 43. Leases

- **v1.** `Lease` (`FIRE.py:360-386`) with `monthly_payment`, term, residual, buyout, optional asset value in net worth. `update_leases` subtracts the payment from cash (`FIRE.py:1180-1185`); `calculate_monthly_expenses` also adds `sum(l.monthly_payment for l in self.leases if l.is_active)` (`FIRE.py:977`).
- **Verdict: wrong.** **Traced:** (1) each lease payment is charged twice, once directly against cash and once inside `exp`. (2) Through the editor it is charged zero times: the handler reads `monthly_payment` (`FIRE.py:1161`) but the editor field is `monthly_lease_payment` (`pages/Edit Profile.py:632`); likewise `buyout_option` versus `buy_residual`. (3) `residual_value` and `buyout_option` are stored and never used: nothing happens at lease end. (4) `consider_in_net_worth`, `asset_value` and `asset_value_change` have no editor fields; if supplied, a leased (not owned) item is added to net worth. (5) The down payment field is shown for leases and not charged.
- **Rigorous.** A lease is a dated spending line with an optional buyout purchase at the end. It does not need its own class.
- **Not exposed.** n/a.

---

## 8. Housing

### 44. Renting

- **v1.** A budget item of type "rent", inflating like everything else (entry 36).
- **Verdict: simplistic.** Rent grows at inflation plus lifestyle creep, not with a rent index, and is unrelated to home prices or location.
- **Rigorous.** Rent follows the simulated shelter or rent index (correlated with home prices and wages); a move changes its level.
- **Not exposed.** Rent growth assumption.

### 45. A home you already own

- **v1.** No `Home` object is created at start (`self.home = None`, `FIRE.py:953`). The editor offers asset types "Home (Being Paid Off)" and "Home (Paid Off)" (`pages/Edit Profile.py:772-778`) and budget type "mortgage".
- **Verdict: wrong.** **Traced:** "Home (Being Paid Off)" is not in the debt list (`FIRE.py:741`), so it is an **asset** whose value compounds at the entered interest rate (the mortgage rate) with no loan and no payment; `remaining_term` is ignored. "Home (Paid Off)" is an asset fixed in nominal dollars. The engine's own "Mortgage" and "Real Estate" types are not offered by the editor. A homeowner who instead puts the payment in the budget (the dual-income example: "Mortgage (PITI)" $2,900) gets a payment that grows with inflation and creep forever, never ends, and builds no equity. There is no way to describe an existing mortgaged home correctly.
- **Rigorous.** A property record: current value, purchase price and date (basis), loan balance, rate, remaining term, escrowed tax and insurance, HOA, maintenance rate. Same object for a home bought later.
- **Not exposed.** All of it.

### 46. Buying a house: the transaction

- **v1.** `handle_buy_house` (`FIRE.py:1083-1100`): `self.cash-= dp`; create `Home(self.time, hp, dp, mr, my, ptr, hoa)`; delete every budget item of type "rent"; append a "mortgage" budget item.
- **Verdict: simplistic, with errors.** No closing costs (2 to 5% of price), no PMI below 20% down, no lender qualification, mortgage rate is whatever the user typed years in advance rather than the simulated rate at purchase, price is a fixed nominal number regardless of what simulated home prices did in between. The down payment comes from cash and, if cash is insufficient, is partly forgiven (entry 20). `annual_maintenance_cost` is collected (`pages/Edit Profile.py:512-519`) and never read. `hoa` is read (`FIRE.py:1089`) but has no editor field. Property-tax default is 2% (`FIRE.py:391`, `1088`), about double the US average. If the purchase month is visited twice (entry 1) the down payment is taken twice, the first home and its equity are overwritten, and a second mortgage budget item is added; if skipped, the purchase never happens.
- **Rigorous.** Price specified in today's dollars and scaled by the simulated home-price index to the purchase date; mortgage rate = simulated long rate plus a spread; closing costs; PMI until 78 to 80% loan-to-value; down payment funded through the withdrawal waterfall with its tax cost; affordability check reported as a warning.
- **Not exposed.** Closing costs, PMI, points, ARM versus fixed, maintenance, insurance rate.

### 47. The mortgage payment

- **v1.** `monthly_payment = PI + purchase_price*0.003/12 + home_value*property_tax_rate/12 + hoa`, computed once at purchase (`FIRE.py:405-406`, `424-428`). The purchase handler adds a budget item with `'value': self.home.monthly_payment` (`FIRE.py:1094-1100`). `step_month` then also does `home_pmt= self.home.make_payment(self.time)` and subtracts it from cash (`FIRE.py:1440-1443`).
- **Verdict: wrong.** **Traced:** the payment is charged twice every month, once inside `exp` through the budget item and once directly. For the early-career example ($420k, $84k down, 6%, 30 years, 0.5% tax) the payment is $2,294.49 and the household is charged $4,588.98 (**replicated**). The budget copy then inflates with inflation plus creep (4% a year in that profile, reaching $7,603 a month by year 30) and never ends, while the direct copy stops completely at payoff, including the tax and insurance, which in reality never stop. Tax is frozen at the purchase-date value and insurance at 0.3% of purchase price; neither follows the home's value or inflation. P&I amortisation itself is correct (`tests/test_fire.py:103-106`).
- **Rigorous.** Fixed nominal P&I until payoff; property tax as a rate on assessed value with the state's reassessment rule; insurance and maintenance (around 1% of value a year) indexed; all charged once. Mortgage interest and property tax feed itemised deductions (SALT cap).
- **Not exposed.** Insurance rate, maintenance rate, extra principal, refinance rule.

### 48. Home price appreciation

- **v1.** `Home.__init__` builds `ReturnGenerator("Rental Real Estate", max(360, term*12), block_size=12, real_estate_type="long term rentals")` (`FIRE.py:411-412`); `make_payment` applies one month of it (`FIRE.py:436-438`).
- **Verdict: simplistic.** An independent bootstrap of the smoothed national index (entry 13): 3.9% nominal a year, 2.9% annualised volatility, no link to inflation, equities, mortgage rates or the user's city. An individual house has 10%+ idiosyncratic annual volatility on top of the index. The path is at most `max(360, term*12)` months long; after that `get_return_for_month` returns 0.0 (`FIRE.py:349-355`) and the home stops appreciating for the rest of the run. Appreciation is a side effect of making a payment.
- **Rigorous.** Home-price growth from the joint generator (real home-price growth near zero to 1% long run, strongly autocorrelated, correlated with the recession state), optional metro index, idiosyncratic house-level noise.
- **Not exposed.** Expected real appreciation, local market.

### 49. Home equity, selling and downsizing

- **v1.** `net+= (self.home.home_value - self.home.loan_balance)` (`FIRE.py:1235-1236`). No sale event exists.
- **Verdict: wrong use.** Equity counts in full toward the retirement target (entry 72) although nothing in the model can turn it into spending: no sale, no downsizing, no HELOC, no reverse mortgage, no selling costs (about 6%), no section 121 exclusion. A change-of-location milestone leaves the house and its mortgage in place.
- **Rigorous.** Home equity excluded from spendable assets by default; explicit sell, downsize and rent-instead events with transaction costs and capital gains exclusion; optional home equity as a last-resort reserve.
- **Not exposed.** Sale and downsizing plans.

---

## 9. Rental property

### 50. Property value and mortgage

- **v1.** `self.property_value = self.number_of_units * self.rent_per_unit * 100` and `self.home = Home(datetime.datetime.now().date(), self.property_value, self.property_value * 0.2, self.mortgage_interest_rate, self.mortgage_remaining_years)` (`FIRE.py:529-531`).
- **Verdict: wrong.** **Traced:** value is invented as 100 months of rent (a 1% rule), the loan is invented as 80% of that, originated today, amortised over the "remaining" years. The user's `mortgage_monthly_payment` (`FIRE.py:524`) is never read. The invented payment also includes insurance at 0.3% and property tax at the 2% default. For the landlord example (2 units at $1,550, entered payment $1,700) the engine charges $1,908.76 a month on an invented $248,000 loan (**replicated**), turning $864 of monthly cash flow into $656. If `has_mortgage` is false the constructor raises `AttributeError`, because line 531 reads `self.mortgage_interest_rate`, which is only set inside `if self.has_mortgage` (`FIRE.py:523-526`); the unit test works around this by flipping `has_mortgage` after construction (`tests/test_fire.py:129-130`). `rental_state` is collected and unused. The scenario engine's `income_multiplier` scales rent and therefore the invented property value and mortgage with it (`scenarios.py:63`).
- **Rigorous.** The same property record as entry 45, plus rent, vacancy and operating costs. Value and loan balance are inputs.
- **Not exposed.** Property value, loan balance, purchase basis.

### 51. Rental cash flow

- **v1.** `gross = units*rent*occupancy; expenses = gross*non_tax_costs% + gross*management%; if has_mortgage: expenses += self.home.make_payment(current_date); income = gross - expenses` (`FIRE.py:617-622`).
- **Verdict: wrong.** **Traced:** (1) rent never grows (entry 24) while the household's own costs inflate, so real rental income falls about 3% a year. (2) `make_payment` is called twice per month (entry 3): the loan amortises at double speed (the landlord example's 26-year loan is gone in 157 months, **replicated**) and the property appreciates twice per month until its 360-month path is exhausted after 15 years. (3) At payoff `make_payment` returns 0, so property tax and insurance stop with the loan. (4) Vacancy is a constant fraction, not spells. No capital expenditure reserve, no rent sensitivity to the economy. It is the only income that continues in retirement.
- **Rigorous.** Rent on the simulated rent index; vacancy as random spells; operating costs, taxes and insurance indexed; capex reserve; mortgage amortised once.
- **Not exposed.** Rent growth, capex.

### 52. Rental taxation and equity

- **v1.** Net rental cash flow (after the whole mortgage payment) is added to `yearly_income` and taxed like wages. The property's `Home` object is not referenced by `calculate_net_worth`.
- **Verdict: wrong.** Principal repayment is deducted from taxable income (it is not deductible); depreciation (27.5-year straight line) is not; FICA is charged on rent (entry 66); passive loss rules are absent. **Traced:** the property's value and equity never enter net worth, so a landlord's largest asset is invisible, while its cash flow is counted. There is no sale, no depreciation recapture.
- **Rigorous.** Schedule E: rent less operating costs, interest, taxes, insurance and depreciation; passive loss limitation and carryforward; equity on the balance sheet; sale with recapture and capital gains.
- **Not exposed.** Basis, depreciation, sale plan.

---

## 10. Milestones

### 53. Milestone dispatch

- **v1.** `handle_milestones` fires every milestone whose `date` has the same year and month as `self.time` (`FIRE.py:1055-1067`). A milestone is a bag of seven boolean flags plus the union of all their fields (`FIRE.py:806-816`).
- **Verdict: wrong because of the clock.** With 30-day steps a milestone fires twice in double-visited months and never in skipped months (entry 1). A milestone dated in the start month or the past never fires. Several flags can be set on one milestone. Milestones are deterministic dates only; none can be conditional ("buy when we have 20% down", "have a child two years after marriage").
- **Rigorous.** Typed events on the integer month grid, each firing exactly once, with optional conditions and optional random timing.
- **Not exposed.** Conditional triggers.

### 54. `buy_a_house`

Handled; see entries 46 to 49. Verdict: wrong (double-counted payment).

### 55. `major_expense`

- **v1.** `purchase_type` "cash": `self.cash-= cost` (`FIRE.py:1144-1145`); "finance": entry 42; "lease": entry 43.
- **Verdict: simplistic to wrong.** The cash branch is right when the buffer covers it and partly free when it does not (entry 20). Cost is nominal as of the day it was typed, not inflated to the purchase date. Not recorded as an expense, so it is invisible in spending output.
- **Rigorous.** One-off or recurring (every N years) spending events in today's dollars, funded through the waterfall.
- **Not exposed.** Recurrence, real versus nominal amount.

### 56. `major_windfall`

- **v1.** `self.cash+= amt` (`FIRE.py:1102-1104`).
- **Verdict: sound, simplistic.** Untaxed (correct for a gift or cash inheritance, wrong for an inherited traditional IRA or a business sale), certain in amount and date, nominal.
- **Rigorous.** Typed windfalls (cash, inherited IRA with the 10-year rule, property with stepped-up basis), optionally uncertain in amount and timing.
- **Not exposed.** Type, uncertainty.

### 57. `change_location`

- **v1.** `handle_change_location` (`FIRE.py:1106-1133`): update `self.location`; take the moving cost with `self.investment_balance-= moving_cost` when investments suffice; then either leave expenses alone, multiply every budget item by `col_to/col_from` where the index is `city_data.iloc[0]['x54']` (`FIRE.py:1135-1139`), or multiply by a manual factor.
- **Verdict: wrong.** **Traced:**
  1. The moving cost is not paid. `self.investment_balance` is overwritten from `vehicle_balances` later in the same step (`FIRE.py:1197`), and the handler never touches `vehicle_balances`.
  2. Column `x54` is average monthly net salary (`docs/methodology.md:78`), not a price index. Tampa is 5,289 and Lisbon 1,125 in the file, so the "similar quality of life" factor for the landlord example would be 0.21.
  3. That option never runs from the editor: it needs `from_city` (`FIRE.py:1118`), the editor has no such field, the default "Current City" is not in the dataset, and the profile's own `city` is not consulted (`self.location = {'city': '', ...}`, `FIRE.py:952`). The choice silently behaves as "leave expenses the same".
  4. The lookup matches on city name only and takes the first row; 49 city names in the file are ambiguous (London, Portland, Barcelona and others).
  5. `self.state` is not changed, so state tax is unaffected by a move, including a move abroad. Federal tax treatment abroad is not considered.
  6. The factor applies to every budget line (including a fixed mortgage) but not to `post_retirement_expenses`, so moving somewhere cheap does not lower the retirement target. Income is unchanged. The house is not sold.
- **Rigorous.** A location carries a price level by category (housing separately from goods and services), a tax jurisdiction and a home-price market. A move re-prices spending by category, changes the tax jurisdiction, optionally sells and buys housing, and optionally changes income.
- **Not exposed.** Category-level price differences, tax residency, housing transaction.

### 58. `change_income` and `change_budget`

- **v1.** `Milestone` parses both flags (`FIRE.py:812-813`). The editor generates per-milestone override fields for every budget item and every income field (`pages/Edit Profile.py:891-914`, with a `#FIX THIS` comment at line 904).
- **Verdict: dead.** `handle_milestones` has no branch for either flag (`FIRE.py:1058-1067`). A user can enter a career change or a budget change and nothing happens. These are the two most common real plans (a raise, a job change, a sabbatical, a child's costs, a paid-off car).
- **Rigorous.** Every income stream and spending line has dated start, end and step changes; career events are first class.
- **Not exposed.** n/a (exposed and ignored).

### 59. `family_change`

See section 11.

---

## 11. Family

### 60. Marital status and the spouse

- **v1.** `married` (default **True**, `FIRE.py:874`) selects the standard deduction (`FIRE.py:992`). A "marriage" event sets `married=True` and stores `spouse_age` (`FIRE.py:1071-1073`).
- **Verdict: simplistic.** The spouse is a tax flag. No second person: no separate earnings, retirement date, Social Security record, health coverage or lifespan. `spouse_age` is never read. The editor tells couples to enter combined income and average age (`pages/Edit Profile.py:704`). Marriage changes neither income nor spending.
- **Rigorous.** A household of one or two people, each with birth date, sex, earnings process, accounts, benefits and mortality. Marriage adds a person with their own balance sheet.
- **Not exposed.** The whole second person.

### 61. Children and dependents

- **v1.** `dependents` is an integer; a "birth" event adds 1, or 2 for twins (`FIRE.py:1074-1075`). Its only use is `ctc = min(self.dependents*2000, total)` (`FIRE.py:1052`).
- **Verdict: wrong sign.** A child has no cost. Its only effect is a $2,000 a year tax credit, forever, because dependents have no age and never leave. In v1 having a baby brings retirement forward.
- **Rigorous.** Each child has a birth date; age drives childcare, general child costs (USDA-style by age and income), optional college (529 funding, tuition inflation), the credit's age limit, the dependent's exit, and the ACA household size.
- **Not exposed.** Child ages, childcare, college plans.

### 62. Divorce, death, dependent adults

- **v1.** "divorce": `self.married=False`. "death": `self.dependents = max(0, self.dependents-1)`. "dependant_adult": `self.dependents += n` (`FIRE.py:1076-1081`); the editor's default n is 0 (`pages/Edit Profile.py:470-477`).
- **Verdict: wrong or trivial.** Divorce splits nothing and costs nothing; it only halves the deduction. "Death" removes a dependent, not a spouse; there is no survivor scenario, no life insurance, no loss of an income or a Social Security benefit. A dependent adult gets the $2,000 child credit (the real credit for other dependents is $500) and has no care cost.
- **Rigorous.** Spousal death falls out of the mortality model (survivor's filing status, survivor benefit, reduced spending, inherited accounts, term life insurance payout). Divorce, if kept, is an explicit scenario with asset division and support. Elder care is a dated spending line.
- **Not exposed.** Life insurance, survivor spending ratio.

---

## 12. Taxes

### 63. Federal income tax

- **v1.** `calculate_federal_tax` (`FIRE.py:980-1005`): 2020 single brackets (9,875 / 40,125 / 85,525 / 163,300 / 207,350 / 518,400 at 10/12/22/24/32/35/37%) on `income - deduction`, deduction `24800 if self.married else 12400`. `income` is the sum of the last 12 months of cash income (`FIRE.py:1433`, `1241`).
- **Verdict: the bracket loop is right; almost everything around it is wrong.** The tax base is gross cash received: no 401(k), HSA or premium deductions, no itemising, no adjustments. All income is ordinary (rental net of principal, gig, assistance, Social Security). Six-year-old law in a tool run in 2026: the 2026 standard deduction is $16,100 single and $32,200 joint and the 10% bracket ends at $12,400 single. No AMT, no NIIT, no senior deduction, no credits other than entry 67.
- **Rigorous.** A tax-year function over typed income (wages, interest, ordinary and qualified dividends, short and long gains, traditional withdrawals, conversions, Social Security, rental, self-employment) with above-the-line adjustments, standard versus itemised deduction, ordinary brackets, the 0/15/20% gain stack, NIIT, AMT where relevant, and credits; parameters per filing status, table-driven by year.
- **Not exposed.** Deductions, pre-tax contributions, income character.

### 64. Filing status

- **v1.** Married filers get the joint standard deduction and the **single** bracket thresholds (`FIRE.py:983-992`).
- **Verdict: wrong.** **Replicated** against 2020 joint brackets: a married household is overcharged $3,705 at $100k gross (43%), $5,004 at $150k (26%), $6,872 at $200k (23%), $16,908 at $300k (31%). No head-of-household or surviving-spouse status. `tests/test_fire.py:133-139` asserts only that married pays less than single at $100k, which the deduction alone satisfies.
- **Rigorous.** Full parameter sets for single, married filing jointly, head of household; status changes on marriage, death (two-year surviving spouse rule) and divorce.
- **Not exposed.** Head of household.

### 65. State income tax

- **v1.** `calculate_state_tax` (`FIRE.py:1007-1022`) applies the state's brackets from `statetaxes.json` to gross income.
- **Verdict: simplistic.** No state standard deduction or personal exemption, one schedule regardless of filing status, no local tax, no special treatment of retirement or Social Security income (which most states have), not indexed, undated source. The state never changes after a move (entry 57). `property_tax`, `sales_tax` and `corporate_income_tax` in the file are unused, and the property-tax figures are not rates (Alabama 6.55, Alaska 10.45). Returns 0 whenever custom federal brackets are set (`FIRE.py:1009-1010`).
- **Rigorous.** Per-state parameters by filing status with deductions and exemptions, retirement-income exclusions, a flag for conformity on capital gains, indexed where the state indexes; state changes with residence.
- **Not exposed.** Local tax.

### 66. Payroll tax (FICA)

- **v1.** `calculate_taxes` (`FIRE.py:1043-1050`): 7.65% of total annual income up to `wage_base=160200`, 1.45% above.
- **Verdict: wrong.** **Traced:** (1) charged on all income: rental profit, government assistance, "other" income and the Social Security benefit itself ($2,295 of payroll tax on $30,000 of rent and benefits, **replicated**). (2) The wage base is per household, not per worker: two $125k earners pay $13,557 instead of $19,125. (3) The wage base is the 2023 figure, mixed with 2020 income tax law, never indexed (2026: $184,500). (4) No 0.9% additional Medicare tax. (5) Gig income pays the employee rate, not self-employment tax.
- **Rigorous.** Per person, on that person's wages and net self-employment earnings only, wage base indexed to simulated wage growth, additional Medicare tax above the (unindexed) thresholds.
- **Not exposed.** n/a.

### 67. Child tax credit

- **v1.** `ctc = min(self.dependents*2000, total)`; `return max(0, total - ctc)` (`FIRE.py:1052-1053`).
- **Verdict: simplistic.** Applied against the sum of federal, state and payroll tax (the real credit offsets federal income tax, with a limited refundable part). No phase-out at $200k/$400k, no age limit, same amount for adult dependents, 2020 amount (2026: $2,200), not indexed.
- **Rigorous.** Per qualifying child under 17, phase-out by modified AGI, refundable portion, $500 credit for other dependents; also the dependent-care credit and education credits if in scope.
- **Not exposed.** n/a.

### 68. Tax on investments and withdrawals; account types

- **v1.** None. Portfolio returns are untaxed; selling investments is untaxed; there is one pooled portfolio (entry 16).
- **Verdict: missing, and the largest structural gap after the success definition.** A dollar in a traditional 401(k) is worth 60 to 90 cents of spending and v1 counts it as a dollar. Withdrawals in retirement generate no tax, so required spending is understated by the retiree's whole tax bill. No dividend or interest drag in taxable accounts, no capital gains, no early-withdrawal penalty, no RMDs, no Roth conversion, no ACA subsidy interaction with realised income.
- **Rigorous.** Five account types with correct tax treatment on the way in, during and out; taxable account tracks cost basis; yearly tax computed from the actual withdrawals and realised gains; RMDs from the Uniform Lifetime Table; 10% penalty with the 72(t), Rule of 55 and Roth-contribution-basis exceptions; Roth conversion ladder with the five-year clocks.
- **Not exposed.** Everything.

### 69. Tax timing

- **v1.** `if self.month%12==0: self.pay_annual_taxes()` (`FIRE.py:1464-1465`): one lump sum every 12 steps on the previous 12 steps' income, from cash, with the forgiveness bug in entry 20 if cash is short.
- **Verdict: wrong.** No withholding: gross pay is invested for up to a year before tax is taken, so the household earns market returns on its tax bill. Net worth is recorded before the tax is deducted (`FIRE.py:1457` precedes `1464`), so every twelfth row, and the **final** row that feeds "final net worth", overstates net worth by a full year of tax. The tax year is 12 steps from the start date, not a calendar year, and drifts against the calendar (entry 1). The `taxes` column is always `None` (`FIRE.py:1285`), so tax never appears in the time series; the only tax number shown anywhere is the first year's (`scenarios.py:131`).
- **Rigorous.** Monthly withholding or estimated payments computed from an annualised projection, with a calendar-year true-up in April; taxes recorded monthly.
- **Not exposed.** n/a.

### 70. Indexing of tax parameters

- **v1.** Every threshold is a literal (`FIRE.py:983-992`, `1044`, `1052`).
- **Verdict: wrong.** With incomes growing 3 to 4% nominal and thresholds frozen, real tax burden rises every year (bracket creep). Over 30 years at 3%, a threshold is worth 41% of its starting real value.
- **Rigorous.** Each parameter carries its statutory indexing rule (chained CPI for brackets and the standard deduction, average wage index for the Social Security wage base and bend points, unindexed for NIIT, additional Medicare and Social Security taxation thresholds) driven by simulated inflation and wage growth. Optional law-change scenarios.
- **Not exposed.** n/a.

### 71. Custom tax brackets

- **v1.** `custom_tax_brackets` (a `{limit: rate}` dict) replaces the federal calculation and zeroes state tax (`FIRE.py:862`, `981-982`, `1009-1010`, `1024-1038`).
- **Verdict: dead and wrong.** No UI, no scenario key, no test. **Traced:** the loop taxes nothing above the highest limit (`FIRE.py:1030-1037` has no final open bracket) and applies no deduction. Payroll tax and the child credit still apply. Not copied into the stress-test clone.
- **Rigorous.** Drop. If non-US users matter, add jurisdictions as parameter tables.
- **Not exposed.** n/a.

---

## 13. The retirement decision

### 72. Method 1: safe withdrawal rate (default)

- **v1.** `req = (self.post_retirement_expenses - self.average_post_retirement_income)/ self.withdrawal_rate`; retire when `self.net_worth>= req`; otherwise `self.post_retirement_expenses*= (1+ self.inflation/12)` (`FIRE.py:1316-1324`).
- **Verdict: wrong.** **Traced:**
  1. The numerator is **total net worth**: home equity, static "other assets", lease asset values and paper option value all count, and pre-tax balances count at face value. A household with $1.0M of home equity and $0.2M of investments "retires" on a $1.2M target.
  2. `post_retirement_expenses` is a separate input, unconnected to the budget. After retirement the simulation spends the budget (with not-working values and continued creep), not this number.
  3. Only the expense term inflates; `average_post_retirement_income` stays nominal, so the gap grows faster than inflation. Landlord example: a $28,000 gap today becomes $33,409 in today's dollars after 20 years (**replicated**).
  4. Both engine and editor default `post_retirement_expenses` to 0 (`FIRE.py:934`, `pages/Edit Profile.py:938`), which makes the target 0: a new profile with non-negative net worth retires in month 1.
  5. The 4% rule was derived for a 30-year retirement, a specific stock/bond mix and tax-deferred balances. v1 applies it at any age (a 35-year-old has a 55 to 60 year horizon) and to any allocation.
  6. The editor labels the inflation switch "Adjust withdrawal rate for inflation" (`pages/Edit Profile.py:962-966`); it inflates the target.
- **Rigorous.** Keep an SWR-style rule only as a labelled rule of thumb, computed on after-tax spendable investable assets against the household's own projected retirement spending. The real test is forward simulation (entry 73).
- **Not exposed.** What counts as retirement assets.

### 73. Method 2: "Simulate Additional Years with Reduced Income"

- **v1.** Every 6 months, if `self.net_worth>= 10* self.calculate_monthly_expenses()*12`, build 10 clones with `clone_user_for_reduced_income_sim(10)`, run each for 10 years, and retire only if all ten end with `sim_clone.net_worth>= 0.9* init_nw` (`FIRE.py:1326-1341`, `1352-1414`).
- **Verdict: right idea, wrong execution.** **Traced:**
  1. The clone is constructed as `User({'investment_other': 100, ...})` (`FIRE.py:1354`). That is only a constructor placeholder: the clone's vehicles are rebuilt from the parent's names and ratios (`FIRE.py:1408-1412`) and its balances copied (`FIRE.py:1372`). **The suspected "allocation replaced by 100% Other" is not true.** The placeholder does waste a 120-month path and shifts the random stream.
  2. The horizon is hard-coded to 10 years. The user's `additional_simulation_years` (10 to 50, default 30) is copied onto the clone and never used. `additional_retirement_income` is parsed (`FIRE.py:940`) and never used.
  3. The clone has `working=False` (`FIRE.py:1405`), so every non-rental income is dropped by entry 34 regardless of the "remove" checkboxes; the selection only matters for rentals.
  4. The pass criterion is nominal: keeping 90% of nominal net worth for 10 years at 3% inflation is a 33% real loss. And it is again net worth, so home appreciation can pass the test while the portfolio drains.
  5. Ten of ten on fresh random paths: to pass half the time each path must pass with probability 0.933; a plan with 90% per-path success passes 35% of the time (**replicated**). The retirement date therefore depends on the luck of 10 nested draws, and a failed check is retried six months later with new draws, so marginal plans eventually pass by repetition.
  6. Clone milestones are emptied (`FIRE.py:1400`): a planned house or child is ignored. Parent's unpaid tax for the year is not carried.
  7. Nested draws come from the global generators, which breaks common random numbers for the rest of the run (entry 84).
  8. In the editor the "additional retirement income" input sits inside the loop over income sources with no key (`pages/Edit Profile.py:970-978`); with two or more income sources Streamlit is expected to raise a duplicate-element error (not executed here). The existing page test never selects this method.
- **Rigorous.** This becomes the primary definition: "financially independent at month t" means that, starting from the full state at t with earned income removed, the probability of funding planned spending to the end of life is at least the user's threshold. Computing it by brute-force nesting is too expensive at scale; v2 should evaluate candidate retirement dates as an outer policy (simulate "retire at age A" for a grid of A on the same paths and report success probability by A), or use a fitted funded-ratio rule checked against that grid.
- **Not exposed.** Success threshold, horizon, which incomes stop.

### 74. Method 3: fixed value

- **v1.** Retire when `self.net_worth>= self.fixed_retirement_goal`, default $3,000,000 (`FIRE.py:942-943`, `1343-1348`).
- **Verdict: simplistic and unreachable from the UI.** The editor's method list is `["Safe Withdrawal Rate", "Simulate Additional Years with Reduced Income"]` (`pages/Edit Profile.py:926`); a saved "Fixed Value" profile is reset to the first option when the page is opened. The goal is nominal and never inflated, and is net worth. Neither the scenario engine nor the assistant can set it.
- **Rigorous.** A target in today's dollars on investable assets, offered as a simple option, plus "retire at age A" as the most common fixed plan (which v1 does not have at all).
- **Not exposed.** Retire-at-age; real-dollar target.

### 75. Retirement as an event

- **v1.** On the first passing check: `self.is_retired=True; self.retirement_age=self.age; self.working=False` (`FIRE.py:1459-1462`).
- **Verdict: simplistic.** Instant, irreversible and household-wide. No notice period, no one-more-year behaviour, no part-time phase, no going back to work after a crash, no minimum age, no difference between spouses. The check is made on the recorded month-end and taxes for the year are settled afterwards.
- **Rigorous.** Per-person retirement policy (age, funded-ratio trigger, or both), optional phased retirement and optional return-to-work rule with a re-employment wage penalty.
- **Not exposed.** All of it.

---

## 14. What happens after retirement

### 76. Decumulation

- **v1.** No dedicated logic. With `working=False`: income is rental only (entry 34); the budget switches to not-working values and keeps inflating with creep; debts and the house keep being paid; `manage_cash` refills cash to "initial six months of expenses plus three months of rental income" by selling every sleeve pro rata (`FIRE.py:1262-1268`); tax is computed on rental income only.
- **Verdict: wrong by omission.** Withdrawals are tax-free, come from one undifferentiated pot and follow no rule. Spending never responds to the portfolio. No Social Security, pension or part-time income arrives (entries 33, 34). No RMDs, no healthcare transition, no sequence-of-returns management.
- **Rigorous.** Explicit withdrawal policy: account ordering (taxable, then traditional, then Roth, or bracket-filling), Roth conversion ladder for pre-59½ access and bracket management, RMDs, a spending rule (constant real, guardrails, VPW, floor-and-ceiling) with essential spending protected, a cash bucket option, and the tax and ACA consequences of each choice computed each year.
- **Not exposed.** Everything.

### 77. Detecting failure

- **v1.** Nothing checks whether the money lasts. The run simply continues to the end of `simulate_years`.
- **Verdict: wrong.** Combined with entry 20 (unfunded spending is dropped and cash is reset to zero each month), a household that runs out is indistinguishable in the output from one with a small positive balance: net worth sits near zero plus home equity. Final net worth is censored at about zero. No statistic counts depleted paths.
- **Rigorous.** Per path: first month in which planned essential spending could not be funded while someone is alive, the shortfall amount, and the age. Headline success = share of paths with no such month. Also report spending shortfall magnitude and years of shortfall, since "failed by one month at 97" and "broke at 61" are different outcomes.
- **Not exposed.** n/a.

---

## 15. Monte Carlo design, statistics and reporting

### 78. Number of paths

- **v1.** `num_simulations`: editor slider 1 to 100, default 10 (`pages/Edit Profile.py:715`); scenario slider 5 to 200, default 25 (`pages/Scenario Analysis.py:200`); examples use 25; `Home.py:93` claims "thousands".
- **Verdict: simplistic.** With 25 paths a share near 50% has a standard error of 10 points (95% interval about ±20); with 200, ±7. The 5th and 95th percentile bands on the run page (`pages/Run Simulation.py:71-75`) are interpolations between the second and third most extreme of 25 paths. No error bars are shown anywhere.
- **Rigorous.** 100k+ paths in the browser worker pool (±0.3 points at 50%), convergence shown progressively, intervals on every headline number.
- **Not exposed.** n/a.

### 79. Random numbers and seeding

- **v1.** `run_monte_carlo_simulation(user_data, seed)` seeds the two process-global generators once (`FIRE.py:1487-1489`). Paths then draw from them in whatever order the code happens to run: `random.randint` for bootstrap blocks, `random.random` and `np.random.normal` for the chains, `np.random.randint` to seed per-income generators (`FIRE.py:480`), `np.random.normal` for option price noise.
- **Verdict: simplistic.** Reproducible for an identical profile on the same day, but path k's randomness depends on how many draws paths 1..k−1 consumed, so any change in draw count changes every later path. Global state is also not safe under concurrency. The run page passes no seed (`pages/Run Simulation.py:18`), so two clicks give two answers with no indication of which differences are noise.
- **Rigorous.** Counter-based or splittable generator: path k, stream s (economy, each person's career, mortality, health, property) gets its own independent substream derived from (seed, k, s). Changing the plan never changes the economy path.
- **Not exposed.** Seed (in the main run).

### 80. Retirement-age statistics

- **v1.** Engine: mean and standard deviation over `user.retirement_age` for paths where it is set (`FIRE.py:1498-1502`). `scenarios.retirement_ages`: age in the first row where `working` is false (`scenarios.py:101-108`), then median and mean (`scenarios.py:152-153`).
- **Verdict: wrong.** **Traced:** (1) Survivorship: paths that never reach the target are dropped, so the mean and median describe only the lucky paths. If 30% of paths retire, the reported "median retirement age" is the median of that 30%. A change that rescues marginal paths (which retire late) can raise the conditional median and be reported as "would delay your retirement" (`pages/Scenario Analysis.py:236-237`). (2) Two definitions disagree: the row-based age is one month later than `retirement_age` because the row is recorded before the check (`FIRE.py:1457-1462`), and a path that retires in the final month is counted by the engine but not by the row-based function. The run page shows both on one screen (`pages/Run Simulation.py:27-36`).
- **Rigorous.** Treat the retirement age as a censored variable: report the probability of having retired by each age (a cumulative curve) and quote the median only if it is reached; otherwise say "later than X".
- **Not exposed.** n/a.

### 81. `success_rate`

- **v1.** `"success_rate": len(ages) / len(results)` (`scenarios.py:150`), shown as "Runs reaching FI" (`pages/Scenario Analysis.py:243`) and passed to the LLM under the name `success_rate` (`assistant.py:104-111`).
- **Verdict: wrong label, wrong question.** It is the share of paths in which net worth touched the target within the horizon. It says nothing about whether the plan then works. The field name invites the standard reading ("probability the plan succeeds"), and the LLM receives no definition.
- **Rigorous.** Success = never ran out of money while alive, for a stated retirement policy. Reported with an interval, alongside the distribution of outcomes for failed paths.
- **Not exposed.** n/a.

### 82. Net-worth bands, the average series and the first-year snapshot

- **v1.** Per-month percentiles across paths (`scenarios.py:111-115`: 10/50/90; run page: 5/25/50/75/95). An average series via `pd.concat` and `groupby('month').mean()` (`FIRE.py:1504-1506`). Final net worth median, p10, p90 (`scenarios.py:154-156`). `first_year_snapshot` runs one extra 12-month path for income, spending, tax and savings rate (`scenarios.py:118-141`).
- **Verdict: simplistic.** All nominal. Net worth, not spendable assets. The mean of a right-skewed distribution is plotted as "Average" next to best and worst single paths. Final net worth is pre-tax for the last year (entry 69) and floored near zero (entry 77). The savings rate excludes debt service and the direct home payment (entry 40) and is from a single random path. `starting_net_worth` is net worth after month 1. `fire_number` is always computed from the SWR fields, even for other goal methods.
- **Rigorous.** Streaming quantile sketches per month per metric (liquid assets, spending, taxes, income by source, account balances) in today's dollars, plus stored exemplar paths (median, 5th, 95th by outcome) for drill-down.
- **Not exposed.** n/a.

---

## 16. Scenario engine

### 83. `apply_changes` vocabulary

- **v1.** Overwrite nine scalars (`scenarios.py:21-31`); `income_multiplier` scales `amount`, `rent_per_unit`, `average_take_home_per_gig`, `sales_target` on every source; `income_growth_percent` sets raises on salaries and bonuses; budget value changes, removals and additions by name or type; allocation rescaled to 100; append milestones (`scenarios.py:39-98`).
- **Verdict: sound design, narrow reach.** Pure function on a copy, tested (`tests/test_scenarios_and_storage.py:38-56`). Cannot: change one income source, add or remove an income source, change a debt or asset, change the state, change the retirement method or fixed goal, change anything from a future date, remove a milestone, or change tax or Social Security assumptions. `_matches` accepts a name or a type, so a key of "other" rewrites every "other" item. `fixed_retirement_goal` and `adjust_for_inflation` are not in the scalar list. Scaling rent scales the invented property value (entry 50).
- **Rigorous.** Typed patch operations against a validated schema (set, add, remove at a path; optionally effective from a date), with the same operations available to the UI and the LLM, and a diff the user can read.
- **Not exposed.** n/a.

### 84. Common random numbers

- **v1.** `compare` runs baseline and scenario with the same seed (`scenarios.py:166-179`). `docs/methodology.md:64`: "Both versions see identical market histories, job changes and commissions".
- **Verdict: only partly true.** Because all randomness comes from two global streams consumed in code order (entry 79), paths stay aligned only while both plans consume exactly the same number of draws. **Traced** cases that break alignment for every later path: adding a house purchase (creating a `Home` draws about 30 bootstrap blocks from `random` mid-run); any allocation change that adds or removes a non-zero sleeve; changing `simulate_years` (different path lengths); adding or removing an income source; the reduced-income method (nested clones draw new paths whenever a check runs, and checks run at different times in the two plans). Budget and scalar changes do stay aligned. In the misaligned cases the comparison is two independent 25-path samples and the reported difference is mostly noise; the page still states the difference comes from the change (`pages/Scenario Analysis.py:64-65`, `292-293`).
- **Rigorous.** Per-path, per-purpose substreams (entry 79). Then the economy, mortality and career shocks are identical across plans by construction, and paired differences have small variance. Report the paired difference with its interval.
- **Not exposed.** n/a.

### 85. Scenario page inputs and defaults

- **v1.** The page always sends `post_retirement_expenses`, `average_post_retirement_income`, `withdrawal_rate`, `inflation`, `expenditure_growth_beyond_inflation`, `simulate_years`, `num_simulations` and (if non-zero) the allocation as "changes" (`pages/Scenario Analysis.py:97-105`, `132-137`, `194-201`).
- **Verdict: one real bug.** **Traced:** the lifestyle-creep slider defaults to 0.0 when the profile has no such key (`pages/Scenario Analysis.py:195-197`), while the engine default for a missing key is 0.03 (`FIRE.py:887`). For a profile that never saved the budget page, the untouched scenario runs with 0% creep against a 3% baseline and the page reports a large improvement caused by nothing. Also: only budget values are diffed, so a frequency edit to an existing row is ignored (`pages/Scenario Analysis.py:122`); the "move" event only offers the manual multiplier.
- **Rigorous.** Defaults live in one place (the schema); a scenario is an explicit diff and an empty diff must reproduce the baseline exactly (v1 has this test for `compare(profile, {})` but not for the page).
- **Not exposed.** n/a.

---

## 17. AI assistant

### 86. Tools

- **v1.** Two tools (`assistant.py:57-74`): `run_simulation` (optional chart list) and `scenario_analysis` (a `changes` object mirroring entry 83, plus charts). `execute_tool` runs them and returns a JSON summary to the model and the full results to the UI for charts (`assistant.py:114-128`). Errors are returned to the model as `is_error` results. Up to 6 tool rounds per turn (`assistant.py:23`, `139`). One seed per chat session (`pages/Try Firesight GPT.py:100`).
- **Verdict: sound pattern.** Clean separation of the tool loop from the client (testable with a scripted model, `tests/test_scenarios_and_storage.py:95-118`). Limits: each `scenario_analysis` is relative to the saved profile, not to the previous scenario, so the model cannot build up a plan step by step; the model sees only summary statistics (no time series, no per-year cash flow, no tax detail); every call reruns the whole Monte Carlo synchronously.
- **Rigorous.** Tools for: reading and patching the profile (with user confirmation), running a plan, comparing plans, querying results (a quantile at an age, the cash flow of a year on the median path, why a path failed), sweeping one parameter, and searching for a target ("earliest retirement age with at least 90% success").
- **Not exposed.** n/a.

### 87. Prompt and guardrails

- **v1.** A fixed guideline block (`assistant.py:76-87`) plus the entire profile as JSON in the system prompt (`assistant.py:90-91`). Rules: 3 to 5 sentences, zero to two charts, one recommendation, round dollars, never quote numbers not obtained from a tool in this conversation, ask for missing details, state assumptions, say when the model cannot represent something.
- **Verdict: sound rules, missing context.** Rules 6 to 9 are the right ones. But the model is given no description of what the simulator assumes or what its outputs mean. In particular `success_rate` (entry 81) will be read as plan success. "One clear, actionable recommendation per response" pushes toward advice on the basis of 25 noisy paths. No instruction about uncertainty or about not giving regulated advice. The default model id is hard-coded (`assistant.py:21`).
- **Rigorous.** A system prompt that includes a short, generated model card (assumptions, definitions of each output, known limits), requires intervals when quoting probabilities, and distinguishes "what the model shows" from "what you should do".
- **Not exposed.** n/a.

### 88. What the assistant can and cannot do

- **Can.** Read the saved profile; run the baseline; run a what-if using the keys in entry 83; add house, purchase, windfall, move (manual multiplier) and family milestones; choose from four run charts and two comparison charts (`assistant.py:54-55`).
- **Cannot.** Build or edit a profile from conversation (the stated v2 goal); save a scenario; change a single income source, a debt, the state, the goal method, tax or Social Security assumptions; express anything dated ("from 2030 my rent drops") other than through milestones, and `change_income`/`change_budget` milestones do nothing (entry 58), which is why the tool description omits them; see or explain a path; ask for anything beyond p10/p50/p90 summaries.
- **Verdict: simplistic.** It is a natural-language front end to the scenario form.
- **Rigorous.** Conversational profile building with structured extraction into the schema, shown to the user as a reviewable diff; scenario runs as above; explanations grounded in queried results.

---

## 18. Profile data model, storage and accounts

### 89. Profile schema

- **v1.** One flat JSON dict read with `data.get(key, default)` throughout `FIRE.py`. Allocation is nine `investment_*` keys. Milestones are flag bags. Stored as a JSON string (`storage.py:105`, `243`).
- **Verdict: simplistic.** No schema, no version, no validation, no migration. Misspelt or stale keys are silently ignored and defaults silently apply (entries 24, 42, 85). Units are inconsistent: percent for `growth_percentage`, loan `interest_rate`, rental mortgage rate and occupancy; fraction for `inflation`, `withdrawal_rate`, `mortgage_rate` and `property_tax_rate`. Things are identified by display name (`remove_income_sources`, budget changes, recorded column names), and the default income name is "Primary Income" for every source. One person, one profile per account. Top-level `city` and `country` are stored and unused by the engine.
- **Rigorous.** A versioned, typed schema (people, accounts, properties, debts, income streams, spending lines, events, policies, assumptions) with stable ids, one unit convention (fractions; today's dollars), validation with readable errors, and migrations. The same schema is the LLM's structured-output target.
- **Not exposed.** n/a.

### 90. Inputs the editor collects that the engine ignores or misreads

Summary of the dead or mismatched inputs found above (all **traced**):

| Input (editor) | What happens |
|---|---|
| `401k_match_eligible`, `401k_match_percentage` | never read (entry 29) |
| `growth_type: inflation` | no growth at all (entry 24) |
| growth checkbox on rental, gig, commission | no effect (entry 24) |
| frequency hourly/weekly/biweekly on assistance and "other" | income is zero (entry 30) |
| `exercise_strategy`, `custom_exercise_percentage` | no effect (entry 28) |
| `rental_state`, `mortgage_monthly_payment` | never read (entry 50) |
| `gig_work_hours_per_month` | never read (entry 27) |
| `annual_maintenance_cost` (house) | never read (entry 46) |
| `monthly_lease_payment`, `buy_residual`, `residual_value` | handler reads other key names; lease is free (entry 43) |
| `change_income`, `change_budget` and their generated fields | no handler (entry 58) |
| "Adjust for similar quality of life" | silently no adjustment (entry 57) |
| interest rate on savings accounts | discarded (entry 18) |
| "Home (Being Paid Off)" with rate and term | treated as an asset growing at the loan rate (entry 45) |
| credit card balance | no interest, no payments (entry 41) |
| `spouse_age`, `city`, `country` | never read (entries 2, 60, 89) |
| `additional_simulation_years`, `additional_retirement_income` | never read (entry 73) |
| "Fixed Value" goal | engine supports it, editor cannot select it (entry 74) |
| `real_estate_investment_type`, `hoa`, `from_city` | engine reads them, editor has no field (entries 13, 46, 57) |

Other editor behaviour: each sub-page writes only its own keys, so a user who never opens a sub-page relies on engine defaults that may differ from that page's defaults (married defaults to True, creep to 3%, retirement spending to 0); the withdrawal-rate slider is limited to 3 to 5% (`pages/Edit Profile.py:952-959`); the inflation slider moves in whole points.

### 91. Storage and accounts

- **v1.** `LocalStore` (one JSON file) or `DynamoStore`, chosen by environment (`storage.py:74-195`). PBKDF2-SHA256 with 240,000 iterations and a per-user salt (`storage.py:35-39`); legacy unsalted SHA-256 hashes are still accepted and never upgraded (`storage.py:47-48`). Optional invite code. Demo profiles live in the browser session only.
- **Verdict: sound for a demo, not a product.** Password hashing is reasonable. Session state is the only session (lost on refresh), there is no email verification, password reset, rate limiting or account deletion, and full household finances sit server-side as plaintext JSON. `Home.py:241` says "All rights reserved" next to an MIT licence.
- **Rigorous.** For a browser-side simulator the profile does not need to leave the device: local-first storage (IndexedDB) with JSON export and import, optional end-to-end-encrypted sync later. Only LLM calls send profile data off the device, and that should be stated to the user.
- **Not exposed.** Export and import.

### 92. Product copy versus model

- **v1.** `Home.py:84-217` claims thousands of simulations, machine-learning market models, tax optimisation across 401(k), IRA and HSA, withdrawal-strategy modelling, healthcare cost projection, education cost forecasting, debt payoff optimisation, behavioural-bias alerts and a community.
- **Verdict: none of it exists** in the engine (entries 5, 17, 39, 68, 76, 78).
- **Rigorous.** Describe what the model does, and publish its assumptions next to the results.

---

## 19. UI and visualisations

### 93. Run Simulation page (`pages/Run Simulation.py`)

What it shows, in order:

1. Text: average retirement age and its standard deviation (engine definition, conditional on retiring); "Runs reaching FI within N years: k of n" (lines 27-36).
2. Histogram of retirement ages, 20 bins, plus 10/25/50/75/90th percentiles as text (lines 37-43), conditional on retiring.
3. Net-worth fan chart against age: 5 to 95 and 25 to 75 bands and the median (lines 70-133). Nominal dollars. The x axis starts at the current age while the scenario page's starts one month later.
4. "Best, worst and average" lines against month number: the paths with the highest and lowest final net worth and the cross-path mean (lines 135-164).
5. Histogram of final net worth and its 10/25/50/75/90th percentiles as text (lines 166-177).
6. The average time series as a raw table (lines 179-182).
7. One randomly chosen path as a raw table (lines 184-188).

- **Verdict: simplistic.** Everything is about net worth and the date the target is touched. The run is not seeded, not cached and recomputed on each click.

### 94. Scenario Analysis page (`pages/Scenario Analysis.py`)

Inputs: income change slider, raise override, three retirement-target fields, editable budget table, nine allocation sliders, four life events (home, major purchase, windfall, move), inflation, lifestyle creep, years, paths.

Outputs: a headline sentence ("would delay/accelerate your retirement by ..."); three metric tiles (baseline and scenario median retirement age, share reaching FI with the change in points); overlaid 10 to 90 net-worth bands with medians; overlaid retirement-age histograms; a table of first-year income, spending, taxes, savings rate, FIRE number, median and 10th-percentile final net worth; a four-line text summary; the seed.

- **Verdict: the best page in v1**, undermined by the conditional median (entry 80), the partial common random numbers (entry 84) and the default mismatch (entry 85). The headline reports differences to the day from 25 paths.

### 95. Assistant page (`pages/Try Firesight GPT.py`)

Charts the model can request: net-worth band (10 to 90 with median), retirement-age histogram, budget pie, allocation pie; for comparisons, overlaid net-worth bands and overlaid retirement-age histograms (lines 29-67). Chat history with inline charts; a clear button; errors roll the conversation back (lines 132-137).

- **Verdict: sound structure, same thin content.** Two of the four run charts are pies of inputs.

### 96. What is missing from every page

- Probability of success by retirement age, and probability of having money left by age (survival curve of the portfolio against the survival curve of the people).
- Anything in today's dollars.
- Cash flow over time: income by source, spending by category, taxes, savings and withdrawals (stacked, on the median path and as bands).
- Balances by account type and by asset class over time; allocation drift.
- Tax over time (the column is `None`), effective rate, bracket use, where withdrawals come from.
- Spending actually achieved versus planned (for spending rules), and shortfall size on failed paths.
- Sequence-of-returns view: outcome against the first ten years' returns.
- Individual path explorer with the economy underneath it (inflation, equity return, recession shading, job loss).
- Sensitivity: which input moves the answer most (tornado), and a one-parameter sweep.
- Sampling error on any number; convergence.
- A statement of assumptions and definitions beside the results.

---

## 20. Performance

Not benchmarked (the v1 tree was not executed). The following is from reading the code.

### 97. Per-step cost

- **v1.** `step_month` is interpreted Python over object graphs: per month it walks every milestone, income source (twice, entry 3), budget item, lease, debt and vehicle with attribute lookups and float arithmetic, performs `datetime` addition, and calls `calculate_net_worth`, which walks them again.
- **Verdict.** Fine for 25 paths of 360 months (9,000 steps). The stated v2 target, 100,000 lifetimes of about 720 months, is 72 million steps, which this design cannot approach.
- **Rigorous.** Struct-of-arrays state in Rust compiled to WebAssembly, no allocation in the loop, integer time, pre-resolved event schedule, one path per call in a worker pool.

### 98. Recording and aggregation

- **v1.** Every month of every path builds a Python dict (`record_monthly_data`, `FIRE.py:1279-1313`) with 13 fixed keys plus one per vehicle, budget item, asset, debt and income source and two per lease, about 30 to 40 keys for the example profiles. Each path becomes a `DataFrame` (`FIRE.py:956-960`); all paths are concatenated and grouped (`FIRE.py:1504-1506`); the caller keeps the full list of frames (and the chat page keeps them in session state for every past answer).
- **Verdict.** Memory and time scale with paths × months × columns of boxed Python objects: 200 paths × 600 months is 120,000 dicts and roughly 4 million objects before pandas copies them. Column sets differ between paths when milestones add items, so concatenation also aligns and fills.
- **Rigorous.** Do not store paths. Accumulate fixed-size per-month summaries (quantile sketches or histograms, sums, counts) in each worker and merge; keep a handful of exemplar paths in full.

### 99. Return generation

- **v1.** Each chain is a Python loop with one scalar `np.random.normal` call through a lambda and one `random.random()` per month (`FIRE.py:165-344`). Bootstrap paths are Python lists of NumPy scalars. A separate path is generated for every sleeve, every owned home (at least 360 months) and every rental property, per run, including the unused 120-month "Other" path in every stress-test clone (entry 73).
- **Verdict.** Scalar NumPy calls are the slowest way to draw normals. Small in absolute terms at 25 paths.
- **Rigorous.** One joint scenario generated per path from a counter-based generator, written into a preallocated buffer.

### 100. Nested stress test

- **v1.** Each check deep-copies the household and runs 10 clones × 120 months (`FIRE.py:1333-1335`), every 6 months once net worth exceeds ten years of expenses.
- **Verdict.** Up to 1,200 extra month-steps per check and two checks per year: a 30-year path can cost 72,000 nested steps against 360 of its own, a 200-fold blow-up, while delivering a decision based on 10 samples.
- **Rigorous.** Entry 73: evaluate retirement ages as an outer grid on shared paths; cost is (number of candidate ages) × paths, with no nesting.

### 101. Caps, concurrency and caching

- **v1.** Path count is capped by the sliders at 100 (editor) and 200 (scenario page); horizon at 50 years. Everything runs on one thread inside the Streamlit request; a scenario comparison runs two full simulations plus two single-path snapshots; each assistant tool call does the same. Nothing is cached, so identical inputs recompute.
- **Verdict.** The caps exist because the engine is slow, and they are what keep the statistics too noisy to trust (entry 78).
- **Rigorous.** Web Worker pool sized to the device, progressive results (show 1,000 paths immediately, refine to 100k), cancellation, results keyed by a hash of (profile, assumptions, seed).

---

## 21. What the tests lock in

`tests/test_fire.py`, `tests/test_scenarios_and_storage.py` and `tests/test_pages.py` cover amortisation, the bracket loop, state bracket parsing, bootstrap contiguity, balance counting, seeded reproducibility, `apply_changes` purity, password hashing, the tool loop and page smoke tests. They check that things run and move in the expected direction. None asserts conservation of money, a known-answer household cash flow for a year, a tax return against a published example, or any property of the output distribution. Several encode the defects: interest on a checking object that no simulation updates (entry 18); the rental test that sidesteps the no-mortgage crash (entry 50); "married pays less than single" (entry 64); "side-hustle landlord reaches FI in at least 5 of 10 runs" (a threshold on a model output, not a correctness check). v2 needs golden-file tests for tax years, ledger conservation checks on every step in debug builds, and distribution tests for the scenario generator.

---

## 22. v1 -> v2 summary

| # | Relationship | v1 | Verdict | v2 |
|---|---|---|---|---|
| 1 | Clock | wall-clock start, +30 days per step | wrong | integer month index, derived calendar |
| 2 | Age, horizon | float age, 1 to 50 years from today | simplistic | per-person stochastic death age ends the run |
| 3 | Step ordering | income computed twice per month with side effects | wrong | one state transition per step, pure reads |
| 4 | Asset co-movement | independent path per sleeve | wrong | joint economic scenario generator |
| 5 | Equities | 12-month block bootstrap of a price-only series | simplistic, data error | real total returns sampled jointly, stationary bootstrap, fees |
| 6 | "Retirement Accounts" | a second, independent stock market | wrong | accounts are tax wrappers holding assets |
| 7 | Bonds | 3-state chain, 2.2%/yr, 0.7% vol | simplistic | yield-curve driven return, duration, TIPS |
| 8 | High-yield savings | 2-state chain, 2.0%/yr | simplistic | simulated bill rate less spread |
| 9 | P2P lending | 2-state chain, 2.9%/yr | simplistic | drop, or credit asset tied to recessions |
| 10 | Individual stocks | 3-state chain, 3.7%/yr, 7% vol | wrong | market beta plus idiosyncratic risk |
| 11 | Crypto | 3-state chain, about -16%/yr | wrong | user-stated return and volatility satellite |
| 12 | "Other" | 3-state chain, 4.1%/yr | simplistic | custom asset with explicit parameters |
| 13 | Real-estate sleeve | home-price index used as REIT | wrong | REIT as equity sub-asset; property as an object |
| 14 | Inflation level | constant 3% | wrong | stochastic, persistent, joint with rates and returns |
| 15 | Indexing and units | inflation touches budget and SWR target only; nominal output | wrong | statutory indexing everywhere; report in today's dollars |
| 16 | Starting balances | pooled, re-split by sliders | wrong | per-account balances, basis and allocation |
| 17 | Contributions, rebalancing | residual cash by target weights, never rebalanced, no fees | simplistic | contribution waterfall, rebalancing, glide path, fees |
| 18 | Cash interest | 0% (entered rate discarded) | wrong | bill rate less spread, taxed |
| 19 | Emergency fund | 6 months of month-zero expenses + 3 months of income | wrong | months of current essential spending, user-set |
| 20 | Shortfalls | forgiven; cash reset to zero; money duplicated near depletion | wrong | funding waterfall, conservation checks, explicit failure state |
| 21 | Other assets | fixed-rate or frozen nominal | simplistic | typed assets with liquidity flag |
| 22 | Net worth | everything, pre-tax, with side effects | simplistic | balance sheet, liquid, and after-tax spendable |
| 23 | Salary | deterministic gross, household-level | simplistic | per-person earnings process with paystub deductions |
| 24 | Raises | nominal constant; "inflation" option does nothing | wrong | inflation + real wage growth + age profile + shocks |
| 25 | Bonus | off-by-one counter, double-stepped; quarterly and monthly never paid | wrong | percent of salary in a calendar month, random payout |
| 26 | Commission | annual draw redrawn monthly, can be negative | simplistic | persistent, recession-sensitive, floored |
| 27 | Gig work | deterministic, taxed as wages | simplistic | self-employment income and tax |
| 28 | Stock options | paper value in net worth, never cash; most fields dead | wrong | grants with vesting, price dynamics, tax, sale |
| 29 | 401(k) match | collected, ignored | dead | deferral and match into typed accounts with limits |
| 30 | Assistance, other income | monthly/annual only; stops at retirement | wrong | typed streams with dates and tax treatment |
| 32 | Job loss | none | missing | employment state machine tied to recession state |
| 33 | Social Security | flat $1,500, only while not retired | wrong | AIME/PIA from earnings, claiming age, COLA, spousal, taxation |
| 34 | Post-retirement income | all non-rental income stops; stated retirement income never paid | wrong | per-stream end conditions; real cash flows |
| 35 | Budget items | undated lines, smoothed frequency | sound | dated, categorised, essential flag |
| 36 | Budget growth | inflation + 3% creep forever, all lines | wrong | real spending by category, creep tied to income, retirement curve |
| 37 | Not-working values | household switch per line | sound idea | work-related flags; modelled healthcare |
| 38 | Unplanned costs | three unused attributes | dead | spending shocks |
| 39 | Healthcare, LTC | a budget line | missing | employer -> ACA (subsidy on MAGI) -> Medicare (IRMAA); LTC shocks |
| 40 | Amortising debt | correct formula; float term can add two phantom payments; excluded from spending | mostly sound | integer terms, exact final payment, prepayment policy |
| 41 | Credit card | no interest, no payment | wrong | revolving debt with APR and minimums |
| 42 | Financed purchase | new personal loan; blank loan amount gives zero loan | sound core | loan derived from price and down payment |
| 43 | Lease | double-charged via API, free via editor, residual unused | wrong | dated spending line with optional buyout |
| 44 | Rent | budget line with creep | simplistic | rent index, changes on move |
| 45 | Existing home | asset growing at mortgage rate, or a perpetual budget line | wrong | property record with value, basis, loan, costs |
| 46 | House purchase | nominal price, typed rate, no closing costs or PMI | simplistic | indexed price, simulated rate, closing costs, PMI |
| 47 | Mortgage payment | charged twice; one copy inflates forever; escrow stops at payoff | wrong | fixed P&I once; indexed tax, insurance, maintenance |
| 48 | Home appreciation | independent smoothed index, stops after 360 months | simplistic | joint home-price process plus idiosyncratic risk |
| 49 | Home equity | counts toward retirement target; cannot be sold | wrong | excluded from spendable; explicit sell and downsize |
| 50 | Rental property value, loan | invented (rent x 100, 80% loan); entered payment ignored; crashes with no mortgage | wrong | property record |
| 51 | Rental cash flow | rent never grows; mortgage stepped twice a month | wrong | indexed rent, vacancy spells, capex |
| 52 | Rental tax and equity | taxed as wages incl. FICA; equity not on balance sheet | wrong | Schedule E, depreciation, passive loss rules, equity counted |
| 53 | Milestone dispatch | year-and-month match on a drifting clock | wrong | typed events on integer months, fire once, optional conditions |
| 55 | Major expense | cash can be partly free; nominal | simplistic | events in today's dollars through the waterfall |
| 56 | Windfall | cash, untaxed | sound | typed, optionally uncertain |
| 57 | Move | moving cost not paid; salary used as price index; option is a no-op in UI; state unchanged | wrong | location with category prices, tax jurisdiction, housing market |
| 58 | Income/budget change events | flags and fields with no handler | dead | dated changes on every stream and line |
| 60 | Marriage, spouse | a tax flag | simplistic | two people with their own records |
| 61 | Children | +$2,000 credit forever, no cost | wrong | dated children with age-based costs and credit rules |
| 62 | Divorce, death | flag changes only | wrong | survivor modelling from mortality; explicit scenarios |
| 63 | Federal income tax | 2020 brackets on gross cash income | wrong | typed-income tax function, current law, table-driven |
| 64 | Filing status | joint deduction with single brackets | wrong | full parameter sets per status |
| 65 | State tax | brackets on gross, no deductions, never changes | simplistic | per-state rules by status, follows residence |
| 66 | FICA | on all income, one wage base per household, 2023 base | wrong | per worker on earned income, indexed base, additional Medicare |
| 67 | Child credit | $2,000 per dependent against all taxes, no limits | simplistic | current-law credit with phase-out and age limit |
| 68 | Investment and withdrawal tax | none; no account types | missing | taxable basis, traditional, Roth, HSA; gains stack; RMDs; penalties |
| 69 | Tax timing | lump sum every 12 steps in arrears; never recorded | wrong | withholding and calendar-year true-up, recorded monthly |
| 70 | Indexing | all thresholds frozen | wrong | statutory indexing from simulated inflation and wages |
| 71 | Custom brackets | unreachable; no top bracket | dead | drop |
| 72 | SWR goal | total net worth vs separate, partly inflated target | wrong | rule of thumb on spendable assets vs own spending |
| 73 | Reduced-income stress test | 10 clones, 10 years, nominal 90%, all must pass | right idea, wrong execution | success-by-retirement-age grid on shared paths |
| 74 | Fixed-value goal | nominal net worth; not selectable | simplistic | real-dollar target; retire-at-age |
| 75 | Retirement event | instant, permanent, whole household | simplistic | per-person policy, phased, optional return to work |
| 76 | Decumulation | pro rata sales to refill cash, untaxed | wrong | withdrawal order, Roth ladder, spending rules |
| 77 | Failure detection | none | wrong | first unfunded month, size and age; success = never ran out while alive |
| 78 | Path count | 10 to 25 typical, 200 max | simplistic | 100k+ with intervals |
| 79 | RNG | two global streams, order-dependent | simplistic | per-path, per-purpose substreams |
| 80 | Retirement-age stats | mean/median of paths that retired; two definitions | wrong | censored distribution, cumulative curve |
| 81 | success_rate | "touched the target" | wrong | "never ran out while alive" |
| 82 | Bands and snapshot | nominal net worth, means, one-path first year | simplistic | streaming quantiles of real, spendable metrics |
| 83 | Scenario changes | flat dict, limited keys | sound, narrow | typed patch operations on the schema |
| 84 | Common random numbers | same seed, alignment breaks on most structural changes | partly true | identical economy per path by construction; paired differences |
| 85 | Scenario page defaults | creep default differs from engine | wrong | one source of defaults; empty diff equals baseline |
| 86 | LLM tools | run, compare | sound, thin | profile patching, queries, sweeps, goal seek |
| 87 | LLM prompt | good rules, no model card | simplistic | generated assumptions and definitions; intervals |
| 88 | LLM reach | front end to the scenario form | simplistic | conversational profile building with reviewable diffs |
| 89 | Profile schema | untyped flat dict, mixed units, names as ids | simplistic | versioned typed schema with ids and validation |
| 90 | Editor vs engine | about twenty inputs ignored or misread | wrong | generated forms from the schema; no unread fields |
| 91 | Storage | server-side JSON or DynamoDB, plaintext | simplistic | local-first, export/import, optional encrypted sync |
| 92 | Product copy | claims features that do not exist | wrong | describe the model; publish assumptions |
| 93-96 | Charts | net-worth fans and retirement-age histograms, nominal | simplistic | success and survival curves, cash flow, accounts, taxes, path explorer, sensitivity |
| 97-101 | Performance | Python objects, per-row dicts, pandas concat, nested clones, single thread | n/a | Rust/wasm struct-of-arrays, streaming summaries, worker pool |

---

## 23. The 15 changes that matter most for the correctness of the answer

Ranked by how much they change the number a user acts on.

1. **Redefine success as "never ran out of money while alive" and simulate to death.** v1 stops at "touched the target" inside a horizon the user picked (entries 2, 77, 81). Everything a retirement plan can get wrong happens after that point. Requires stochastic longevity and an explicit failure state.
2. **Make money conserve.** Shortfalls are forgiven, negative cash is reset to zero monthly, a nearly depleted portfolio is paid out repeatedly, down payments larger than the cash buffer are partly free, moving costs are not paid (entries 20, 57). No downstream statistic is trustworthy until every dollar has a source and a destination. One funding waterfall, asserted in tests.
3. **Replace independent per-asset paths and constant inflation with one joint economic scenario** (entries 4, 14): inflation, equity, bond, bill, home-price and wage series that move together, with recessions. Use real total returns (v1's equity series omits dividends, entry 5) and delete the invented chains (crypto at -16% a year, single stocks at 3.7%, bonds with 0.7% volatility; entries 7 to 12).
4. **Model account types and tax what comes out** (entries 6, 16, 68). Taxable with basis, traditional, Roth, HSA, cash. Without this, pre-tax balances are overvalued and retirement spending is understated by the retiree's entire tax bill.
5. **Test retirement readiness on after-tax spendable assets by forward simulation, not on net worth against a rule of thumb** (entries 22, 49, 72 to 74). Home equity and paper option value must not fund retirement unless the plan sells them. Report success probability by retirement age.
6. **Tie the retirement target to the spending actually simulated, and fix the spending path** (entries 34, 36, 72). Remove the default 3% real growth compounding forever (2.45 times real spending after 30 years); derive retirement spending from the budget; pay the post-retirement income that the target assumes.
7. **One state transition per month on an integer calendar** (entries 1, 3, 53). Fixes milestones that fire twice or never, bonuses that are underpaid or never paid, rental mortgages amortising at double speed, and the divergence of age, date and tax year.
8. **Current tax law, correct filing status, statutory indexing, tax paid when due** (entries 63 to 70). v1 overcharges married households by 22 to 43%, charges payroll tax on rent and benefits, undercharges two-earner payroll tax, lets brackets creep for decades and lets a year of unpaid tax earn market returns.
9. **Report in today's dollars** (entry 15). Nominal net worth 30 years out is not a number a person can use.
10. **Housing as a real asset, charged once** (entries 45 to 49). No double-counted mortgage, no escrow that vanishes at payoff, an existing home that can be described, price and rate from the simulated economy, transaction costs, and equity excluded from spendable wealth unless sold.
11. **Social Security computed from earnings and paid in retirement** (entry 33). For most households it is the largest retirement asset; v1 pays a flat amount only to people who have not retired.
12. **Healthcare by phase and long-term-care risk** (entry 39). The ACA subsidy depends on the same income the withdrawal strategy controls, so it cannot be a fixed budget line.
13. **Two earners with stochastic careers and recession-linked job loss** (entries 23, 24, 32, 60, 75). Earnings certainty until a household-wide retirement switch overstates how reliably the target is reached and removes the scenario that most often delays early retirement.
14. **A real decumulation policy** (entry 76): withdrawal ordering, Roth conversion ladder, RMDs and a spending rule such as guardrails, with their tax effects. This is where an early retiree has the most control over the outcome.
15. **Enough paths, per-path random streams and honest statistics** (entries 78 to 84). 100k paths with intervals; unconditional (censored) retirement-age distributions instead of survivor means; scenario comparisons on identical economies by construction, so that a reported difference is the decision and not sampling noise.

Close behind: rental property as a real asset (entries 50 to 52), children with costs (entry 61), and an input layer in which every field the user fills in is read by the engine (entries 58, 90).
