// Locate a Chromium-based browser for the browser tests and benchmarks.
// Set the CHROME environment variable to use a specific executable.
import { existsSync } from "node:fs";

const candidates = {
  win32: [
    "C:/Program Files/Google/Chrome/Application/chrome.exe",
    "C:/Program Files (x86)/Google/Chrome/Application/chrome.exe",
    "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
  ],
  darwin: [
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
  ],
  linux: ["/usr/bin/google-chrome", "/usr/bin/google-chrome-stable", "/usr/bin/chromium", "/usr/bin/chromium-browser", "/usr/bin/microsoft-edge"],
};

export function chromePath() {
  const fromEnv = process.env.CHROME;
  if (fromEnv) return fromEnv;
  const found = (candidates[process.platform] ?? []).find((p) => existsSync(p));
  if (found) return found;
  console.error("No Chrome or Edge found. Install one, or set the CHROME environment variable to its executable.");
  process.exit(1);
}
