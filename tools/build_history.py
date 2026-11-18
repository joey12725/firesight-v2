#!/usr/bin/env python3
"""Build the aligned monthly historical dataset for the FIRESight v2 economic
scenario generator, plus calibration statistics.

Standard library only.  Run from anywhere, in isolated mode:

    python3 -I tools/build_history.py            # build from data/history/raw
    python3 -I tools/build_history.py --fetch    # re-download raw files first (curl)
    python3 -I tools/build_history.py --home-source=fhfa
        # licence-clean variant: FHFA house price indexes (US government work) instead of
        # the S&P CoreLogic Case-Shiller index from 1975 on (see README "Licences")

Inputs : data/history/raw/*            (see SOURCES below and data/history/README.md)
Outputs: data/history/monthly.csv
         data/history/calibration.json

Every transformation (splices, interpolation, synthetic values, the bond
total-return formula) is documented in data/history/README.md; keep the two in
sync when editing.
"""
import csv
import io
import json
import math
import os
import struct
import subprocess
import sys
import zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
HIST = os.path.join(ROOT, "data", "history")
RAW = os.path.join(HIST, "raw")

FRED = "https://fred.stlouisfed.org/graph/fredgraph.csv?id="
FRENCH = "https://mba.tuck.dartmouth.edu/pages/faculty/ken.french/ftp/"
SOURCES = {
    "F-F_Research_Data_Factors_CSV.zip": FRENCH + "F-F_Research_Data_Factors_CSV.zip",
    "Developed_ex_US_3_Factors_CSV.zip": FRENCH + "Developed_ex_US_3_Factors_CSV.zip",
    "F-F_International_Countries.zip": FRENCH + "F-F_International_Countries.zip",
    "fred_CPIAUCNS.csv": FRED + "CPIAUCNS",
    "fred_GS10.csv": FRED + "GS10",
    "fred_DGS10.csv": FRED + "DGS10",
    "fred_LTGOVTBD.csv": FRED + "LTGOVTBD",
    "fred_TB3MS.csv": FRED + "TB3MS",
    "fred_CSUSHPINSA.csv": FRED + "CSUSHPINSA",
    "fred_MORTGAGE30US.csv": FRED + "MORTGAGE30US",
    "fred_UNRATE.csv": FRED + "UNRATE",
    "fred_USREC.csv": FRED + "USREC",
    "fred_USSTHPI.csv": FRED + "USSTHPI",
    "fred_HPIPONM226N.csv": FRED + "HPIPONM226N",
    "fred_M0892AUSM156SNBR.csv": FRED + "M0892AUSM156SNBR",
    "fred_M0892BUSM156SNBR.csv": FRED + "M0892BUSM156SNBR",
    "fred_M0892CUSM156NNBR.csv": FRED + "M0892CUSM156NNBR",
    "shiller_Fig3-1.xls": "http://www.econ.yale.edu/~shiller/data/Fig3-1.xls",
    "datahub_sp500.csv": "https://raw.githubusercontent.com/datasets/s-and-p-500/main/data/data.csv",
    "datahub_gold_monthly.csv": "https://raw.githubusercontent.com/datasets/gold-prices/main/data/monthly.csv",
}

START = 1926 * 12 + 6  # 1926-07, first month of the French factor file
GOLD_START = 1968 * 12 + 3  # 1968-04, first month after the London Gold Pool collapsed
# Countries in the French international file with complete 1975-01.. history.
INTL_COUNTRIES = ["UK", "Austrlia", "Belgium", "France", "Germany", "HongKong", "Italy",
                  "Japan", "Nethrlnd", "Singapor", "Spain", "Sweden", "Swtzrlnd"]
INTL_PROXY_START = 1975 * 12  # 1975-01
INTL_FIT_MONTHS = 120  # weights fitted on 1990-07 .. 2000-06


# --------------------------------------------------------------------------
# small helpers
# --------------------------------------------------------------------------
def ym(y, m):
    return y * 12 + (m - 1)


def ym_str(i):
    return "%04d-%02d" % (i // 12, i % 12 + 1)


def fetch():
    os.makedirs(RAW, exist_ok=True)
    for name, url in SOURCES.items():
        # NB: FRED rejects browser-like User-Agents from scripts; curl's default works.
        print("fetching", name)
        subprocess.run(["curl", "-sS", "-L", "-f", "-m", "120", "-o", os.path.join(RAW, name), url], check=True)


def read_fred(name, daily=False):
    """FRED graph CSV -> {ym: value} (monthly) or {ym: [values in date order]} (daily/weekly)."""
    out = {}
    with open(os.path.join(RAW, name), newline="") as fh:
        rd = csv.reader(fh)
        next(rd)
        for row in rd:
            if len(row) < 2 or row[1] in ("", "."):
                continue
            key = ym(int(row[0][:4]), int(row[0][5:7]))
            if daily:
                out.setdefault(key, []).append(float(row[1]))
            else:
                out[key] = float(row[1])
    return out


def read_french_monthly(zipname, member=None):
    """First (monthly) table of a Ken French CSV -> {ym: {col: percent}}."""
    with zipfile.ZipFile(os.path.join(RAW, zipname)) as z:
        text = z.read(member or z.namelist()[0]).decode("latin-1")
    out, cols = {}, None
    for line in text.splitlines():
        parts = [p.strip() for p in line.split(",")]
        if cols is None:
            if len(parts) > 2 and parts[0] == "" and parts[1] == "Mkt-RF":
                cols = parts[1:]
            continue
        if len(parts[0]) != 6 or not parts[0].isdigit():
            break
        out[ym(int(parts[0][:4]), int(parts[0][4:]))] = {c: float(v) for c, v in zip(cols, parts[1:])}
    return out


def read_french_country(member):
    """'Mkt' column of the first table (value-weight dollar returns) -> {ym: fraction}."""
    with zipfile.ZipFile(os.path.join(RAW, "F-F_International_Countries.zip")) as z:
        text = z.read(member + ".Dat").decode("latin-1")
    out, started = {}, False
    for line in text.splitlines():
        p = line.split()
        if p and len(p[0]) == 6 and p[0].isdigit():
            started = True
            v = float(p[1])
            if v > -99:
                out[ym(int(p[0][:4]), int(p[0][4:]))] = v / 100.0
        elif started:
            break
    return out


# --- minimal BIFF8 (.xls) numeric-cell reader (Shiller only publishes .xls) ---
def _xls_workbook_stream(path):
    with open(path, "rb") as fh:
        d = fh.read()
    if d[:8] != b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1":
        raise ValueError("not an OLE2 file: " + path)
    ssz = 1 << struct.unpack_from("<H", d, 30)[0]
    nfat = struct.unpack_from("<I", d, 44)[0]
    dirstart = struct.unpack_from("<I", d, 48)[0]
    difat_start, ndifat = struct.unpack_from("<II", d, 68)

    def sect(i):
        return d[512 + i * ssz: 512 + (i + 1) * ssz]

    difat = list(struct.unpack_from("<109I", d, 76))
    s = difat_start
    for _ in range(ndifat):
        blk = struct.unpack("<%dI" % (ssz // 4), sect(s))
        difat += blk[:-1]
        s = blk[-1]
    fat = []
    for s in difat[:nfat]:
        fat += struct.unpack("<%dI" % (ssz // 4), sect(s))

    def chain(s):
        out = []
        while s < 0xFFFFFFFC:
            out.append(sect(s))
            s = fat[s]
        return b"".join(out)

    dirs = chain(dirstart)
    for i in range(0, len(dirs), 128):
        e = dirs[i:i + 128]
        nlen = struct.unpack_from("<H", e, 64)[0]
        if e[:max(nlen - 2, 0)].decode("utf-16-le", "replace") in ("Workbook", "Book"):
            start, size = struct.unpack_from("<I", e, 116)[0], struct.unpack_from("<I", e, 120)[0]
            return chain(start)[:size]
    raise ValueError("no Workbook stream in " + path)


def _rk(v):
    if v & 2:
        x = float(struct.unpack("<i", struct.pack("<I", v))[0] >> 2)
    else:
        x = struct.unpack("<d", b"\0\0\0\0" + struct.pack("<I", v & 0xFFFFFFFC))[0]
    return x / 100.0 if v & 1 else x


def read_xls_numbers(path):
    """{sheet name: {(row, col): float}} for numeric cells incl. cached formula results."""
    wb = _xls_workbook_stream(path)
    pos, sheets, cur, names = 0, {}, None, {}
    while pos + 4 <= len(wb):
        op, ln = struct.unpack_from("<HH", wb, pos)
        body = wb[pos + 4: pos + 4 + ln]
        if op == 0x85:  # BOUNDSHEET
            n = body[6]
            names[struct.unpack_from("<I", body)[0]] = (
                body[8:8 + 2 * n].decode("utf-16-le") if body[7] & 1 else body[8:8 + n].decode("latin-1"))
        elif op == 0x809:  # BOF
            cur = sheets.setdefault(names[pos], {}) if pos in names else None
        elif cur is not None:
            if op == 0x203:  # NUMBER
                r, c = struct.unpack_from("<HH", body)
                cur[(r, c)] = struct.unpack_from("<d", body, 6)[0]
            elif op == 0x27E:  # RK
                r, c = struct.unpack_from("<HH", body)
                cur[(r, c)] = _rk(struct.unpack_from("<I", body, 6)[0])
            elif op == 0xBD:  # MULRK
                r, c = struct.unpack_from("<HH", body)
                for k in range((ln - 6) // 6):
                    cur[(r, c + k)] = _rk(struct.unpack_from("<I", body, 6 + 6 * k)[0])
            elif op == 0x06 and body[12:14] != b"\xff\xff":  # FORMULA with numeric result
                r, c = struct.unpack_from("<HH", body)
                cur[(r, c)] = struct.unpack_from("<d", body, 6)[0]
        pos += 4 + ln
    return sheets


def read_shiller_home():
    """Shiller Fig3-1.xls 'Data' sheet, nominal home price index (cols H/I).
    Returns (annual {year: idx} for rows dated as whole years, monthly {ym: idx})."""
    cells = read_xls_numbers(os.path.join(RAW, "shiller_Fig3-1.xls"))["Data"]
    annual, monthly = {}, {}
    for (r, c), date in cells.items():
        if c != 7 or (r, 8) not in cells or not 1880 < date < 2100:
            continue
        v = cells[(r, 8)]
        if abs(date - round(date)) < 1e-6:
            annual[int(round(date))] = v
        else:  # 1953.0417 = Jan 1953 (mid-month decimal dates)
            y = int(math.floor(date))
            monthly[ym(y, int(round((date - y) * 12 + 0.5)))] = v
    return annual, monthly


# --------------------------------------------------------------------------
# bond maths
# --------------------------------------------------------------------------
BOND_YEARS = 10


def bond_return(y0, y1):
    """One-month total return of a 10-year semi-annual par bond bought at yield y0
    (coupon = y0, price = 1) and revalued one month later at yield y1 (exact
    repricing of the remaining cash flows; includes accrued coupon)."""
    n = 2 * BOND_YEARS
    f = 1.0 / 6.0  # one month = 1/6 of a coupon period
    v = 1.0 / (1.0 + y1 / 2.0)
    price = sum((y0 / 2.0) * v ** (k - f) for k in range(1, n + 1)) + v ** (n - f)
    return price - 1.0


def bond_return_approx(y0, y1):
    """Textbook carry + duration + convexity approximation (used only as a check)."""
    n = 2 * BOND_YEARS
    v = 1.0 / (1.0 + y0 / 2.0)
    dmod = (1.0 - v ** n) / y0 if y0 > 1e-9 else BOND_YEARS
    cfs = [(y0 / 2.0)] * (n - 1) + [1.0 + y0 / 2.0]
    conv = sum(cf * k * (k + 1) * v ** (k + 2) for k, cf in enumerate(cfs, 1)) / 4.0
    dy = y1 - y0
    return y0 / 12.0 - dmod * dy + 0.5 * conv * dy * dy


# --------------------------------------------------------------------------
# statistics
# --------------------------------------------------------------------------
def mean(x):
    return sum(x) / len(x)


def var(x, ddof=1):
    m = mean(x)
    return sum((a - m) ** 2 for a in x) / (len(x) - ddof)


def sd(x):
    return math.sqrt(var(x))


def corr(x, y):
    mx, my = mean(x), mean(y)
    sxy = sum((a - mx) * (b - my) for a, b in zip(x, y))
    sxx = sum((a - mx) ** 2 for a in x)
    syy = sum((b - my) ** 2 for b in y)
    return sxy / math.sqrt(sxx * syy) if sxx > 0 and syy > 0 else None


def autocorr(x, lag):
    m = mean(x)
    den = sum((a - m) ** 2 for a in x)
    if den == 0 or len(x) <= lag + 12:
        return None
    return sum((x[i] - m) * (x[i - lag] - m) for i in range(lag, len(x))) / den


def rnd(v, nd=6):
    if v is None:
        return None
    if isinstance(v, float):
        return round(v, nd)
    if isinstance(v, dict):
        return {k: rnd(a, nd) for k, a in v.items()}
    if isinstance(v, (list, tuple)):
        return [rnd(a, nd) for a in v]
    return v


def contiguous(dates, vals):
    """Longest-prefix guard: series must be gap-free once it starts."""
    for a, b in zip(dates, dates[1:]):
        if b != a + 1:
            raise ValueError("gap in series at " + ym_str(a))
    return vals


def drawdown(dates, rets):
    idx, peak, peak_d = 1.0, 1.0, dates[0] - 1
    worst = (0.0, peak_d, peak_d)
    path = []
    for d, r in zip(dates, rets):
        idx *= 1.0 + r
        if idx > peak:
            peak, peak_d = idx, d
        dd = idx / peak - 1.0
        path.append((d, idx))
        if dd < worst[0]:
            worst = (dd, peak_d, d)
    rec = None
    if worst[0] < 0:
        peak_val = None
        for d, v in path:
            if d == worst[1]:
                peak_val = v
        if peak_val is None:
            peak_val = 1.0
        for d, v in path:
            if d > worst[2] and v >= peak_val:
                rec = d
                break
    return {"depth": worst[0], "peak": ym_str(worst[1]), "trough": ym_str(worst[2]),
            "recovered": ym_str(rec) if rec is not None else None,
            "months_peak_to_recovery": (rec - worst[1]) if rec is not None else None}


def worst_window(dates, rets, k=12):
    best, when = None, None
    logs = [math.log1p(r) for r in rets]
    run = sum(logs[:k])
    for i in range(k - 1, len(logs)):
        if i >= k:
            run += logs[i] - logs[i - k]
        if best is None or run < best:
            best, when = run, dates[i]
    return {"return": math.expm1(best), "window_end": ym_str(when)}


def series_stats(dates, rets):
    n = len(rets)
    m, s = mean(rets), sd(rets)
    m2 = sum((r - m) ** 2 for r in rets) / n
    m3 = sum((r - m) ** 3 for r in rets) / n
    m4 = sum((r - m) ** 4 for r in rets) / n
    geo = math.expm1(sum(math.log1p(r) for r in rets) * 12.0 / n)
    return {
        "start": ym_str(dates[0]), "end": ym_str(dates[-1]), "months": n,
        "mean_arith_ann": 12.0 * m,
        "mean_geo_ann": geo,
        "vol_ann": s * math.sqrt(12.0),
        "skew_monthly": m3 / m2 ** 1.5 if m2 > 0 else None,
        "excess_kurtosis_monthly": m4 / m2 ** 2 - 3.0 if m2 > 0 else None,
        "min_month": min(rets), "max_month": max(rets),
        "worst_12m": worst_window(dates, rets, 12) if n >= 12 else None,
        "max_drawdown": drawdown(dates, rets),
    }


def variance_ratio(logrets, q):
    """Lo-MacKinlay overlapping variance ratio with small-sample bias correction:
    Var(q-month log return) / (q * Var(1-month log return))."""
    n = len(logrets)
    mu = mean(logrets)
    v1 = sum((r - mu) ** 2 for r in logrets) / (n - 1)
    cs = [0.0]
    for r in logrets:
        cs.append(cs[-1] + r)
    m = q * (n - q + 1) * (1.0 - q / n)
    vq = sum((cs[i] - cs[i - q] - q * mu) ** 2 for i in range(q, n + 1)) / m
    return vq / v1


def corr_matrix(names, cols):
    """Pairwise-complete correlation matrix; cols[name] is a list with None for missing."""
    mat, nobs = {}, {}
    for a in names:
        mat[a], nobs[a] = {}, {}
        for b in names:
            pairs = [(x, y) for x, y in zip(cols[a], cols[b]) if x is not None and y is not None]
            nobs[a][b] = len(pairs)
            mat[a][b] = corr([p[0] for p in pairs], [p[1] for p in pairs]) if len(pairs) > 2 else None
    return {"order": names, "corr": mat, "n_obs": nobs}


def simplex_project(v):
    u = sorted(v, reverse=True)
    css, rho, theta = 0.0, 0, 0.0
    for i, ui in enumerate(u, 1):
        css += ui
        t = (css - 1.0) / i
        if ui - t > 0:
            rho, theta = i, t
    return [max(a - theta, 0.0) for a in v]


def fit_simplex_weights(X, y, iters=20000):
    """min ||Xw - y||^2  s.t. w >= 0, sum w = 1   (projected gradient)."""
    k = len(X[0])
    xtx = [[sum(r[i] * r[j] for r in X) for j in range(k)] for i in range(k)]
    xty = [sum(r[i] * t for r, t in zip(X, y)) for i in range(k)]
    b = [1.0] * k
    lam = 1.0
    for _ in range(200):  # power iteration for the Lipschitz constant
        nb = [sum(xtx[i][j] * b[j] for j in range(k)) for i in range(k)]
        lam = math.sqrt(sum(a * a for a in nb))
        b = [a / lam for a in nb]
    w = [1.0 / k] * k
    for _ in range(iters):
        g = [sum(xtx[i][j] * w[j] for j in range(k)) - xty[i] for i in range(k)]
        w = simplex_project([w[i] - g[i] / lam for i in range(k)])
    return w


def hmm2_fit(x, init_state, iters=500, tol=1e-10):
    """Two-state Gaussian hidden Markov model by Baum-Welch (EM), scaled
    forward-backward.  init_state: list of 0/1 used for starting values.
    Returns parameters and smoothed P(state 1)."""
    n = len(x)
    mu, sg = [0.0, 0.0], [1.0, 1.0]
    for s in (0, 1):
        xs = [a for a, st in zip(x, init_state) if st == s]
        mu[s], sg[s] = mean(xs), sd(xs)
    P = [[0.97, 0.03], [0.10, 0.90]]
    pi = [0.75, 0.25]
    ll_old, ll = None, None
    gam = None
    for it in range(iters):
        dens = [[math.exp(-0.5 * ((a - mu[s]) / sg[s]) ** 2) / (sg[s] * math.sqrt(2 * math.pi)) for s in (0, 1)]
                for a in x]
        alpha, scale = [], []
        prev = [pi[s] * dens[0][s] for s in (0, 1)]
        c = sum(prev)
        alpha.append([p / c for p in prev])
        scale.append(c)
        for t in range(1, n):
            cur = [(alpha[-1][0] * P[0][s] + alpha[-1][1] * P[1][s]) * dens[t][s] for s in (0, 1)]
            c = sum(cur)
            alpha.append([p / c for p in cur])
            scale.append(c)
        ll = sum(math.log(c) for c in scale)
        beta = [[1.0, 1.0] for _ in range(n)]
        for t in range(n - 2, -1, -1):
            for s in (0, 1):
                beta[t][s] = sum(P[s][j] * dens[t + 1][j] * beta[t + 1][j] for j in (0, 1)) / scale[t + 1]
        gam = [[alpha[t][s] * beta[t][s] for s in (0, 1)] for t in range(n)]
        xi = [[0.0, 0.0], [0.0, 0.0]]
        for t in range(n - 1):
            for i in (0, 1):
                for j in (0, 1):
                    xi[i][j] += alpha[t][i] * P[i][j] * dens[t + 1][j] * beta[t + 1][j] / scale[t + 1]
        for i in (0, 1):
            tot = xi[i][0] + xi[i][1]
            P[i] = [xi[i][0] / tot, xi[i][1] / tot]
        pi = gam[0][:]
        for s in (0, 1):
            w = sum(g[s] for g in gam)
            mu[s] = sum(g[s] * a for g, a in zip(gam, x)) / w
            sg[s] = math.sqrt(sum(g[s] * (a - mu[s]) ** 2 for g, a in zip(gam, x)) / w)
        if ll_old is not None and abs(ll - ll_old) < tol:
            break
        ll_old = ll
    return {"mu": mu, "sigma": sg, "P": P, "loglik": ll, "iterations": it + 1}, [g[1] for g in gam]


# --------------------------------------------------------------------------
# build
# --------------------------------------------------------------------------
def build(home_source="caseshiller"):
    notes = {"home_source": home_source}
    ff = read_french_monthly("F-F_Research_Data_Factors_CSV.zip")
    cpi_idx = read_fred("fred_CPIAUCNS.csv")
    gs10 = read_fred("fred_GS10.csv")
    dgs10 = read_fred("fred_DGS10.csv", daily=True)
    ltgov = read_fred("fred_LTGOVTBD.csv")
    tb3 = read_fred("fred_TB3MS.csv")
    cs = read_fred("fred_CSUSHPINSA.csv")
    mort_w = read_fred("fred_MORTGAGE30US.csv", daily=True)
    unrate = read_fred("fred_UNRATE.csv")
    usrec = read_fred("fred_USREC.csv")
    un_a = read_fred("fred_M0892AUSM156SNBR.csv")
    un_b = read_fred("fred_M0892BUSM156SNBR.csv")
    un_c = read_fred("fred_M0892CUSM156NNBR.csv")

    end = max(ff)
    dates = list(range(START, end + 1))
    for d in dates:
        if d not in ff:
            raise ValueError("French factors missing " + ym_str(d))

    equity = {d: (ff[d]["Mkt-RF"] + ff[d]["RF"]) / 100.0 for d in dates}
    tbill = {d: ff[d]["RF"] / 100.0 for d in dates}

    # ---- CPI: fill missing index months by geometric interpolation, flag both sides
    cpi, cpi_flag = {}, {}
    filled = dict(cpi_idx)
    missing = [d for d in range(START - 1, end + 1) if d not in cpi_idx]
    for d in missing:
        lo = d - 1
        while lo not in cpi_idx:
            lo -= 1
        hi = d + 1
        while hi not in cpi_idx:
            hi += 1
            if hi > end + 24:
                raise ValueError("CPI not available through " + ym_str(d))
        filled[d] = cpi_idx[lo] * (cpi_idx[hi] / cpi_idx[lo]) ** ((d - lo) / (hi - lo))
    for d in dates:
        cpi[d] = filled[d] / filled[d - 1] - 1.0
        cpi_flag[d] = 1 if (d not in cpi_idx or d - 1 not in cpi_idx) else 0
    notes["cpi_interpolated_index_months"] = [ym_str(d) for d in missing]

    # ---- long yield & 10-year bond total return
    # yield observations (percent): month-end daily DGS10 from 1962-01; monthly average
    # GS10 from 1953-04; monthly average LTGOVTBD before.  A month's return always uses
    # two yields from the SAME source so splices do not create artificial price jumps.
    dg_end = {d: v[-1] for d, v in dgs10.items()}
    long_yield, bond, bond_flag, bond_approx = {}, {}, {}, {}
    GS_START, DG_START = ym(1953, 4), ym(1962, 1)
    for d in dates:
        if d - 1 >= DG_START and d in dg_end and d - 1 in dg_end:
            y0, y1, fl = dg_end[d - 1], dg_end[d], 0
        elif d - 1 >= GS_START:
            y0, y1, fl = gs10[d - 1], gs10[d], 1
        else:
            y0, y1, fl = ltgov[d - 1], ltgov[d], 2
        bond[d] = bond_return(y0 / 100.0, y1 / 100.0)
        bond_approx[d] = bond_return_approx(y0 / 100.0, y1 / 100.0)
        bond_flag[d] = fl
        if d >= DG_START and d in dg_end:
            long_yield[d] = dg_end[d] / 100.0
        elif d >= GS_START:
            long_yield[d] = gs10[d] / 100.0
        else:
            long_yield[d] = ltgov[d] / 100.0
    # monthly-average long yield (for the mortgage model and cross-checks)
    long_avg = {d: (gs10[d] if d >= GS_START else ltgov[d]) / 100.0 for d in dates}

    # ---- home prices
    sh_annual, sh_monthly = read_shiller_home()
    SH_M_START = min(sh_monthly)  # 1953-01
    CS_START = min(cs)  # 1987-01
    home_idx = {}
    for y in sorted(sh_annual):  # annual observations are January values (see README)
        if ym(y, 1) < SH_M_START:
            home_idx[ym(y, 1)] = sh_annual[y]
    home_idx.update({d: v for d, v in sh_monthly.items()})
    anchors = sorted(d for d in home_idx if d <= SH_M_START)
    for a, b in zip(anchors, anchors[1:]):  # geometric interpolation between Januaries
        for d in range(a + 1, b):
            home_idx[d] = home_idx[a] * (home_idx[b] / home_idx[a]) ** ((d - a) / (b - a))
    home, home_flag = {}, {}
    for d in dates:
        if d > CS_START and d in cs and d - 1 in cs:
            home[d], home_flag[d] = cs[d] / cs[d - 1] - 1.0, 0
        elif d <= CS_START and d in home_idx and d - 1 in home_idx:
            home[d] = home_idx[d] / home_idx[d - 1] - 1.0
            home_flag[d] = 2 if d <= SH_M_START else 1
        else:
            home[d], home_flag[d] = None, ""
    # FHFA alternative (public domain): quarterly all-transactions index 1975Q1.. placed at the
    # middle month of each quarter and interpolated geometrically, then the monthly
    # purchase-only index from 1991-01.
    fh_q = read_fred("fred_USSTHPI.csv")  # keyed by first month of quarter
    fh_m = read_fred("fred_HPIPONM226N.csv")
    fq = {d + 1: v for d, v in fh_q.items()}
    fq_dates = sorted(fq)
    for a, b in zip(fq_dates, fq_dates[1:]):
        for d in range(a + 1, b):
            fq[d] = fq[a] * (fq[b] / fq[a]) ** ((d - a) / (b - a))
    FQ_START, FM_START = min(fq), min(fh_m)
    if home_source == "fhfa":
        for d in dates:
            if d > FM_START + 1 and d in fh_m and d - 1 in fh_m:
                home[d], home_flag[d] = fh_m[d] / fh_m[d - 1] - 1.0, 3
            elif FQ_START < d <= FM_START + 1:
                home[d], home_flag[d] = fq[d] / fq[d - 1] - 1.0, 4
            elif d > FM_START + 1:
                home[d], home_flag[d] = None, ""
    elif home_source != "caseshiller":
        raise ValueError("unknown --home-source " + home_source)
    yrs_h = [y for y in range(1992, end // 12 + 1) if ym(y, 12) in cs and ym(y, 12) in fh_m]
    cs_a = [cs[ym(y, 12)] / cs[ym(y - 1, 12)] - 1 for y in yrs_h]
    fh_a = [fh_m[ym(y, 12)] / fh_m[ym(y - 1, 12)] - 1 for y in yrs_h]
    notes["home_caseshiller_vs_fhfa_purchase_only_dec_dec"] = {
        "years": [yrs_h[0], yrs_h[-1]], "corr": corr(cs_a, fh_a),
        "mean_caseshiller": mean(cs_a), "mean_fhfa": mean(fh_a), "sd_caseshiller": sd(cs_a), "sd_fhfa": sd(fh_a),
        "worst_year_caseshiller": min(cs_a), "worst_year_fhfa": min(fh_a)}
    overlap = [d for d in range(CS_START + 1, max(sh_monthly) + 1) if d in cs and d - 1 in cs and d - 1 in sh_monthly]
    notes["home_shiller_vs_caseshiller_overlap"] = {
        "months": len(overlap),
        "corr_monthly_change": corr([sh_monthly[d] / sh_monthly[d - 1] - 1 for d in overlap],
                                    [cs[d] / cs[d - 1] - 1 for d in overlap]),
        "start": ym_str(overlap[0]), "end": ym_str(overlap[-1])}

    # ---- mortgage rate
    mort_m = {d: mean(v) / 100.0 for d, v in mort_w.items()}
    MORT_START = min(mort_m)
    common = [d for d in dates if d in mort_m and d in gs10]
    spread = mean([mort_m[d] - gs10[d] / 100.0 for d in common])
    mortgage, mort_flag = {}, {}
    for d in dates:
        if d in mort_m:
            mortgage[d], mort_flag[d] = mort_m[d], 0
        else:
            mortgage[d], mort_flag[d] = long_avg[d] + spread, 1
    notes["mortgage_spread_over_gs10"] = {
        "mean": spread, "sd": sd([mort_m[d] - gs10[d] / 100.0 for d in common]),
        "start": ym_str(common[0]), "end": ym_str(common[-1]), "months": len(common)}

    # ---- unemployment
    unemp, unemp_flag = {}, {}
    for d in dates:
        v, fl = None, ""
        if d >= ym(1948, 1):
            if d in unrate:
                v, fl = unrate[d], 0
            elif d - 1 in unrate and d + 1 in unrate:
                v, fl = 0.5 * (unrate[d - 1] + unrate[d + 1]), 2
        elif d >= ym(1947, 1):
            v, fl = un_c.get(d), 1
        elif d >= ym(1940, 1):
            if d in un_b:
                v, fl = un_b[d], 1
            elif d - 1 in un_b and d + 1 in un_b:
                v, fl = 0.5 * (un_b[d - 1] + un_b[d + 1]), 2
        elif d in un_a:
            v, fl = un_a[d], 1
        unemp[d] = v / 100.0 if v is not None else None
        unemp_flag[d] = fl if v is not None else ""

    recession = {d: int(usrec[d]) for d in dates}

    # ---- international developed equities
    dx = read_french_monthly("Developed_ex_US_3_Factors_CSV.zip")
    dx_ret = {d: (v["Mkt-RF"] + v["RF"]) / 100.0 for d, v in dx.items()}
    DX_START = min(dx_ret)
    countries = {c: read_french_country(c) for c in INTL_COUNTRIES}
    fit_dates = list(range(DX_START, DX_START + INTL_FIT_MONTHS))
    X = [[countries[c][d] for c in INTL_COUNTRIES] for d in fit_dates]
    w = fit_simplex_weights(X, [dx_ret[d] for d in fit_dates])

    def proxy(d):
        return sum(wi * countries[c][d] for wi, c in zip(w, INTL_COUNTRIES))

    hold = [d for d in range(DX_START + INTL_FIT_MONTHS, max(dx_ret) + 1) if all(d in countries[c] for c in INTL_COUNTRIES)]
    intl, intl_flag = {}, {}
    for d in dates:
        if d in dx_ret:
            intl[d], intl_flag[d] = dx_ret[d], 0
        elif d >= INTL_PROXY_START and all(d in countries[c] for c in INTL_COUNTRIES):
            intl[d], intl_flag[d] = proxy(d), 1
        else:
            intl[d], intl_flag[d] = None, ""
    notes["equity_intl_proxy"] = {
        "method": "fixed non-negative weights summing to 1 on French country value-weight USD market "
                  "returns, least-squares fitted to the French Developed ex US market return",
        "fit_window": [ym_str(fit_dates[0]), ym_str(fit_dates[-1])],
        "weights": {c: wi for c, wi in zip(INTL_COUNTRIES, w)},
        "fit_tracking_error_ann": sd([proxy(d) - dx_ret[d] for d in fit_dates]) * math.sqrt(12),
        "fit_corr": corr([proxy(d) for d in fit_dates], [dx_ret[d] for d in fit_dates]),
        "holdout_window": [ym_str(hold[0]), ym_str(hold[-1])],
        "holdout_tracking_error_ann": sd([proxy(d) - dx_ret[d] for d in hold]) * math.sqrt(12),
        "holdout_corr": corr([proxy(d) for d in hold], [dx_ret[d] for d in hold]),
        "holdout_mean_diff_ann": 12 * mean([proxy(d) - dx_ret[d] for d in hold]),
    }

    # ---- gold
    gold_px = {}
    with open(os.path.join(RAW, "datahub_gold_monthly.csv"), newline="") as fh:
        rd = csv.reader(fh)
        next(rd)
        for row in rd:
            if len(row) >= 2 and row[1]:
                gold_px[ym(int(row[0][:4]), int(row[0][5:7]))] = float(row[1])
    gold = {d: (gold_px[d] / gold_px[d - 1] - 1.0 if d >= GOLD_START and d in gold_px and d - 1 in gold_px else None)
            for d in dates}

    # ---- write monthly.csv
    def f6(v):
        return "" if v is None else "%.6f" % v

    def f5(v):
        return "" if v is None else "%.5f" % v

    header = ["date", "equity_us", "tbill", "bond10", "cpi", "home", "long_yield", "mortgage30",
              "unemployment", "recession", "equity_intl", "gold",
              "bond10_flag", "cpi_flag", "home_flag", "mortgage30_flag", "unemployment_flag", "equity_intl_flag"]
    out_path = os.path.join(HIST, "monthly.csv")
    with open(out_path, "w", newline="") as fh:
        wr = csv.writer(fh, lineterminator="\n")
        wr.writerow(header)
        for d in dates:
            wr.writerow([ym_str(d), f6(equity[d]), f6(tbill[d]), f6(bond[d]), f6(cpi[d]), f6(home[d]),
                         f5(long_yield[d]), f5(mortgage[d]), f5(unemp[d]), recession[d], f6(intl[d]), f6(gold[d]),
                         bond_flag[d], cpi_flag[d], home_flag[d], mort_flag[d], unemp_flag[d], intl_flag[d]])

    # ======================================================================
    # calibration
    # ======================================================================
    ret = {"equity_us": equity, "tbill": tbill, "bond10": bond, "cpi": cpi, "home": home,
           "equity_intl": intl, "gold": gold}
    names = list(ret)
    cal = {
        "generated_by": "tools/build_history.py",
        "units": "decimal fractions; '_ann' = annualised (mean x12, vol x sqrt(12)); skew/kurtosis are of monthly returns",
        "sample": {"start": ym_str(dates[0]), "end": ym_str(dates[-1]), "rows": len(dates)},
        "coverage": {}, "build_notes": notes,
    }

    def avail(name, lo=None, hi=None):
        ds = [d for d in dates if ret[name][d] is not None and (lo is None or d >= lo) and (hi is None or d <= hi)]
        return ds

    for nme in names + ["unemployment"]:
        src = ret[nme] if nme in ret else unemp
        ds = [d for d in dates if src[d] is not None]
        cal["coverage"][nme] = {"start": ym_str(ds[0]), "end": ym_str(ds[-1]), "months": len(ds),
                                "gap_free": ds[-1] - ds[0] + 1 == len(ds)}

    def real(name, d):
        return (1.0 + ret[name][d]) / (1.0 + cpi[d]) - 1.0

    def stats_block(lo=None):
        blk = {}
        for nme in names:
            ds = avail(nme, lo)
            if len(ds) < 24:
                continue
            contiguous(ds, None)
            e = {"nominal": series_stats(ds, [ret[nme][d] for d in ds])}
            if nme != "cpi":
                e["real"] = series_stats(ds, [real(nme, d) for d in ds])
            blk[nme] = e
        return blk

    cal["series_stats"] = {
        "full_sample": stats_block(),
        "from_1953": stats_block(ym(1953, 2)),
        "from_1975": stats_block(ym(1975, 1)),
        "from_1990": stats_block(ym(1990, 7)),
    }
    # calendar-year (Jan-Dec) compounded returns: the right place to read volatility for
    # smoothed / interpolated series (home, pre-1962 bonds, gold) and for annual-step models
    def annual_block(realise):
        blk = {}
        y0, y1 = dates[0] // 12 + 1, (dates[-1] + 1) // 12 - 1
        for nme in names:
            vals, ys = [], []
            for y in range(y0, y1 + 1):
                ms = range(y * 12, y * 12 + 12)
                if any(ret[nme][d] is None for d in ms):
                    continue
                g = 1.0
                for d in ms:
                    g *= (1.0 + ret[nme][d]) / ((1.0 + cpi[d]) if realise else 1.0)
                vals.append(g - 1.0)
                ys.append(y)
            if realise and nme == "cpi":
                continue
            blk[nme] = {"first_year": ys[0], "last_year": ys[-1], "n": len(vals), "mean_arith": mean(vals),
                        "mean_geo": math.expm1(mean([math.log1p(v) for v in vals])), "sd": sd(vals),
                        "min": [ys[vals.index(min(vals))], min(vals)], "max": [ys[vals.index(max(vals))], max(vals)],
                        "ac1": autocorr(vals, 1) if len(vals) > 14 else None,
                        "share_negative_years": mean([1.0 if v < 0 else 0.0 for v in vals])}
        return blk

    cal["annual_return_stats"] = {"nominal": annual_block(False), "real": annual_block(True)}
    # equity premium / term premium convenience numbers
    cal["premia_full_sample_ann"] = {
        "equity_minus_tbill_arith": 12 * mean([equity[d] - tbill[d] for d in dates]),
        "bond10_minus_tbill_arith": 12 * mean([bond[d] - tbill[d] for d in dates]),
        "equity_minus_bond10_arith": 12 * mean([equity[d] - bond[d] for d in dates]),
        "tbill_real_arith": 12 * mean([real("tbill", d) for d in dates]),
    }

    # ---- correlations
    un_chg = {d: (unemp[d] - unemp[d - 1]) if (unemp[d] is not None and unemp.get(d - 1) is not None) else None
              for d in dates}
    dly = {d: (long_avg[d] - long_avg[d - 1]) if d - 1 in long_avg else None for d in dates}
    cnames = names + ["unemp_chg", "long_yield_chg"]

    def monthly_cols(lo=None, hi=None, realise=False):
        ds = [d for d in dates if (lo is None or d >= lo) and (hi is None or d <= hi)]
        cols = {}
        for nme in names:
            if realise and nme != "cpi":
                cols[nme] = [real(nme, d) if ret[nme][d] is not None else None for d in ds]
            else:
                cols[nme] = [ret[nme][d] for d in ds]
        cols["unemp_chg"] = [un_chg[d] for d in ds]
        cols["long_yield_chg"] = [dly[d] for d in ds]
        return cols

    def annual_cols(lo_year=None, realise=False):
        """Non-overlapping calendar-year compounded returns (Jan-Dec)."""
        y0 = dates[0] // 12 + (0 if dates[0] % 12 == 0 else 1)
        y1 = (dates[-1] + 1) // 12 - 1
        years = [y for y in range(y0, y1 + 1) if lo_year is None or y >= lo_year]
        cols = {n: [] for n in cnames}
        for y in years:
            ms = range(y * 12, y * 12 + 12)
            infl = 1.0
            for d in ms:
                infl *= 1.0 + cpi[d]
            for nme in names:
                if any(ret[nme][d] is None for d in ms):
                    cols[nme].append(None)
                    continue
                g = 1.0
                for d in ms:
                    g *= 1.0 + ret[nme][d]
                cols[nme].append(g / infl - 1.0 if (realise and nme != "cpi") else g - 1.0)
            a, b = unemp.get(y * 12 + 11), unemp.get(y * 12 - 1)
            cols["unemp_chg"].append(a - b if a is not None and b is not None else None)
            cols["long_yield_chg"].append(long_avg[y * 12 + 11] - long_avg[y * 12 - 1] if y * 12 - 1 in long_avg else None)
        return years, cols

    yrs, acols = annual_cols()
    cal["correlations"] = {
        "note": "pairwise-complete Pearson correlations; n_obs gives the sample size of each pair. "
                "unemp_chg = change in unemployment rate, long_yield_chg = change in monthly-average long yield. "
                "'real' matrices deflate every return series except cpi by cpi.",
        "monthly_nominal_full": corr_matrix(cnames, monthly_cols()),
        "monthly_real_full": corr_matrix(cnames, monthly_cols(realise=True)),
        "monthly_nominal_from_1975": corr_matrix(cnames, monthly_cols(lo=ym(1975, 1))),
        "monthly_nominal_from_2000": corr_matrix(cnames, monthly_cols(lo=ym(2000, 1))),
        "annual_nominal_full": dict(corr_matrix(cnames, acols), years=[yrs[0], yrs[-1]]),
        "annual_real_full": dict(corr_matrix(cnames, annual_cols(realise=True)[1]), years=[yrs[0], yrs[-1]]),
        "annual_nominal_from_1975": dict(corr_matrix(cnames, annual_cols(1975)[1]), years=[1975, yrs[-1]]),
        "equity_bond_monthly_corr_by_era": {
            era: corr([equity[d] for d in dates if lo <= d <= hi], [bond[d] for d in dates if lo <= d <= hi])
            for era, lo, hi in [("1926-1965", START, ym(1965, 12)), ("1966-1999", ym(1966, 1), ym(1999, 12)),
                                ("2000-2020", ym(2000, 1), ym(2020, 12)), ("2021-end", ym(2021, 1), end)]},
    }

    # ---- autocorrelations and variance ratios
    lags = [1, 12, 24, 36, 60]
    ac = {}
    for nme in names:
        ds = avail(nme)
        x = [ret[nme][d] for d in ds]
        e = {"nominal": {str(l): autocorr(x, l) for l in lags}}
        if nme != "cpi":
            xr = [real(nme, d) for d in ds]
            e["real"] = {str(l): autocorr(xr, l) for l in lags}
        e["abs_nominal"] = {str(l): autocorr([abs(a) for a in x], l) for l in lags}
        ac[nme] = e
    for nme, src in (("long_yield_chg", dly), ("unemp_chg", un_chg)):
        x = [src[d] for d in dates if src[d] is not None]
        ac[nme] = {"nominal": {str(l): autocorr(x, l) for l in lags}}
    ds_home = avail("home", ym(1953, 2))
    ac["home_from_1953"] = {"nominal": {str(l): autocorr([home[d] for d in ds_home], l) for l in lags}}
    ds_b = avail("bond10", ym(1962, 2))
    ac["bond10_month_end_from_1962"] = {"nominal": {str(l): autocorr([bond[d] for d in ds_b], l) for l in lags}}
    cal["autocorrelation"] = {
        "note": "sample autocorrelation of monthly series at the given lags (months); abs_nominal = "
                "autocorrelation of |return| (volatility clustering). Home-price and pre-1962 bond "
                "autocorrelations are inflated by index smoothing / interpolation / monthly-average yields.",
        "lags": lags, "series": ac}

    vr = {}
    for nme in ("equity_us", "bond10", "tbill", "equity_intl", "home"):
        ds = avail(nme)
        e = {}
        for kind in ("nominal", "real"):
            lr = [math.log1p(ret[nme][d] if kind == "nominal" else real(nme, d)) for d in ds]
            e[kind] = {str(q): variance_ratio(lr, q) for q in (12, 24, 60, 120) if len(lr) > 3 * q}
        vr[nme] = e
    lr = [math.log1p(cpi[d]) for d in dates]
    vr["cpi"] = {"nominal": {str(q): variance_ratio(lr, q) for q in (12, 24, 60, 120)}}
    ex = [math.log1p(equity[d]) - math.log1p(tbill[d]) for d in dates]
    vr["equity_us_excess"] = {"nominal": {str(q): variance_ratio(ex, q) for q in (12, 24, 60, 120)}}
    lr75 = [math.log1p(real("equity_us", d)) for d in dates if d >= ym(1950, 1)]
    vr["equity_us_from_1950"] = {"real": {str(q): variance_ratio(lr75, q) for q in (12, 24, 60, 120)}}
    cal["variance_ratios"] = {
        "note": "Lo-MacKinlay overlapping variance ratio of log returns, VR(q) = Var(q-month)/(q*Var(1-month)), "
                "horizons in months (60 = 5y, 120 = 10y). <1 mean reversion, >1 momentum/persistence. "
                "Implied annualised vol at horizon q = vol_1m_annualised * sqrt(VR(q)).",
        "series": vr}

    # ---- recession vs expansion
    def cond_block(mask_fn, lo=None):
        blk = {}
        ds = [d for d in dates if mask_fn(d) and (lo is None or d >= lo)]
        blk["months"] = len(ds)
        for nme in names:
            x = [ret[nme][d] for d in ds if ret[nme][d] is not None]
            if len(x) < 6:
                continue
            e = {"n": len(x), "mean_ann": 12 * mean(x), "vol_ann": sd(x) * math.sqrt(12)}
            if nme != "cpi":
                xr = [real(nme, d) for d in ds if ret[nme][d] is not None]
                e["real_mean_ann"] = 12 * mean(xr)
            blk[nme] = e
        u = [un_chg[d] for d in ds if un_chg[d] is not None]
        ul = [unemp[d] for d in ds if unemp[d] is not None]
        blk["unemployment"] = {"n": len(u), "avg_change_per_month": mean(u), "avg_change_per_year": 12 * mean(u),
                               "avg_level": mean(ul)}
        y = [dly[d] for d in ds if dly[d] is not None]
        blk["long_yield"] = {"avg_change_per_year": 12 * mean(y), "avg_level": mean([long_avg[d] for d in ds])}
        return blk

    def spells(lo=None):
        ds = [d for d in dates if lo is None or d >= lo]
        runs, cur, length = [], recession[ds[0]], 0
        for d in ds:
            if recession[d] == cur:
                length += 1
            else:
                runs.append((cur, length, d - length))
                cur, length = recession[d], 1
        runs.append((cur, length, ds[-1] - length + 1))
        inner = runs[1:-1]  # drop the two censored end spells
        rec = [l for s, l, _ in inner if s == 1]
        exp = [l for s, l, _ in inner if s == 0]
        n = {(a, b): 0 for a in (0, 1) for b in (0, 1)}
        for a, b in zip(ds, ds[1:]):
            n[(recession[a], recession[b])] += 1
        p01 = n[(0, 1)] / (n[(0, 0)] + n[(0, 1)])
        p10 = n[(1, 0)] / (n[(1, 0)] + n[(1, 1)])
        return {
            "window": [ym_str(ds[0]), ym_str(ds[-1])],
            "recession_months_share": sum(recession[d] for d in ds) / len(ds),
            "n_complete_recessions": len(rec), "n_complete_expansions": len(exp),
            "avg_recession_months": mean(rec), "avg_expansion_months": mean(exp) if exp else None,
            "min_max_recession_months": [min(rec), max(rec)],
            "min_max_expansion_months": [min(exp), max(exp)] if exp else None,
            "recession_lengths": rec, "expansion_lengths": exp,
            "censored_first_spell": {"state": runs[0][0], "months": runs[0][1]},
            "censored_last_spell": {"state": runs[-1][0], "months": runs[-1][1]},
            "transition_counts": {"exp_to_exp": n[(0, 0)], "exp_to_rec": n[(0, 1)],
                                  "rec_to_rec": n[(1, 1)], "rec_to_exp": n[(1, 0)]},
            "p_expansion_to_recession": p01, "p_recession_to_expansion": p10,
            "transition_matrix_rows_from_exp_rec": [[1 - p01, p01], [p10, 1 - p10]],
            "implied_avg_expansion_months": 1 / p01, "implied_avg_recession_months": 1 / p10,
            "stationary_recession_prob": p01 / (p01 + p10),
        }

    # unemployment rise peak-to-trough style: change over each complete recession (postwar)
    def unemp_rise_by_recession(lo):
        out, d = [], lo
        while d <= end:
            if recession[d] == 1 and recession.get(d - 1, 0) == 0:
                e = d
                while e + 1 <= end and recession[e + 1] == 1:
                    e += 1
                if unemp.get(d - 1) is not None and unemp.get(e) is not None:
                    window = [unemp[k] for k in range(d - 1, min(e + 13, end + 1)) if unemp[k] is not None]
                    out.append({"start": ym_str(d), "end": ym_str(e), "months": e - d + 1,
                                "unemp_before": unemp[d - 1], "unemp_at_end": unemp[e],
                                "unemp_peak_within_12m_after": max(window),
                                "equity_real_return_during": math.expm1(sum(
                                    math.log1p(real("equity_us", k)) for k in range(d, e + 1)))})
                d = e + 1
            else:
                d += 1
        return out

    cal["business_cycle"] = {
        "note": "recession = NBER USREC (1 from the month after the peak through the trough). "
                "mean_ann/vol_ann are annualised from the months in each state.",
        "conditional_full_sample": {"recession": cond_block(lambda d: recession[d] == 1),
                                    "expansion": cond_block(lambda d: recession[d] == 0)},
        "conditional_from_1948": {"recession": cond_block(lambda d: recession[d] == 1, ym(1948, 1)),
                                  "expansion": cond_block(lambda d: recession[d] == 0, ym(1948, 1))},
        "durations_full_sample": spells(),
        "durations_from_1948": spells(ym(1948, 1)),
        "recessions": unemp_rise_by_recession(ym(1929, 5)),
    }

    # ---- two-regime (calm / turbulent) summary for equities
    W = 12
    tv = {}
    for i, d in enumerate(dates):
        if i >= W - 1:
            tv[d] = sd([equity[k] for k in dates[i - W + 1: i + 1]]) * math.sqrt(12)
    tvs = sorted(tv.values())
    thr = tvs[int(0.75 * len(tvs))]
    rdates = [d for d in dates if d in tv]
    state = {d: 1 if tv[d] > thr else 0 for d in rdates}

    def regime_summary(st, ds, soft=None):
        out = {}
        for s, label in ((0, "calm"), (1, "turbulent")):
            sub = [d for d in ds if st[d] == s]
            e = {"months": len(sub), "share": len(sub) / len(ds),
                 "recession_share": mean([recession[d] for d in sub])}
            for nme in names:
                x = [ret[nme][d] for d in sub if ret[nme][d] is not None]
                if len(x) > 12:
                    e[nme] = {"n": len(x), "mean_ann": 12 * mean(x), "vol_ann": sd(x) * math.sqrt(12)}
                    if nme != "cpi":
                        e[nme]["real_mean_ann"] = 12 * mean([real(nme, d) for d in sub if ret[nme][d] is not None])
            rn = ["equity_us", "bond10", "tbill", "cpi", "home", "equity_intl"]
            e["corr_monthly"] = corr_matrix(rn, {n: [ret[n][d] for d in sub] for n in rn})
            out[label] = e
        n = {(a, b): 0 for a in (0, 1) for b in (0, 1)}
        for a, b in zip(ds, ds[1:]):
            n[(st[a], st[b])] += 1
        p01 = n[(0, 1)] / (n[(0, 0)] + n[(0, 1)])
        p10 = n[(1, 0)] / (n[(1, 0)] + n[(1, 1)])
        out["transition_matrix_rows_from_calm_turbulent"] = [[1 - p01, p01], [p10, 1 - p10]]
        out["expected_duration_months"] = {"calm": 1 / p01, "turbulent": 1 / p10}
        out["stationary_turbulent_prob"] = p01 / (p01 + p10)
        return out

    thr_summary = regime_summary(state, rdates)
    # Gaussian HMM on monthly log excess equity returns, started from the threshold split
    xs = [math.log1p(equity[d]) - math.log1p(tbill[d]) for d in dates]
    init = [state.get(d, 0) for d in dates]
    par, p1 = hmm2_fit(xs, init)
    hi = 1 if par["sigma"][1] >= par["sigma"][0] else 0  # label turbulent = higher vol
    pt = [p if hi == 1 else 1 - p for p in p1]
    hstate = {d: 1 if p > 0.5 else 0 for d, p in zip(dates, pt)}
    lo_ = 1 - hi
    P = par["P"]
    p01, p10 = P[lo_][hi], P[hi][lo_]
    cal["equity_regimes"] = {
        "threshold_method": dict(
            {"description": "month t is 'turbulent' when the annualised standard deviation of the trailing "
                            "12 monthly nominal US equity returns (months t-11..t) exceeds the 75th percentile "
                            "of that statistic over the sample; otherwise 'calm'.",
             "window_months": W, "threshold_vol_ann": thr,
             "trailing_vol_median": tvs[len(tvs) // 2], "first_classified_month": ym_str(rdates[0])},
            **thr_summary),
        "gaussian_hmm": dict(
            {"description": "two-state Gaussian hidden Markov model fitted by Baum-Welch EM to monthly log "
                            "excess returns ln(1+equity_us) - ln(1+tbill), initialised from the threshold "
                            "classification. Parameters are monthly (log excess return); *_ann fields are "
                            "annualised. Per-regime statistics of the other series use the smoothed-"
                            "probability classification P(turbulent)>0.5.",
             "loglik": par["loglik"], "iterations": par["iterations"],
             "calm": {"mu_monthly": par["mu"][lo_], "sigma_monthly": par["sigma"][lo_],
                      "mu_ann": 12 * par["mu"][lo_], "sigma_ann": par["sigma"][lo_] * math.sqrt(12)},
             "turbulent": {"mu_monthly": par["mu"][hi], "sigma_monthly": par["sigma"][hi],
                           "mu_ann": 12 * par["mu"][hi], "sigma_ann": par["sigma"][hi] * math.sqrt(12)},
             "hmm_transition_matrix_rows_from_calm_turbulent": [[1 - p01, p01], [p10, 1 - p10]],
             "hmm_expected_duration_months": {"calm": 1 / p01, "turbulent": 1 / p10},
             "hmm_stationary_turbulent_prob": p01 / (p01 + p10),
             "agreement_with_threshold_method": mean([1.0 if hstate[d] == state[d] else 0.0 for d in rdates])},
            classified=regime_summary(hstate, dates)),
    }

    # ---- levels and cross-checks
    def level_stats(x):
        return {"n": len(x), "mean": mean(x), "sd": sd(x), "min": min(x), "max": max(x),
                "ac1": autocorr(x, 1), "ac12": autocorr(x, 12)}

    infl12 = [math.expm1(sum(math.log1p(cpi[k]) for k in range(d - 11, d + 1))) for d in dates[11:]]
    tb_ann = [(1 + tbill[d]) ** 12 - 1 for d in dates]
    cal["levels"] = {
        "long_yield": level_stats([long_yield[d] for d in dates]),
        "tbill_annualised": level_stats(tb_ann),
        "term_spread_long_minus_tbill": level_stats([long_avg[d] - t for d, t in zip(dates, tb_ann)]),
        "inflation_12m": level_stats(infl12),
        "real_tbill_12m_expost": level_stats([
            math.expm1(sum(math.log1p(tbill[k]) - math.log1p(cpi[k]) for k in range(d - 11, d + 1)))
            for d in dates[11:]]),
        "mortgage30_observed": level_stats([mortgage[d] for d in dates if mort_flag[d] == 0]),
        "unemployment": level_stats([unemp[d] for d in dates if unemp[d] is not None]),
        "unemployment_from_1948": level_stats([unemp[d] for d in dates if d >= ym(1948, 1)]),
        "latest": {"date": ym_str(end), "long_yield": long_yield[end], "tbill_annualised": tb_ann[-1],
                   "mortgage30": mortgage[end], "unemployment": unemp[end], "inflation_12m": infl12[-1]},
    }

    tb_common = [d for d in dates if d in tb3]
    sh_ly = {}
    with open(os.path.join(RAW, "datahub_sp500.csv"), newline="") as fh:
        for row in csv.DictReader(fh):
            v = float(row["Long Interest Rate"] or 0)
            if v > 0:
                sh_ly[ym(int(row["Date"][:4]), int(row["Date"][5:7]))] = v / 100.0
    pre = [d for d in dates if d < GS_START and d in sh_ly]
    jan = [d for d in pre if d % 12 == 0]
    me = [d for d in dates if bond_flag[d] == 0]
    # what the monthly-average method would have produced on the month-end sample
    avg_me = [bond_return(gs10[d - 1] / 100.0, gs10[d] / 100.0) for d in me]

    def cal_year_returns(lo_y, hi_y):
        out = []
        for y in range(lo_y, hi_y + 1):
            g = 1.0
            for d in range(y * 12, y * 12 + 12):
                g *= 1.0 + bond[d]
            out.append(g - 1.0)
        return out

    by = cal_year_returns(1928, min(2025, (end + 1) // 12 - 1))
    cal["cross_checks"] = {
        "tbill_french_rf_vs_fred_tb3ms": {
            "months": len(tb_common), "start": ym_str(tb_common[0]),
            "mean_french_rf_annualised": 12 * mean([tbill[d] for d in tb_common]),
            "mean_tb3ms_discount_yield": mean([tb3[d] / 100.0 for d in tb_common]),
            "corr_levels": corr([tbill[d] * 12 for d in tb_common], [tb3[d] / 100.0 for d in tb_common])},
        "long_yield_ltgovtbd_vs_shiller_pre1953": {
            "months": len(pre), "mean_ltgovtbd": mean([ltgov[d] / 100.0 for d in pre]),
            "mean_shiller": mean([sh_ly[d] for d in pre]),
            "corr_january_levels": corr([ltgov[d] for d in jan], [sh_ly[d] for d in jan]),
            "note": "Shiller's pre-1953 long rate is annual (January) data linearly interpolated to months, "
                    "so LTGOVTBD (true monthly averages) is used for returns; Shiller is a level cross-check only."},
        "bond_formula_exact_vs_duration_convexity": {
            "max_abs_diff_monthly": max(abs(bond[d] - bond_approx[d]) for d in dates),
            "mean_diff_ann": 12 * mean([bond[d] - bond_approx[d] for d in dates]),
            "corr": corr([bond[d] for d in dates], [bond_approx[d] for d in dates])},
        "bond_month_end_vs_monthly_average_yields_from_1962": {
            "months": len(me),
            "vol_ann_month_end": sd([bond[d] for d in me]) * math.sqrt(12),
            "vol_ann_monthly_average": sd(avg_me) * math.sqrt(12),
            "ac1_month_end": autocorr([bond[d] for d in me], 1),
            "ac1_monthly_average": autocorr(avg_me, 1),
            "mean_ann_month_end": 12 * mean([bond[d] for d in me]),
            "mean_ann_monthly_average": 12 * mean(avg_me)},
        "bond10_calendar_year_returns_1928_on": {
            "years": [1928, 1928 + len(by) - 1], "arith_mean": mean(by),
            "geo_mean": math.expm1(mean([math.log1p(a) for a in by])), "sd": sd(by),
            "worst_year": [1928 + by.index(min(by)), min(by)], "best_year": [1928 + by.index(max(by)), max(by)],
            "selected_years": {str(y): by[y - 1928] for y in (1931, 1969, 1982, 1994, 2008, 2009, 2013, 2022)
                               if y - 1928 < len(by)},
            "published_reference": "Damodaran 'Historical Returns on Stocks, Bonds and Bills' 10-year T-bond "
                                   "1928-2025 (histretSP.html, retrieved 2026-11-17): arithmetic 4.82%, "
                                   "geometric 4.53%, sd 7.90%; 1928-2023: 4.86% / 4.57% / 7.95% (1982 "
                                   "+32.81%, 2008 +20.10%, 2009 -11.12%, 2022 -17.83%, 2025 +7.80%); "
                                   "correlation of his annual returns with this series 0.989. Ibbotson "
                                   "SBBI 1926-2023 (quoted from memory, NOT verified): intermediate-term "
                                   "government arithmetic ~5.0% / sd ~5.6%; long-term government arithmetic "
                                   "~5.7% / sd ~9.7%."},
    }

    with open(os.path.join(HIST, "calibration.json"), "w") as fh:
        json.dump(rnd(cal), fh, indent=1)
        fh.write("\n")
    return cal, len(dates)


if __name__ == "__main__":
    if "--fetch" in sys.argv[1:]:
        fetch()
    hs = [a.split("=", 1)[1] for a in sys.argv[1:] if a.startswith("--home-source=")]
    cal, n = build(hs[0] if hs else "caseshiller")
    s = cal["series_stats"]["full_sample"]
    print("rows %d  %s..%s" % (n, cal["sample"]["start"], cal["sample"]["end"]))
    for k, v in s.items():
        a = v["nominal"]
        r = v.get("real", {})
        print("%-12s %s..%s arith %6.2f%% geo %6.2f%% vol %6.2f%%  real geo %s  maxDD %6.1f%% realDD %s" % (
            k, a["start"], a["end"], 100 * a["mean_arith_ann"], 100 * a["mean_geo_ann"], 100 * a["vol_ann"],
            ("%6.2f%%" % (100 * r["mean_geo_ann"])) if r else "   n/a",
            100 * a["max_drawdown"]["depth"], ("%6.1f%%" % (100 * r["max_drawdown"]["depth"])) if r else "n/a"))
