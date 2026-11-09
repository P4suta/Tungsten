import { defineConfig } from "@playwright/test";
import standard from "./playwright.config.js";

export default defineConfig({
  testDir: "scenarios",
  testMatch: "demo.spec.ts",
  workers: 1,
  retries: 0,
  timeout: 30000,
  outputDir: "test-results/demo",
  use: {
    baseURL: process.env.TUNGSTEN_DEMO_URL ?? "http://127.0.0.1:8081",
    trace: "retain-on-failure",
  },
  projects: standard.projects ?? [],
});
