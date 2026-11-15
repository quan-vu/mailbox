import type { Page } from '@playwright/test';

export class ConnectPage {
  constructor(readonly page: Page) {}

  provider(key: string) {
    return this.page.getByTestId(`connect-provider-${key}`);
  }

  autodetect() {
    return this.page.getByTestId('connect-autodetect');
  }

  field(key: 'email' | 'password' | 'imap-host' | 'imap-port' | 'smtp-host' | 'smtp-port') {
    return this.page.getByTestId(`connect-${key}`);
  }

  error(key: string) {
    return this.page.getByTestId(`connect-error-${key}`);
  }

  step(n: 1 | 2 | 3) {
    return this.page.getByTestId(`connect-step-${n}`);
  }

  async submit() {
    await this.page.getByTestId('connect-submit').click();
  }

  scenario(key: string) {
    return this.page.getByTestId(`connect-scenario-${key}`);
  }

  result() {
    return this.page.getByTestId('connect-result');
  }

  finish() {
    return this.page.getByTestId('connect-finish');
  }

  done() {
    return this.page.getByTestId('connect-done');
  }
}
