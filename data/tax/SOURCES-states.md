# Sources and judgement calls: `us-states-2026.json`

Compiled 2026-11-17 for tax year 2026 (50 states plus DC). Rates are decimal fractions; dollar amounts are nominal 2026 USD.

## How the file was built

1. Base layer: the Tax Foundation spreadsheet "State Individual Income Tax Rates and Brackets, 2026" (data as of 2026-02-11), parsed mechanically: single and joint brackets, standard deduction, personal and dependent exemptions, and the footnote flags for inflation indexing.
2. Overrides for laws enacted after February 2026 with effect for tax year 2026: AR, AZ, GA, SC, UT, WV.
3. Overrides where the state has since published 2026 inflation-adjusted figures that the Tax Foundation table carried at 2025 levels: CA, MA, ME, VT, ND, plus HI (scheduled 2026 standard deduction step-up).
4. Qualitative fields (Social Security, retirement distributions, capital gains, local taxes) from the secondary sources below.
5. Property and sales tax rates parsed from Tax Foundation tables.

## URLs used

Income tax brackets, deductions, exemptions
- https://taxfoundation.org/data/all/state/state-income-tax-rates-2026/
- https://taxfoundation.org/wp-content/uploads/2026/02/2026-State-Individual-Income-Tax-Rates-Brackets.xlsx
- https://taxfoundation.org/research/all/state/2026-state-tax-changes/
- https://www.ntu.org/foundation/detail/arkansas-joins-twelve-other-states-cutting-income-taxes-this-year (list of 2026 cuts, used as a checklist)

State sources for overrides
- California: https://www.ftb.ca.gov/about-ftb/newsroom/tax-news/2026/10.html (2026 indexed schedules X and Y, standard deduction, exemption credits)
- Maine: https://www.maine.gov/revenue/sites/maine.gov.revenue/files/2026-05/ind_tax_rate_sched_2026_rev.pdf
- West Virginia: https://tax.wv.gov/Individuals/Pages/PersonalIncomeTaxReductionBill.aspx (SB 392)
- South Carolina: https://www.dor.sc.gov/news/information-about-h-4216 and https://www.scstatehouse.gov/sess126_2025-2026/fiscalimpactstatements/H4216%202026-03-30%20signed.pdf
- Arkansas: https://www.dfa.arkansas.gov/wp-content/uploads/whformula_2026.pdf and https://taxfoundation.org/blog/arkansas-income-tax-cut-rates/
- Arizona: https://www.azleg.gov/legtext/57leg/2R/summary/H.HB4168_061726_SIGNED.DOCX.htm
- Georgia: https://www.bdo.com/insights/tax/georgia-enacts-income-tax-reduction-bill , https://www.paylocity.com/resources/tax-compliance/alerts/georgia-reduces-individual-income-tax-rates-for-2026/ , https://www.legis.ga.gov/api/legislation/document/20252026/249080 (HB 463)
- Utah: https://taxnews.ey.com/news/2026-0913-utah-law-lowers-state-income-tax-rate-retroactive-to-january-1-2026 , https://le.utah.gov/Session/2026/bills/enrolled/SB0060.pdf
- Vermont: https://tax.vermont.gov/sites/tax/files/documents/GB-1210-2026.pdf (2026 withholding tables)
- Massachusetts: https://www.mass.gov/info-details/massachusetts-tax-rates
- Hawaii: https://files.hawaii.gov/tax/news/announce/ann24-03.pdf
- Idaho: https://tax.idaho.gov/taxes/income-tax/individual-income/individual-income-tax-rate-schedule/ (2025 only)
- Washington: https://dor.wa.gov/taxes-rates/other-taxes/capital-gains-tax
- Kentucky: https://taxnews.ey.com/news/2026-0815 (HB 757, no rate change beyond the 3.5% already in the table)
- North Dakota: https://ustax.tools/north-dakota-tax-brackets-2026/ (secondary, citing 2026 Form ND-1ES)

Other fields
- Capital gains: https://taxfoundation.org/data/all/state/state-capital-gains-tax-rates/
- Social Security: https://www.kiplinger.com/taxes/states-that-tax-social-security-benefits
- Retirement distributions: https://www.kiplinger.com/retirement/602202/taxes-in-retirement-how-all-50-states-tax-retirees
- Local income taxes: https://taxfoundation.org/local-income-taxes-2023/ and footnote (a) of the 2026 rates table
- Property tax (effective rate on owner-occupied housing, calendar 2024): https://taxfoundation.org/data/all/state/property-taxes-by-state-county/
- Sales tax (combined state plus average local, 2026-07-01): https://taxfoundation.org/data/all/state/2026-sales-tax-rates-midyear/

## Judgement calls and unconfirmed figures

Brackets and rates
- AR: two tax tables exist. The file uses the standard table (0% / 2% / 3% / 3.4% / top), with 2026 thresholds read from the DFA 2026 withholding formula and the top rate set to 3.7% (cut enacted May 2026, retroactive). The DFA document itself still prints 3.9% because it predates the cut. The high-income table (net income above about $94,700) is stored as `alt_high_income_brackets` and not otherwise modelled; the difference is roughly $330 of tax. Whether DFA re-indexed thresholds after the cut is not confirmed.
- CA: the 1% Mental Health Services Tax over $1,000,000 is folded into the brackets, so the joint schedule has an extra break at $1,000,000. SDI payroll tax is excluded.
- GA: 4.99%, the $15,000/$30,000 standard deduction and the $5,000 dependent exemption are all confirmed in the HB 463 bill text (legis.ga.gov document 249080).
- ID: zero-bracket threshold ($4,811 / $9,622) is the 2025 amount. Idaho had not published 2026.
- MA: $1,107,750 surtax threshold is from mass.gov; the 2026 Schedule 4 form was still a draft.
- ME: the 2% surcharge over $1,000,000 / $1,500,000 is folded into the brackets as a 9.15% top rate.
- ND: 2026 thresholds are confirmed against the Tax Rate Schedules on the 2026 Form ND-1ES (single $49,575/$250,400; joint $82,800/$304,850).
- SC: new two-rate schedule and the phase-out of the SC Income Adjusted Deduction are from SCDOR and the signed fiscal impact statement. Whether the new $30,000 threshold and the deduction are indexed is not stated, so `inflation_indexed` is false. SCDOR's 2026 forms were not yet published.
- VT: brackets are derived from the 2026 withholding tables (annual table minus the built-in zero bracket). The standard deduction ($7,850 / $15,700) is an indexed estimate that matches one secondary source. The official 2026 rate schedule was not yet published.
- WA: capital gains standard deduction is the 2025 amount ($278,000). The 2026 amount was not published. `has_income_tax` is true and `taxes_wages` is false, because only long-term capital gains are taxed.
- AZ: standard deduction is $16,100 / $32,200. HB 4168 set a base of $15,750 / $31,500 in A.R.S. 43-1041(A), retroactive to tax year 2025 (session law sec. 35), and subsection H indexes that base in the same manner as the federal standard deduction. This is a statutory derivation; the 2026 Form 140 was not yet published.
- MD: standard deduction is $3,400 / $6,850. The $3,350 / $6,700 in the Tax Foundation table is the 2025 amount; Tax-General 10-217(c) indexes it after 2025 with the increase rounded down to $50. $3,400 is printed in the Comptroller's 2026 withholding guide; $6,850 is computed from the same rule and not yet confirmed on a 2026 form.
- CT, NY: benefit recapture at high incomes is not represented in the marginal brackets.
- AL, WI, MN, RI, ME, CT, SC: standard deduction or exemption phase-outs are described in `notes` (SC also as structured data) but the stored amounts are the maximums.
- UT: the standard deduction is a credit (6% of the federal deduction), stored as `standard_deduction.type = "credit"` with `credit_amount`; its income phase-out thresholds are 2025 figures.
- OR, KY and other Tax Foundation rows not listed above are as of 2026-02-11 and were not rechecked against state forms. Tax Foundation footnotes say CA, VT and WA deductions and the UT and ME dependent amounts were 2025 values; CA and VT have been updated here, the UT dependent amount ($2,111) has not.
- States that conform to the federal standard deduction (CO, IA, ID, MO, MT, ND, NM, DC) carry $16,100 / $32,200 and `follows_federal: true`.

Qualitative fields
- Social Security: eight states tax benefits in 2026 (CO, CT, MN, MT, NM, RI, UT, VT). Thresholds are from Kiplinger and were not checked against state forms. MT is coded `follows_federal`; UT's credit phase-out thresholds were not captured. WV is `none` from 2026.
- Retirement distributions: classification and exclusion amounts are from Kiplinger's 50-state guide, which mixes 2025 and 2026 figures and does not always say whether an amount is per person or per return. Treat `exclusion.amount` as approximate. Income limits on the exclusions (CT, NJ, NM, RI, VA, WI) are only described in notes.
- Capital gains: from the Tax Foundation 2026 table. Massachusetts' separate rate on short-term gains and other narrow state preferences are not captured.
- Local income taxes: city rates are from the Tax Foundation's 2023 primer and may have changed (for example Philadelphia). `statewide_avg_effective_rate` is local income tax as a share of AGI (2023 data) from the 2026 table footnote. Indiana's figure (0.35%) looks low against county rates but is recorded as published.
- Property tax: Tax Foundation's 2026 release shows New Jersey and Illinois both at 1.88% for 2024, noticeably lower than the 2.23% and 2.07% in its previous release for 2023. Values are recorded as published.
- Sales tax: New Jersey's combined rate (6.60%) is below its state rate (6.625%) because of reduced-rate zones.

## Validation

The JSON parses, has 51 jurisdictions plus `meta`, and every bracket list is strictly ascending by `over`, starts at 0, and has rates between 0 and 0.15. States without a wage tax have empty bracket lists.

## Verification pass (2026-11-17)

An independent check re-derived the figures below from primary sources. Only confirmed errors were changed.

Corrections made
- AZ `standard_deduction`: $15,750 / $31,500 -> $16,100 / $32,200. The old value was the tax year 2025 amount. Sources: HB 4168 as chaptered (https://www.azleg.gov/legtext/57leg/2R/laws/0140.htm, sec. 16 and sec. 35) and A.R.S. 43-1041(H) (https://www.azleg.gov/ars/43/01041.htm); the ADOR 2025 filing thresholds of $15,750 / $31,500 confirm the base year.
- MD `standard_deduction`: $3,350 / $6,700 -> $3,400 / $6,850, and `inflation_indexed` false -> true. The old value was the tax year 2025 amount. Sources: Comptroller 2026 withholding guide (https://www.marylandcomptroller.gov/content/dam/mdcomp/tax/instructions/withholding/2026/withholding-guide.pdf, "Standard Deduction is $3,400") and Tax-General 10-217(c). The joint figure is computed, not printed.
- GA note: the $5,000 dependent exemption is now recorded as confirmed in the HB 463 text (no number changed).
- ND note: thresholds now recorded as confirmed against the 2026 Form ND-1ES (no number changed).

Checked and found correct
- CA: schedules X and Y, standard deduction $5,900 / $11,800, exemption credits $158 / $316 / $491 (FTB Tax News, October 2026).
- NY: all nine brackets for single and joint, and the $8,000 / $16,050 standard deduction (2026 Form IT-2105-I).
- GA: 4.99%, $15,000 / $30,000, $5,000 dependent (HB 463 text).
- SC: 1.99% to $30,000 and 5.21% above; SCIAD $15,000 / $30,000 with phase-out ranges $40,000-$95,000 and $80,000-$190,000 (SCDOR page and the signed fiscal impact statement).
- ME: brackets, 2% surcharge thresholds, $15,700 / $31,400 standard deduction, $5,300 exemption (Maine Revenue Services 2026 rate schedule).
- WV: 2.11% to 4.58% schedule (tax.wv.gov). UT: 4.45% retroactive to 2026-01-01 (SB 60 enrolled text).
- AR: 2026 thresholds $5,600 / $11,200 / $16,000 / $26,400 and $2,470 standard deduction (DFA 2026 withholding formula); 3.9% to 3.7% cut signed 2026-05-06 (Arkansas House, Talk Business).
- MA: $1,107,750 surtax threshold (DOR certification). MI: 4.25% and $5,900 exemption (michigan.gov withholding page). IN: 2.95% (in.gov). LA: 3% and $12,875 / $25,750 (payroll guides only, no LDR page found).
- TX, FL, PA, IL, OH, NC and the other no-tax states (AK, NV, NH, SD, TN, WY) match the Tax Foundation 2026 table and known statute; WA taxes only capital gains.

Still open
- AZ and MD (joint) standard deductions are derived from statute, not read from a 2026 form.
- ID zero-bracket threshold, VT schedule and standard deduction, and the WA capital gains deduction for 2026 were still not published or not found.
- IN: Indiana also allows an additional exemption for dependent children on top of the $1,000 stored; the amount was not confirmed against the IT-40 instructions and is not in the file.
- SC dependent exemption ($4,930) and UT dependent amount ($2,111) are 2025 values.

## State detail rules (`detail`, used with `assumptions.taxes.state_detail: full`)

Three state rules the base tables leave out, stored under a `detail` key of the state and read only when the profile asks for `state_detail: full`. The numbers were read from the sources below on 2026-11-18; where a figure could not be checked it is said so.

- CA, State Disability Insurance (`detail.sdi`): employee-paid SDI is 1.3% of all wages for 2026 with no wage ceiling (SB 951 removed the ceiling from 2024). EDD, "Rates and withholding" (https://edd.ca.gov/en/Payroll_taxes/Rates_and_withholding) lists 1.3% for 2026 (1.2% for 2025, 1.4% for 2027); the EDD 2026 employer guide DE 44 says the same. Treated as part of state tax: it is not on self-employment profit, and mandatory SDI contributions are deductible as a state income tax by itemisers.
- NY, benefit recapture (`detail.benefit_recapture`): the structure and thresholds are those of the "Tax computation - New York AGI of more than $107,650" worksheets in the 2024 IT-201-I instructions (https://tax.ny.gov/pdf/2024/inc/it201i_2024.pdf), Tax Law 601(d)-(e). The engine's recapture bases were checked against the printed ones: for joint filers 333 and 807 (then 1,140 cumulative) and 2,747 (3,887 cumulative); for head of household 787, 2,289, 3,076 and 48,337. UNVERIFIED: the data holds NY's 2026 rates (3.9% to 5.9% in the first five brackets, cut 0.1 point for 2026), and the structure is applied to them; no 2026 worksheet was read, and the 2026 head-of-household schedule is not in the data (it uses the single one), so head-of-household recapture is approximate. The 2025 IT-201-I was found but not read.
- UT, Social Security benefits credit (`detail.social_security_credit`): Utah Code 59-10-1042 as amended in 2025 (https://le.utah.gov/Session/2025/bills/introduced/HB0130.pdf, the introduced text of H.B. 130): a nonrefundable credit equal to the flat rate (4.45% for 2026) times Social Security benefits included in federal AGI, reduced by $0.025 per dollar of modified AGI above $54,000 single or $90,000 joint and head of household. The final text (S.B. 71, 2025) was not read; the Utah State Tax Commission's January 2026 training slides were reported to say it raised the thresholds to the same figures and to measure income on state rather than federal taxable income, which was not confirmed, so the engine uses federal AGI. The thresholds are in statute and are not indexed.
