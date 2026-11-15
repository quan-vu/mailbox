import { expect } from '@playwright/test';
import { test } from './fixtures';
import { AppShell } from './page-objects/AppShell';
import { ComposePage } from './page-objects/ComposePage';
import { InboxPage } from './page-objects/InboxPage';
import { ThreadPage } from './page-objects/ThreadPage';

test.describe('MB-102.3 S03 Conversation', () => {
  test.beforeEach(async ({ page }) => {
    const shell = new AppShell(page);
    await shell.open('/');
    await new InboxPage(page).select('m01');
    await shell.openThreadFromReader();
    await shell.expectScreen('thread');
  });

  test('shows the whole conversation with its receiving account', async ({ page }) => {
    const thread = new ThreadPage(page);
    await expect(thread.subject()).toHaveText('Kế hoạch ra mắt phiên bản 2.0');
    await expect(thread.count()).toHaveAttribute('data-count', '5');
    await expect(thread.account()).toHaveAttribute('data-account', 'work');
    for (const id of ['m01', 'm10', 'm11', 'm12', 'm13']) {
      await expect(thread.message(id)).toHaveAttribute('data-account', 'work');
    }
  });

  test('first message is expanded; others collapse and expand on demand', async ({ page }) => {
    const thread = new ThreadPage(page);
    await expect(thread.toggle('m01')).toHaveAttribute('aria-expanded', 'true');
    await expect(thread.toggle('m10')).toHaveAttribute('aria-expanded', 'false');
    await thread.toggle('m10').click();
    await expect(thread.toggle('m10')).toHaveAttribute('aria-expanded', 'true');
    await expect(page.getByTestId('thread-body-m10')).toBeVisible();
    await thread.toggle('m10').click();
    await expect(page.getByTestId('thread-body-m10')).toBeHidden();
  });

  test('reply opens the composer from the receiving account', async ({ page }) => {
    const thread = new ThreadPage(page);
    const compose = new ComposePage(page);
    await thread.reply();
    await new AppShell(page).expectScreen('compose');
    await expect(compose.from()).toHaveValue('work');
    await expect(compose.chips('to').first()).toHaveAttribute('data-address', 'linh.tran@company.vn');
    await expect(compose.subject()).toHaveValue('Re: Kế hoạch ra mắt phiên bản 2.0');
  });

  test('back returns to the inbox', async ({ page }) => {
    await new ThreadPage(page).back();
    await new AppShell(page).expectScreen('inbox');
  });
});
