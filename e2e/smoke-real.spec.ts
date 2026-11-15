import { expect, test } from '@playwright/test';
import { AppShell } from './page-objects/AppShell';

/**
 * MB-101 smoke: the real Dioxus Web build loads and every reference screen
 * is reachable through real UI navigation. Data is demo fixtures; the shell
 * must say so.
 */
test.describe('MB-101 smoke — five screens reachable', () => {
  test('navigate Inbox → Thread → Compose → Connect → Settings', async ({ page }, info) => {
    test.skip(info.project.name !== 'desktop-1440', 'mobile navigation is covered in MB-102');
    const shell = new AppShell(page);
    await shell.open('/');

    await shell.expectScreen('inbox');
    await expect(shell.demoBadge()).toBeVisible();

    await shell.openThreadFromReader();
    await shell.expectScreen('thread');

    await shell.goCompose();
    await shell.expectScreen('compose');

    await shell.goConnect();
    await shell.expectScreen('connect');

    await shell.goSettings();
    await shell.expectScreen('settings');

    await shell.goInbox();
    await shell.expectScreen('inbox');
  });
});
