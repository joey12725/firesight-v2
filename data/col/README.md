# Cost-of-living index by city

`cities.csv` holds price-level indices for 4,421 cities in 210 countries. The model uses it for the "move to another city" event: each budget line is rescaled by the ratio of the destination index to the origin index for the category that line belongs to.

Both files in this directory are generated. Do not edit them by hand; change `tools/build_col.py` and rebuild:

```
python3 -I tools/build_col.py /path/to/cost-of-living.csv
```

## What changed from v1

v1 used column `x54` of the source file as its cost-of-living index. `x54` is the average monthly net salary, an income measure, so a move to a high-wage, moderate-price city was treated as a move to an expensive one. v1 also applied that single factor to every budget line, including mortgage and debt payments, which a move does not change.

v2 builds price indices from the item prices, one per spending category, and leaves fixed contracts alone.

## Columns

| Column | Meaning |
| --- | --- |
| `city`, `country` | Key. The pair is unique. |
| `data_quality` | From the source: 1 if Numbeo had enough contributors, 0 if it wanted more. |
| `overall_single` | Weighted index for one adult, using 1-bedroom rents. |
| `overall_family` | Weighted index for a family with children, using 3-bedroom rents. |
| `housing_rent` | Rent: geometric mean of the 1-bedroom and 3-bedroom indices, each an equal blend of city centre and outside centre (`x48`-`x51`). |
| `home_price` | Purchase price per square metre, centre and outside (`x52`, `x53`). Not part of the overall indices. |
| `groceries` | Fixed monthly grocery basket (`x9`-`x23`). |
| `dining` | Fixed monthly restaurant basket (`x1`-`x4`, `x6`, `x7`). |
| `transport` | Fuel, vehicle purchase, transit pass, taxi (`x29`-`x31`, `x33`-`x35`). |
| `utilities` | Basic utilities for an 85 m2 flat plus broadband (`x36`, `x38`). |
| `childcare` | Private full-day preschool (70%) and international primary school (30%) (`x42`, `x43`). |
| `goods_services` | Clothing, shoes, gym, cinema (`x39`, `x41`, `x44`-`x47`). |
| `imputed` | One letter per category that was imputed, empty if none: `r` housing_rent, `p` home_price, `g` groceries, `d` dining, `t` transport, `u` utilities, `c` childcare, `o` goods_services. |

All indices are rounded to one decimal. 100 means "costs the same as the reference basket", so 150 is 50% dearer.

To move a budget line from city A to city B, multiply it by `index_B / index_A`.

## Reference

The reference price of each item is its median across all 1,074 United States cities in the source, unweighted (the source has no population figures). A category index is 100 for a city that charges those median prices. The median of the published US city indices is therefore close to 100 but not exactly 100: it ranges from 96.7 (childcare) to 102.9 (dining).

## Method

1. Read the source and keep one row per (city, country). Where a pair repeats, the row with the better `data_quality` and then the more prices wins. The current source has no repeats.
2. Blank implausible prices:
   - zero or negative values;
   - a price outside 0.02x to 30x the reference price;
   - a price outside 1/6x to 6x its country median, where the country has at least 8 observations of that item;
   - rent and price pairs that contradict each other within a row (centre against outside beyond 1/3x to 5x, 3-bedroom against 1-bedroom beyond 0.6x to 6x). The member further from its country median is blanked;
   - a price per m2 outside 0.5x to 60x the monthly 1-bedroom rent in the same zone.
3. Compute each category index.
   - Groceries, dining, transport, utilities, goods and services are fixed baskets: the cost of the basket in the city over its cost at reference prices. If prices are missing, the ratio is taken over the observed items, provided they cover at least 60% of the reference basket's cost.
   - Rent, home price and childcare are weighted geometric means of price relatives, because their items are alternatives, not a basket.
4. Drop a city if fewer than 3 of the 5 core categories (rent, groceries, dining, transport, utilities) could be computed. This removes 536 of 4,957 rows.
5. Impute each remaining gap as the country median of that category, multiplied by how expensive the city is relative to its country on the core categories it does report (geometric mean, clamped to 0.5x to 2x). If the country has no value for the category, the city's own mean core index is used. Every imputed value is flagged in `imputed`.
6. Compute the two overall indices as weighted arithmetic means of the category indices.

### Basket quantities (per month)

| Basket | Items |
| --- | --- |
| Groceries | milk 7.5 l, bread 7.5 loaves, rice 3 kg, eggs 2.5 dozen, cheese 1.5 kg, chicken 4.5 kg, beef 2.5 kg, apples 4 kg, bananas 4 kg, oranges 4 kg, tomatoes 4 kg, potatoes 5 kg, onions 2 kg, lettuce 4 heads, water 6 x 1.5 l |
| Dining | 6 inexpensive meals, 1.5 mid-range meals for two, 3 fast-food meals, 8 cappuccinos, 4 soft drinks, 4 draught beers |
| Transport | 150 l gasoline, 1/120 of a new car (average of the two models, ten-year straight line), 0.25 monthly transit pass, 2 taxi trips of 5 km |
| Utilities | 1 basic utilities bill, 1 broadband subscription |
| Goods and services | 1 gym membership, 2 cinema tickets, 0.25 jeans, 0.25 dress, 1/6 running shoes, 1/12 leather shoes |

Alcohol and cigarettes bought in shops (`x24`-`x27`) are left out because their prices mostly reflect excise tax. Prepaid mobile minutes (`x37`), tennis (`x40`), taxi waiting (`x32`) and imported beer (`x5`) are left out as noisy or unrepresentative. Salary (`x54`) and mortgage rate (`x55`) are not prices and are not used.

### Overall weights

The weights are close to US CPI-U relative importance, restricted to the categories the source can price. Medical care, insurance, education other than childcare, and miscellaneous items are not priced and carry no weight.

| Category | Single adult (raw) | Single adult (normalised) | Family (raw) | Family (normalised) |
| --- | --- | --- | --- | --- |
| Housing (rent) | 35.0 | 44.0% | 33.0 | 39.3% |
| Groceries | 8.0 | 10.1% | 10.0 | 11.9% |
| Dining | 6.0 | 7.5% | 5.0 | 6.0% |
| Transport | 16.0 | 20.1% | 16.0 | 19.0% |
| Utilities | 4.5 | 5.7% | 5.0 | 6.0% |
| Goods and services | 10.0 | 12.6% | 9.0 | 10.7% |
| Childcare | 0.0 | 0.0% | 6.0 | 7.1% |

The housing weight is applied to the 1-bedroom rent index for a single adult and to the 3-bedroom rent index for a family. Those two indices are not published separately, so `overall_*` cannot be reproduced exactly from the other columns.

## Budget mapping

`budget_mapping.json` is the machine-readable copy of this table and of the weights.

| Budget type | Scaled by | Reason |
| --- | --- | --- |
| `rent` | `housing_rent` | |
| `mortgage` | not scaled | Fixed nominal contract. A home bought after the move is priced with `home_price` by the housing model. |
| `utilities` | `utilities` | |
| `groceries` | `groceries` | |
| `dining_out` | `dining` | |
| `gas` | `transport` | |
| `public_transport` | `transport` | |
| `healthcare` | not scaled | Not priced by the source; the healthcare model sets premiums and out-of-pocket costs. |
| `clothing` | `goods_services` | |
| `entertainment` | `goods_services` | |
| `gym` | `goods_services` | |
| `other` | `overall_single` or `overall_family` | Chosen by household size. |
| `childcare` (new in v2) | `childcare` | |
| `debt_payment` (new in v2) | not scaled | Fixed nominal contract. |
| `insurance` (new in v2) | not scaled | Priced by the insurer, not by local consumer prices. |
| `property_tax` (new in v2) | not scaled | Follows assessed value; handled by the housing model. |
| `subscriptions` (new in v2) | not scaled | Nationally priced. |

The first twelve rows are the budget types v1 offers in `pages/Edit Profile.py`.

## Source and licence

- Dataset: "Global Cost of Living" ("Cost of living in 4500+ world cities") by Miguel Piedade (`mvieira101`) on Kaggle, <https://www.kaggle.com/datasets/mvieira101/global-cost-of-living>, version 2, last updated 2022-12-03.
- Licence as stated on Kaggle: CC0: Public Domain.
- The dataset's description says the data were gathered by scraping Numbeo (<https://www.numbeo.com>). Numbeo's own terms of use are separate from the Kaggle licence label; check them before redistributing the raw file. This repository ships only derived indices.
- All prices in the source are in USD. Column meanings (`x1` to `x55`) are listed in the dataset description and were checked against the data: `x48`-`x51` are monthly rents (New York 3,852 / 2,347 / 7,147 / 4,279), `x52`-`x53` are prices per m2, `x54` is a monthly net salary (New York 5,944), `x55` is a mortgage rate in percent.

## Limitations

- **Snapshot.** Prices are from late 2022, converted to USD at the exchange rates of that time. The index is used only as a ratio between two cities, which ages better than the price levels, but relative rents and exchange rates have moved since.
- **Crowd-sourced.** Numbeo prices are user submissions. 3,497 of the 4,421 published cities have `data_quality` 0, and those rows can be badly wrong (the row for "Manhattan" has rents below the US median). Prefer rows with `data_quality` 1 and warn when the user picks another.
- **Imputation.** 2,111 rows have at least one imputed category. `home_price` is imputed most often (about 1,680 rows), then `housing_rent`, `transport` and `childcare`.
- **Missing cities.** The source has no row for some well-known cities, including San Francisco and Chiang Mai. Nearby rows exist (Oakland, San Jose; Bangkok, Phuket).
- **Ambiguous names.** US cities carry no state, so "Springfield" or "Portland" is whichever one Numbeo meant. Boroughs such as Brooklyn and Queens appear as separate cities.
- **One fixed basket.** Quantities are the same everywhere. The transport basket is car-centred, as in the US, so it overstates transport costs in cities where a mover would give up the car, and European fuel and vehicle taxes push the index well above 100 there.
- **Home prices.** The source prices apartments per m2. The US reference is dominated by small cities with cheap housing, so large world cities score in the hundreds. Use the ratio between two cities, not the level.
- **Childcare.** The index prices private preschool and international schools. Where public provision is subsidised (Berlin scores 27) it reflects what residents pay, and international school fees matter only to families who would use one.
- **Not covered.** Healthcare, insurance, taxes and domestic services are not priced. Taxes are computed by the tax model, not by this index.
