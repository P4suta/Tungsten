import { randomUUID } from "node:crypto";
import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "tests",
  testMatch: "**/*.spec.ts",
  fullyParallel: false,
  workers: 1,
  retries: 0,
  timeout: 30000,
  use: { baseURL: "http://127.0.0.1:18080", trace: "retain-on-failure" },
  webServer: {
    command: `../target/debug/tungsten --plaintext --data-dir ../target/browser-notes-${randomUUID()} --listen 127.0.0.1:18080`,
    url: "http://127.0.0.1:18080/healthz",
    reuseExistingServer: false,
    timeout: 10000,
  },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
    {
      name: "firefox",
      use: {
        ...devices["Desktop Firefox"],
      },
    },
    { name: "webkit", use: { ...devices["Desktop Safari"] } },
    { name: "mobile-chrome", use: { ...devices["Pixel 7"] } },
    { name: "mobile-safari", use: { ...devices["iPhone 15"] } },
  ],
});
