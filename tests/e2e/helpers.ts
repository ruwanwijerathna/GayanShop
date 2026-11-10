// Shared WebdriverIO helpers for the acceptance suites: find controls the way a person does (by their
// visible label or name), and sign in on the login form.
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
  // The English sign-in form: username, then password; Enter in Password signs in.
  await type("Username", name);
  await type("Password", pin);
  await click("Sign In");
  await $("a[aria-label='Counter']").waitForExist();
}
