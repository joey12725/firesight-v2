#!/usr/bin/env python3
"""Build the FIRESight v2 cost-of-living index (data/col/cities.csv).

Usage (standard library only; run isolated):

    python3 -I tools/build_col.py /path/to/cost-of-living.csv [out_dir]

Input: the Kaggle "Global Cost of Living" dataset by Miguel Piedade
(mvieira101), scraped from Numbeo, CC0. Columns x1..x55 are item prices in
USD; see data/col/README.md for the dictionary and the method.

Output (out_dir defaults to <repo>/data/col):
    cities.csv           one row per (city, country), indices with the
                         US-median reference basket = 100
    budget_mapping.json  which index scales which budget line on a move,
                         plus the weights used, so the app has one source
"""
import csv
import json
import math
import os
import statistics
import sys

REF_COUNTRY = "United States"

# --- category definitions -------------------------------------------------
# "basket": fixed monthly quantities, priced in the city and at the reference
#           (Laspeyres). "geo": weighted geometric mean of price relatives,
#           used where the items are alternatives rather than a basket.
GROCERIES = {  # monthly quantities for one adult
    "x9": 7.5,    # milk, litres
    "x10": 7.5,   # bread, 500 g loaves
    "x11": 3.0,   # rice, kg
    "x12": 2.5,   # eggs, dozens
    "x13": 1.5,   # local cheese, kg
    "x14": 4.5,   # chicken fillets, kg
    "x15": 2.5,   # beef round, kg
    "x16": 4.0,   # apples, kg
    "x17": 4.0,   # bananas, kg
    "x18": 4.0,   # oranges, kg
    "x19": 4.0,   # tomatoes, kg
    "x20": 5.0,   # potatoes, kg
    "x21": 2.0,   # onions, kg
    "x22": 4.0,   # lettuce, heads
    "x23": 6.0,   # water, 1.5 l bottles
}
DINING = {
    "x1": 6.0,    # inexpensive restaurant meal
    "x2": 1.5,    # mid-range three-course meal for two
    "x3": 3.0,    # fast-food combo meal
    "x6": 8.0,    # cappuccino
    "x7": 4.0,    # soft drink in a restaurant
    "x4": 4.0,    # domestic draught beer
}
TRANSPORT = {  # car-centred, as in the US reference household
    "x33": 150.0,        # gasoline, litres (about 40 US gallons)
    "x34": 1.0 / 240.0,  # new compact car, half weight, 10-year straight line
    "x35": 1.0 / 240.0,  # new sedan, half weight, 10-year straight line
    "x29": 0.25,         # monthly transit pass (one household in four)
    "x30": 2.0,          # taxi flag-fall, two trips
    "x31": 10.0,         # taxi distance, two 5 km trips
}
UTILITIES = {
    "x36": 1.0,   # electricity, heating, cooling, water, garbage (85 m2)
    "x38": 1.0,   # broadband internet
}
GOODS = {  # apparel, recreation, personal services
    "x39": 1.0,       # fitness club, monthly
    "x41": 2.0,       # cinema ticket
    "x44": 0.25,      # jeans
    "x45": 0.25,      # summer dress
    "x46": 1.0 / 6,   # running shoes
    "x47": 1.0 / 12,  # leather shoes
}
RENT_1BR = {"x48": 0.5, "x49": 0.5}
RENT_3BR = {"x50": 0.5, "x51": 0.5}
HOME_PRICE = {"x52": 0.5, "x53": 0.5}
CHILDCARE = {"x42": 0.7, "x43": 0.3}  # private preschool, intl primary school

# name -> (method, items, minimum share of the reference basket observed)
CATEGORIES = {
    "rent_1br": ("geo", RENT_1BR, 0.5),
    "rent_3br": ("geo", RENT_3BR, 0.5),
    "home_price": ("geo", HOME_PRICE, 0.5),
    "groceries": ("basket", GROCERIES, 0.6),
    "dining": ("basket", DINING, 0.6),
    "transport": ("basket", TRANSPORT, 0.6),
    "utilities": ("basket", UTILITIES, 0.6),
    "childcare": ("geo", CHILDCARE, 0.3),
    "goods_services": ("basket", GOODS, 0.6),
}

# Published columns, with the one-letter code used in the `imputed` column.
OUT_CATS = [
    ("housing_rent", "r"),
    ("home_price", "p"),
    ("groceries", "g"),
    ("dining", "d"),
    ("transport", "t"),
    ("utilities", "u"),
    ("childcare", "c"),
    ("goods_services", "o"),
]
CORE = ["housing_rent", "groceries", "dining", "transport", "utilities"]
MIN_CORE_OBSERVED = 3

# Overall weights: close to US CPI-U relative importance, restricted to the
# categories this dataset can price and renormalised to 1 at run time.
# Medical care, insurance, education other than childcare and "other" items
# are not priced by the source and are left out. The "housing" weight is
# applied to the 1-bedroom rent index for a single adult and to the
# 3-bedroom rent index for a family.
WEIGHTS = {
    "single": {"housing": 35.0, "groceries": 8.0, "dining": 6.0,
               "transport": 16.0, "utilities": 4.5, "goods_services": 10.0,
               "childcare": 0.0},
    "family": {"housing": 33.0, "groceries": 10.0, "dining": 5.0,
               "transport": 16.0, "utilities": 5.0, "goods_services": 9.0,
               "childcare": 6.0},
}

# v1 budget line type -> index that scales it when the household moves.
# null = not scaled by a move (fixed nominal contract, or modelled elsewhere).
BUDGET_MAPPING = {
    "rent": "housing_rent",
    "mortgage": None,
    "utilities": "utilities",
    "groceries": "groceries",
    "dining_out": "dining",
    "gas": "transport",
    "healthcare": None,
    "clothing": "goods_services",
    "public_transport": "transport",
    "entertainment": "goods_services",
    "gym": "goods_services",
    "other": "overall",
    # types v2 adds
    "childcare": "childcare",
    "debt_payment": None,
    "insurance": None,
    "property_tax": None,
    "subscriptions": None,
}
BUDGET_NOTES = {
    "mortgage": "Fixed nominal contract on the old home; never scaled. A home "
                "bought after the move is priced with home_price by the "
                "housing model.",
    "healthcare": "Not priced by the source. Premiums and out-of-pocket costs "
                  "come from the healthcare model (employer, ACA, Medicare).",
    "other": "Use overall_single or overall_family according to household "
             "size.",
    "debt_payment": "Fixed nominal contract (student loan, car loan, credit "
                    "card); never scaled.",
    "insurance": "Contract priced by the insurer, not by local consumer "
                 "prices; never scaled by this index.",
    "property_tax": "Follows the home's assessed value and local rate; "
                    "handled by the housing model.",
    "subscriptions": "Nationally priced; never scaled.",
    "gas": "The transport index mixes fuel, vehicle, transit and taxi prices.",
}

# Outlier screens for raw item prices.
GLOBAL_REL_RANGE = (0.02, 30.0)   # relative to the reference price
COUNTRY_REL_RANGE = (1 / 6, 6.0)  # relative to the country median
COUNTRY_MIN_OBS = 8
# Within-row consistency: (numerator, denominator, lowest, highest ratio).
# When a pair is inconsistent the member further from its country median is
# blanked; both are blanked if the country offers no median to judge by.
PAIR_RANGES = [
    ("x48", "x49", 1 / 3, 5.0),   # 1BR centre / outside
    ("x50", "x51", 1 / 3, 5.0),   # 3BR centre / outside
    ("x50", "x48", 0.6, 6.0),     # 3BR / 1BR, centre
    ("x51", "x49", 0.6, 6.0),     # 3BR / 1BR, outside
    ("x52", "x53", 1 / 3, 5.0),   # price per m2 centre / outside
]
# Price per m2 over monthly 1BR rent in the same zone; only the price is
# blanked, because purchase prices are the thinner, noisier series.
PRICE_RENT = [("x52", "x48"), ("x53", "x49")]
PRICE_RENT_RANGE = (0.5, 60.0)


def fnum(s):
    try:
        v = float(s)
    except (TypeError, ValueError):
        return None
    if math.isnan(v) or math.isinf(v) or v <= 0:
        return None
    return v


def median(xs):
    xs = [x for x in xs if x is not None]
    return statistics.median(xs) if xs else None


def load(path):
    items = ["x%d" % i for i in range(1, 56)]
    best = {}
    with open(path, newline="", encoding="utf-8") as f:
        for raw in csv.DictReader(f):
            city = (raw.get("city") or "").strip()
            country = (raw.get("country") or "").strip()
            if not city or not country:
                continue
            prices = {k: fnum(raw.get(k)) for k in items}
            dq = 1 if (raw.get("data_quality") or "").strip() in ("1", "1.0") else 0
            n = sum(1 for v in prices.values() if v is not None)
            row = {"city": city, "country": country, "dq": dq, "p": prices, "n": n}
            key = (city.casefold(), country.casefold())
            old = best.get(key)
            if old is None or (dq, n) > (old["dq"], old["n"]):
                best[key] = row
    return list(best.values())


def screen_outliers(rows, ref):
    """Blank item prices that are implausible. Returns the number blanked."""
    by_country = {}
    for r in rows:
        by_country.setdefault(r["country"], []).append(r)
    blanked = 0
    for k, refp in ref.items():
        if refp is None:
            continue
        for r in rows:
            v = r["p"][k]
            if v is not None and not (GLOBAL_REL_RANGE[0] <= v / refp <= GLOBAL_REL_RANGE[1]):
                r["p"][k] = None
                blanked += 1
        for grp in by_country.values():
            vals = [r["p"][k] for r in grp if r["p"][k] is not None]
            if len(vals) < COUNTRY_MIN_OBS:
                continue
            m = statistics.median(vals)
            for r in grp:
                v = r["p"][k]
                if v is not None and not (COUNTRY_REL_RANGE[0] <= v / m <= COUNTRY_REL_RANGE[1]):
                    r["p"][k] = None
                    blanked += 1
    return blanked


def screen_pairs(rows):
    """Blank rent and price entries that contradict each other."""
    keys = sorted({k for a, b, _, _ in PAIR_RANGES for k in (a, b)})
    by_country = {}
    for r in rows:
        by_country.setdefault(r["country"], []).append(r)
    cmed = {}
    for co, grp in by_country.items():
        cmed[co] = {}
        for k in keys:
            vals = [r["p"][k] for r in grp if r["p"][k] is not None]
            cmed[co][k] = statistics.median(vals) if len(vals) >= 3 else None
    blanked = 0
    for r in rows:
        p, med = r["p"], cmed[r["country"]]
        for a, b, lo, hi in PAIR_RANGES:
            if p[a] is None or p[b] is None or lo <= p[a] / p[b] <= hi:
                continue
            if med[a] and med[b]:
                da = abs(math.log(p[a] / med[a]))
                db = abs(math.log(p[b] / med[b]))
                p[a if da >= db else b] = None
                blanked += 1
            else:
                p[a] = p[b] = None
                blanked += 2
        for price, rent in PRICE_RENT:
            if p[price] is not None and p[rent] is not None and not (
                    PRICE_RENT_RANGE[0] <= p[price] / p[rent] <= PRICE_RENT_RANGE[1]):
                p[price] = None
                blanked += 1
    return blanked


def cat_index(prices, ref, method, items, min_share):
    """Index (reference = 100) from the observed items, or None."""
    if method == "basket":
        tot_ref = sum(q * ref[k] for k, q in items.items())
        num = den = 0.0
        for k, q in items.items():
            v = prices[k]
            if v is not None:
                num += q * v
                den += q * ref[k]
        if den <= 0 or den / tot_ref < min_share:
            return None
        return 100.0 * num / den
    wsum = lsum = 0.0
    for k, w in items.items():
        v = prices[k]
        if v is not None:
            wsum += w
            lsum += w * math.log(v / ref[k])
    if wsum <= 0 or wsum / sum(items.values()) < min_share:
        return None
    return 100.0 * math.exp(lsum / wsum)


def geo(a, b):
    if a is None:
        return b
    if b is None:
        return a
    return math.sqrt(a * b)


def build(rows):
    us = [r for r in rows if r["country"] == REF_COUNTRY]
    if len(us) < 50:
        raise SystemExit("too few reference-country rows: %d" % len(us))
    items = ["x%d" % i for i in range(1, 56)]
    ref = {k: median(r["p"][k] for r in us) for k in items}
    blanked = screen_outliers(rows, ref) + screen_pairs(rows)
    ref = {k: median(r["p"][k] for r in us) for k in items}  # after screening

    for r in rows:
        c = {name: cat_index(r["p"], ref, *spec) for name, spec in CATEGORIES.items()}
        c["housing_rent"] = geo(c["rent_1br"], c["rent_3br"])
        r["c"] = c
        r["obs"] = {name: c[name] is not None for name, _ in OUT_CATS}

    kept = [r for r in rows if sum(r["obs"][k] for k in CORE) >= MIN_CORE_OBSERVED]
    dropped = len(rows) - len(kept)

    # Country medians of each category, from observed values only.
    all_cats = [name for name, _ in OUT_CATS] + ["rent_1br", "rent_3br"]
    cmed = {}
    for r in kept:
        d = cmed.setdefault(r["country"], {k: [] for k in all_cats})
        for k in all_cats:
            if r["c"][k] is not None:
                d[k].append(r["c"][k])
    cmed = {co: {k: (statistics.median(v) if v else None) for k, v in d.items()}
            for co, d in cmed.items()}

    for r in kept:
        med = cmed[r["country"]]
        # How expensive is this city relative to its country, on what we see?
        logs = [math.log(r["c"][k] / med[k]) for k in CORE
                if r["obs"][k] and med[k]]
        adj = math.exp(sum(logs) / len(logs)) if logs else 1.0
        adj = min(2.0, max(0.5, adj))
        own = [math.log(r["c"][k]) for k in CORE if r["obs"][k]]
        own_level = math.exp(sum(own) / len(own))
        flags = ""
        for name, code in OUT_CATS:
            if r["c"][name] is None:
                r["c"][name] = med[name] * adj if med[name] else own_level
                flags += code
        r["imputed"] = flags
        # Bedroom-specific rents feed the overall indices only.
        for k in ("rent_1br", "rent_3br"):
            if r["c"][k] is None:
                hr = r["c"]["housing_rent"]
                if med[k] and med["housing_rent"]:
                    r["c"][k] = hr * med[k] / med["housing_rent"]
                else:
                    r["c"][k] = hr
        for who, rent in (("single", "rent_1br"), ("family", "rent_3br")):
            w = WEIGHTS[who]
            tot = sum(w.values())
            v = w["housing"] * r["c"][rent]
            for k in ("groceries", "dining", "transport", "utilities",
                      "goods_services", "childcare"):
                v += w[k] * r["c"][k]
            r["overall_" + who] = v / tot
    return kept, ref, {"dropped": dropped, "blanked": blanked,
                       "input_rows": len(rows), "us_rows": len(us)}


def main(argv):
    if len(argv) < 2:
        raise SystemExit(__doc__)
    src = argv[1]
    here = os.path.dirname(os.path.abspath(__file__))
    out_dir = argv[2] if len(argv) > 2 else os.path.join(here, os.pardir, "data", "col")
    out_dir = os.path.abspath(out_dir)
    os.makedirs(out_dir, exist_ok=True)

    rows, ref, stats = build(load(src))
    rows.sort(key=lambda r: (r["country"].casefold(), r["city"].casefold()))

    cols = ["city", "country", "data_quality", "overall_single", "overall_family"] \
        + [n for n, _ in OUT_CATS] + ["imputed"]
    out = os.path.join(out_dir, "cities.csv")
    with open(out, "w", newline="", encoding="utf-8") as f:
        w = csv.writer(f, lineterminator="\n")
        w.writerow(cols)
        for r in rows:
            w.writerow([r["city"], r["country"], r["dq"],
                        "%.1f" % r["overall_single"], "%.1f" % r["overall_family"]]
                       + ["%.1f" % r["c"][n] for n, _ in OUT_CATS] + [r["imputed"]])

    norm = {who: {k: round(v / sum(w.values()), 4) for k, v in w.items()}
            for who, w in WEIGHTS.items()}
    mapping = {
        "reference": "item-wise median price across %s cities = 100" % REF_COUNTRY,
        "imputed_codes": {code: name for name, code in OUT_CATS},
        "overall_weights": norm,
        "budget_mapping": BUDGET_MAPPING,
        "notes": BUDGET_NOTES,
    }
    with open(os.path.join(out_dir, "budget_mapping.json"), "w", encoding="utf-8") as f:
        json.dump(mapping, f, indent=2, sort_keys=True)
        f.write("\n")

    n_imp = sum(1 for r in rows if r["imputed"])
    print("input rows (unique city,country): %d" % stats["input_rows"])
    print("reference rows (%s): %d" % (REF_COUNTRY, stats["us_rows"]))
    print("item prices blanked as outliers: %d" % stats["blanked"])
    print("rows dropped (too little data): %d" % stats["dropped"])
    print("rows written: %d (with any imputed category: %d)" % (len(rows), n_imp))
    print("output: %s (%d bytes)" % (out, os.path.getsize(out)))


if __name__ == "__main__":
    main(sys.argv)
