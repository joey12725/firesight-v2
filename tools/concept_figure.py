#!/usr/bin/env python3
"""Draws docs/images/one-line-vs-many-lives.svg, the README's picture of the core idea.

It is a schematic, not engine output: a toy saver (save for 17 years, then live on the
portfolio) under a constant 5% real return on the left, and the same saver under 100
random sequences of returns (5% real on average, 10% volatility) on the right. Standard
library only, deterministic (fixed seed), so re-running gives the same file.

    python3 tools/concept_figure.py
"""
import math
import os
import random

W, H = 960, 400
PAD_X, TOP, BOTTOM = 40, 96, 64
GAP = 48
PANEL_W = (W - 2 * PAD_X - GAP) / 2
PLOT_H = H - TOP - BOTTOM

START_AGE, RETIRE_AGE, END_AGE = 35, 52, 95
START, SAVE, SPEND = 150_000, 45_000, 48_000
Y_MAX = 6_000_000

ACCENT, ACCENT_SOFT, BAD, INK, INK2, INK3, LINE, BG, CARD = (
    '#d9531e', '#f0a37f', '#b3264a', '#1c1b19', '#57534e', '#8a857d', '#dcd8cf', '#f6f5f1', '#ffffff',
)


def life(returns):
    """Portfolio balance at each age for one sequence of yearly real returns."""
    balance, out = START, [START]
    for i, r in enumerate(returns):
        age = START_AGE + i
        balance = balance * (1 + r) + (SAVE if age < RETIRE_AGE else -SPEND)
        balance = max(balance, 0)
        out.append(balance)
        if balance == 0:
            break
    return out


def path(points, x0):
    def x(i):
        return x0 + i / (END_AGE - START_AGE) * PANEL_W

    def y(v):
        return TOP + PLOT_H - min(v, Y_MAX) / Y_MAX * PLOT_H

    return 'M' + ' L'.join(f'{x(i):.1f},{y(v):.1f}' for i, v in enumerate(points))


def axes(x0, title, subtitle):
    parts = [
        f'<text x="{x0}" y="40" class="t">{title}</text>',
        f'<text x="{x0}" y="62" class="s">{subtitle}</text>',
        f'<line x1="{x0}" y1="{TOP + PLOT_H}" x2="{x0 + PANEL_W}" y2="{TOP + PLOT_H}" stroke="{LINE}"/>',
    ]
    for age in (35, 52, 65, 80, 95):
        x = x0 + (age - START_AGE) / (END_AGE - START_AGE) * PANEL_W
        parts.append(f'<text x="{x:.1f}" y="{TOP + PLOT_H + 20}" class="a" text-anchor="middle">{age}</text>')
    retire_x = x0 + (RETIRE_AGE - START_AGE) / (END_AGE - START_AGE) * PANEL_W
    parts.append(f'<line x1="{retire_x:.1f}" y1="{TOP}" x2="{retire_x:.1f}" y2="{TOP + PLOT_H}" stroke="{INK3}" stroke-dasharray="3 3"/>')
    parts.append(f'<text x="{retire_x + 6:.1f}" y="{TOP + 12}" class="a">stop working</text>')
    parts.append(f'<text x="{x0 + PANEL_W}" y="{TOP + PLOT_H + 40}" class="a" text-anchor="end">age</text>')
    parts.append(f'<text x="{x0}" y="{TOP + 12}" class="a">savings</text>')
    return parts


def main():
    years = END_AGE - START_AGE
    rng = random.Random(7)
    # Lognormal yearly real returns with an arithmetic mean of 5% and volatility near 10%.
    sigma = 0.10
    mu = math.log(1.05) - sigma ** 2 / 2
    lives = [life([math.exp(rng.gauss(mu, sigma)) - 1 for _ in range(years)]) for _ in range(100)]
    broke = [l for l in lives if len(l) <= years]
    lasted = [l for l in lives if len(l) > years]
    steady = life([0.05] * years)

    left, right = PAD_X, PAD_X + PANEL_W + GAP
    svg = [
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="{W}" height="{H}" role="img" '
        f'aria-label="Left: a typical calculator draws one smooth line and says the money lasts. Right: the same plan under 100 random sequences of market returns; in {len(lasted)} the money lasts and in {len(broke)} it runs out.">',
        '<style>text{font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Helvetica,Arial,sans-serif}'
        f'.t{{font-size:17px;font-weight:600;fill:{INK}}}.s{{font-size:13px;fill:{INK2}}}.a{{font-size:11px;fill:{INK3}}}'
        f'.k{{font-size:13px;font-weight:600;paint-order:stroke;stroke:{BG};stroke-width:5px;stroke-linejoin:round}}</style>',
        f'<rect width="{W}" height="{H}" rx="12" fill="{BG}"/>',
        f'<defs><clipPath id="plot"><rect x="{PAD_X + PANEL_W + GAP}" y="{TOP}" width="{PANEL_W}" height="{PLOT_H}"/></clipPath></defs>',
    ]
    svg += axes(left, 'A typical calculator', 'One average return, every year. One answer.')
    svg.append(f'<path d="{path(steady, left)}" fill="none" stroke="{INK}" stroke-width="2.5"/>')
    svg.append(f'<text x="{left + 140}" y="{TOP + 40}" class="k" fill="{INK}">“Stop at 52. The money lasts.”</text>')

    svg += axes(right, 'FIRESight', 'The same plan lived 100 times, through good and bad markets.')
    svg.append('<g clip-path="url(#plot)">')
    for l in lasted:
        svg.append(f'<path d="{path(l, right)}" fill="none" stroke="{ACCENT_SOFT}" stroke-width="1" opacity="0.5"/>')
    for l in broke:
        svg.append(f'<path d="{path(l, right)}" fill="none" stroke="{BAD}" stroke-width="1.5" opacity="0.9"/>')
    svg.append('</g>')
    svg.append(f'<text x="{right + PANEL_W - 4}" y="{TOP + 22}" class="k" fill="{ACCENT}" text-anchor="end">In {len(lasted)} of 100 lives the money lasts.</text>')
    svg.append(f'<text x="{right + PANEL_W - 4}" y="{TOP + 42}" class="k" fill="{BAD}" text-anchor="end">In {len(broke)} it runs out first.</text>')
    svg.append(f'<text x="{W - PAD_X}" y="{H - 12}" class="a" text-anchor="end">Illustration with a toy saver, not FIRESight output.</text>')
    svg.append('</svg>')

    out = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'docs', 'images', 'one-line-vs-many-lives.svg')
    with open(out, 'w') as f:
        f.write('\n'.join(svg) + '\n')
    print(f'wrote {os.path.normpath(out)}: {len(lasted)} lasted, {len(broke)} ran out')


if __name__ == '__main__':
    main()
