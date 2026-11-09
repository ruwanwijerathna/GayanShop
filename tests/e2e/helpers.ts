// Shared WebdriverIO helpers for the acceptance suites: find controls the way a person does (by their
// visible label or name), and log in at the PIN screen.
import { $ } from "@wdio/globals";

/** The input a visible <label> points at. */
export async function field(label: string) {
  const el = await $(`//label[normalize-space()="${label}"]`);
  await el.waitForExist();
  const id = await el.getAttribute("for");
  return $(`[id="${id}"]`);
}

export async function button(text: string) {
  // Buttons and nav links (icon-only below xl, so matched by aria-label too).
  const el = await $(
    `//*[self::button or self::a][normalize-space()="${text}" or @aria-label="${text}"][not(@disabled)]`,
  );
  await el.waitForClickable();
  return el;
}

export async function click(text: string) {
  await (await button(text)).click();
}

export async function type(label: string, value: string) {
  const el = await field(label);
  await el.waitForDisplayed();
  await el.click();
  await el.clearValue();
  await el.setValue(value);
}

export async function seeText(text: string) {
  await $(`//*[contains(normalize-space(), "${text}")]`).waitForExist();
}

export async function login(name: string, pin: string) {
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
