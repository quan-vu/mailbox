import { defineConfig, devices } from '@playwright/test';

const PORT = Number(process.env.MAILBOX_E2E_PORT ?? 8080);

export default defineConfig({
  testDir: '.',
  testMatch: /.*-real\.spec\.ts/,
  outputDir: '../test-results/e2e',
  timeout: 60_000,
  expect: { timeout: 15_000 },
  fullyParallel: false,
  workers: 1,
  reporter: [['list']],
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    video: 'on',
    screenshot: 'on',
    trace: 'retain-on-failure',
  },
  projects: [
    { name: 'desktop-1440', use: { ...devices['Desktop Chrome'], viewport: { width: 1440, height: 1086 } } },
    { name: 'mobile-375', use: { ...devices['Desktop Chrome'], viewport: { width: 375, height: 812 }, isMobile: false } },
  ],
  // The Web build reads data from the real local demo API (MB-103), so both servers are needed.
  // Starting them by hand first is the most reliable: both entries reuse a running server.
  webServer: [
    {
      command: 'cargo run -p mailbox-api',
      cwd: '..',
      url: 'http://127.0.0.1:3000/healthz',
      env: { PATH: `${process.env.HOME}/.cargo/bin:${process.env.PATH ?? ''}` },
      reuseExistingServer: true,
      timeout: 600_000,
    },
    {
    // Real Dioxus web dev server (Rust/WASM). Started from apps/mailbox-ui.
    command: `dx serve -p mailbox-ui --web --port ${PORT} --addr 127.0.0.1 --open false --interactive false --hot-reload false`,
    cwd: '../apps/mailbox-ui',
    url: `http://127.0.0.1:${PORT}`,
    // rustup proxies must win over a system (e.g. Homebrew) cargo so rust-toolchain.toml applies.
    env: { PATH: `${process.env.HOME}/.cargo/bin:${process.env.PATH ?? ''}` },
    reuseExistingServer: true,
    timeout: 600_000,
  },
  ],
});
