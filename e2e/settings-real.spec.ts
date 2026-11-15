import { expect } from '@playwright/test';
import { desktopOnly, test } from './fixtures';
import { AppShell } from './page-objects/AppShell';
import { SETTINGS_TABS, SettingsPage } from './page-objects/SettingsPage';

test.describe('MB-102.6 S05 Accounts & settings', () => {
  test.beforeEach(async ({ page }) => {
    const shell = new AppShell(page);
    await shell.open('/');
    await shell.goSettings();
    await shell.expectScreen('settings');
  });

  test('accounts tab lists demo accounts without claiming a connection', async ({ page }) => {
    const settings = new SettingsPage(page);
    await expect(settings.panel('accounts')).toBeVisible();
    for (const id of ['work', 'personal', 'support']) {
      await expect(settings.accountStatus(id)).toHaveAttribute('data-status', 'demo');
    }
  });

  test('every settings tab opens its panel', async ({ page }) => {
    const settings = new SettingsPage(page);
    for (const key of SETTINGS_TABS) {
      await settings.tab(key).click();
      await expect(settings.tab(key)).toHaveAttribute('aria-current', 'page');
      await expect(settings.panel(key)).toBeVisible();
    }
  });

  test('detaching an account confirms that server mail is not deleted', async ({ page }, info) => {
    test.skip(desktopOnly(info.project.name));
    const settings = new SettingsPage(page);
    await settings.detach('support');
    await expect(settings.dialog()).toBeVisible();
    await expect(page.getByTestId('confirm-dialog-body')).toContainText('máy chủ');
    await page.getByTestId('confirm-dialog-cancel').click();
    await expect(settings.account('support')).toBeVisible();
    await settings.detach('support');
    await page.getByTestId('confirm-dialog-ok').click();
    await expect(settings.account('support')).toHaveCount(0);
    await expect(new AppShell(page).toast()).toBeVisible();
  });

  test('appearance theme and notification toggles apply in-session', async ({ page }) => {
    const shell = new AppShell(page);
    const settings = new SettingsPage(page);
    await settings.tab('appearance').click();
    await settings.theme('dark').click();
    await expect(shell.root()).toHaveAttribute('data-theme', 'dark');
    await settings.tab('notifications').click();
    const toggle = settings.toggle('desktop');
    await expect(toggle).toHaveAttribute('aria-checked', 'false');
    await toggle.click();
    await expect(toggle).toHaveAttribute('aria-checked', 'true');
    await settings.save();
    await expect(shell.toast()).toBeVisible();
  });

  test('Escape closes the confirmation dialog', async ({ page }, info) => {
    test.skip(desktopOnly(info.project.name));
    const settings = new SettingsPage(page);
    await settings.detach('work');
    await expect(settings.dialog()).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(settings.dialog()).toBeHidden();
  });
});
