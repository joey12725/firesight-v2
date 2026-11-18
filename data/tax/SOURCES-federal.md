# Sources for `us-federal-2026.json`

All sources were fetched on **2026-11-17**. Each block in the JSON carries a `source` key that maps to the `sources` object at the bottom of the JSON; this file lists the same sources by number group, plus what could not be confirmed from a primary source.

SSA pages reject plain scripted requests (HTTP 403); they were fetched with full browser headers. IRS PDFs were downloaded and text-extracted; bracket tables were cross-checked two ways (against the cumulative-tax constants printed in each Rev. Proc. table, and against the IRS news release).

## Income tax

| Number group | Source | URL |
|---|---|---|
| 2026 ordinary brackets (single, MFJ, HoH, MFS), LTCG 0%/15% ceilings, standard deduction, additional 65+/blind deduction, dependent standard deduction, AMT exemption/phase-out, CTC $2,200 / $1,700 refundable, QBI thresholds, LTC premium limits, gift exclusion, $15M estate exclusion | IRS Rev. Proc. 2025-32 | https://www.irs.gov/pub/irs-drop/rp-25-32.pdf |
| Cross-check of 2026 brackets and standard deduction | IRS news release | https://www.irs.gov/newsroom/irs-releases-tax-inflation-adjustments-for-tax-year-2026-including-amendments-from-the-one-big-beautiful-bill |
| 2027 brackets, LTCG ceilings, standard deduction, AMT, CTC $2,300 / $1,800, QBI thresholds and $415 / $1,030 minimums | IRS Rev. Proc. 2026-35 (released 2026-11-05) | https://www.irs.gov/pub/irs-drop/rp-26-35.pdf |
| Cross-check of 2027 brackets | IRS news release | https://www.irs.gov/newsroom/irs-releases-tax-inflation-adjustments-for-tax-year-2027-taxpayers-continue-to-benefit-from-the-working-families-tax-cuts |
| OBBBA statute text: permanent rates (sec. 70101), standard deduction (70102), senior deduction $6,000 / 6% over $75k/$150k / 2025-2028 (70103), CTC (70104), QBI phase-in $75k/$150k and $400 minimum (70105), mortgage limit permanent (70108), itemized deduction 2/37 limit (70111), SALT cap schedule and 30% phase-down (70120), dependent care FSA $7,500 (70404), CDCC rate schedule (70405), non-itemizer charitable $1,000/$2,000 (70424), 0.5% charitable floor (70425), PTC repayment cap repeal (71305), bronze/catastrophic HSA eligibility (71307) | Public Law 119-21 | https://www.congress.gov/119/plaws/publ21/PLAW-119publ21.htm |
| Senior deduction plain-language confirmation | IRS | https://www.irs.gov/newsroom/one-big-beautiful-bill-act-tax-deductions-for-working-americans-and-seniors |
| CTC phase-out $200k/$400k, $50 per $1,000, $500 other-dependent credit | 26 USC 24; IRS CTC page | https://www.law.cornell.edu/uscode/text/26/24 ; https://www.irs.gov/credits-deductions/individuals/child-tax-credit |
| CDCC expense limits $3,000 / $6,000 | IRS Topic 602 | https://www.irs.gov/taxtopics/tc602 |
| NIIT 3.8%, thresholds $200k / $250k / $125k | IRS Topic 559 | https://www.irs.gov/taxtopics/tc559 |
| Additional Medicare tax 0.9%, thresholds | IRS Topic 560 | https://www.irs.gov/taxtopics/tc560 |
| Self-employment tax: 92.35% factor, $400 floor, half-SE deduction | IRS Topic 554 | https://www.irs.gov/taxtopics/tc554 |
| Mortgage interest $750,000 / $1,000,000 grandfathered | IRS Pub. 936 | https://www.irs.gov/publications/p936 |
| Home sale exclusion $250k / $500k, 2-of-5-year tests | IRS Topic 701 | https://www.irs.gov/taxtopics/tc701 |
| Capital loss limit $3,000 / $1,500; 28% collectibles, 25% unrecaptured 1250 | IRS Topic 409 | https://www.irs.gov/taxtopics/tc409 |

## Retirement accounts

| Number group | Source | URL |
|---|---|---|
| 2026: 401(k) $24,500, catch-up $8,000, 60-63 catch-up $11,250, 415(c) $72,000, 401(a)(17) $360,000, Roth catch-up wage threshold $150,000, SIMPLE, IRA $7,500 + $1,100, Roth IRA and traditional IRA phase-outs, saver's credit, QCD $111,000 | IRS Notice 2025-67 | https://www.irs.gov/pub/irs-drop/n-25-67.pdf |
| 2027: 401(k) $25,500, catch-up $8,500, 60-63 $11,750, 415(c) $75,000, IRA unchanged, phase-outs | IRS Notice 2026-66 (released 2026-11-12) | https://www.irs.gov/pub/irs-drop/n-26-66.pdf |
| 2026 HSA $4,400 / $8,750, HDHP $1,700 / $3,400, OOP $8,500 / $17,000 | IRS Rev. Proc. 2025-19 | https://www.irs.gov/pub/irs-drop/rp-25-19.pdf |
| 2027 HSA $4,500 / $9,000, HDHP $1,750 / $3,500, OOP $8,700 / $17,400 | IRS Rev. Proc. 2026-24 | https://www.irs.gov/pub/irs-drop/rp-26-24.pdf |
| HSA 55+ catch-up, 20% non-medical penalty | 26 USC 223 | https://www.law.cornell.edu/uscode/text/26/223 |
| 10% early distribution tax, age 59.5, separation at 55 (50 public safety) | IRS exceptions table | https://www.irs.gov/retirement-plans/plan-participant-employee/retirement-topics-exceptions-to-tax-on-early-distributions |
| Roth qualified distribution 5-year rule, per-conversion 5-year clock, ordering rules | IRS Pub. 590-B | https://www.irs.gov/pub/irs-pdf/p590b.pdf |
| 72(t) SEPP: three methods, rate cap = greater of 5% or 120% mid-term AFR; later of 5 years or 59.5 | IRS Notice 2022-6; IRS SEPP FAQ | https://www.irs.gov/pub/irs-drop/n-22-06.pdf ; https://www.irs.gov/retirement-plans/substantially-equal-periodic-payments |
| Uniform Lifetime Table (ages 72 to 120+) | 26 CFR 1.401(a)(9)-9(c), eCFR as of 2026-01-01 | https://www.ecfr.gov/current/title-26/section-1.401(a)(9)-9 |
| RMD applicable age by birth year (73 for 1951-1958, 75 for 1960+, 1959 reserved) | Final regs 89 FR 58886 | https://www.federalregister.gov/documents/2024/07/19/2024-14542/required-minimum-distributions |
| RMD basics, 25% / 10% excise tax | IRS RMD FAQs | https://www.irs.gov/retirement-plans/retirement-plan-and-ira-required-minimum-distributions-faqs |

## Social Security

| Number group | Source | URL |
|---|---|---|
| 2026 COLA 2.8%, wage base $184,500, FICA rates, quarter of coverage $1,890, earnings test, max benefit $4,152 | SSA 2026 COLA fact sheet | https://www.ssa.gov/news/en/cola/factsheets/2026.html |
| 2027 COLA 3.6%, wage base $192,300, quarter of coverage $1,970, earnings test, max benefit $4,188 | SSA 2027 COLA fact sheet | https://www.ssa.gov/news/en/cola/factsheets/2027.html |
| COLA history 1975-2026 | SSA | https://www.ssa.gov/oact/cola/colaseries.html |
| PIA bend points 1979-2027 and family-maximum bend points | SSA | https://www.ssa.gov/oact/cola/bendpoints.html |
| PIA formula 90/32/15, derivation from 1979 base and AWI | SSA | https://www.ssa.gov/oact/cola/piaformula.html |
| Average wage index 1951-2025 (2025 = 72,800.33) | SSA | https://www.ssa.gov/oact/cola/AWI.html |
| Contribution and benefit base 1937-2027 | SSA | https://www.ssa.gov/oact/cola/cbb.html |
| Full retirement age by birth year | SSA | https://www.ssa.gov/oact/progdata/nra.html |
| Early reduction 5/9 and 5/12 of 1%; delayed credit 8%/yr (2/3 of 1%/mo) for 1943+; no credit after 69/70 | SSA | https://www.ssa.gov/oact/quickcalc/early_late.html ; https://www.ssa.gov/benefits/retirement/planner/delayret.html |
| Earnings test exempt amounts 2000-2027, $1 per $2 / $1 per $3 | SSA | https://www.ssa.gov/oact/cola/rtea.html |
| Spousal benefit 50% of PIA, 25/36 of 1% reduction, 32.5% minimum | SSA | https://www.ssa.gov/oact/quickcalc/spouse.html |
| Survivor benefit 100% at FRA, 71.5% at 60, 75% child / child-in-care, $255 lump sum | SSA Pub. 05-10084; SSA survivors page | https://www.ssa.gov/pubs/EN-05-10084.pdf ; https://www.ssa.gov/benefits/survivors/survivorchartred.html |
| Benefit taxation base amounts $25k / $32k, adjusted base $34k / $44k, 50% / 85% formula | 26 USC 86; SSA | https://www.law.cornell.edu/uscode/text/26/86 ; https://www.ssa.gov/benefits/retirement/planner/taxes.html |
| Family-maximum formula factors 150 / 272 / 134 / 175% and 2027 bend points | SSA | https://www.ssa.gov/oact/cola/familymax.html |
| Trust fund: OASI depleted Q4 2032, 78% payable (62% by 2100); combined OASDI Q3 2034, 83% payable (65% by 2100); HI Q2 2033, 89% | Summary of the 2026 Trustees Reports | https://www.ssa.gov/oact/TRSUM/index.html |

## Medicare

| Number group | Source | URL |
|---|---|---|
| 2026 Part B premium $202.90, deductible $283, Part A deductible $1,736, Part B and Part D IRMAA tiers (individual, joint, married-separate) | CMS fact sheet, 2025-11-14 | https://www.cms.gov/newsroom/fact-sheets/2026-medicare-parts-b-premiums-deductibles |
| 2027 Part B premium $209.40, deductible $292, Part A deductible $1,776, IRMAA tiers | CMS fact sheet, 2026-11-17 | https://www.cms.gov/newsroom/fact-sheets/2027-medicare-parts-b-premiums-deductibles |
| 2026 Part D base beneficiary premium $38.99 | CMS fact sheet, 2025-07-28 | https://www.cms.gov/newsroom/fact-sheets/2026-medicare-part-d-bid-information-and-part-d-premium-stabilization-demonstration-parameters |
| 2027 Part D base beneficiary premium $41.33 | CMS fact sheet, 2026-07-28 | https://www.cms.gov/newsroom/fact-sheets/medicare-part-d-2027-national-average-monthly-bid-amount-information |
| IRMAA two-year lookback, MAGI = AGI + tax-exempt interest | SSA | https://www.ssa.gov/benefits/medicare/medicare-premiums.html |

## ACA

| Number group | Source | URL |
|---|---|---|
| 2026 applicable percentage table (2.10% to 9.96%), 9.96% employer affordability | IRS Rev. Proc. 2025-25 | https://www.irs.gov/pub/irs-drop/rp-25-25.pdf |
| 2027 applicable percentage table (2.15% to 10.22%) | IRS Rev. Proc. 2026-26 | https://www.irs.gov/pub/irs-drop/rp-26-26.pdf |
| Enhanced subsidies expired 2025-12-31 and were not extended; no credit above 400% FPL in 2026 and 2027 | KFF (2027 open enrollment brief); HealthCare.gov glossary (100%-400% FPL) | https://www.kff.org/affordable-care-act/9-things-to-watch-for-the-2027-aca-open-enrollment-period/ ; https://www.healthcare.gov/glossary/premium-tax-credit/ |
| PTC MAGI definition | IRS PTC Q&A | https://www.irs.gov/affordable-care-act/individuals-and-families/questions-and-answers-on-the-premium-tax-credit |
| 2026 poverty guidelines ($15,960 + $5,680; AK, HI) | HHS ASPE | https://aspe.hhs.gov/topics/poverty-economic-mobility/poverty-guidelines |
| 2025 poverty guidelines ($15,650 + $5,500), used for 2026 coverage | HHS ASPE | https://aspe.hhs.gov/topics/poverty-economic-mobility/poverty-guidelines/prior-hhs-poverty-guidelines-federal-register-references |
| 2025 Alaska ($19,550 + $6,880) and Hawaii ($17,990 + $6,330) guidelines, used for 2026 coverage | HHS ASPE Poverty Guidelines API | https://aspe.hhs.gov/topics/poverty-economic-mobility/poverty-guidelines/poverty-guidelines-api |
| Benchmark (second-lowest silver, age 40) US average: $497 (2025), $625 (2026), $703 (2027) | KFF State Health Facts | https://www.kff.org/health-costs/state-indicator/marketplace-average-benchmark-premiums/ |
| Federal default age curve, ages 0-14 through 64+ | CMS CCIIO | https://www.cms.gov/cciio/programs-and-initiatives/health-insurance-market-reforms/downloads/statespecagecrv053117.pdf |
| Out-of-pocket maximum 2026 $10,600 / $21,200; 2027 $12,000 / $24,000 | HealthCare.gov glossary; 90 FR 27074 (2026 figure) | https://www.healthcare.gov/glossary/out-of-pocket-maximum-limit/ ; https://www.federalregister.gov/documents/2025/06/25/2025-11606/patient-protection-and-affordable-care-act-marketplace-integrity-and-affordability |
| Average 2026 deductibles: silver without CSR $5,304, silver CSR up to 150% FPL $80 | KFF | https://www.kff.org/affordable-care-act/what-we-know-so-far-about-2026-aca-marketplace-enrollment-premiums-and-deductibles/ |
| Average 2026 deductibles: bronze $7,476, gold $1,722 (KFF figures as quoted) | healthinsurance.org (secondary) | https://www.healthinsurance.org/glossary/deductible/ |

## Not confirmed from a primary source

These values are in the JSON but were recalled or derived rather than read from a primary document during this session. Each is also marked in a `note` next to the value.

- **Rounding rules for inflation indexing** ($50 for brackets and standard deduction, $500 for 401(k)/IRA, $1,000 for phase-outs, $50 for HSA, $300 for the wage base): recalled from statute, not re-fetched.
- **SALT cap for 2027-2029** ($40,804; $41,212.04; $41,624.16 and thresholds $510,050; $515,150.50; $520,302.01): computed as 101% of the prior year per IRC 164(b)(7). The statute gives no rounding rule and IRS has not published rounded figures.
- **Bronze $7,476 and gold $1,722 average deductibles, and the $3,786 enrollment-weighted average**: KFF figures taken from secondary coverage (healthinsurance.org, web search summaries); KFF publishes them in charts that were not machine-readable.
- **Widow(er)'s limit** (82.5% of the deceased's PIA floor): recalled, not found on the pages fetched.
- **IRMAA top-tier threshold frozen through 2027 and indexed from 2028; surcharge multiples of the standard premium**: recalled. The 2026 and 2027 thresholds themselves are from CMS.
- **CTC refundable formula** (15% of earned income over $2,500) and qualifying child under 17: the $2,500 floor is on the IRS page; the 15% rate and age limit are recalled.
- **RMD age for people born in 1959**: encoded as 73. The final regulations reserve this birth year.
- **Medicaid expansion threshold (138% FPL), CSR eligibility ceiling (250% FPL), metal-tier actuarial values, three-child rating cap, medical expense 7.5% floor, step-up in basis, Roth first-home $10,000 limit, AIME mechanics (35 years, index to age 60, 40 credits)**: standard rules recalled, not re-fetched.
- **`claiming_factor_examples_fra_67`**: computed from the fetched reduction and credit formulas.
- **Indexing flags** (which CPI measure each parameter follows): assigned from knowledge of the statutes; the "not indexed" flags for NIIT, additional Medicare tax, Social Security benefit taxation thresholds, CTC phase-out, home sale exclusion, capital loss limit, senior deduction and mortgage limit reflect the absence of those items from the annual Rev. Procs.

## Independent verification pass (2026-11-17)

A second agent re-fetched the primary documents and compared them with the JSON, mostly by script. No numeric value in the file was found to be wrong. What was checked, and what changed:

**Checked and matching**

- Rev. Proc. 2025-32 and 2026-35 (PDF text): all four bracket tables for 2026 and 2027, standard / additional / dependent deductions, capital-gains ceilings, AMT exemptions, phase-out start and complete amounts and the 28% breakpoint, CTC amounts, QBI thresholds and minimums, long-term-care premium limits, gift and estate amounts.
- Notice 2025-67 and 2026-66: every retirement-plan limit, IRA phase-out, saver's credit limit and the QCD limit in the file.
- Rev. Proc. 2025-19 and 2026-24 (HSA), Rev. Proc. 2025-25 and 2026-26 (ACA applicable percentages, 9.96% and 10.22%).
- SSA pages, compared programmatically with zero differences: average wage index 1951-2025 (75 values), PIA and family-maximum bend points 1979-2027, contribution and benefit base 1937-2027, earnings-test amounts 2000-2027, COLA series 1975-2026. Both COLA fact sheets (wage base, quarter of coverage, earnings test, maximum and average benefits). The 2026 Trustees summary (OASI Q4 2032 / 78%, combined Q3 2034 / 83% / 65% by 2100, HI Q2 2033 / 89%).
- CMS 2026 and 2027 Parts A/B fact sheets: every IRMAA threshold and Part B and Part D surcharge for individual, joint and married-separate filers; Part A and Part B premiums and deductibles. CMS Part D fact sheets ($38.99, $41.33, 6% cap 2024-2029).
- Uniform Lifetime Table, all 49 rows, against eCFR 26 CFR 1.401(a)(9)-9(c) as of 2026-01-01.
- OBBBA statute text (govinfo copy of P.L. 119-21): SALT amounts, 101% rule, 30% phase-down and $10,000 floor (sec. 70120); AMT 50% phase-out rate (sec. 70107(c)); 2/37 limitation (sec. 70111); senior deduction (sec. 70103); child-care credit schedule (sec. 70405); CTC indexing and $100 rounding (sec. 70104).
- HHS 2026 guidelines (48 states, Alaska, Hawaii) and 2025 base figures; HealthCare.gov out-of-pocket limits for 2026 and 2027; KFF benchmark premiums 497 / 625 / 703; KFF 2027 open-enrollment brief (enhanced credits expired, no credit above 400% FPL, repayment caps gone from tax year 2026).

**Changed in the JSON (wording fixes and additions; no existing number changed)**

- `medicare.irmaa.indexing.surcharges`: the text said the surcharges are 1.4x-3.4x the standard premium. Those multiples are the TOTAL premium; the stored surcharges are about 0.4x, 1.0x, 1.6x, 2.2x, 2.4x. Reworded so an engine that projects surcharges from the standard premium does not overstate them by one full premium.
- `medicare.irmaa.tier_boundary_note`: added that the married-separate top tier also starts at (>=) its threshold ($391,000 / $388,000), per the CMS tables.
- `income_tax.alternative_minimum_tax.note`: the 50% phase-out rate is now cited to the statute rather than inferred.
- `income_tax.kiddie_tax_note` (new): the 1,350 figure is the section 1(g)(4)(A)(ii)(I) amount; the tax applies above twice that.
- `credits.savers_credit_agi_limits.note` (new): the credit is replaced by the Saver's Match from tax year 2027 (Notice 2026-66, footnote 2).
- `social_security.pia_formula.family_maximum_factors_note`: now confirmed from SSA; `ssa_family_max` added to `sources`.
- `social_security.trust_fund.oasi_payable_fraction_2100` (new): 0.62.
- `aca.federal_poverty_guidelines.alaska_2025` / `hawaii_2025` (new): the 2025 guidelines govern 2026 coverage, and the file previously carried Alaska and Hawaii only for 2026.
- `published_2027` additions read from the same documents: kiddie-tax amount 1,400; 401(a)(17) limit 375,000; highly-compensated threshold 170,000; SIMPLE 17,500 / 4,000 / 5,250; QCD 114,000; Part A premiums 328 / 597; gift exclusion 20,000; estate exclusion 15,480,000; long-term-care premium limits 500 / 945 / 1,890 / 5,030 / 6,290.

**Still not confirmed from a primary source in either pass**

- Average bronze ($7,476) and gold ($1,722) deductibles: the KFF article text gives only silver $5,304, silver CSR $80 and the $3,786 weighted average.
- The remaining items in the list above (rounding increments, widow(er)'s limit, IRMAA top-tier freeze, CTC 15% rate, standard ACA and Roth rules) were not re-fetched; they agree with the verifier's own knowledge of the statutes but that is not a source.
- `required_minimum_distributions.start_age_by_birth_year` gives 70.5 for everyone born in 1949 or earlier in its structured fields; people born July-December 1949 are actually age 72 (stated only in the row's note). Irrelevant for anyone not already taking distributions.
- Rev. Proc. 2026-35 as posted still carries the placeholder "[Release Date, 2026]" in its purpose section, so it may be a pre-publication copy; this pass did not re-check it against the IRS news release.

