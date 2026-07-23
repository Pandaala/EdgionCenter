import { describe, expect, it } from 'vitest';
import {
  GEP2257_DURATION_PATTERN,
  isGep2257DurationInRange,
  isValidGep2257Duration,
  parseGep2257DurationMilliseconds,
} from './validation';

describe('GEP-2257 duration validation', () => {
  it('accepts canonical single and combined duration forms', () => {
    for (const value of [
      '0s',
      '500ms',
      '1h',
      '1h30m',
      '1m30s',
      '1h2m3s4ms',
      '99999h99999m99999s99999ms',
    ]) {
      expect(isValidGep2257Duration(value)).toBe(true);
      expect(GEP2257_DURATION_PATTERN.test(value)).toBe(true);
    }
  });

  it('accepts zero in every supported unit', () => {
    expect(parseGep2257DurationMilliseconds('0h')).toBe(0);
    expect(parseGep2257DurationMilliseconds('0m')).toBe(0);
    expect(parseGep2257DurationMilliseconds('0s')).toBe(0);
    expect(parseGep2257DurationMilliseconds('0ms')).toBe(0);
  });

  it('rejects values outside the exact grammar', () => {
    for (const value of [
      '',
      ' 1s',
      '1s ',
      '1 s',
      '30',
      '1.5h',
      '1d',
      '2hours',
      '100millis',
      '123456s',
      '1h1m1s1ms1s',
      '-1s',
      '+1s',
      '1S',
    ]) {
      expect(isValidGep2257Duration(value)).toBe(false);
      expect(parseGep2257DurationMilliseconds(value)).toBeNull();
    }
  });

  it('parses milliseconds before the shorter minute unit', () => {
    expect(parseGep2257DurationMilliseconds('1ms')).toBe(1);
    expect(parseGep2257DurationMilliseconds('1s1ms')).toBe(1_001);
    expect(parseGep2257DurationMilliseconds('1m1s1ms')).toBe(61_001);
    expect(parseGep2257DurationMilliseconds('1h2m3s4ms')).toBe(3_723_004);
  });

  it('supports inclusive and exclusive range boundaries', () => {
    expect(isGep2257DurationInRange('0s', {
      minimumMilliseconds: 0,
      maximumMilliseconds: 3_600_000,
    })).toBe(true);
    expect(isGep2257DurationInRange('1s', {
      minimumMilliseconds: 1_000,
      maximumMilliseconds: 3_600_000,
    })).toBe(true);
    expect(isGep2257DurationInRange('1h', {
      minimumMilliseconds: 1_000,
      maximumMilliseconds: 3_600_000,
    })).toBe(true);
    expect(isGep2257DurationInRange('0s', {
      minimumMilliseconds: 0,
      minimumInclusive: false,
    })).toBe(false);
    expect(isGep2257DurationInRange('1s', {
      maximumMilliseconds: 1_000,
      maximumInclusive: false,
    })).toBe(false);
    expect(isGep2257DurationInRange('1001ms', {
      minimumMilliseconds: 1_000,
      minimumInclusive: false,
      maximumMilliseconds: 3_600_000,
      maximumInclusive: false,
    })).toBe(true);
    expect(isGep2257DurationInRange('1h', {
      maximumMilliseconds: 3_600_000,
      maximumInclusive: false,
    })).toBe(false);
  });

  it('rejects invalid durations and invalid numeric boundaries', () => {
    expect(isGep2257DurationInRange('1 second', {})).toBe(false);
    expect(isGep2257DurationInRange('1s', { minimumMilliseconds: -1 })).toBe(false);
    expect(isGep2257DurationInRange('1s', { maximumMilliseconds: 1.5 })).toBe(false);
  });
});
