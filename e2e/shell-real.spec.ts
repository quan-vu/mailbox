import { expect } from '@playwright/test';
import { desktopOnly, mobileOnly, test } from './fixtures';
import { AppShell } from './page-objects/AppShell';
import { InboxPage } from './page-objects/InboxPage';

test.describe('MB-102.1 App shell', () => {
  test('demo status is always visible and never claims a real sync', async ({ page }) => {
    const shell = new AppShell(page);
    await shell.open('/');
    await expect(shell.demoBadge()).toBeVisible();
    await expect(shell.demoBadge()).toHaveAttribute('data-status', 'demo');
  });

  test('theme toggle switches light/dark tokens', async ({ page }, info) => {
    test.skip(desktopOnly(info.project.name));
    const shell = new AppShell(page);
    await shell.open('/');
    await expect(shell.root()).toHaveAttribute('data-theme', 'light');
    await shell.toggleTheme();
    await expect(shell.root()).toHaveAttribute('data-theme', 'dark');
    await shell.toggleTheme();
    await expect(shell.root()).toHaveAttribute('data-theme', 'light');
  });

  test('sidebar folders and account filter change the list', async ({ page }, info) => {
    test.skip(desktopOnly(info.project.name));
    const shell = new AppShell(page);
    const inbox = new InboxPage(page);
    await shell.open('/');
    await expect(shell.sidebar()).toBeVisible();

    await shell.goFolder('starred');
    await expect(inbox.title()).toHaveText('Thư gắn sao');
    await inbox.expectRowCount(2);

    await shell.goFolder('sent');
    await expect(inbox.emptyState()).toBeVisible();

    await shell.goInbox();
    await shell.goAccount('support');
    await inbox.expectRowCount(2);
    for (const account of await inbox.rows().getByTestId('message-row-account').all()) {
      await expect(account).toHaveAttribute('data-account', 'support');
    }
  });

  test('mobile uses bottom nav and a menu drawer instead of the sidebar', async ({ page }, info) => {
    test.skip(mobileOnly(info.project.name));
    const shell = new AppShell(page);
    await shell.open('/');
    await expect(shell.sidebar()).toBeHidden();
    await expect(shell.bottomNav()).toBeVisible();
    await shell.openMobileMenu();
    await expect(shell.sidebar()).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(shell.sidebar()).toBeHidden();
  });
});
