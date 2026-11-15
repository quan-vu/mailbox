import type { Page } from '@playwright/test';

export const SETTINGS_TABS = [
  'accounts',
  'sync',
  'appearance',
  'security',
  'signature',
  'notifications',
  'integrations',
  'advanced',
  'about',
] as const;
export type SettingsTab = (typeof SETTINGS_TABS)[number];

export class SettingsPage {
  constructor(readonly page: Page) {}

  tab(key: SettingsTab) {
    return this.page.getByTestId(`settings-tab-${key}`);
  }

  panel(key: SettingsTab) {
    return this.page.getByTestId(`settings-panel-${key}`);
  }

  account(id: string) {
    return this.page.getByTestId(`settings-account-${id}`);
  }

  accountStatus(id: string) {
    return this.page.getByTestId(`settings-account-status-${id}`);
  }

  async detach(id: string) {
    await this.page.getByTestId(`settings-detach-${id}`).click();
  }

  dialog() {
    return this.page.getByTestId('confirm-dialog');
  }

  toggle(key: string) {
    return this.page.getByTestId(`settings-toggle-${key}`);
  }

  theme(key: 'light' | 'dark' | 'system') {
    return this.page.getByTestId(`settings-theme-${key}`);
  }

  async save() {
    await this.page.getByTestId('settings-save').click();
  }
}
