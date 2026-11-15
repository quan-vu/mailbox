import { expect } from '@playwright/test';
import { desktopOnly, mobileOnly, test } from './fixtures';
import { AppShell } from './page-objects/AppShell';
import { InboxPage } from './page-objects/InboxPage';

test.describe('MB-102.2 S01 Unified inbox', () => {
  test.beforeEach(async ({ page }) => {
    await new AppShell(page).open('/');
  });

  test('unified list groups conversations and keeps account tags', async ({ page }) => {
    const inbox = new InboxPage(page);
    await inbox.expectRowCount(9);
    await expect(inbox.row('m01')).toHaveAttribute('data-thread-count', '5');
    await expect(inbox.rows().getByTestId('message-row-account')).toHaveCount(9);
  });

  test('selecting a message opens it in the reader and marks it read', async ({ page }, info) => {
    test.skip(desktopOnly(info.project.name));
    const inbox = new InboxPage(page);
    await expect(inbox.row('m03')).toHaveAttribute('data-unread', 'true');
    await inbox.select('m03');
    await expect(inbox.readerSubject()).toHaveText('Báo cáo tiến độ sprint tuần 42');
    await expect(inbox.row('m03')).toHaveAttribute('data-unread', 'false');
    await expect(inbox.row('m03')).toHaveAttribute('aria-current', 'true');
  });

  test('filter chips narrow the list and can be cleared', async ({ page }) => {
    const inbox = new InboxPage(page);
    await inbox.filter('unread').click();
    await expect(inbox.filter('unread')).toHaveAttribute('aria-pressed', 'true');
    await inbox.expectRowCount(5);
    await inbox.filter('attachments').click();
    await inbox.expectRowCount(3);
    for (const row of await inbox.rows().all()) {
      await expect(row).toHaveAttribute('data-attachments', 'true');
    }
    await inbox.filter('unread').click();
    await expect(inbox.filter('unread')).toHaveAttribute('aria-pressed', 'false');
    await inbox.filter('attachments').click();
    await expect(inbox.filter('attachments')).toHaveAttribute('aria-pressed', 'false');
    await inbox.expectRowCount(9);
    await inbox.filter('today').click();
    await inbox.expectRowCount(6);
  });

  test('account dropdown filter keeps only that account', async ({ page }, info) => {
    test.skip(desktopOnly(info.project.name));
    const inbox = new InboxPage(page);
    await inbox.accountFilter().selectOption('personal');
    await inbox.expectRowCount(2);
  });

  test('search narrows results and shows a helpful empty state', async ({ page }, info) => {
    test.skip(desktopOnly(info.project.name));
    const shell = new AppShell(page);
    const inbox = new InboxPage(page);
    await shell.search('greentech');
    await inbox.expectRowCount(1);
    await expect(inbox.row('m07')).toBeVisible();
    await shell.search('không tồn tại xyz');
    await expect(inbox.emptyState()).toBeVisible();
    await page.getByTestId('empty-state-clear').click();
    await inbox.expectRowCount(9);
  });

  test('star toggle on a row is a pressed-state button', async ({ page }) => {
    const inbox = new InboxPage(page);
    await expect(inbox.rowStar('m02')).toHaveAttribute('aria-pressed', 'false');
    await inbox.rowStar('m02').click();
    await expect(inbox.rowStar('m02')).toHaveAttribute('aria-pressed', 'true');
  });

  test('archive shows an undo toast; undo restores the conversation', async ({ page }, info) => {
    test.skip(desktopOnly(info.project.name));
    const shell = new AppShell(page);
    const inbox = new InboxPage(page);
    await inbox.select('m01');
    await inbox.readerAction('archive').click();
    await expect(inbox.row('m01')).toHaveCount(0);
    await expect(shell.toast()).toBeVisible();
    await page.getByTestId('toast-undo').click();
    await expect(inbox.row('m01')).toBeVisible();
    await expect(shell.toast()).toBeHidden();
  });

  test('delete moves the conversation to Trash', async ({ page }, info) => {
    test.skip(desktopOnly(info.project.name));
    const shell = new AppShell(page);
    const inbox = new InboxPage(page);
    await inbox.select('m02');
    await inbox.readerAction('delete').click();
    await expect(inbox.row('m02')).toHaveCount(0);
    await shell.goFolder('trash');
    await expect(inbox.row('m02')).toBeVisible();
  });

  test('mark unread toggles the row state', async ({ page }, info) => {
    test.skip(desktopOnly(info.project.name));
    const inbox = new InboxPage(page);
    await inbox.select('m06');
    await expect(inbox.row('m06')).toHaveAttribute('data-unread', 'false');
    await inbox.readerAction('toggle-unread').click();
    await expect(inbox.row('m06')).toHaveAttribute('data-unread', 'true');
  });

  test('keyboard: Enter on a focused row opens it', async ({ page }, info) => {
    test.skip(desktopOnly(info.project.name));
    const inbox = new InboxPage(page);
    await inbox.row('m05').focus();
    await page.keyboard.press('Enter');
    await expect(inbox.readerSubject()).toHaveText('Thông báo: Lịch nghỉ cuối năm');
  });

  test('mobile shows one pane at a time with a back button', async ({ page }, info) => {
    test.skip(mobileOnly(info.project.name));
    const inbox = new InboxPage(page);
    await expect(page.getByTestId('message-list')).toBeVisible();
    await expect(inbox.reader()).toBeHidden();
    await inbox.select('m03');
    await expect(inbox.reader()).toBeVisible();
    await expect(page.getByTestId('message-list')).toBeHidden();
    await inbox.readerAction('back').click();
    await expect(page.getByTestId('message-list')).toBeVisible();
    await expect(inbox.reader()).toBeHidden();
  });
});
