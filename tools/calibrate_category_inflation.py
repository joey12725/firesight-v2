#!/usr/bin/env python3
"""Calibrates the category price wedge (`spending.category_inflation`).

For each spending group, takes the annual average of a BLS CPI-U sub-index (not
seasonally adjusted) relative to CPI-U all items, both from FRED (St. Louis Fed,
which republishes the Bureau of Labor Statistics series), and fits
    ln(sub_t / cpi_t) = a + b * t + x_t,      x_t = phi * x_{t-1} + eps_t
by least squares: the linear trend is the category's long-run real growth (already
modelled by `real_growth`), and what is left is the persistent, mean-zero wedge.

    python3 -I tools/calibrate_category_inflation.py [--fetch DIR | --raw DIR] [FIRST LAST]

Prints a JSON object for the `spending_realism.category_inflation` section of
data/actuarial/params.json. Standard library only.
"""

import json
import math
import os
import sys
import urllib.request

SERIES = {
    "food": ("CPIUFDNS", "CPI-U Food, NSA"),
    "utilities": ("CUUR0000SAH2", "CPI-U Fuels and utilities, NSA"),
    "transport": ("CPITRNSL", "CPI-U Transportation, SA"),
    "shelter": ("CUUR0000SAH1", "CPI-U Shelter, NSA"),
    "medical": ("CPIMEDNS", "CPI-U Medical care, NSA"),
    "education": ("CUUR0000SEEB", "CPI-U Tuition, other school fees and childcare, NSA"),
}
ALL_ITEMS = "CPIAUCNS"
PHI_CAP = 0.95  # judgement: keeps the wedge stationary (see docs/methodology.md)


def fetch(series, directory):
    url = "https://fred.stlouisfed.org/graph/fredgraph.csv?id=" + series
    with urllib.request.urlopen(url, timeout=60) as r:
        text = r.read().decode()
    with open(os.path.join(directory, series + ".csv"), "w") as f:
        f.write(text)


def annual(directory, series):
    by = {}
    with open(os.path.join(directory, series + ".csv")) as f:
        next(f)
        for line in f:
            date, _, value = line.strip().partition(",")
            try:
                by.setdefault(int(date[:4]), []).append(float(value))
            except ValueError:
                pass
    return {y: sum(v) / len(v) for y, v in by.items() if len(v) == 12}


def main(argv):
    directory = None
    do_fetch = False
    rest = []
    it = iter(argv)
    for a in it:
        if a == "--fetch":
            directory, do_fetch = next(it), True
        elif a == "--raw":
            directory = next(it)
        else:
            rest.append(a)
    if directory is None:
        sys.exit(__doc__)
    first, last = (int(rest[0]), int(rest[1])) if len(rest) == 2 else (1980, 2025)
    if do_fetch:
        os.makedirs(directory, exist_ok=True)
        for s, _ in list(SERIES.values()) + [(ALL_ITEMS, "")]:
            fetch(s, directory)
    cpi = annual(directory, ALL_ITEMS)
    out = {}
    for group, (series, label) in SERIES.items():
        sub = annual(directory, series)
        years = [y for y in range(first, last + 1) if y in sub and y in cpi]
        r = [math.log(sub[y] / cpi[y]) for y in years]
        n = len(years)
        ym, rm = sum(years) / n, sum(r) / n
        b = sum((y - ym) * (v - rm) for y, v in zip(years, r)) / sum((y - ym) ** 2 for y in years)
        res = [v - rm - b * (y - ym) for y, v in zip(years, r)]
        phi = sum(res[i] * res[i - 1] for i in range(1, n)) / sum(res[i - 1] ** 2 for i in range(1, n))
        raw_phi = phi
        # A wedge must be stationary to be mean-zero: where the detrended series is
        # a unit root in this sample (medical, education), persistence is capped.
        phi = min(phi, PHI_CAP)
        eps = [res[i] - phi * res[i - 1] for i in range(1, n)]
        sigma = math.sqrt(sum(e * e for e in eps) / (len(eps) - 2))
        out[group] = {
            "series": series,
            "label": label,
            "sample": f"{years[0]}-{years[-1]} annual averages, n={n}",
            "real_trend_per_year": round(b, 4),
            "phi_estimated": round(raw_phi, 3),
            "phi": round(phi, 3),
            "sigma_eps": round(sigma, 4),
            "sd_of_level_deviation": round(math.sqrt(sum(e * e for e in res) / (n - 2)), 4),
        }
    print(json.dumps(out, indent=2))


if __name__ == "__main__":
    main(sys.argv[1:])
