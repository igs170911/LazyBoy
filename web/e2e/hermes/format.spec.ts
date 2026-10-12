import { test, expect } from '@playwright/test';
import { unicodeHost } from '../../src/workspace/format';

test('punycode domains are shown in Unicode', () => {
  // Expected values from Python's IDNA codec.
  expect(unicodeHost('xn--mnchen-3ya.de')).toBe('münchen.de');
  expect(unicodeHost('xn--fiqs8s')).toBe('中国');
  expect(unicodeHost('xn--rsso55b.tw')).toBe('天氣.tw');
  expect(unicodeHost('xn--kpry57d.tw')).toBe('台灣.tw');
  expect(unicodeHost('example.com')).toBe('example.com');
  expect(unicodeHost('xn--!!bad.tw')).toBe('xn--!!bad.tw');
});
