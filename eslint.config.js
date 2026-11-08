import js from "@eslint/js";
import { defineConfig } from "eslint/config";
import prettier from "eslint-config-prettier";
import jsxA11y from "eslint-plugin-jsx-a11y";
import reactHooks from "eslint-plugin-react-hooks";
import globals from "globals";
import tseslint from "typescript-eslint";

const MONEY_HINT = "Money is integer minor units (cents): use the helpers in src/lib/money.ts.";

export default defineConfig(
  { ignores: ["dist/**", "coverage/**", "src-tauri/**", "src/db/bindings.ts"] },
  js.configs.recommended,
  tseslint.configs.strict,
  {
    files: ["**/*.{ts,tsx}"],
    languageOptions: { globals: globals.browser },
    plugins: { "react-hooks": reactHooks, "jsx-a11y": jsxA11y },
    rules: {
      ...reactHooks.configs.recommended.rules,
      ...jsxA11y.flatConfigs.recommended.rules,
      "@typescript-eslint/no-explicit-any": "error",
      "@typescript-eslint/ban-ts-comment": "error",
      "no-console": "error",
      "no-debugger": "error",
      "no-restricted-globals": ["error", { name: "parseFloat", message: MONEY_HINT }],
      "no-restricted-properties": [
        "error",
        { object: "Number", property: "parseFloat", message: MONEY_HINT },
        { property: "toFixed", message: MONEY_HINT },
      ],
      "no-restricted-syntax": [
        "error",
        { selector: "CallExpression[callee.name='Number']", message: MONEY_HINT },
      ],
    },
  },
  {
    files: ["scripts/**/*.mjs", "*.config.{js,ts}"],
    languageOptions: { globals: globals.node },
  },
  prettier,
);
