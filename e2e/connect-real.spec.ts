import { expect } from '@playwright/test';
import { test } from './fixtures';
import { AppShell } from './page-objects/AppShell';
import { ConnectPage } from './page-objects/ConnectPage';

test.describe('MB-102.5 S02 Connect account (demo: no network, no password)', () => {
  test.beforeEach(async ({ page }, info) => {
    const shell = new AppShell(page);
    await shell.open('/');
    if (info.project.name === 'mobile-375') {
      await shell.openMobileMenu();
    }
    await shell.goConnect();
    await shell.expectScreen('connect');
  });

  test('password field is disabled and step 1 is current', async ({ page }) => {
    const connect = new ConnectPage(page);
    await expect(connect.field('password')).toBeDisabled();
    await expect(connect.step(1)).toHaveAttribute('aria-current', 'step');
  });

  test('submitting an empty form shows field errors', async ({ page }) => {
    const connect = new ConnectPage(page);
    await connect.submit();
    for (const key of ['email', 'imap-host', 'smtp-host']) {
      await expect(connect.error(key)).toBeVisible();
    }
    await expect(connect.step(1)).toHaveAttribute('aria-current', 'step');
  });

  test('cPanel + email domain auto-fills hosts; auto-detect off keeps manual values', async ({ page }) => {
    const connect = new ConnectPage(page);
    await connect.field('email').fill('minh@company.vn');
    await connect.provider('cpanel').click();
    await expect(connect.field('imap-host')).toHaveValue('imap.company.vn');
    await expect(connect.field('smtp-host')).toHaveValue('smtp.company.vn');
    await connect.autodetect().click();
    await connect.field('imap-host').fill('mail.company.vn');
    await connect.provider('google').click();
    await expect(connect.field('imap-host')).toHaveValue('mail.company.vn');
  });

  test('step 2 simulates outcomes; certificate errors cannot be bypassed', async ({ page }) => {
    const connect = new ConnectPage(page);
    await connect.field('email').fill('minh@company.vn');
    await connect.provider('cpanel').click();
    await connect.submit();
    await expect(connect.step(2)).toHaveAttribute('aria-current', 'step');

    await connect.scenario('certificate-error').click();
    await expect(connect.result()).toHaveAttribute('data-tone', 'danger');
    await expect(page.getByTestId('connect-bypass')).toHaveCount(0);
    await expect(connect.finish()).toBeDisabled();

    await connect.scenario('completed').click();
    await expect(connect.finish()).toBeEnabled();
    await connect.finish().click();
    await expect(connect.step(3)).toHaveAttribute('aria-current', 'step');
    await expect(connect.done()).toHaveAttribute('data-saved', 'false');
  });
});
