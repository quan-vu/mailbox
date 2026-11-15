import { expect, type Page } from '@playwright/test';

export type ScreenId = 'inbox' | 'thread' | 'compose' | 'connect' | 'settings';
export type FolderKey = 'starred' | 'sent' | 'drafts' | 'scheduled' | 'archive' | 'trash';

/** Shared MailBox shell: top bar, sidebar (desktop), bottom nav + drawer (mobile). */
export class AppShell {
  constructor(readonly page: Page) {}

  root() {
    return this.page.getByTestId('app-shell');
  }

  async open(path = '/') {
    await this.page.goto(path);
    // First paint waits for the WASM bundle; allow for a busy machine/CI runner.
    await expect(this.root()).toBeVisible({ timeout: 45_000 });
  }

  screen(id: ScreenId) {
    return this.page.getByTestId(`screen-${id}`);
  }

  async expectScreen(id: ScreenId) {
    await expect(this.screen(id)).toBeVisible();
  }

  demoBadge() {
    return this.page.getByTestId('topbar-demo-status');
  }

  sidebar() {
    return this.page.getByTestId('sidebar');
  }

  toast() {
    return this.page.getByTestId('toast');
  }

  async toggleTheme() {
    await this.page.getByTestId('topbar-theme-toggle').click();
  }

  /** True when the viewport uses the mobile layout (< 768px). */
  isMobile() {
    return (this.page.viewportSize()?.width ?? 1440) < 768;
  }

  async goCompose() {
    await this.page.getByTestId(this.isMobile() ? 'fab-compose' : 'nav-compose').click();
  }

  async goInbox() {
    await this.page.getByTestId(this.isMobile() ? 'bottom-nav-inbox' : 'nav-inbox').click();
  }

  async goFolder(folder: FolderKey) {
    await this.page.getByTestId(`nav-folder-${folder}`).click();
  }

  async goAccount(id: string) {
    await this.page.getByTestId(`nav-account-${id}`).click();
  }

  async goConnect() {
    await this.page.getByTestId('nav-add-account').click();
  }

  async goSettings() {
    await this.page.getByTestId('topbar-settings').click();
  }

  async openThreadFromReader() {
    await this.page.getByTestId('reader-open-thread').click();
  }

  async search(text: string) {
    await this.page.getByTestId('global-search-input').fill(text);
  }

  // Mobile
  bottomNav() {
    return this.page.getByTestId('bottom-nav');
  }

  async openMobileMenu() {
    await this.page.getByTestId('mobile-menu-button').click();
  }
}
