# Stochastic life models: specification and parameters

Research pass of 2026-11-17 for FIRESight v2. This document specifies every random process in a simulated lifetime that is not a market return: how long people live, what they earn, when they lose a job, what health and care cost, what children and houses cost, and how spending and withdrawals respond. Market variables (inflation, equity, bond and bill returns, the national home price index, a recession flag) come from the economic scenario generator and are treated here as inputs.

Machine-readable values live in:

- `data/actuarial/params.json` (one section per heading below, each with `confidence` and source URLs)
- `data/actuarial/ssa-life-table.csv` (SSA 2023 period life table, ages 0-119)
- `data/actuarial/ssa-mortality-improvement.csv` (Trustees' improvement rates by sex and age band)

## Conventions

- Dollar figures carry their own year. The engine converts them to simulation-start dollars and then grows them with simulated CPI plus any excess stated here.
- "Real" means deflated by simulated CPI. Results are reported in today's dollars.
- Time step: annual for earnings, health, spending and mortality; monthly for employment status (aggregated to the year).
- Confidence labels: **high** = read from the cited primary source in this pass; **medium** = well-known figure quoted from memory or a secondary source, or a mild derivation; **low** = judgement.
- ssa.gov, bls.gov and FRED refuse scripted downloads. SSA material was parsed from Internet Archive captures of the cited URLs; BLS figures were read through a fetch tool.

## Summary of defaults

| Module | Model | Key defaults |
|---|---|---|
| Mortality | SSA 2023 period table, Trustees' improvement by band, health multiplier | multiplier 0.75; improvement 0.5-1.5%/yr by band |
| Earnings | Age profile + AR(1) + transitory, in logs | rho 0.97, sigma_eta 0.10, sigma_eps 0.10 |
| Unemployment | Monthly hazard by education, age, regime; lognormal spell | BA 0.5%/month; median spell 2.2 months; recession x1.6 hazard, x1.8 duration; pay cut 8% / 15% |
| Healthcare | Premium by phase + capped lognormal out-of-pocket | ACA benchmark $704/month at 40 (2027); Medicare $7,300/person; CPI + 1.5 |
| Long-term care | End-of-life episode given age at death | lifetime risk 38% men / 55% women; blended $81,900/yr; CPI + 1.5 |
| Expense shocks | Compound Poisson, lognormal severity | lambda 0.5/yr, median 2.5% of annual spending, sigma 1.0, mean-neutral |
| Children | USDA table by age and income + childcare + college | core cost $6-8k (2015 $) middle income; infant care $15,500; college $31,900 / $52,600 / $67,700 |
| Retirement spending | Blanchett smile, clamped | about -1%/yr real from 70 to 85 |
| Housing | Carrying costs + local drift + sale noise | buy 3%, sell 7%, maintenance 1%, insurance 0.5%; sigma_local 4%/yr, sigma_sale 10% |
| Equity comp | Private: lottery at random date. Public: beta + fat-tailed idiosyncratic | private counted at zero in base plan; single stock idio vol 25-50% |
| Crypto | Fat-tailed, ruin hazard | vol 70%, median real growth 0, ruin 1%/yr |
| Withdrawals | Seven rule definitions | Guyton-Klinger 20% bands / 10% moves; Vanguard +5% / -2.5% |

---

## 1. Mortality and longevity

### Model

For each person, each simulated year:

```
q(sex, age, year) = q2023(sex, age)
                    * (1 - r_near[sex, band(age)]) ^ (min(year, 2050) - 2023)
                    * (1 - r_ult [sex, band(age)]) ^ max(0, year - 2050)
dies in year  <=>  U < min(1, m * q(sex, age, year)),   U ~ Uniform(0,1)
```

`year = start_year + (age - start_age)`, so each person follows a cohort diagonal. `m` is the health multiplier.

### Data

- Base table: SSA period life table for 2023, the one used in the 2026 Trustees Report. Male e0 75.79, female 81.06; e65 18.12 and 20.66. Life expectancies recomputed from the qx column match SSA's ex column to 0.01. **High.** [SSA actuarial life table](https://www.ssa.gov/oact/STATS/table4c6.html)
- Improvement: Trustees' intermediate assumptions, average annual percent reduction in central death rates. **High.** [2026 Long-Range Demographic Assumptions, Tables 2.2-2.3](https://www.ssa.gov/oact/TR/2026/2026_Long-Range_Demographic_Assumptions.pdf), [TR section V.A](https://www.ssa.gov/oact/TR/2026/V_A_demo.html)

| Age band | Men 2024-2050 | Men 2050+ | Women 2024-2050 | Women 2050+ |
|---|---|---|---|---|
| 0-14 | 1.43 | 1.50 | 1.45 | 1.53 |
| 15-49 | 0.51 | 0.82 | 0.67 | 0.89 |
| 50-64 | 0.85 | 0.92 | 0.86 | 0.96 |
| 65-84 | 0.85 | 0.73 | 0.80 | 0.66 |
| 85+ | 0.67 | 0.59 | 0.61 | 0.54 |

The Trustees' headline figure is a 0.73%/yr decline in the total age-sex-adjusted death rate for 2035-2100 (low-cost 0.27, high-cost 1.24). History: 1.00%/yr for 1900-2024, 0.82% for 1979-2024. Both recent Technical Panels recommended 1.0%. A documented fallback of 1.0% under 85 and 0.5% above is in the JSON.

### Health multiplier (default 0.75)

The SSA table describes the whole population. People planning early retirement are mostly upper-income non-smokers, and [Chetty et al. (2016)](https://jamanetwork.com/journals/jama/fullarticle/2513561) measure a 14.6-year (men) and 10.1-year (women) gap in life expectancy at 40 between the top and bottom 1% of income. A multiplier of 0.75 adds about 2.5 years at 65. **Medium, judgement.**

Computed for a person alive in 2026 (cohort basis, with improvement):

| Person | m = 1.0: LE, P(alive at 95) | m = 0.75: LE, P(alive at 95) |
|---|---|---|
| Man, 40 | 81.3, 15% | 84.7, 24% |
| Woman, 40 | 85.2, 23% | 88.2, 33% |
| Man, 65 | 84.2, 12% | 86.7, 21% |
| Woman, 65 | 86.7, 20% | 89.1, 30% |

For a healthy 65-year-old couple the chance at least one is alive at 95 is 45%. This is why "success" must be measured against stochastic lifetimes and not a fixed horizon.

### Knobs

Health (poor 1.5 / average 1.0 / healthy 0.75 / very healthy 0.6). Advanced: improvement scenario (0.33x / 1x / 2x, matching the Trustees' low-cost and high-cost scalers).

### Left out

Cause of death, spousal mortality correlation, race/education/geography beyond the single multiplier, pandemic shocks.

---

## 2. Earnings

### Model

```
ln Y(t) = ln Y0 + [f(age_t) - f(age_0)] + z(t) + e(t) - c(t)
z(t)    = rho * z(t-1) + eta(t),    eta ~ N(0, sigma_eta^2),   z(0) = 0
e(t)    ~ N(0, sigma_eps^2)
c(t)    = 0.5 * Var[z(t)] + 0.5 * sigma_eps^2      (so E[Y(t)] equals the profile)
```

`Y0` is the user's current real pay. `f` is a cubic in age by education from [Cocco, Gomes and Maenhout (2005)](https://doi.org/10.1093/rfs/hhi017), Table 2: `f(age) = const + b1*age + b2*age^2/10 + b3*age^3/100`.

| Education | const | b1 | b2 | b3 | Growth at 25 | at 35 | at 45 | at 55 | Peak age | Y(55)/Y(25) |
|---|---|---|---|---|---|---|---|---|---|---|
| No high school | -2.1361 | 0.1684 | -0.0353 | 0.0023 | 3.5% | 0.6% | -1.0% | -1.1% | 38 | 1.05 |
| High school | -2.1700 | 0.1682 | -0.0323 | 0.0020 | 4.4% | 1.6% | -0.1% | -0.6% | 44 | 1.36 |
| College | -4.3148 | 0.3194 | -0.0577 | 0.0033 | 9.3% | 3.7% | 0.1% | -1.6% | 45 | 2.03 |

The profiles were estimated with household fixed effects on PSID 1970-1992, so economy-wide real wage growth of that era (about 1%/yr) is already inside them. Do not add a productivity trend on top. **Medium-high**: coefficients are the widely replicated values quoted from memory; check against the paper's Table 2 before release.

Cross-check from [Guvenen, Karahan, Ozkan and Song (2021)](https://www.newyorkfed.org/medialibrary/media/research/staff_reports/sr710.pdf), SSA records for men (**high**, read from the paper): average earnings rise 60% from 25 to 55 for the median lifetime earner, 150% in a pooled regression, 2.5x at the 80th percentile, 4.8x at the 95th and 27.9x for the top 1%. The college profile above (2.03x) sits between the median and the 80th percentile.

### Shock sizes: "career volatility"

| Setting | rho | sigma_eta | sigma_eps | sd of log pay after 20 years (persistent part) | Who |
|---|---|---|---|---|---|
| Low | 0.97 | 0.05 | 0.05 | 0.17 | tenured, government, union scale |
| Medium (default) | 0.97 | 0.10 | 0.10 | 0.34 | typical salaried private sector |
| High | 0.97 | 0.17 | 0.25 | 0.59 | commission, bonus-heavy, self-employed, startup |

Literature calibrations for reference: CGM (rho 1, sigma_eta 0.10-0.13, sigma_eps 0.24-0.27); [Storesletten, Telmer, Yaron (2004)](https://doi.org/10.1086/383105) (0.952; 0.125 in expansions, 0.211 in contractions; 0.255); [Krueger, Mitman, Perri (2016)](https://www.nber.org/papers/w22319) (0.9695; 0.196; 0.228); [Guvenen (2009)](https://doi.org/10.1016/j.red.2008.06.004) (0.988 or 0.821 with heterogeneous profiles).

The medium default is deliberately about half the literature variance. **Judgement**, for two reasons. First, job loss and its scarring are modelled explicitly in section 3; Guvenen et al. show that once nonemployment shocks are separate, the Gaussian part shrinks to rho 0.97 and sigma_eps 0.135 (a pure Gaussian fit to the same data needs rho 1.02 and sigma_eps 0.76). Second, a large share of measured transitory variance in survey data is measurement error.

Keep sigma constant over the business cycle. [Guvenen, Ozkan and Song (2014)](https://doi.org/10.1086/675535) find that recessions do not raise the variance of earnings shocks; they fatten the left tail, which the job-loss module produces.

### Knobs

Education; career trajectory (flat / typical / fast-track = college profile growth x1.5 while positive / custom raise and stop age); career volatility (low / medium / high).

### Left out

Random heterogeneous growth rates (the user knows their trajectory), occupation-specific processes, hours choices, disability.

---

## 3. Unemployment

### Model

Per earner, monthly:

```
P(job loss this month) = s_edu * a_age * k_security * (1.6 if recession else 1)
Spell length D (months) ~ LogNormal(ln(med), 1.0), clipped to [0.25, 36]
   med = 2.2 * g_age * (1.8 if spell starts in a recession or within 12 months after)
On re-employment:  z <- z - L,   L ~ N(mu_L, 0.15^2)
   mu_L = 0.08 (expansion) or 0.15 (recession), x1.5 if age >= 50,
          + 0.01 per month of spell beyond 6, capped at +0.10
UI:  weekly = min(cap, 0.50 * prior weekly wage), for min(D, 26 weeks) after a 1-week wait
```

| Education | s (monthly, age 35-44, expansion) |
|---|---|
| Less than high school | 1.8% |
| High school | 1.2% |
| Some college | 0.9% |
| Bachelor's | 0.5% |
| Advanced degree | 0.35% |

Age multipliers `a`: 2.0 (20-24), 1.3 (25-34), 1.0 (35-44), 0.9 (45+). Duration multipliers `g`: 0.85 under 35, 1.0 for 35-54, 1.4 at 55+. A bachelor's-degree holder aged 40 has a 5.8% chance of at least one spell per year in an expansion.

Implied spell statistics: expansion mean 3.6 months, 16% longer than 6 months, 4.5% longer than a year; recession mean 6.6 months, 34% and 14%.

### Evidence

- Education gradient. [Cairo and Cajner (2018)](https://www.federalreserve.gov/pubs/feds/2014/201409/index.html), CPS 1976-2010, ages 25+, monthly: job-finding rates are the same across education (43-47%) while separation rates are 4.45% (less than high school), 2.48% (high school), 2.05% (some college), 1.09% (college); unemployment rates 9.0 / 5.5 / 4.4 / 2.6%. **High.** Our hazards are about half theirs because their time-aggregation-corrected flows count very short spells and quits; we pair lower incidence with longer completed spells. **Medium for the level.**
- Duration. BLS Table A-12, October 2026: mean 27.1 weeks, median 11.9 weeks, 27.6% of the unemployed out for 27 weeks or more, unemployment rate 4.1%. **High.** [BLS A-12](https://www.bls.gov/web/empsit/cpseea12.htm). These are spells in progress, which are length-biased; completed spells are shorter.
- Recession shift. In 2008-09 the employment-to-unemployment flow rose from about 1.3% to about 2.1% of employment per month and the job-finding rate fell about 40% ([Elsby, Hobijn, Sahin 2010](https://www.brookings.edu/articles/the-labor-market-in-the-great-recession/)); mean duration went from about 17 to about 40 weeks. **Medium.**
- Wage loss. [Davis and von Wachter (2011)](https://www.nber.org/system/files/working_papers/w17638/w17638.pdf): men with 3+ years of tenure displaced in mass layoffs lose 1.4 years of pre-displacement earnings in present value when unemployment is below 6% and 2.8 years when above 8% (20 years, 5% discount). [Jacobson, LaLonde, Sullivan (1993)](https://www.jstor.org/stable/2117574): 25% lower earnings six years on. [Couch and Placzek (2010)](https://doi.org/10.1257/aer.100.1.572): 13-15% six years on. [Farber (2017)](https://doi.org/10.1086/690054): full-time to full-time job changers lose 5-10% on average and about half earn as much or more. High-tenure mass-layoff estimates are an upper bound; the defaults sit between Farber and Davis-von Wachter. **Medium.**
- Unemployment insurance. [DOL, Significant Provisions of State UI Laws, January 2026](https://oui.doleta.gov/unemploy/content/sigpros/2020-2029/January2026.pdf): typical formula is 1/26 of high-quarter wages (50% of the weekly wage); maximum weekly benefit ranges from $275 (AL, FL) and $320 (AZ) through $450 (CA), $605 (TX), $767 (CO), $905 (NJ) to $1,152 (WA); 26 weeks in most states, 12 in FL. **High.** Default cap $600/week is a national-typical judgement. For a household earning $150k the cap binds and UI replaces under 20% of pay.

### Knobs

Job security (0.4x / 1x / 2x hazard); severance weeks; advanced: state UI cap and weeks.

### Left out

Quits, job-to-job moves, labour-force exit, disability. Employer-stock-linked layoff risk is handled in section 9.

---

## 4. Healthcare

### Model

```
cost(person, year) = premium(phase, age) + OOP(person, year)
ln OOP(t) = ln(mean_oop(age, plan)) - 0.5*sigma^2 + p(t) + u(t)
p(t) = 0.92 * p(t-1) + w(t),  Var[p] = 0.33 * sigma^2,  Var[u] = 0.67 * sigma^2
OOP  = min(OOP, plan out-of-pocket maximum)
all components grow at CPI + 1.5 points
```

### Phases and defaults

| Phase | Premium | Mean out-of-pocket | sigma | Cap |
|---|---|---|---|---|
| Employer | worker share: $1,765 single, $7,471 family (2026) | $800 (18-44), $1,200 (45-54), $1,700 (55-64) | 1.0 | plan maximum, about $4-5k |
| ACA, before 65 | benchmark silver by age, see below | same by age, x1.5 for bronze/high-deductible | 1.0 | $10,600 self / $21,200 family (2026) |
| Medicare + Medigap G | $5,135 (Part B $2,435 + Part D $500 + Medigap $2,200) | $2,183 (Part B deductible $283, drugs $600, dental/vision/hearing $1,300) | 0.5 | none needed |
| Medicare Advantage (alternative) | Part B + $0-240 | $1,800 | 1.0 | $5,421 in-network (2026 average) |

Sources and confidence:

- Employer: [KFF Employer Health Benefits Survey 2026](https://www.kff.org/health-costs/report/2026-employer-health-benefits-survey/). Average premium $9,804 single and $28,303 family; workers pay 18% and 27%; average single deductible $1,955 ($2,624 at small firms, $2,912 in HSA-qualified plans); family premiums +28% over five years against 24.4% inflation. **High.**
- ACA: national average benchmark silver premium for a 40-year-old was $497/month in 2025, $625 in 2026 (+26%) and $704 in 2027 (+13%) ([KFF](https://kff.org/affordable-care-act/9-things-to-watch-for-the-2027-aca-open-enrollment-period/)). **High.** With the CMS default 3:1 age curve (21 = 1.000, 40 = 1.278, 50 = 1.786, 55 = 2.230, 60 = 2.714, 64 = 3.000; full table in the JSON, **medium**, from memory), the unsubsidised 2027 benchmark is about $11,800 a year at 50, $14,700 at 55, $17,900 at 60 and $19,800 at 64. A 60-year-old couple faces about $36,000 before subsidies. Premium tax credits, and the 400%-of-poverty cliff that returned in 2026, are computed by the tax module from simulated MAGI.
- Medicare: Part B $202.90/month and $283 deductible in 2026 ([Federal Register](https://www.govinfo.gov/content/pkg/FR-2025-11-19/html/2025-20251.htm)); the 2027 amounts had not been announced on 2026-11-17 (Trustees' projection about $209.50). Part D base beneficiary premium $38.99 (2026), $41.33 (2027) ([KFF](https://www.kff.org/medicare/issue-brief/a-current-snapshot-of-the-medicare-part-d-prescription-drug-benefit/)). **High** for statutory amounts, **medium** for Medigap and residual out-of-pocket. Real cost rises a further 1.5%/yr with age (attained-age Medigap rating, utilisation; judgement).
- Cross-check: [Fidelity's 2026 estimate](https://www.foxbusiness.com/economy/fidelity-estimates-retirees-spend-185500-healthcare-medical-expenses-retirement) is $185,500 per 65-year-old for retirement, excluding long-term care. Our $7,300/yr default sums to about $210k over 21 years in today's dollars, or $172k without dental, vision and hearing, which Fidelity leaves out.
- Variance structure: [French and Jones (2004)](https://doi.org/10.1002/jae.790) fit log out-of-pocket health cost as an AR(1) with rho 0.922 plus white noise carrying about two thirds of the variance. Their data include nursing homes, which we model separately, so sigma is lower here. **Medium**; sigma is judgement.
- Pre-65 out-of-pocket means are order-of-magnitude [MEPS](https://meps.ahrq.gov/mepsweb/data_stats/quick_tables.jsp) values rolled to 2026 dollars. **Medium-low.**

### Excess medical inflation (default CPI + 1.5 points, range 0-3)

Medical-care CPI outran CPI-U by about 1.6 points a year over 1948-2024, 2.7 points in 1980-2000 and 0.7 points in 2000-2024. What households actually pay has grown faster: the Part B premium went from $45.50 in 2000 to $202.90 in 2026 (5.9%/yr against roughly 2.5% CPI), and ACA benchmark premiums rose 26% and 13% in the last two years. **Medium, judgement.**

### Knobs

Plan richness while working; pre-65 coverage (ACA benchmark / own quote / spouse's plan / COBRA); Medicare style; advanced: excess inflation.

### Left out

A joint health-state Markov chain driving cost, mortality and care together (the multiplier and section 5 carry the main links); employer retiree health; state-specific ACA curves; HSA mechanics (account module).

---

## 5. Long-term care

### Model (end-of-life episode)

Once a person's age at death `T` is known from section 1:

```
p(sex, T) = min(0.90, k * logistic(-1.208 + 0.065*(T - 70) + shift_sex))
            shift_male = -0.173,  shift_female = +0.402,  defined for T >= 65
with probability p:  paid care from age max(65, T - D) until T
D  ~ empirical distribution below (draw bin, then uniform inside it)
type ~ {home care 45%, assisted living 15%, nursing home 40%}
annual cost (2025 $) = 55,000 / 74,400 / 114,975, growing at CPI + 1.5 points
while in a facility, that person's share of ordinary spending is cut by 50%
```

| Years of paid care | 2 or less | 2-4 | 4-6 | 6-8 | 8-10 | over 10 | Mean |
|---|---|---|---|---|---|---|---|
| Men | 59% | 22% | 9% | 5% | 2% | 3% | 2.4 |
| Women | 44% | 24% | 13% | 8% | 5% | 6% | 3.4 |

Conditioning on the age at death keeps the mortality model untouched and reproduces the strongest fact in the data: care is concentrated at the end of long lives. The slope reproduces the observed 23% / 36% / 54% chance of paid care for deaths at 65-74 / 75-84 / 85+, and the sex shifts reproduce lifetime risks of 38% for men and 55% for women under the 2023 period table. With mortality improvement and the 0.75 health multiplier, lifetime risk rises to about 43% and 60%.

Expected lifetime cost for a 65-year-old at base settings is roughly 0.38 x 2.4 x $82k = $75k for a man and 0.55 x 3.4 x $82k = $153k for a woman in today's dollars, before the spending offset, with a long right tail (6% of women who use care need more than 10 years).

For engines that need a forward yearly hazard: `h(age) = A * exp(0.115 * (age - 65))`, A = 0.00608 (men), 0.00862 (women), calibrated to the same lifetime risks with independent mortality. Annual onset hazard at 75 / 85 / 95: men 1.9% / 6.1% / 19%; women 2.7% / 8.6% / 27%. The forward form weakens the link to imminent death; prefer the end-of-life form.

### Evidence

- [Johnson (2019), ASPE, HRS 1995-2014](https://aspe.hhs.gov/sites/default/files/migrated_legacy_files//188046/LifetimeRisk.pdf): 70% of adults who reach 65 develop severe needs (2+ ADLs or severe cognitive impairment); 48% receive some paid care (men 38%, women 55%); 29% paid home care, 5% residential care, 28% at least 90 days in a nursing home (men 20%, women 34%), 13% Medicaid-financed. 24% receive more than two years of paid care and 13% more than four. Among users, paid care lasts 2.4 years on average (nursing home 2.3, home care 1.8, residential 2.5). Education barely changes the risk of paid care (49 / 48 / 48%) but strongly changes Medicaid use (17% without high school, 5% with a bachelor's). **High.**
- [Favreault and Dey (2021), ASPE, DYNASIM projection](https://aspe.hhs.gov/sites/default/files/private/pdf/265066/MostLikelyIB.pdf): over half of every earnings quintile will use paid care (56% in the top quintile); 37% any nursing home; 9% two or more years. **High.**
- [CareScout (Genworth) Cost of Care Survey 2025](https://investor.genworth.com/news-events/press-releases/detail/1054), national medians: in-home caregiver $35/hour ($80,080 a year at 44 hours a week); assisted living $6,200/month ($74,400); nursing home semi-private $315/day ($114,975), private $129,575; adult day health $24,700. **High.** The $55,000 home-care default assumes about 30 paid hours a week.
- Care-type mix and the 50% spending offset are **judgement**.

### Medicaid backstop

Medicaid pays for nursing-home care once a single person's countable assets are about $2,000, with nearly all income going to the facility; a community spouse keeps the home and a resource allowance (federal range roughly $32,500-$162,660 for 2026, **medium**). The model does not credit Medicaid as income: a path that exhausts liquid assets is a failure. The results screen should say that Medicaid is the real floor.

### Knobs

Care risk (0.7x / 1x / 1.3x); cost level (national / low-cost / high-cost area, or custom); optional long-term-care insurance (premium, benefit, benefit period, elimination period, inflation rider).

### Left out

Unpaid family care, multiple episodes, transitions between settings within an episode, short Medicare-covered rehab stays, Medicaid planning.

---

## 6. Irregular expense shocks

### Model

```
N(year) ~ Poisson(lambda)
each shock = base annual spending * LogNormal(ln 0.025, 1.0)
if mean-neutral: baseline spending is reduced by lambda * 0.041 * base
```

Defaults: lambda 0.5 a year (0.4 renters, 0.6 owners). Implied: 39% of years have at least one shock; mean shock 4.1% of annual spending, 95th percentile 13%, 99th percentile 26%; expected cost 2.1% of spending a year.

Mean-neutral is on by default because most people's stated annual spending already averages in irregular items. The shocks then add lumpiness without quietly raising the budget. Medical shocks are excluded (section 4).

### Evidence

- [JPMorgan Chase Institute, Coping with Costs (2017)](https://www.jpmorganchase.com/institute/all-topics/household-financial-health/report-coping-with-costs): 37% of families make at least one extraordinary payment a year (medical 16%, auto repair 8%, tax 19%); 10% make more than one; medians $1,143 / $953 / $2,142, $1,520 overall; a year later savings are still depleted for 48%. **High** (via the report's summary table).
- [Pew (2015)](https://www.pew.org/en/research-and-analysis/articles/2015/10/the-role-of-emergency-savings-in-family-financial-security): 60% of households had a financial shock in the past year; the median most-expensive shock cost $2,000, about half a month of income. **Medium** (from memory; page not re-fetched).
- [Federal Reserve SHED 2025](https://www.federalreserve.gov/publications/2026-economic-well-being-of-us-households-in-2025-executive-summary.htm): 63% would cover a $400 emergency with cash or its equivalent. **High.**

The severity as a share of spending is **judgement**: $1,520 was about 2.5-3% of median household spending at the time.

### Knobs

Irregular expenses off / typical / high (lambda 0, 0.5, 1.0).

### Left out

Correlation with recessions or job loss; per-policy deductibles; large planned purchases (entered as goals).

---

## 7. Children

Deterministic by default.

### Annual cost per child

[USDA, Expenditures on Children by Families, 2015](https://fns-prod.azureedge.us/sites/default/files/crc2015_March2017.pdf), Table 1, married couples, 2015 dollars (**high**):

| Age | Low income (under $59,200) | Middle ($59,200-107,400) | High (over $107,400) |
|---|---|---|---|
| 0-2 | 9,690 | 12,680 | 19,770 |
| 3-5 | 9,700 | 12,730 | 19,790 |
| 6-8 | 9,330 | 12,350 | 19,380 |
| 9-11 | 9,960 | 13,180 | 20,700 |
| 12-14 | 9,570 | 13,030 | 21,050 |
| 15-17 | 9,980 | 13,900 | 23,380 |
| Total 0-17 | 174,690 | 233,610 | 372,210 |

Only child x1.27; each child in a family of three or more x0.76. Inflate by CPI-U(now) / 237.017, roughly 1.38-1.40 in late 2026 (**medium**); the same factor moves the income boundaries to about $82k and $149k. [Sawhill and Welch at Brookings (2022)](https://www.brookings.edu/wp-content/uploads/2022/08/Brookings_Cost-to-raise-a-child_inflation-adjusted-2.pdf) restated the middle-income total as $310,605 nominal for a child born in 2015, assuming 4% inflation.

Recommended default is the **core** cost, USDA total minus housing minus childcare and education, because the user's actual home is modelled and childcare is explicit. Middle income, 2015 dollars: 6,130 (0-2), 6,180, 6,960, 7,790, 7,920, 8,130 (15-17). All tiers are in the JSON with every budget component.

### Childcare

[Child Care Aware of America, 2025](https://info.childcareaware.org/price-and-supply-2025): national average $13,184 (2024: $13,128); infant centre care $15,015-15,728 depending on weighting; infant family child care $11,673-13,834; four-year-old centre $12,165-12,555. Prices rose 23% over 2021-2025 against 24% CPI. **High.**

Defaults for full-time care, 2025 dollars: $15,500 (age 0-1), $14,000 (age 2), $12,400 (age 3-4), $6,000 for before/after-school and summer care (age 5-12; estimate, **low**). Growth CPI + 1 point (judgement).

### College

[College Board, Trends in College Pricing 2026](https://research.collegeboard.org/trends/college-pricing/highlights) (**high**):

| Sector | Tuition and fees 2026-27 | Full budget 2025-26 | Estimated budget 2026-27 |
|---|---|---|---|
| Public two-year, in-district | $4,210 | $21,320 | |
| Public four-year, in-state | $12,290 | $30,990 | $31,900 |
| Public four-year, out-of-state | $32,730 | $50,920 | $52,600 |
| Private nonprofit four-year | $46,350 | $65,470 | $67,700 |

Net of grant aid, average tuition and fees are $1,920 at in-state public four-year colleges and $17,280 at private nonprofits. Those averages reflect need-based aid that high-income households will not get, so default to published prices for them. Full-pay cost at the most expensive private universities is $95-100k.

College inflation default is CPI + 1 point (range 0-3). Published tuition rose about 3 points a year above CPI from the 1980s to the mid-2010s; over the last decade it fell in real terms at public colleges and rose about 2% in total at private ones. **Judgement.**

### Knobs

Children and birth years (including planned); childcare none / part-time / full-time; college plan per child and the share parents pay.

### Left out

529 and aid formulas (other modules), private K-12 (custom expense), fertility uncertainty, support for adult children.

---

## 8. Retirement spending path

### Blanchett smile (default)

[Blanchett (2014)](https://www.financialplanningassociation.org/article/journal/MAY14-exploring-retirement-consumption-puzzle):

```
delta(age) = 0.00008*age^2 - 0.0125*age - 0.0066*ln(ExpTar) + 0.546
spend(t+1) = spend(t) * (1 + inflation) * (1 + delta(age_t))
```

`ExpTar` is target annual spending in dollars.

| ExpTar | delta at 65 | 70 | 75 | 80 | 85 | 90 | 95 | Real level at 85 vs 65 | at 95 |
|---|---|---|---|---|---|---|---|---|---|
| $50,000 | 0.0% | -0.8% | -1.3% | -1.3% | -1.0% | -0.2% | +0.9% | 0.82 | 0.80 |
| $100,000 | -0.5% | -1.3% | -1.8% | -1.8% | -1.5% | -0.7% | +0.5% | 0.75 | 0.69 |
| $150,000 | -0.7% | -1.6% | -2.0% | -2.1% | -1.7% | -1.0% | +0.2% | 0.71 | 0.64 |

**Medium-high**: formula quoted from memory; the table reproduces the published shape.

The late-life upturn in Blanchett's data is health and care spending. This model simulates both explicitly, so apply the smile only to the non-health budget and clamp delta at zero once it turns positive.

### Alternatives

- Flat real spending (the classic assumption; conservative).
- Consumer Expenditure Survey pattern. Average annual expenditures by age of reference person, 2024 ([BLS](https://www.bls.gov/opub/reports/consumer-expenditures/2024/) via FRED, **high**): $74,475 (25-34), $91,229 (35-44), $100,327 (45-54), $84,946 (55-64), $65,354 (65-74), $55,834 (75+); all units $78,535 (2025: $81,905). The 75+ group spends 66% of the 55-64 group. This is a cross-section of different cohorts and household sizes and overstates what one household would choose, so it is offered as a "steeper decline" preset: -2%/yr real from 65 to 75, -1% to 85, flat after.

### Knobs

Spending path: flat / smile / steeper decline. Survivor spending after the first death defaults to 70% of the couple's baseline.

---

## 9. Housing

Prices: the scenario generator supplies the national index. Each owned home adds its own risk:

```
ln P(t) = ln P(0) + [ln HPI(t) - ln HPI(0)] + m(t) + x(at sale)
m(t) = m(t-1) + N(0, 0.04^2)          local drift (metro + neighbourhood)
x    ~ N(0, 0.10^2)                    drawn once per actual transaction
```

Implied idiosyncratic standard deviation: 10.8% at one year, 16% at ten, 24% at thirty.

Evidence: [Flavin and Yamashita (2002)](https://doi.org/10.1257/000282802760015757) find a 14.2% annual standard deviation of real returns on an individual house in the PSID; [Case and Shiller (1989)](https://www.jstor.org/stable/1804778) about 15%, mostly house-specific; [Giacoletti (2021)](https://doi.org/10.1093/rfs/hhab033) shows this risk grows far more slowly than the square root of the holding period, being realised mainly at sale. Hence a small annual drift plus a one-off sale shock, not a 12-15% annual random walk that would give absurd 30-year dispersion. **Medium**; the split is judgement.

Costs:

| Item | Default | Range | Confidence |
|---|---|---|---|
| Buyer closing costs | 3% of price | 2-5% | medium |
| Seller costs (commission about 5%, transfer tax, title, concessions) | 7% of price | 6-10% | medium |
| Maintenance | 1.0% of value a year, plus 0.5% capital replacement | 1-2% | medium |
| Homeowners insurance | 0.5% of value, growing CPI + 2 for ten years | 0.3-2% | medium |
| Property tax | 0.9% of value | 0.3-2.3% | medium |
| PMI | 0.5% of loan a year above 80% LTV with good credit; 1.0% fair credit | 0.2-1.9% | medium-low |
| Rent growth | CPI + 0.75 points, sd 2% | | medium |

PMI stops automatically when the scheduled balance reaches 78% of the original value and on request at 80% ([CFPB](https://www.consumerfinance.gov/ask-cfpb/when-can-i-remove-private-mortgage-insurance-pmi-from-my-loan-en-202/), **high**). A full LTV-by-FICO grid is in the JSON (0.19% at 85% LTV / 760+ to 1.86% at 97% / 620-639); it is a representative rate card from memory and insurers now price loan by loan. Maintenance: [Harding, Rosenthal, Sirmans (2007)](https://doi.org/10.1016/j.jue.2006.07.007) estimate gross depreciation near 2.5% a year. Insurance: average premium on new agency loans rose from $1,270 (2018) to $1,856 (2024) per the [Urban Institute](https://www.urban.org/sites/default/files/2026-09/Housing_Finance_at_a_Glance_A_Monthly_Chartbook_September_2026.pdf).

Knobs: home value, mortgage and move plans; location cost tier or custom tax and insurance rates; credit tier.

Left out: metro-specific indices, HOA fees, uninsured disaster loss, refinancing choices.

---

## 10. Equity compensation and concentrated stock

### Private-company equity

Evidence: [Correlation Ventures](https://medium.com/correlation-ventures/venture-capital-no-were-not-normal-32a26edea7c7), 21,000+ US venture financings 2004-2013: 64.8% returned less than 1x, 25.3% 1-5x, 5.9% 5-10x, 2.5% 10-20x, 1.1% 20-50x, 0.4% over 50x. Those are investors' preferred-stock outcomes; employees hold common stock behind preferences. [Hall and Woodward (2010)](https://doi.org/10.1257/aer.100.3.1163): about three quarters of founders of venture-backed firms get nothing at exit. Median company age at IPO has been 10-12 years since 2010 ([Ritter](https://site.warrington.ufl.edu/ritter/ipo-data/)).

Model: the stake pays `V * M` at random time `T` and nothing before. `T ~ LogNormal(ln(median), 0.5)`; no liquidity events in recession years. `M` is log-uniform inside the drawn class.

| Stage | Zero | Modest (0.1-1x paper) | Good (1-4x) | Large (4-20x) | Median years | Expected multiple |
|---|---|---|---|---|---|---|
| Seed to A | 70% | 20% | 8% | 2% | 7 | 0.45 |
| B to C | 50% | 30% | 16% | 4% | 5 | 0.86 |
| Late, pre-IPO | 30% | 45% | 22% | 3% | 3 | 0.95 |

**Low-medium**, judgement anchored to the venture distribution. Product default: count private equity at zero in the headline success rate and show the upside as a separate scenario.

### Single listed stock

```
r(t) = rf + beta*(r_mkt(t) - rf) + e(t),   e ~ scaled Student-t(4), no alpha
each year, with probability h, the position goes to zero
```

| Preset | beta | Idiosyncratic vol | Ruin hazard |
|---|---|---|---|
| Large cap | 1.0 | 25% | 0.3% |
| Tech / growth | 1.2 | 35% | 1% |
| Small or recent IPO | 1.3 | 50% | 3% |

A stock with 40% total volatility grows at the median about 0.5 x (0.40^2 - 0.16^2) = 6.7 points a year slower than a 16%-volatility market with the same expected return. The simulation should show that drag; it does not need a lower mean.

[Bessembinder (2018)](https://doi.org/10.1016/j.jfineco.2018.06.004): of about 26,000 US common stocks from 1926 to 2016, only 42.6% beat one-month Treasury bills over their lifetimes; the most common outcome was a total loss; 4.3% of firms produced all net wealth creation. Globally 1991-2020, 55.2% of US and 57.4% of non-US stocks underperformed bills ([ASU](https://wpcarey.asu.edu/department-finance/faculty-research/do-stocks-outperform-treasury-bills)). **High.** Firm-level volatility: [Campbell, Lettau, Malkiel, Xu (2001)](https://doi.org/10.1111/0022-1082.00318).

If the stock is the user's employer, multiply that earner's job-loss hazard by 3 in any year the idiosyncratic return is below -40% (judgement).

Knobs: private stage, paper value, include in base plan (default no); single-stock preset and a diversification schedule.

Left out: exercise strategy, AMT, 83(b), QSBS (tax module); preference waterfalls.

---

## 11. Crypto

These are assumptions, and the interface should say so.

```
monthly log return = mu_log/12 + (0.70/sqrt(12)) * t4,   corr(t4, equity shock) = 0.35
each year, with probability h, permanent 100% loss
```

| Parameter | Default | Reasoning |
|---|---|---|
| Annual volatility | 70% | Bitcoin has mostly realised 50-80% since 2018; smaller tokens 80-120%. Drawdowns of 75-85% in 2014, 2018, 2022. |
| Tails | Student-t, 4 degrees of freedom | Daily and monthly moves far beyond normal. |
| Correlation with equities | 0.35 (0.6 in equity drawdowns) | Near zero before 2020, about 0.3-0.4 since, higher in sell-offs. It has not hedged when it mattered. |
| Median real growth (mu_log) | 0 (skeptic -10%, believer +5%) | No cash flows to anchor a return. Neutral = the median path keeps pace with inflation. |
| Ruin hazard | 1%/yr bitcoin or ether; 8%/yr other tokens | Protocol, regulatory, custody and cryptographic risk; most tokens ever listed are dead. 1%/yr is 26% over 30 years. |

With 70% volatility, a median real growth of zero still means an arithmetic mean near +24% a year: most paths drift down and a few rise enormously. Giving crypto an equity-like arithmetic mean instead would imply the median path loses about 18% a year. Show sensitivity to position size: 5% of a portfolio barely moves plan success; 30% makes outcomes bimodal. **Low by construction.**

---

## 12. Withdrawal rules

Notation: `B(t)` portfolio at the start of year t before withdrawal; `W(t)` withdrawal; `IWR = W(0)/B(0)`; `cpi(t)` inflation over the prior year. Withdrawals at the start of the year.

**Constant dollar (Bengen).** `W(0) = IWR * B(0)`; `W(t) = W(t-1) * (1 + cpi(t))`. Default IWR 4%. [Bengen (1994)](https://www.financialplanningassociation.org/sites/default/files/2021-04/MAR04%20Determining%20Withdrawal%20Rates%20Using%20Historical%20Data.pdf) found 4.15% survived every 30-year period from 1926 with 50-75% stocks.

**Constant percentage.** `W(t) = p * B(t)`. Default p 4%.

**Guyton-Klinger guardrails.** Checked against the [2006 paper](https://www.financialplanningassociation.org/sites/default/files/2021-11/2006%20-%20Guyton%20and%20Klinger%20-%20Decision%20Rules%20and%20SWR%20%281%29.PDF) (**high**). Each year, in order:

1. Withdrawal rule (modified 2006 version): `W(t) = W(t-1) * (1 + cpi(t))`, except no increase if last year's portfolio total return was negative **and** the resulting withdrawal rate would exceed IWR. Missed increases are never made up.
2. Capital preservation rule: if `W(t)/B(t) > 1.20 * IWR`, cut `W(t)` by 10%. Switched off in the last 15 years of the planning horizon.
3. Prosperity rule: if `W(t)/B(t) < 0.80 * IWR`, raise `W(t)` by 10%.
4. The result is next year's base.

Portfolio management rule: after a year in which an asset class's gains push it above target, move the excess to cash; fund withdrawals from (1) overweight equities, (2) overweight fixed income, (3) cash, (4) remaining fixed income, (5) remaining equities in order of prior-year performance; take nothing from equities after a negative equity year if cash and fixed income suffice.

Guyton's 2004 inflation rule capped each increase at 6%; the 2006 paper tests it and drops it from the final set. Offer it as an optional switch. The paper's result: 5.2-5.6% initial rates sustained for 40 years at 99% confidence with at least 65% equities. Caveat: because the bands are on the withdrawal rate, a deep bear market can trigger several consecutive cuts and real spending falls of 30-50%. Always show the spending distribution.

**Vanguard dynamic spending.** `target = p * B(t)`; `prev = W(t-1) * (1 + cpi(t))`; `W(t) = min(max(target, prev * (1 - 0.025)), prev * (1 + 0.05))`. Ceiling 5%, floor 2.5%, limits relative to last year's real spending. [Vanguard](https://corporate.vanguard.com/content/dam/corp/research/pdf/from_assets_to_income_a_goals_based_approach_to_retirement_spending.pdf).

**VPW (variable percentage withdrawal).** `W(t) = B(t) * r / (1 - (1+r)^(-n)) / (1+r)`, with `n` years remaining including this one and `r = equity weight * 5.0% + bond weight * 1.9%` (real); `B/n` if r is zero; cap the percentage at 10%. For a 60/40 portfolio (r = 3.76%): 4.7% with 40 years left, 5.4% with 30, 6.9% with 20, 11.7% with 10. [Bogleheads](https://www.bogleheads.org/wiki/Variable_percentage_withdrawal). Formula **high**; return assumptions **medium**.

**Floor and ceiling (Bengen 2001).** `target = p * B(t)`; `W(t) = min(max(target, 0.85 * W0_real(t)), 1.20 * W0_real(t))`, where `W0_real` is the first withdrawal carried forward with inflation. Limits are relative to the first year, unlike Vanguard's. Default p 4.5%. Band values from [Pfau's summary](https://retirementresearcher.com/floor-ceiling-retirement-spending-strategy/) (**medium**).

**Rising equity glidepath.** `E(t) = E_start + (E_end - E_start) * min(t, N)/N`. Defaults 30% to 70% over 30 years. [Pfau and Kitces (2014)](https://www.financialplanningassociation.org/article/journal/JAN14-reducing-retirement-risk-rising-equity-glide-path): starting at 20-40% equities and rising to 60-80% beats static and declining paths in bad outcomes.

**Bond tent.** Equity falls linearly from `E_work` (80%) to `E_min` (40%) over the 10 years before retirement, then rises to `E_late` (70%) over the next 15. [Kitces](https://www.kitces.com/blog/managing-portfolio-size-effect-with-bond-tent-in-retirement-red-zone/).

Glidepaths are allocation rules and combine with any spending rule. Left out: RMD-based spending, utility-optimal dynamic programming, risk-based guardrails that need nested simulation, annuitisation.

---

## 13. Social Security: points that are easy to get wrong

Benefit computation belongs to its own module. For early retirees and couples:

1. AIME is the top 35 years of indexed earnings over 420 months. Retiring at 45 after 23 years leaves 12 zeros. Earnings before 60 are indexed to average wages in the year the worker turns 60, so credits already earned keep pace with wage growth after work stops.
2. Bend points are those of the year the worker turns 62 and grow with average wages: 90% of the first $1,286 of AIME, 32% to $7,749, 15% above (2026). Project them with wage growth, not CPI.
3. COLAs accrue from 62 whether or not the benefit has been claimed.
4. With full retirement age 67: 70% at 62, 75% at 63, 80% at 64, 86.67% at 65, 93.33% at 66, then +8% a year to 124% at 70. Reductions are 5/9 of 1% per month for the first 36 months and 5/12 of 1% beyond; credits are 2/3 of 1% per month. Claiming is monthly.
5. Spousal benefit is up to 50% of the worker's PIA at the spouse's full retirement age, with no delayed credits, falling to 32.5% at 62. Deemed filing applies to anyone born in 1954 or later.
6. The survivor keeps the larger benefit, including the deceased's delayed credits (floor 82.5% of the deceased's PIA). The higher earner delaying to 70 is therefore insurance for whoever lives longer; the lower earner's claiming age matters only until the first death.
7. Household benefits fall by a third to a half at the first death while the survivor moves to single tax brackets.
8. The earnings test before full retirement age ($1 withheld per $2 above $24,480 in 2026) is a deferral; withheld months are credited back.
9. WEP and GPO were repealed (Social Security Fairness Act); do not model them.
10. Benefit-taxation thresholds are not indexed, so the taxable share creeps up in a long nominal simulation.
11. Offer a trust-fund scenario: scheduled benefits (default) or a 20-23% cut from 2033 (2025 Trustees: OASI depletion 2033, 77% payable).

Formulas **high**; 2026 dollar thresholds **medium** (from memory); 2027 values not retrieved. [SSA PIA formula](https://www.ssa.gov/oact/cola/piaformula.html), [early/late factors](https://www.ssa.gov/oact/quickcalc/early_late.html).

---

## 14. Cross-module dependencies

| Driver | Affects | Mechanism |
|---|---|---|
| Recession flag from the scenario generator | job loss, spell length, pay cut on re-employment, private-equity liquidity | regime multipliers in sections 3 and 10 |
| Simulated CPI | every dollar amount | plus 1.5 points (health, long-term care), 1.0 (childcare, college), 0.75 (rent), 2.0 for ten years (home insurance) |
| Age at death | long-term care | probability and timing of the episode |
| Health multiplier | mortality, lifetime care risk | longer life raises care risk |
| Employer stock crash | that earner's job loss | hazard x3 |
| Persistent earnings state z | Social Security AIME, savings | scarring from job loss lowers both |

## 15. Items to verify before release

- Cocco-Gomes-Maenhout coefficients against the paper's Table 2.
- CMS default age curve factors and the 2027 ACA out-of-pocket maximum.
- 2027 Medicare Part B premium and deductible once CMS publishes them; 2027 Social Security bend points, wage base, earnings-test limits and COLA.
- College Board 2026-27 housing-and-food and full-budget figures (only tuition was read for 2026-27).
- Medigap Plan G premium level; MEPS out-of-pocket means by age.
- PMI rate grid against a current insurer rate card.
- Pew (2015) shock statistics; Blanchett coefficient values against the published article.
- 2026 Trustees Report trust-fund depletion date and payable percentage.
