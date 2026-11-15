import type { Page } from '@playwright/test';

export type Field = 'to' | 'cc' | 'bcc';

export class ComposePage {
  constructor(readonly page: Page) {}

  from() {
    return this.page.getByTestId('compose-from');
  }

  input(field: Field) {
    return this.page.getByTestId(`compose-${field}-input`);
  }

  chips(field: Field) {
    return this.page.getByTestId(`compose-${field}-field`).getByTestId('compose-chip');
  }

  async addRecipient(field: Field, value: string) {
    await this.input(field).fill(value);
    await this.input(field).press('Enter');
  }

  async removeChip(field: Field, index: number) {
    await this.chips(field).nth(index).getByTestId('compose-chip-remove').click();
  }

  recipientError() {
    return this.page.getByTestId('compose-recipient-error');
  }

  subject() {
    return this.page.getByTestId('compose-subject');
  }

  body() {
    return this.page.getByTestId('compose-body');
  }

  draftStatus() {
    return this.page.getByTestId('compose-draft-status');
  }

  identityWarning() {
    return this.page.getByTestId('compose-identity-warning');
  }

  async showCcBcc() {
    await this.page.getByTestId('compose-ccbcc-toggle').click();
  }

  async applyTemplate(key: string) {
    await this.page.getByTestId(`compose-template-${key}`).click();
  }

  async send() {
    await this.page.getByTestId('compose-send').click();
  }

  blockers() {
    return this.page.getByTestId('compose-send-blockers');
  }

  sendDialog() {
    return this.page.getByTestId('compose-send-dialog');
  }

  async close() {
    await this.page.getByTestId('compose-close').click();
  }
}
