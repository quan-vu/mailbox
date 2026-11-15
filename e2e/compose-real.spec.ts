import { expect } from '@playwright/test';
import { desktopOnly, test } from './fixtures';
import { AppShell } from './page-objects/AppShell';
import { ComposePage } from './page-objects/ComposePage';
import { InboxPage } from './page-objects/InboxPage';

test.describe('MB-102.4 S04 Compose (demo: never sends)', () => {
  test('new draft shows sender identity and demo draft status', async ({ page }) => {
    const shell = new AppShell(page);
    const compose = new ComposePage(page);
    await shell.open('/');
    await shell.goCompose();
    await shell.expectScreen('compose');
    await expect(compose.from()).toHaveValue('work');
    await expect(compose.draftStatus()).toHaveAttribute('data-state', 'local-demo');
  });

  test('recipient chips: add, flag invalid, remove', async ({ page }) => {
    const shell = new AppShell(page);
    const compose = new ComposePage(page);
    await shell.open('/');
    await shell.goCompose();
    await compose.addRecipient('to', 'duc.nguyen@company.vn');
    await compose.addRecipient('to', 'ketoan@company');
    await expect(compose.chips('to')).toHaveCount(2);
    await expect(compose.chips('to').nth(0)).toHaveAttribute('data-valid', 'true');
    await expect(compose.chips('to').nth(1)).toHaveAttribute('data-valid', 'false');
    await expect(compose.recipientError()).toBeVisible();
    await compose.removeChip('to', 1);
    await expect(compose.chips('to')).toHaveCount(1);
    await expect(compose.recipientError()).toBeHidden();
  });

  test('Cc/Bcc are revealed on demand', async ({ page }) => {
    const shell = new AppShell(page);
    const compose = new ComposePage(page);
    await shell.open('/');
    await shell.goCompose();
    await expect(compose.input('cc')).toBeHidden();
    await compose.showCcBcc();
    await compose.addRecipient('cc', 'huong.mai@company.vn');
    await compose.addRecipient('bcc', 'support@company.vn');
    await expect(compose.chips('cc')).toHaveCount(1);
    await expect(compose.chips('bcc')).toHaveCount(1);
  });

  test('templates fill an empty draft and confirm before overwriting', async ({ page }, info) => {
    test.skip(desktopOnly(info.project.name), 'template rail is desktop-only');
    const shell = new AppShell(page);
    const compose = new ComposePage(page);
    await shell.open('/');
    await shell.goCompose();
    await compose.applyTemplate('meeting');
    await expect(compose.subject()).toHaveValue('Mời họp dự án');
    await compose.applyTemplate('report');
    await expect(page.getByTestId('confirm-dialog')).toBeVisible();
    await page.getByTestId('confirm-dialog-ok').click();
    await expect(compose.subject()).toHaveValue('Báo cáo tiến độ');
  });

  test('send is blocked with reasons, then shows the demo dialog (no real send)', async ({ page }) => {
    const shell = new AppShell(page);
    const compose = new ComposePage(page);
    await shell.open('/');
    await shell.goCompose();
    await compose.send();
    await expect(compose.blockers()).toBeVisible();
    await expect(compose.sendDialog()).toBeHidden();
    await compose.addRecipient('to', 'duc.nguyen@company.vn');
    await compose.subject().fill('Cập nhật');
    await compose.send();
    await expect(compose.sendDialog()).toBeVisible();
    await expect(compose.sendDialog()).toHaveAttribute('data-sent', 'false');
    await page.getByTestId('compose-send-dialog-ok').click();
    await expect(compose.sendDialog()).toBeHidden();
    await shell.expectScreen('compose');
  });

  test('reply keeps receiving identity and warns when it is changed', async ({ page }, info) => {
    test.skip(desktopOnly(info.project.name));
    const shell = new AppShell(page);
    const inbox = new InboxPage(page);
    const compose = new ComposePage(page);
    await shell.open('/');
    await inbox.select('m07');
    await inbox.readerAction('reply').click();
    await expect(compose.from()).toHaveValue('support');
    await expect(compose.identityWarning()).toBeHidden();
    await compose.from().selectOption('work');
    await expect(compose.identityWarning()).toBeVisible();
  });

  test('closing a non-empty draft asks for confirmation', async ({ page }) => {
    const shell = new AppShell(page);
    const compose = new ComposePage(page);
    await shell.open('/');
    await shell.goCompose();
    await compose.subject().fill('Bản nháp');
    await compose.close();
    await expect(page.getByTestId('confirm-dialog')).toBeVisible();
    await page.getByTestId('confirm-dialog-cancel').click();
    await shell.expectScreen('compose');
    await compose.close();
    await page.getByTestId('confirm-dialog-ok').click();
    await shell.expectScreen('inbox');
  });
});
