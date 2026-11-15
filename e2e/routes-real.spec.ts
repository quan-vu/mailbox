import { expect } from '@playwright/test';
import { desktopOnly, test } from './fixtures';
import { AppShell } from './page-objects/AppShell';
import { InboxPage } from './page-objects/InboxPage';
import { SettingsPage } from './page-objects/SettingsPage';
import { ThreadPage } from './page-objects/ThreadPage';

/**
 * MB-104: stable deep links. Every screen has a URL that survives reload and
 * works with browser back/forward. Demo state (filters, drafts) is in-memory
 * and intentionally not part of the URL.
 */
test.describe('MB-104 Routes and deep links', () => {
  test('root redirects to /inbox', async ({ page }) => {
    const shell = new AppShell(page);
    await shell.open('/');
    await shell.expectScreen('inbox');
    await expect(page).toHaveURL(/\/inbox$/);
  });

  const deepLinks = [
    ['/inbox', 'inbox'],
    ['/compose', 'compose'],
    ['/accounts/connect', 'connect'],
    ['/settings', 'settings'],
    ['/messages/m01/thread', 'thread'],
  ] as const;

  for (const [path, screen] of deepLinks) {
    test(`deep link ${path} opens ${screen} and survives reload`, async ({ page }) => {
      const shell = new AppShell(page);
      await shell.open(path);
      await shell.expectScreen(screen);
      await page.reload();
      await shell.expectScreen(screen);
      await expect(page).toHaveURL(new RegExp(`${path.replace(/\//g, '\\/')}$`));
    });
  }

  test('settings tab is part of the URL', async ({ page }) => {
    const shell = new AppShell(page);
    const settings = new SettingsPage(page);
    await shell.open('/settings/notifications');
    await expect(settings.panel('notifications')).toBeVisible();
    await settings.tab('appearance').click();
    await expect(page).toHaveURL(/\/settings\/appearance$/);
    await page.reload();
    await expect(settings.panel('appearance')).toBeVisible();
  });

  test('thread deep link resolves the conversation of that message', async ({ page }) => {
    const shell = new AppShell(page);
    const thread = new ThreadPage(page);
    await shell.open('/messages/m11/thread');
    await expect(thread.count()).toHaveAttribute('data-count', '5');
    await expect(thread.subject()).toHaveText('Kế hoạch ra mắt phiên bản 2.0');
  });

  test('unknown message or path shows a not-found state with a way back', async ({ page }) => {
    const shell = new AppShell(page);
    await shell.open('/messages/does-not-exist/thread');
    await expect(page.getByTestId('not-found')).toBeVisible();
    await shell.open('/no/such/page');
    await expect(page.getByTestId('not-found')).toBeVisible();
    await page.getByTestId('not-found-home').click();
    await shell.expectScreen('inbox');
  });

  test('navigation updates the URL; back and forward restore screens', async ({ page }, info) => {
    test.skip(desktopOnly(info.project.name));
    const shell = new AppShell(page);
    const inbox = new InboxPage(page);
    await shell.open('/inbox');
    await inbox.select('m01');
    await shell.openThreadFromReader();
    await expect(page).toHaveURL(/\/messages\/m01\/thread$/);
    await shell.goSettings();
    await expect(page).toHaveURL(/\/settings$/);
    await page.goBack();
    await shell.expectScreen('thread');
    await page.goBack();
    await shell.expectScreen('inbox');
    await page.goForward();
    await shell.expectScreen('thread');
  });
});
