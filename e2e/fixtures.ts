import { test as base } from '@playwright/test';

/** Helpers to scope a test to one viewport project. */
export const test = base;
export const desktopOnly = (name: string) => name !== 'desktop-1440';
export const mobileOnly = (name: string) => name !== 'mobile-375';
