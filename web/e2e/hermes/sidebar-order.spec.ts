import { test, expect } from '@playwright/test';
import { moveGroup } from '../../src/workspace/order';

test('a dragged section can move one place down, to the end, or up', () => {
  expect(moveGroup(['a', 'b', 'c'], 'a', 'b')).toEqual(['b', 'a', 'c']);
  expect(moveGroup(['a', 'b', 'c'], 'a', 'c')).toEqual(['b', 'c', 'a']);
  expect(moveGroup(['a', 'b', 'c'], 'c', 'a')).toEqual(['c', 'a', 'b']);
  expect(moveGroup(['a', 'b', 'c'], 'b', 'b')).toEqual(['a', 'b', 'c']);
  expect(moveGroup(['a', 'b'], 'x', 'a')).toEqual(['a', 'b']);
});
