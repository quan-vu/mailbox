import { expect } from '@playwright/test';
import { test } from './fixtures';
import { AppShell } from './page-objects/AppShell';
import { SettingsPage } from './page-objects/SettingsPage';

// MB-105 / MB-221: interface language. This suite runs against the Web demo build, which keeps no
// storage: the choice applies at once and is forgotten on reload (Desktop remembers it; that path
// is covered by the store test `language_is_remembered_and_leaves_other_settings_alone`).
test.describe('MB-105 interface language', () => {
  test('Vietnamese is the default and English applies without a restart', async ({ page }) => {
    const shell = new AppShell(page);
    const settings = new SettingsPage(page);
    await shell.open('/');
    await expect(shell.root()).toHaveAttribute('lang', 'vi');
    await expect(shell.demoBadge()).toContainText('Chế độ demo');

    await shell.goSettings();
    await settings.tab('appearance').click();
    await page.getByTestId('settings-lang-en').click();

    await expect(shell.root()).toHaveAttribute('lang', 'en');
    await expect(page.getByTestId('settings-lang-en')).toHaveAttribute('aria-checked', 'true');
    await expect(page.getByTestId('settings-lang-vi')).toHaveAttribute('aria-checked', 'false');
    // The demo label stays visible, now in English.
    await expect(shell.demoBadge()).toContainText('Demo mode');
    await expect(page.getByTestId('settings-panel-appearance')).toContainText('Language');

    // Other screens follow the language too.
    await shell.goInbox();
    await expect(page.getByTestId('nav-compose')).toContainText('Compose');
    await shell.goConnect();
    await expect(page.getByTestId('screen-connect')).toContainText('Add an email account');
  });

  test('switching back to Vietnamese restores the Vietnamese text', async ({ page }) => {
    const shell = new AppShell(page);
    const settings = new SettingsPage(page);
    await shell.open('/settings/appearance');
    await page.getByTestId('settings-lang-en').click();
    await expect(shell.demoBadge()).toContainText('Demo mode');
    await page.getByTestId('settings-lang-vi').click();
    await expect(shell.root()).toHaveAttribute('lang', 'vi');
    await expect(shell.demoBadge()).toContainText('Chế độ demo');
    await expect(settings.panel('appearance')).toContainText('Ngôn ngữ');
  });

  test('Web demo does not store the choice: a reload shows the default language', async ({ page }) => {
    const shell = new AppShell(page);
    await shell.open('/settings/appearance');
    await page.getByTestId('settings-lang-en').click();
    await expect(shell.root()).toHaveAttribute('lang', 'en');
    await page.reload();
    await expect(shell.root()).toBeVisible({ timeout: 45_000 });
    await expect(shell.root()).toHaveAttribute('lang', 'vi');
  });
});
