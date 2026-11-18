# Historical monthly dataset for the economic scenario generator

`monthly.csv` is one aligned row per month, **1926-07 to 2026-09 (1,203 rows)**, of the
series the joint economic scenario generator (ESG) is calibrated to or bootstraps from.
`calibration.json` holds the statistics computed from it. Both are produced by

```
python3 -I tools/build_history.py                      # build from data/history/raw
python3 -I tools/build_history.py --fetch              # re-download the raw files with curl first
python3 -I tools/build_history.py --home-source=fhfa   # licence-clean home-price variant (see Licences)
```

The script uses only the Python standard library (it contains a ~70-line reader for the
old binary `.xls` format because Shiller publishes nothing else). Raw downloads live in
`raw/` (2.2 MB, retrieved 2026-11-17). Do not edit `monthly.csv` or `calibration.json`
by hand.

FIRESight v1 bootstrapped stocks and home prices independently against a constant 3%
inflation rate, so nothing was correlated. Here every row is one real month, so sampling
rows (or blocks of rows) preserves the joint behaviour of all series.

## Columns

All rates and returns are **decimal fractions per month** (0.01 = 1%), not percent.
Levels (`long_yield`, `mortgage30`, `unemployment`) are annualised fractions (0.045 = 4.5%).
Returns are for the calendar month named in `date`; levels are as of that month.

| column | meaning | coverage |
|---|---|---|
| `date` | `YYYY-MM` | 1926-07 .. 2026-09 |
| `equity_us` | US total stock market nominal total return (CRSP value-weighted, all NYSE/AMEX/NASDAQ firms) = French `Mkt-RF + RF` | full |
| `tbill` | 1-month Treasury bill nominal return = French `RF` | full |
| `bond10` | Nominal total return of a constant-maturity 10-year Treasury par bond, computed from yields (formula below) | full |
| `cpi` | Monthly inflation rate, CPI-U all items, not seasonally adjusted: `CPI_t / CPI_{t-1} - 1` | full |
| `home` | Nominal national home price change (price only: no rent, upkeep, taxes or transaction costs) | 1926-07 .. 2026-08 |
| `long_yield` | Long (10-year) Treasury yield level used for `bond10` | full |
| `mortgage30` | 30-year fixed mortgage rate, monthly average of the weekly survey | full (synthetic before 1971-04) |
| `unemployment` | Civilian unemployment rate | 1929-04 .. 2026-09 |
| `recession` | NBER recession indicator, 1 = recession month | full |
| `equity_intl` | Developed markets ex-US equity total return in USD | 1975-01 .. 2026-09, blank before |
| `gold` | Gold price change in USD (monthly-average price) | 1968-04 .. 2026-09, blank before |
| `bond10_flag` | 0 = month-end daily yields; 1 = monthly-average GS10 yields; 2 = monthly-average LTGOVTBD yields | |
| `cpi_flag` | 1 = one of the two index values behind this rate was interpolated (2025-10 and 2025-11 only) | |
| `home_flag` | 0 = Case-Shiller monthly; 1 = Shiller monthly series; 2 = Shiller annual, geometrically interpolated; (3 = FHFA purchase-only monthly, 4 = FHFA quarterly interpolated: `--home-source=fhfa` only); blank = no value | |
| `mortgage30_flag` | 0 = observed; 1 = synthetic (long yield + average spread) | |
| `unemployment_flag` | 0 = BLS UNRATE; 1 = NBER Macrohistory estimate; 2 = single missing month linearly interpolated; blank = no value | |
| `equity_intl_flag` | 0 = French Developed ex US; 1 = fixed-weight country proxy; blank = no value | |

Core columns `equity_us, tbill, bond10, cpi, long_yield, recession` have no gaps (the
script fails rather than write one). The only blank inside a covered range is `home` for
2026-09 (index not yet published).

## Sources

| file in `raw/` | URL | used for |
|---|---|---|
| `F-F_Research_Data_Factors_CSV.zip` | https://mba.tuck.dartmouth.edu/pages/faculty/ken.french/ftp/F-F_Research_Data_Factors_CSV.zip | `equity_us`, `tbill` (built from the 2026-09 CRSP database) |
| `Developed_ex_US_3_Factors_CSV.zip` | .../ftp/Developed_ex_US_3_Factors_CSV.zip | `equity_intl` from 1990-07 |
| `F-F_International_Countries.zip` | .../ftp/F-F_International_Countries.zip | `equity_intl` proxy 1975-01 .. 1990-06 |
| `fred_CPIAUCNS.csv` | https://fred.stlouisfed.org/graph/fredgraph.csv?id=CPIAUCNS | `cpi` |
| `fred_DGS10.csv` | ...?id=DGS10 (daily 10-year constant maturity, 1962-01-02 on) | `long_yield`, `bond10` from 1962 |
| `fred_GS10.csv` | ...?id=GS10 (monthly average, 1953-04 on) | `long_yield`, `bond10` 1953-04 .. 1962-01; mortgage spread |
| `fred_LTGOVTBD.csv` | ...?id=LTGOVTBD (long-term US government securities, monthly average, 1925-01 .. 2000-06, discontinued) | `long_yield`, `bond10` before 1953-04 |
| `fred_TB3MS.csv` | ...?id=TB3MS | cross-check of `tbill` only |
| `fred_CSUSHPINSA.csv` | ...?id=CSUSHPINSA | `home` from 1987-02 |
| `shiller_Fig3-1.xls` | http://www.econ.yale.edu/~shiller/data/Fig3-1.xls (sheet `Data`, nominal home price index) | `home` before 1987-02 |
| `fred_USSTHPI.csv`, `fred_HPIPONM226N.csv` | ...?id=USSTHPI, ...?id=HPIPONM226N (FHFA) | cross-check; `home` from 1975 with `--home-source=fhfa` |
| `fred_MORTGAGE30US.csv` | ...?id=MORTGAGE30US (weekly, 1971-04-02 on) | `mortgage30` |
| `fred_UNRATE.csv` | ...?id=UNRATE | `unemployment` from 1948-01 |
| `fred_M0892AUSM156SNBR.csv`, `fred_M0892BUSM156SNBR.csv`, `fred_M0892CUSM156NNBR.csv` | ...?id=M0892AUSM156SNBR etc. (NBER Macrohistory Database) | `unemployment` 1929-04 .. 1947-12 |
| `fred_USREC.csv` | ...?id=USREC | `recession` |
| `datahub_gold_monthly.csv` | https://raw.githubusercontent.com/datasets/gold-prices/main/data/monthly.csv | `gold` |
| `datahub_sp500.csv` | https://raw.githubusercontent.com/datasets/s-and-p-500/main/data/data.csv (CSV mirror of Shiller's `ie_data.xls`) | cross-check of the pre-1953 long yield only |

Download notes: FRED answers curl's default User-Agent but drops connections that send a
browser-like User-Agent string, so `--fetch` sends none. No source refused download.

## Licences and terms (read before publishing the repo)

| data | status |
|---|---|
| CPI, UNRATE (BLS); GS10, DGS10, TB3MS, LTGOVTBD (Federal Reserve Board); FHFA indexes | US government works, public domain. FRED asks for citation. |
| USREC | Federal Reserve Bank of St. Louis, derived from NBER business-cycle dates; public domain with citation requested. |
| NBER Macrohistory unemployment | NBER, freely distributed via FRED; citation requested. |
| Kenneth French Data Library | Copyright Kenneth R. French; free download, no formal licence. Derived from CRSP (US) and Bloomberg/MSCI (international). Widely redistributed with attribution in academic and open-source work, but not a public-domain grant. |
| Shiller home price data | Posted for free public use by Robert Shiller; no formal licence. Cite *Irrational Exuberance*. |
| **S&P CoreLogic Case-Shiller index (CSUSHPINSA)** | **Copyrighted. FRED's series notes read "Copyright, 2016, Standard & Poor's Financial Services LLC. Reprinted with permission." and FRED tags the series "Copyrighted: Pre-Approval Required" (checked 2026-11-17). FRED's permission does not pass to us.** This is the one series that is not clearly redistributable in an MIT-licensed public repository. |
| **Freddie Mac PMMS (MORTGAGE30US)** | Copyrighted. FRED's series notes read "Copyright, 2016, Freddie Mac. Reprinted with permission." Freddie Mac's PMMS pages say the information "may be used with proper attribution" and must not be altered; its general site terms otherwise bar republication without written consent. Not public domain: attribute Freddie Mac, and treat redistribution of the raw weekly file as unconfirmed (checked 2026-11-17). |
| Gold (datahub `gold-prices`) | Packaged under ODC-PDDL; underlying sources are the World Bank Commodity "Pink Sheet" (CC BY 4.0) from 1960 and Timothy Green's historical table before. |
| datahub `s-and-p-500` | ODC-PDDL packaging of Shiller's data; used here for a cross-check only. |

If the Case-Shiller terms are a problem, build with `--home-source=fhfa`: it keeps
Shiller's series through 1975-02, then uses the FHFA all-transactions index (quarterly,
placed at the middle month of each quarter and interpolated geometrically, flag 4)
through 1991-02 and the FHFA purchase-only monthly index (flag 3) after. December-to-
December changes of the two indexes for 1992-2025 correlate 0.97; FHFA is somewhat
smoother (annual sd 4.9% vs 6.0%, worst year -10.5% vs -12.0%, full-sample real
drawdown -30% vs -36%), because it covers only conforming mortgages. In that variant
you can also delete `raw/fred_CSUSHPINSA.csv` only after removing the cross-check that
reads it. Note that Shiller's own spreadsheet contains Case-Shiller values after 1987;
they are not used in either variant.

## Splice points and constructions

**Equities and bills.** No splice. French notes that RF is from Ibbotson Associates
through 2024-05 and the ICE BofA US 1-Month Treasury Bill Index after. Cross-check
against FRED TB3MS (1934-01 on, 1,113 months): mean annualised RF 3.32% vs mean 3-month
discount yield 3.42%, level correlation 0.990.

**Long yield and `bond10`.**

| months | yield observation | `bond10_flag` |
|---|---|---|
| 1926-07 .. 1953-04 | LTGOVTBD, monthly average of daily yields | 2 |
| 1953-05 .. 1962-01 | GS10, monthly average | 1 |
| 1962-02 .. 2026-09 | DGS10, last daily observation of the month | 0 |

Each month's return uses two yields from the *same* source, so a splice never creates an
artificial price jump. `long_yield` reports the month-end DGS10 value from 1962-01 and the
monthly average before.

The task brief suggested Shiller's long-rate series for the years before 1953. It is not
used: Shiller's pre-1953 values are annual (January) observations linearly interpolated
to months, which would give smooth, nearly constant monthly returns. LTGOVTBD is a true
monthly series from the Federal Reserve. The two agree in level (pre-1953 means 2.75% vs
2.73%, January-level correlation 0.95).

Bond total return, month t, with y0 = yield at the end of month t-1 and y1 = yield at
the end of month t (both as fractions, semi-annual bond-equivalent):

```
buy a 10-year par bond: coupon rate c = y0, price 1, 20 semi-annual coupons of c/2
one month later (1/6 of a coupon period) revalue the remaining cash flows at y1:
    v = 1 / (1 + y1/2)
    P = sum_{k=1..20} (c/2) * v^(k - 1/6)  +  v^(20 - 1/6)
    bond10_t = P - 1        (P is the full price, so accrued coupon is included)
then sell and buy the new 10-year par bond (constant maturity)
```

This is exact repricing rather than a duration approximation. The script also computes
the textbook form `y0/12 - D*(y1-y0) + 0.5*C*(y1-y0)^2` with modified duration
`D = (1 - (1+y0/2)^-20) / y0` and convexity
`C = sum_k CF_k * k(k+1) * (1+y0/2)^-(k+2) / 4`; the two agree to within 0.06% in every
month (largest gap 0.052%) (correlation 0.999995, mean difference 0.06% a year from compounding of carry).
The curve is assumed flat around the 10-year point, so roll-down return is ignored.

Sanity check against published figures. The Damodaran column was recomputed on
2026-11-17 from his table at
https://pages.stern.nyu.edu/~adamodar/New_Home_Page/datafile/histretSP.html (1928-2025);
the Ibbotson figures are quoted from memory and are not verified:

| | this dataset | published |
|---|---|---|
| calendar years 1928-2025, arithmetic / geometric / sd | 4.96% / 4.66% / 8.2% | Damodaran 10-year T-bond 1928-2025: 4.82% / 4.53% / 7.9% (1928-2023: 4.86% / 4.57% / 7.95%) |
| full sample monthly, arithmetic / geometric / vol | 4.71% / 4.59% / 6.5% | Ibbotson SBBI 1926-2023: intermediate (5y) about 5.0% arithmetic, sd 5.6%; long (20y) about 5.7%, sd 9.7% |
| 1982 / 2008 / 2009 / 2022 / 2025 | +39.0% / +20.4% / -10.1% / -16.4% / +7.7% | Damodaran +32.8% / +20.1% / -11.1% / -17.8% / +7.8% |

The level and volatility sit between the published 5-year and 20-year series as they
should. The 98 calendar-year returns correlate 0.989 with Damodaran's and average 0.14
points a year higher; 1982 is the only year more than 6 points apart. Individual years differ from Damodaran because he reprices once a year from
annual yields while this series compounds monthly from month-end yields.

**CPI.** No splice. The BLS did not publish October 2025 CPI (government shutdown); the
index for that month is geometrically interpolated between September and November 2025,
and both affected monthly rates are flagged.

**Home prices.**

| months | source | `home_flag` |
|---|---|---|
| 1926-07 .. 1953-01 | Shiller annual index, geometric interpolation between January values | 2 |
| 1953-02 .. 1987-01 | Shiller monthly index (CPI home-purchase component to 1975, then OFHEO-based) | 1 |
| 1987-02 .. 2026-08 | S&P CoreLogic Case-Shiller US National, not seasonally adjusted | 0 |

Shiller's annual observations are dated January: his own real series deflates each
annual value by that January's CPI (checked: his CPI column reads 17.1 for 1930 and 15.9
for 1931, the January CPI-U values), and the annual rows end at 1952 with the monthly rows
starting at 1953.0417 (January 1953), one year later. Damodaran's real-estate column
instead treats the value labelled Y as the end of year Y, which puts every pre-1953 change
one calendar year later than here; the underlying sources are annual and do not settle it
(see Known limitations). Flag-2 rows therefore carry one twelfth (geometrically) of the
January-to-January change and have no within-year information. Shiller's monthly series
and Case-Shiller overlap for 1987-2023 with a monthly-change correlation of 0.99999, so
the splice is seamless.

**Mortgage rate.** Monthly mean of the weekly Freddie Mac survey rate from 1971-04.
Before that, `mortgage30 = monthly-average long yield + 0.01762`, where 1.762 points is
the mean of (monthly mortgage rate - GS10) over 1971-04 .. 2026-09 (sd of the spread
0.52 points). Flag 1. The synthetic values ignore the regulated, regionally segmented
mortgage market of the period and the low 1930s-40s yields make them look low
(4.0-5.5%); use them only as a placeholder for a rate-linked mortgage model.

**Unemployment.**

| months | source | flag |
|---|---|---|
| 1926-07 .. 1929-03 | none | blank |
| 1929-04 .. 1939-12 | NBER M0892A (NICB estimates, seasonally adjusted) | 1 |
| 1940-01 .. 1946-12 | NBER M0892B (Census, seasonally adjusted); 1940-02 missing, interpolated (flag 2) | 1 |
| 1947-01 .. 1947-12 | NBER M0892C (not seasonally adjusted) | 1 |
| 1948-01 .. 2026-09 | BLS UNRATE (seasonally adjusted); 2025-10 not published, interpolated (flag 2) | 0 |

**Recession.** FRED USREC: 1 from the month after an NBER peak through the trough.

**International equities.** From 1990-07: French "Developed ex US" market return
(`Mkt-RF + RF`, USD, value-weighted, includes Canada). For 1975-01 .. 1990-06 no free
monthly composite exists, so a proxy is built from French's per-country value-weighted
USD market returns (MSCI-based) for the 13 countries with complete history: UK,
Australia, Belgium, France, Germany, Hong Kong, Italy, Japan, Netherlands, Singapore,
Spain, Sweden, Switzerland. Fixed non-negative weights summing to one are fitted by
least squares to the Developed ex US return over 1990-07 .. 2000-06 (Japan 41%, UK 13%,
France 8%, Germany 7%, Belgium 7%, others smaller; exact values in
`calibration.json` under `build_notes.equity_intl_proxy`). Tracking error is 2.7% a year
in the fit window and 2.7% in the 2000-07 .. 2025-12 hold-out (correlation 0.987, mean
difference -0.4% a year). Flag 1. Nothing is available before 1975.

**Gold.** Percentage change of the monthly-average USD price, starting 1968-04, the
first month after the London Gold Pool collapsed and a free market price existed.
Earlier prices were official (fixed) and are left blank.

## calibration.json

Computed by the same script, all from `monthly.csv` values. Sections:

- `sample`, `coverage`, `build_notes` (splice diagnostics, proxy weights, mortgage spread).
- `series_stats`: for `full_sample`, `from_1953`, `from_1975`, `from_1990`, and for each
  return series, nominal and real: annualised arithmetic mean (12 x monthly mean),
  annualised geometric mean, annualised volatility (monthly sd x sqrt 12), skew and excess
  kurtosis of monthly returns, worst rolling 12-month return, and the deepest
  peak-to-trough drawdown with peak, trough and recovery dates. Real = `(1+r)/(1+cpi) - 1`.
- `annual_return_stats`: the same for non-overlapping calendar-year returns, including
  lag-1 autocorrelation and share of negative years. Use these, not the monthly figures,
  for smoothed series (home prices, pre-1962 bonds).
- `premia_full_sample_ann`.
- `correlations`: pairwise-complete matrices of monthly and of calendar-year returns,
  nominal and real, including CPI, the change in unemployment and the change in the long
  yield; sub-period versions; and the equity-bond correlation by era.
- `autocorrelation`: lags 1, 12, 24, 36, 60 months for returns (nominal and real) and
  for absolute returns (volatility clustering).
- `variance_ratios`: Lo-MacKinlay overlapping variance ratios of log returns at 12, 24,
  60 and 120 months, bias-corrected.
- `business_cycle`: statistics conditional on recession vs expansion (full sample and
  from 1948), spell durations, monthly transition probabilities, and a table of each
  recession with the unemployment rise and the real equity return during it.
- `equity_regimes`: two calm/turbulent classifications (below).
- `levels`: mean, sd, range and persistence of yields, spreads, 12-month inflation,
  ex-post real bill rate, mortgage rate and unemployment, plus the latest values.
- `cross_checks`: the checks quoted in this file.

### Regime methods

1. **Threshold (simple, transparent).** Month t is *turbulent* when the annualised
   standard deviation of the 12 monthly US equity returns ending in t exceeds the 75th
   percentile of that statistic over the sample (18.2%); otherwise *calm*. Per-regime
   means, vols and cross-correlations are computed over the months in each class and the
   transition matrix by counting month-to-month changes. Because the window trails, the
   classification lags the true onset of turbulence by a few months, which is why
   turbulent-regime mean returns are positive under this method.
2. **Gaussian hidden Markov model.** Two-state model with state-dependent mean and
   variance fitted by Baum-Welch (EM) to monthly log excess returns
   `ln(1+equity_us) - ln(1+tbill)`, started from the threshold classification. Months
   are then assigned to the state with smoothed probability above 0.5 for the cross-series
   statistics. This is the better basis for a regime-switching generator.

## Headline numbers (1926-07 .. 2026-09 unless stated)

| series | arith. mean | geo. mean | vol | real geo. | worst 12m (real) | max drawdown (real) |
|---|---|---|---|---|---|---|
| equity_us | 11.57% | 10.36% | 18.3% | 7.16% | -62.0% | -79.2% (1929-08 to 1932-06, recovered 1945-02) |
| bond10 | 4.71% | 4.59% | 6.5% | 1.56% | -23.8% | -60.1% (1940-12 to 1981-09, recovered 1989-06) |
| tbill | 3.24% | 3.29% | 0.9% | 0.30% | -16.1% | -48.7% (1933-05 to 1951-12, recovered 2001-08) |
| cpi | 2.96% | 2.98% | 1.8% | | | |
| home | 4.05% | 4.11% | 2.1% (monthly); 6.1% (calendar-year sd) | 1.10% | -13.9% | -35.7% (2005-12 to 2012-02) |
| equity_intl (1975 on) | 11.44% | 10.55% | 16.5% | 6.63% | -49.9% | -56.6% |
| gold (1968-04 on) | 9.51% | 8.48% | 16.9% | 4.32% | -42.6% | -83.1% (1980-01 to 2001-04, recovered 2025-02) |

- Equity premium over bills 8.3% arithmetic; 10-year bond over bills 1.5%.
- Monthly US equity returns: skew +0.13, excess kurtosis 7.4, worst month -28.7%.
- Equity-bond monthly correlation: 0.06 overall, but +0.01 (1926-65), +0.28 (1966-99),
  -0.35 (2000-20), +0.51 (2021 on). A single constant is a poor model.
- Calendar-year correlations: equity/CPI 0.00 nominal and -0.18 real; bond/CPI -0.50
  real; home/CPI +0.54 nominal; equity/intl 0.61; equity vs unemployment change -0.38.
- Autocorrelation at lag 1: equity 0.09, bond 0.10 (month-end sample), CPI 0.48, home
  0.89, bills 0.98. At 12 months: CPI 0.30, home 0.65. Equity absolute returns 0.22 at
  lag 1 and 0.15 at lag 12 (volatility clusters for a year or more).
- Real equity variance ratios: 0.85 at 5 years and 0.75 at 10 years over the full
  sample, i.e. 10-year risk about 13% lower than a random walk implies. From 1950 on
  they are 1.08 and 1.24: the evidence for mean reversion rests on the 1929-1949 period.
- Recessions: 16.7% of months; 16 complete recessions averaging 12.6 months and 15
  expansions averaging 61.4 months; monthly P(expansion to recession) 1.60%,
  P(recession to expansion) 7.96%. From 1948: 10.3 and 66.6 months, 1.46% and 9.68%.
- In recession months (from 1948) US equities average +4.8% a year nominal with 20.7%
  vol against +13.3% and 13.8% in expansions; unemployment rises 4.1 points a year in
  recessions and falls 0.6 points a year in expansions.
- HMM regimes: calm +12.9% a year log excess return with 12.5% vol; turbulent -24.6%
  with 33.6% vol; expected durations 39.9 and 8.1 months; 16.8% of time turbulent; 55%
  of turbulent months are NBER recession months against 10% of calm months. US/intl
  equity correlation rises from 0.57 (calm) to 0.85 (turbulent).

## Known limitations

- **US survivorship bias.** The United States had the best-documented successful equity
  market of the twentieth century. A 7% real equity return is high relative to the
  roughly 4-5% real return of the world ex-US over 1900-2020 reported by Dimson, Marsh
  and Staunton. Consider shrinking the equity mean in the generator rather than taking
  the sample mean at face value.
- **One history.** 100 years contains about ten independent 10-year periods and three
  independent 30-year retirements. Long-horizon statistics (variance ratios, drawdown
  durations, the equity-bond correlation regime) are weakly identified.
- **Equity returns are gross** of fund costs and taxes. `equity_intl` is gross of
  dividend withholding tax and unhedged.
- **Bond returns are synthetic**, derived from a single yield with a flat-curve
  assumption (no roll-down, no bid-ask). Before 1962-02 they come from monthly-average
  yields, which understates volatility and inflates lag-1 autocorrelation: on the
  1962-2026 sample the monthly-average method gives 6.6% vol and 0.31 autocorrelation
  against 7.8% and 0.10 from month-end yields. Means are unaffected. Do not bootstrap
  single months of flag 1/2 rows expecting realistic bond volatility; block sampling of
  a year or more is fine.
- **LTGOVTBD is not a 10-year yield.** FRED describes it as an "unweighted average on all
  outstanding bonds neither due nor callable in less than 10 years" (the cut-off was 12-15
  years in parts of the sample) and before 1941 includes partially
  tax-exempt bonds, so pre-1953 yields are slightly understated and are applied to a
  10-year bond regardless. Yields were also pegged by the Federal Reserve in 1942-1951.
- **Home prices are an index of price only**, smoothed (Case-Shiller is a three-month
  moving average of repeat sales; lag-1 autocorrelation 0.85-0.89), national (an
  individual house is far more volatile, around 10-15% a year idiosyncratic) and
  seasonal after 1987 (not seasonally adjusted). Rows with `home_flag` 2 contain no
  monthly information at all. Monthly volatility of this column (2.1%) badly understates
  risk; use the calendar-year figure (6.1% nominal, 5.0% real) plus an idiosyncratic term.
- **Mortgage rates before 1971-04 are synthetic** (see above).
- **Unemployment before 1948 is a rough estimate.** The NICB-based series reads near
  zero in mid-1929 (true rate about 3%) and the wartime Census series about 1%. The
  1929-33 rise to 25% matches other estimates. Series definitions change at 1940, 1947
  and 1948. The April 2020 reading (14.8%) dominates post-war monthly-change statistics.
- **CPI is not seasonally adjusted** (appropriate for indexation, which uses NSA values,
  but monthly rates carry a seasonal pattern) and early CPI was rounded to one decimal,
  producing months of exactly zero inflation.
- **International proxy 1975-1990** uses fixed 1990s weights; Japan's true weight rose
  from about a quarter to over half of the index during that period.
- **Gold** prices are monthly averages rounded to whole dollars in the source, which
  adds noise at the $35-45 level of 1968-72 and induces lag-1 autocorrelation of about
  0.27. US residents could not legally hold bullion before 1975.
- **TIPS, corporate bonds, small/value equities and REITs are not included.**
- **Pre-1953 home-price timing is uncertain by up to a year.** The build follows Shiller's
  January dating; Damodaran's table dates the same index values a year later. Calendar-year
  home/CPI correlation is 0.54 either way, but home/US-equity correlation is 0.24 as built
  and 0.15 with the later dating.
- **International proxy accuracy before 1990 is not measured.** The 2.7% tracking error is
  from 1990-2025. Against MSCI EAFE calendar-year returns as recalled, the proxy differs by
  up to 6-9 points in single years (1985, 1986, 1989).
- The Ibbotson SBBI reference figures used to sanity-check `bond10` are quoted from memory;
  the Damodaran figures were recomputed from his published table.

## Verification log (2026-11-17)

An independent check re-downloaded all 20 raw files (byte-identical), rebuilt both outputs
(byte-identical), recomputed the headline statistics with separate code, and compared
values with BLS (CPI-U and unemployment API), Treasury.gov (daily par yields) and
Damodaran's annual table. No value in `monthly.csv` changed. Corrections made:

- Damodaran reference: was "arithmetic 4.57%, geometric 4.32%, sd ~8.0%" for 1928-2023
  (from memory); his table gives 4.86% / 4.57% / 7.95% (4.82% / 4.53% / 7.90% for 1928-2025).
  Fixed here, in `tools/build_history.py` and in `calibration.json`
  (`cross_checks.bond10_calendar_year_returns_1928_on.published_reference`).
- Bond formula agreement: "within 0.05%" corrected to 0.06% (largest gap is 0.052%).
- Case-Shiller and Freddie Mac licence rows now quote FRED's actual series notes; the
  earlier wording ("prior written permission", "Source: Freddie Mac") was not found there.
- Shiller dating: there is no 1953 annual row; wording corrected and the Damodaran dating
  difference documented.
- LTGOVTBD maturity definition now quotes FRED.
