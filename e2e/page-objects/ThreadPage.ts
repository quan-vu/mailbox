import type { Page } from '@playwright/test';

export class ThreadPage {
  constructor(readonly page: Page) {}

  subject() {
    return this.page.getByTestId('thread-subject');
  }

  count() {
    return this.page.getByTestId('thread-count');
  }

  account() {
    return this.page.getByTestId('thread-account');
  }

  message(id: string) {
    return this.page.getByTestId(`thread-message-${id}`);
  }

  toggle(id: string) {
    return this.page.getByTestId(`thread-toggle-${id}`);
  }

  async reply() {
    await this.page.getByTestId('thread-reply').click();
  }

  async back() {
    await this.page.getByTestId('thread-back').click();
  }
}
