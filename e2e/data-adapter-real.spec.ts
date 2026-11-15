import { expect } from '@playwright/test';
import { startApi, stopApi } from './api-server';
import { desktopOnly, test } from './fixtures';
import { AppShell } from './page-objects/AppShell';
import { InboxPage } from './page-objects/InboxPage';

/**
 * MB-103: the Web build reads mailbox data from the real local API
 * (GET /api/v1/demo/bootstrap) through the data-source adapter. No request is
 * stubbed; the outage test stops and restarts the real API process.
 */
test.describe('MB-103 Web data source', () => {
  test.beforeAll(startApi);
  test.afterAll(startApi);

  test('inbox is loaded from the API', async ({ page }) => {
    const shell = new AppShell(page);
    const inbox = new InboxPage(page);
    const request = page.waitForRequest((r) => r.url().endsWith('/api/v1/demo/bootstrap'));
    await shell.open('/inbox');
    const response = await (await request).response();
    expect(response?.status()).toBe(200);
    expect((await response?.json()).mode).toBe('demo');
    await expect(shell.root()).toHaveAttribute('data-source', 'http-api');
    await expect(shell.root()).toHaveAttribute('data-load', 'ready');
    await inbox.expectRowCount(9);
    await expect(shell.demoBadge()).toBeVisible();
  });

  test('API outage shows an error with retry and recovers', async ({ page }, info) => {
    test.skip(desktopOnly(info.project.name), 'restarting the API once is enough');
    const shell = new AppShell(page);
    const inbox = new InboxPage(page);
    await stopApi();
    await shell.open('/inbox');
    await expect(page.getByTestId('data-error')).toBeVisible();
    await expect(shell.root()).toHaveAttribute('data-load', 'failed');
    await inbox.expectRowCount(0);
    await startApi();
    await page.getByTestId('data-retry').click();
    await expect(page.getByTestId('data-error')).toBeHidden();
    await inbox.expectRowCount(9);
  });
});
