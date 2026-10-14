import { test, expect } from '@playwright/test';

const info = {hosts:['192.168.1.20','100.101.1.2'],hostname:'studio',port:8790,https_port:8791,fingerprint:'AB:CD',token:'secret-token'};

async function openDevices(page: import('@playwright/test').Page) {
  await page.addInitScript(() => {localStorage.setItem('lazyboy.locale','zh-Hant');(window as any).EventSource=class{close(){}};});
  await page.route('**/api/**', route => {
    const path = new URL(route.request().url()).pathname;
    return route.fulfill({json: path === '/api/auth/status' ? {authenticated:true} : path === '/api/connect' ? info : path === '/api/agents' ? {agents:[],groups:[]} : {}});
  });
  await page.goto('/');
  await page.getByRole('button', {name:'設定', exact:true}).first().click();
  if ((page.viewportSize()?.width || 1280) > 700) await page.locator('.lb-settings-nav__item', {hasText:'其他裝置與 App'}).click();
  return page.getByRole('region', {name:'從其他裝置連線'});
}

test('other devices get HTTPS and HTTP addresses, the certificate first and a sign-in QR code only on request', async ({page}) => {
  const connect = await openDevices(page);
  await expect(connect.getByText('https://192.168.1.20:8791/', {exact:true})).toBeVisible();
  await expect(connect.getByText('http://192.168.1.20:8790/', {exact:true})).toBeVisible();
  await connect.getByRole('combobox').selectOption('100.101.1.2');
  await expect(connect.getByText('https://100.101.1.2:8791/', {exact:true})).toBeVisible();
  await expect(connect.getByRole('img', {name:'下載憑證的 QR code'})).toBeVisible();
  await connect.getByRole('tab', {name:'2 開啟並登入'}).click();
  await expect(connect.getByRole('img', {name:'開啟並登入 LazyBoy 的 QR code'})).toHaveCount(0);
  await connect.getByRole('button', {name:'顯示登入 QR code'}).click();
  await expect(connect.getByRole('img', {name:'開啟並登入 LazyBoy 的 QR code'})).toBeVisible();
  const certificate = page.getByRole('region', {name:'HTTPS 憑證'});
  await expect(certificate.getByText('SHA-256 AB:CD')).toBeVisible();
  await expect(certificate.getByRole('link', {name:'下載憑證'})).toHaveAttribute('href', '/lazyboy-ca.crt');
});

test('the browser\'s install offer becomes an install button', async ({page}) => {
  await expect(await openDevices(page)).toBeVisible();
  // Without an offer: the browser's own install steps (iPhone: 加入主畫面).
  await expect(page.getByText(/^(從瀏覽器安裝|加入主畫面)$/)).toBeVisible();
  await page.evaluate(() => {
    const offer = Object.assign(new Event('beforeinstallprompt', {cancelable:true}), {
      prompt: async () => { (window as any).prompted = true; }, userChoice: Promise.resolve({outcome:'accepted'})});
    window.dispatchEvent(offer);
  });
  await page.getByRole('button', {name:'安裝', exact:true}).click();
  await expect(page.getByText('已經以 App 開啟')).toBeVisible();
  expect(await page.evaluate(() => (window as any).prompted)).toBe(true);
});
