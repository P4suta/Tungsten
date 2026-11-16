import { randomUUID } from "node:crypto";
import { defineConfig, devices } from "@playwright/test";

const port = Number(process.env.TUNGSTEN_TEST_PORT ?? 18080);
if (!Number.isInteger(port) || port < 1 || port > 65535) {
  throw new Error("TUNGSTEN_TEST_PORT must be an integer from 1 to 65535");
}
const address = `127.0.0.1:${port}`;
const baseURL = `http://${address}`;

export default defineConfig({
  testDir: "tests",
  testMatch: "**/*.spec.ts",
  fullyParallel: false,
  workers: 1,
  retries: 0,
  maxFailures: process.env.CI ? 1 : 0,
  reporter: "list",
  timeout: 30000,
  use: { baseURL, trace: "retain-on-failure" },
  webServer: {
    command: `../target/debug/tungsten --data-dir ../target/browser-notes-${randomUUID()} --listen ${address}`,
    url: `${baseURL}/healthz`,
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
