// Block 1.12 — Phase 1 acceptance (doc §8 Phase 1 exit), scripted against the real app:
// first run → staff → a product → open the drawer → a cashier sells offline by keyboard only (F2 quick
// sales, F3 search, F4 phone, F9 pay with change), a 5% bill discount goes through, a 6% one asks a
// supervisor, a void asks a supervisor, and the Z-report closes the session (after which nothing can be sold
// or voided). Rules that have no UI path (negative payments, the SQL CHECKs) are proven by the Rust suites.
import { $, browser, expect } from "@wdio/globals";
import { Key } from "webdriverio";

const OWNER_PIN = "246810";
const CASHIER_PIN = "1357";
const SUP_PIN = "8642";

/** The input a visible <label> points at. */
async function field(label: string) {
  const el = await $(`//label[normalize-space()="${label}"]`);
  await el.waitForExist();
  const id = await el.getAttribute("for");
  return $(`[id="${id}"]`);
}

async function button(text: string) {
  // Buttons and nav links (icon-only below xl, so matched by aria-label too).
  const el = await $(
    `//*[self::button or self::a][normalize-space()="${text}" or @aria-label="${text}"][not(@disabled)]`,
  );
  await el.waitForClickable();
  return el;
}

async function click(text: string) {
  await (await button(text)).click();
}

async function type(label: string, value: string) {
  const el = await field(label);
  await el.waitForDisplayed();
  await el.click();
  await el.clearValue();
  await el.setValue(value);
}

/**
 * Type at a person's pace, then Enter. WebDriver types faster than 30 ms per key, which the till rightly
 * treats as a barcode scanner burst (doc §7.1) when Enter follows at once.
 */
async function typeThenEnter(text: string) {
  await browser.keys(text);
  await browser.pause(120);
  await browser.keys(Key.Enter);
}

async function pressKeys(...keys: string[]) {
  for (const k of keys) await browser.keys(k);
}

async function seeText(text: string) {
  await $(`//*[contains(normalize-space(), "${text}")]`).waitForExist();
}

async function login(name: string, pin: string) {
  // Login tiles show the name and the role underneath.
  const tile = await $(`//button[.//span[normalize-space()="${name}"]]`);
  await tile.waitForClickable();
  await tile.click();
  const pinField = await $("input[inputmode='numeric']");
  await pinField.waitForDisplayed();
  await pinField.setValue(pin);
  await click("Log in");
  await $("a[aria-label='Counter']").waitForExist();
}

async function lock() {
  await browser.keys([Key.Ctrl, "l"]);
  await seeText("Who is working?");
}

async function approve(as: string, pin: string) {
  await seeText("Supervisor approval");
  await click(as);
  for (const d of pin) await click(`Digit ${d}`);
  await click("Approve");
}

async function grandTotal(): Promise<string> {
  return $("[data-testid='grand-total']").getText();
}

/** Back to the scan field and wait for the last sale to clear. */
async function saleDone() {
  await seeText("Sale complete");
  await browser.waitUntil(async () => (await grandTotal()) === "Rs 0.00");
}

describe("Phase 1 acceptance: an offline sale day", () => {
  it("sets up the counter", async () => {
    await click("English");
    await click("Continue");
    await type("Store name", "GayanShop");
    await type("Counter code", "T01");
    await click("Continue");
    await type("Owner's name", "Gayan");
    await type("Owner PIN (6–8 digits)", OWNER_PIN);
    await type("Confirm PIN", OWNER_PIN);
    await click("Finish setup");
    await seeText("Who is working?");
  });

  it("the Owner adds staff, a product and opens the drawer", async () => {
    await login("Gayan", OWNER_PIN);

    await click("Staff");
    for (const [name, role, pin] of [
      ["Nimal", "Cashier", CASHIER_PIN],
      ["Kamal", "Supervisor", SUP_PIN],
    ] as const) {
      await click("Add staff");
      await type("Name", name);
      await (await field("Role")).selectByVisibleText(role);
      await type("PIN (4–8 digits)", pin);
      await click("Add");
      await $(`//td[normalize-space()="${name}"]`).waitForExist();
    }

    await click("Products");
    await click("Add product");
    await type("Name (English, printed on receipts and labels)", "Blue Bucket");
    await type("Selling price (Rs)", "450");
    await click("Save");
    await $(`//*[normalize-space()="Blue Bucket"]`).waitForExist();

    await click("Cash");
    await type("Opening float (Rs)", "5000");
    await click("Open session");
    await seeText("Session open");
    await lock();
  });

  it("a cashier sells by keyboard only: quick sales, search, phone and cash with change", async () => {
    await login("Nimal", CASHIER_PIN);
    await click("Till");
    await $("#scan-field").waitForDisplayed();
    // The till never scrolls (doc §1.9): PAY is on screen even with the printer banner showing.
    const pay = await $("//button[starts-with(normalize-space(), 'PAY')]");
    expect(await pay.isDisplayed({ withinViewport: true })).toBe(true);

    for (let i = 1; i <= 5; i++) {
      await pressKeys(Key.F2);
      await typeThenEnter(`${i}* Plastic Basin 10L 300`);
      await browser.waitUntil(async () => (await grandTotal()) !== "Rs 0.00");
      await pressKeys(Key.F3);
      await browser.keys("Blue Bucket");
      await $("[role='option']").waitForExist();
      await browser.pause(120);
      await browser.keys(Key.Enter);
      if (i === 1) {
        await pressKeys(Key.F4);
        await browser.keys("0771234567");
        await seeText("New customer");
        await browser.pause(120);
        await browser.keys(Key.Enter);
      }
      const total = await grandTotal();
      expect(total).toBe(`Rs ${(300 * i + 450).toLocaleString("en-US")}.00`);
      await pressKeys(Key.F9);
      const cash = await field("Cash · Cash received (Rs)");
      await cash.waitForDisplayed();
      await cash.setValue("5000");
      await seeText("Change");
      await browser.pause(120);
      await browser.keys(Key.Enter);
      await saleDone();
    }
  });

  it("a 5% bill discount goes through; 6% asks a supervisor", async () => {
    await pressKeys(Key.F3);
    await browser.keys("Blue Bucket");
    await $("[role='option']").waitForExist();
    await browser.pause(120);
    await browser.keys(Key.Enter);
    await pressKeys(Key.F7);
    await type("Discount (Rs or %)", "5%");
    await click("Apply");
    expect(await grandTotal()).toBe("Rs 427.50");
    await pressKeys(Key.F9);
    await click("Complete sale");
    await saleDone();

    await pressKeys(Key.F3);
    await browser.keys("Blue Bucket");
    await $("[role='option']").waitForExist();
    await browser.pause(120);
    await browser.keys(Key.Enter);
    await pressKeys(Key.F7);
    await type("Discount (Rs or %)", "6%");
    await click("Apply");
    await seeText("Above your discount limit");
    await pressKeys(Key.F9);
    await click("Complete sale");
    await approve("Kamal", SUP_PIN);
    await saleDone();
  });

  it("a cashier's void asks a supervisor and hands the cash back", async () => {
    await pressKeys(Key.F10);
    const first = await $("//ul//button[not(@disabled)]");
    await first.waitForClickable();
    await first.click();
    await click("Void sale");
    await approve("Kamal", SUP_PIN);
    await seeText("voided");
    await lock();
  });

  it("closing the session ends selling and voiding on this counter", async () => {
    await login("Gayan", OWNER_PIN);
    await click("Cash");
    await click("Close session");
    await type("Counted cash (Rs)", "8000");
    await click("Close and print Z-report");
    await click("Yes, close");
    await seeText("Z-report");
    await click("Till");
    await seeText("Open a cash session before selling.");
  });
});
