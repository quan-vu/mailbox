import { expect, type Page } from '@playwright/test';

export class InboxPage {
  constructor(readonly page: Page) {}

  title() {
    return this.page.getByTestId('inbox-title');
  }

  rows() {
    return this.page.getByTestId('message-list').locator('[data-testid^="message-row-"][data-row]');
  }

  row(id: string) {
    return this.page.getByTestId(`message-row-${id}`);
  }

  async select(id: string) {
    await this.row(id).click();
  }

  rowStar(id: string) {
    return this.page.getByTestId(`message-row-star-${id}`);
  }

  filter(key: 'unread' | 'attachments' | 'today') {
    return this.page.getByTestId(`filter-${key}`);
  }

  accountFilter() {
    return this.page.getByTestId('filter-account');
  }

  emptyState() {
    return this.page.getByTestId('empty-state');
  }

  reader() {
    return this.page.getByTestId('reader');
  }

  readerSubject() {
    return this.page.getByTestId('reader-subject');
  }

  readerAction(key: 'archive' | 'delete' | 'toggle-unread' | 'star' | 'reply' | 'reply-all' | 'forward' | 'back') {
    return this.page.getByTestId(`reader-${key}`);
  }

  async expectRowCount(n: number) {
    await expect(this.rows()).toHaveCount(n);
  }
}
