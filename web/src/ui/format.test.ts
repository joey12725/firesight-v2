import { describe, expect, it } from 'vitest';
import { formatForInput, money, moneyCompact, moneyPrice, parseNumber, percent, yearMonth, yearsBetween } from './format';

describe('parseNumber', () => {
  it('reads money with suffixes and punctuation', () => {
    expect(parseNumber('85k', 'money')).toBe(85_000);
    expect(parseNumber('$1.2m', 'money')).toBe(1_200_000);
    expect(parseNumber('1,250.50', 'money')).toBe(1250.5);
    expect(parseNumber(' 2 B ', 'money')).toBe(2e9);
    expect(parseNumber('(500)', 'money')).toBe(-500);
    expect(parseNumber('.5k', 'money')).toBe(500);
    expect(parseNumber('abc', 'money')).toBeNull();
    expect(parseNumber('12q', 'money')).toBeNull();
    expect(parseNumber('', 'money')).toBeNull();
  });

  it('reads percents as fractions', () => {
    expect(parseNumber('4%', 'percent')).toBe(0.04);
    expect(parseNumber('4', 'percent')).toBe(0.04);
    expect(parseNumber('0.5', 'percent')).toBe(0.005);
    expect(parseNumber('4.25 %', 'percent')).toBe(0.0425);
    expect(parseNumber('4k', 'percent')).toBeNull();
  });

  it('reads durations', () => {
    expect(parseNumber('30y', 'years')).toBe(30);
    expect(parseNumber('18mo', 'years')).toBe(1.5);
    expect(parseNumber('2 years', 'months')).toBe(24);
    expect(parseNumber('18', 'months')).toBe(18);
    expect(parseNumber('2.6', 'integer')).toBe(3);
  });

  it('groups a count of shares and leaves a year alone', () => {
    // 200000 against 2000000 is hard to see without separators.
    expect(formatForInput(200_000, 'count')).toBe('200,000');
    expect(formatForInput(2_000_000, 'count')).toBe('2,000,000');
    expect(formatForInput(999, 'count')).toBe('999');
    expect(parseNumber('200,000', 'count')).toBe(200_000);
    expect(parseNumber('120k', 'count')).toBe(120_000);
    expect(parseNumber('1.5m', 'count')).toBe(1_500_000);
    expect(parseNumber(formatForInput(1_234_567, 'count'), 'count')).toBe(1_234_567);
    // A year, a seed and a number of months are whole numbers too, and must not read "2,035".
    expect(formatForInput(2035, 'integer')).toBe('2035');
    expect(formatForInput(120, 'months')).toBe('120');
  });
});

describe('formatting', () => {
  it('round-trips through the input format', () => {
    for (const [value, kind] of [
      [85_000, 'money'],
      [0.0425, 'percent'],
      [0.07, 'percent'],
      [67, 'years'],
      [312, 'months'],
    ] as const) {
      expect(parseNumber(formatForInput(value, kind), kind)).toBe(value);
    }
    expect(formatForInput(0.07, 'percent')).toBe('7');
    expect(formatForInput(1_250_000, 'money')).toBe('1,250,000');
    // Per-share prices: cents are kept, and a strike under ten cents keeps its significant figures.
    expect(formatForInput(1.2, 'money')).toBe('1.20');
    expect(formatForInput(0.0125, 'money')).toBe('0.0125');
    expect(parseNumber(formatForInput(0.0125, 'money'), 'money')).toBe(0.0125);
    expect(formatForInput(-0.05, 'money')).toBe('-0.05');
    expect(formatForInput(0, 'money')).toBe('0');
  });

  it('formats for reading', () => {
    expect(money(85_000)).toBe('$85,000');
    expect(moneyCompact(85_000)).toBe('$85k');
    expect(moneyCompact(1_250_000)).toBe('$1.25M');
    expect(moneyCompact(7_400)).toBe('$7.4k');
    expect(moneyCompact(950)).toBe('$950');
    expect(percent(0.0425)).toBe('4.25%');
    expect(yearMonth('2031-09')).toBe('Sep 2031');
    expect(yearsBetween('1989-09', '2026-11')).toBe(37);
    expect(yearsBetween('1989-12', '2026-11')).toBe(36);
  });
});

describe('moneyPrice', () => {
  it('keeps the cents of a price per share', () => {
    expect(moneyPrice(1.2)).toBe('$1.20');
    expect(moneyPrice(30.25)).toBe('$30.25');
    expect(moneyPrice(0.85)).toBe('$0.85');
    expect(moneyPrice(1250)).toBe('$1,250.00');
    expect(moneyPrice(0.1)).toBe('$0.10');
    expect(moneyPrice(4)).toBe('$4.00');
  });

  it('keeps four significant figures below ten cents', () => {
    expect(moneyPrice(0.0125)).toBe('$0.0125');
    expect(moneyPrice(0.05)).toBe('$0.05');
    expect(moneyPrice(0.001)).toBe('$0.001');
    expect(moneyPrice(0.00012345678)).toBe('$0.0001235');
    expect(moneyPrice(0.0000001)).toBe('$0.0000001');
    // Rounds up across the ten-cent line into the two-decimal form.
    expect(moneyPrice(0.099999)).toBe('$0.10');
  });

  it('writes zero plainly and uses the same minus sign as money', () => {
    expect(moneyPrice(0)).toBe('$0');
    expect(moneyPrice(-0)).toBe('$0');
    expect(moneyPrice(-1.5)).toBe('−$1.50');
    expect(moneyPrice(-0.0125)).toBe('−$0.0125');
    expect(moneyPrice(-1.5).charAt(0)).toBe(money(-1).charAt(0));
  });
});
