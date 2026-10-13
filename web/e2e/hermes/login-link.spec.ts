import { test, expect } from '@playwright/test';

test('a sign-in link with #token= signs in once and leaves the token out of the address bar', async ({page}) => {
  let signedIn = false; const tokens: string[] = [];
  await page.addInitScript(() => {localStorage.setItem('lazyboy.locale','zh-Hant');(window as any).EventSource=class{close(){}};});
  await page.route('**/api/**', async route => {
    const req = route.request(), path = new URL(req.url()).pathname;
    if (path === '/api/auth/login') {
      tokens.push(req.postDataJSON().token); signedIn = true;
      await route.fulfill({json:{ok:true}});
    } else if (path === '/api/auth/status') await route.fulfill({json:{authenticated:signedIn}});
    else await route.fulfill({json:path === '/api/agents' ? {agents:[],groups:[]} : {}});
  });
  await page.goto('/#token=link-token');
  await expect(page.getByRole('complementary', {name:'Bot 清單'})).toBeVisible();
  expect(tokens).toEqual(['link-token']);
  expect(new URL(page.url()).hash).toBe('');
});

test('without a link the sign-in form still asks for the access token', async ({page}) => {
  await page.route('**/api/**', route => route.fulfill({json:new URL(route.request().url()).pathname === '/api/auth/status' ? {authenticated:false} : {}}));
  await page.goto('/');
  await expect(page.getByRole('button', {name:'登入'})).toBeVisible();
});
