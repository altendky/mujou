// Capture tutorial screenshots for the mujou docs.
//
// Walks through the app's pipeline stages in both light and dark
// themes, capturing a screenshot at each step.  The system color
// scheme is emulated via Playwright so the theme-toggle button
// always shows the "system" icon (sun-moon), matching what most
// users will see on their first visit.
//
// Prerequisites:
//   - The app must be reachable at the given URL.  Either:
//       * `dx serve` for local development, or
//       * a static HTTP server hosting the `dx bundle` output.
//   - Playwright must be installed (`npx playwright install chromium`).
//
// Usage:
//   node scripts/capture-tutorial-screenshots.js                # defaults (dx serve on :8080)
//   node scripts/capture-tutorial-screenshots.js --url http://localhost:8080/app/
//   node scripts/capture-tutorial-screenshots.js --out path/to/dir

const { chromium } = require("playwright");
const path = require("path");
const fs = require("fs");

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

const defaults = {
  url: "http://localhost:8080",
  out: path.resolve(__dirname, "..", "target", "tutorial"),
  width: 390,
  height: 844,
};

function parseArgs() {
  const args = process.argv.slice(2);
  const config = { ...defaults };
  for (let i = 0; i < args.length; i++) {
    if (args[i] === "--url" && args[i + 1]) {
      config.url = args[++i];
    } else if (args[i] === "--out" && args[i + 1]) {
      config.out = path.resolve(args[++i]);
    } else if (args[i] === "--help" || args[i] === "-h") {
      console.log(
        [
          "Usage: node capture-tutorial-screenshots.js [options]",
          "",
          "Options:",
          `  --url <url>   App URL (default: ${defaults.url})`,
          `  --out <dir>   Output directory (default: ${defaults.out})`,
          "  --help        Show this message",
        ].join("\n"),
      );
      process.exit(0);
    }
  }
  return config;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async function waitForPipelineIdle(page) {
  // After navigation or a parameter change the pipeline worker runs
  // asynchronously.  The processing overlay shows "Processing..." while
  // running, then transitions to "Completed" for 1 second (auto-close
  // delay), then disappears entirely.  We need to wait through the
  // full lifecycle so screenshots never include the overlay.
  //
  // Strategy:
  //   1. Wait for "Processing..." to appear (worker startup).
  //   2. Wait for the Cancel/Done button to disappear (overlay fully closed).
  //   3. Confirm a stage thumbnail loaded (data available).

  // 1. Wait for processing to start.  On a fresh page load the worker
  //    may take a moment to spin up.
  try {
    await page
      .getByText("Processing...")
      .first()
      .waitFor({ state: "visible", timeout: 5_000 });
  } catch {
    // Already idle or completed instantly — fall through.
  }

  // 2. Wait for the overlay to fully close.  After "Processing..."
  //    disappears the dialog shows "Completed" for AUTO_CLOSE_DELAY_MS
  //    (1 s).  The "Cancel" button becomes "Done" on completion — once
  //    *neither* is visible the overlay has been dismissed.
  try {
    await page
      .getByRole("button", { name: "Cancel" })
      .or(page.getByRole("button", { name: "Done" }))
      .waitFor({ state: "hidden", timeout: 60_000 });
  } catch {
    // Already hidden — fall through.
  }

  // 3. Confirm at least one stage thumbnail is loaded.
  await page
    .getByAltText("Original thumbnail")
    .waitFor({ state: "visible", timeout: 30_000 });
}

async function selectStage(page, stageName) {
  await page
    .getByRole("button", { name: `Show ${stageName} stage` })
    .click();
}

async function screenshot(page, dir, slug, theme) {
  const file = path.join(dir, `${slug}-${theme}.png`);
  await page.screenshot({
    path: file,
    type: "png",
    fullPage: true,
    animations: "disabled",
  });
  console.log(`  captured ${path.basename(file)}`);
}

// ---------------------------------------------------------------------------
// Icon extraction
// ---------------------------------------------------------------------------

// Icons referenced in the tutorial text, keyed by filename stem.
// Each entry maps to an aria-label selector used to find the button
// containing the SVG in the app's DOM.
const ICONS = {
  "icon-upload": "Upload image",
  "icon-download": "Export",
  "icon-layers": "Toggle diagnostic overlay",
};

async function extractIcons(page, outDir) {
  for (const [name, ariaLabel] of Object.entries(ICONS)) {
    const inner = await page.evaluate(
      (label) => {
        const svg = document.querySelector(
          `[aria-label="${label}"] svg`,
        );
        return svg ? svg.innerHTML : null;
      },
      ariaLabel,
    );
    if (!inner) {
      console.warn(`  warning: icon "${name}" not found (aria-label="${ariaLabel}")`);
      continue;
    }
    // Strip Dioxus placeholder comments.
    const cleaned = inner.replace(/<!--[\s\S]*?-->/g, "");
    const html =
      `<svg class="inline-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${cleaned}</svg>`;
    const file = path.join(outDir, `${name}.html`);
    fs.writeFileSync(file, html);
    console.log(`  extracted ${name}.html`);
  }
}

// ---------------------------------------------------------------------------
// Step definitions
// ---------------------------------------------------------------------------

// Each step describes:
//   slug     – filename component
//   before   – async function run before the screenshot (click stage, etc.)
//   after    – async function run after each theme's screenshot for this step
//              (used for cleanup like resetting slider values or closing dialogs)
//
// Steps are executed in order.  The `before` callback receives the page.
// Each step is re-executed for each theme; page.reload() at dark-theme start
// resets all state, so no extra guard is needed.

function defineSteps() {
  return [
    {
      slug: "01-landing",
      before: async (page) => {
        await selectStage(page, "Output");
      },
    },
    {
      slug: "02-original",
      before: async (page) => {
        await selectStage(page, "Original");
      },
    },
    {
      slug: "03-edges",
      before: async (page) => {
        await selectStage(page, "Edges");
        await page.getByRole("slider", { name: "Canny Low" }).fill("5");
        await waitForPipelineIdle(page);
      },
      after: async (page) => {
        // Reset Canny Low to default so later steps use standard values.
        await page.getByRole("slider", { name: "Canny Low" }).fill("15");
        await waitForPipelineIdle(page);
      },
    },
    {
      slug: "04-join",
      before: async (page) => {
        await selectStage(page, "Join");
      },
    },
    {
      slug: "05-join-diagnostics",
      before: async (page) => {
        // Diagnostics toggle is independent of stage selection — Join
        // should already be selected from the previous step.
        await page
          .getByRole("button", { name: "Toggle diagnostic overlay" })
          .click();
      },
      after: async (page) => {
        // Turn diagnostics back off.
        await page
          .getByRole("button", { name: "Toggle diagnostic overlay" })
          .click();
      },
    },
    {
      slug: "06-output",
      before: async (page) => {
        await selectStage(page, "Output");
      },
    },
    {
      slug: "07-export",
      before: async (page) => {
        await page.getByRole("button", { name: "Export" }).click();
      },
      after: async (page) => {
        await page.getByRole("button", { name: "Cancel" }).click();
      },
    },
  ];
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main() {
  const config = parseArgs();

  fs.mkdirSync(config.out, { recursive: true });

  const themes = ["light", "dark"];
  const steps = defineSteps();

  const browser = await chromium.launch();
  try {
    // Start in light system theme.  localStorage is clean in a fresh
    // context, so the theme-toggle.js defaults to "system" mode and
    // resolves via prefers-color-scheme.
    const context = await browser.newContext({
      viewport: { width: config.width, height: config.height },
      colorScheme: "light",
    });
    const page = await context.newPage();

    console.log(`navigating to ${config.url}`);
    await page.goto(config.url, { waitUntil: "load" });
    await waitForPipelineIdle(page);
    console.log("pipeline idle\n");

    console.log("--- icons ---");
    await extractIcons(page, config.out);
    console.log("");

    for (const theme of themes) {
      console.log(`--- ${theme} theme ---`);

      if (theme === "dark") {
        // Reload so we start from the same default state (Output stage
        // selected, default slider values, diagnostics off, export closed).
        await page.emulateMedia({ colorScheme: "dark" });
        await page.reload({ waitUntil: "load" });
        await waitForPipelineIdle(page);
      }

      for (const step of steps) {
        await step.before(page);
        // Brief settle for any transitions / re-renders.
        await page.waitForTimeout(200);
        await screenshot(page, config.out, step.slug, theme);
        // Run cleanup immediately while the page is in the right state.
        if (step.after) {
          await step.after(page);
        }
      }
    }

    console.log(
      `\ndone — ${themes.length * steps.length} screenshots in ${config.out}`,
    );
  } finally {
    await browser.close();
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
