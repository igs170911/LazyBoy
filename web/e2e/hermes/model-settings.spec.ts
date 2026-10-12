import { test, expect } from '@playwright/test';
import { mergeProviderModels, modelsToText, parseModels } from '../../src/model-settings';

test('refresh and save preserve mixed protocols and manually configured models', () => {
  const configured = parseModels('claude:messages\nreasoner:responses\nprivate:messages');
  const refreshed = mergeProviderModels(['claude', 'reasoner', 'new'], configured, 'opencode-go');
  expect(parseModels(modelsToText(refreshed))).toEqual([
    {id:'claude',protocol:'messages'}, {id:'reasoner',protocol:'responses'},
    {id:'new',protocol:'chat_completions'}, {id:'private',protocol:'messages'},
  ]);
});

test('native Anthropic discovery uses Messages', () => {
  expect(mergeProviderModels(['claude'], [], 'anthropic')).toEqual([{id:'claude',protocol:'messages'}]);
});

test('opencode go discovery uses each model native endpoint', () => {
  const refreshed = mergeProviderModels(
    ['grok-4.7', 'muse-spark-1.3-contributor', 'qwen3.8-max', 'kimi-k3', 'deepseek-v4-flash'],
    [{id:'grok-4.7', protocol:'chat_completions'}, {id:'kept', protocol:'responses'}],
    'opencode-go',
  );
  expect(refreshed).toEqual([
    {id:'grok-4.7', protocol:'responses'},
    {id:'muse-spark-1.3-contributor', protocol:'responses'},
    {id:'qwen3.8-max', protocol:'messages'},
    {id:'kimi-k3', protocol:'chat_completions'},
    {id:'deepseek-v4-flash', protocol:'chat_completions'},
    {id:'kept', protocol:'responses'},
  ]);
});

test('model IDs containing colons survive a settings round trip', () => {
  const configured = [{id:'local:latest',protocol:'chat_completions' as const},
    {id:'vendor:model',protocol:'responses' as const}];
  expect(parseModels(modelsToText(configured))).toEqual(configured);
});
