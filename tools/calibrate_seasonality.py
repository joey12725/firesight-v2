#!/usr/bin/env python3
"""Calibrates the monthly spending weights of `spending.seasonality = "typical"`.

For each category that has a monthly, not seasonally adjusted national series, takes
every full calendar year of the sample, divides each month by that year's monthly
mean, and averages the twelve ratios over the years. The result averages 1 by
construction. Series (all retrieved from FRED, St. Louis Fed):

  groceries  MRTSSM4451USN  Census Monthly Retail Trade: grocery stores, sales, NSA
  dining     MRTSSM722USN   Census Monthly Retail Trade: food services and drinking places, NSA
  transport  MRTSSM447USN   Census Monthly Retail Trade: gasoline stations, NSA
  utilities  IPG2211A2N     Federal Reserve G.17: industrial production of electric and
                            gas utilities, NSA (a quantity index of energy delivered)

Each is a proxy: fuel is only part of transport, and energy only part of a utilities
line that also holds water, internet and phone. Categories with no such series
(travel, education, giving, entertainment, personal) stay judgements in params.json.

    python3 -I tools/calibrate_seasonality.py [--fetch DIR | --raw DIR] [FIRST LAST]

Default sample 2010 to 2019: ten full years before the 2020 disruption. Prints a JSON
object for `spending_realism.seasonality.weights`. Standard library only.
"""

import json
import os
import sys
import urllib.request

SERIES = {
    "utilities": "IPG2211A2N",
    "groceries": "MRTSSM4451USN",
    "dining": "MRTSSM722USN",
    "transport": "MRTSSM447USN",
}


def fetch(series, directory):
    url = "https://fred.stlouisfed.org/graph/fredgraph.csv?id=" + series
    with urllib.request.urlopen(url, timeout=60) as r:
        text = r.read().decode()
    with open(os.path.join(directory, series + ".csv"), "w") as f:
        f.write(text)


def monthly(directory, series):
    out = {}
    with open(os.path.join(directory, series + ".csv")) as f:
        next(f)
        for line in f:
            date, _, value = line.strip().partition(",")
            try:
                out[(int(date[:4]), int(date[5:7]))] = float(value)
            except ValueError:
                pass
    return out


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
    first, last = (int(rest[0]), int(rest[1])) if len(rest) == 2 else (2010, 2019)
    if do_fetch:
        os.makedirs(directory, exist_ok=True)
        for s in SERIES.values():
            fetch(s, directory)
    out = {}
    for category, series in SERIES.items():
        v = monthly(directory, series)
        ratios = [[] for _ in range(12)]
        for y in range(first, last + 1):
            year = [v.get((y, m)) for m in range(1, 13)]
            if None in year:
                continue
            mean = sum(year) / 12
            for m in range(12):
                ratios[m].append(year[m] / mean)
        out[category] = [round(sum(r) / len(r), 3) for r in ratios]
    print(json.dumps(out))


if __name__ == "__main__":
    main(sys.argv[1:])
