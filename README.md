# Gradual

> **Stop the bleeding. Improve codebase health incrementally.**

Gradual is a CLI tool that helps teams adopt strict TypeScript and ESLint rules without halting feature development. It records your existing linting and type errors into a baseline, allowing you to enforce strict rules globally while only failing the build if **new** violations are introduced.

## 📦 Installation

Install Gradual as a development dependency:

```bash
npm install --save-dev @julienguillot/gradual
```

## 🚀 Getting Started

1. **Create a configuration file:**
   Create a `gradual.json` file in the root of your project to configure your linters and targets.

   ```json
   {
     "tsconfig": "./tsconfig.target.json",
     "eslint_config": "./eslint.config.js"
   }
   ```

2. **Initialize your baseline:**
   Run the `init` command to scan your codebase and record all existing violations. 
   ```bash
   npx gradual init
   ```
   *Note: This will generate a baseline file. You should commit this file to your version control system.*

3. **Add a script to your `package.json`:**
   ```json
   {
     "scripts": {
       "gradual:check": "gradual check",
       "gradual:update": "gradual update"
     }
   }
   ```

## 🛠️ Commands

### `gradual check`
**Gate commits and CI pipelines.** Compares the current state of your codebase against the saved baseline. If any *new* findings appear that were not in the baseline, the command exits with code `1`.

### `gradual update`
**Shrink the baseline.** As you fix legacy tech debt, run this command to record the current findings as a new baseline delta event. This prevents regressions by ensuring old errors can never be reintroduced once fixed.

### `gradual init`
**Start fresh.** Initializes the baseline from the current finding state. Only run this when setting up Gradual for the first time.

### `gradual status`
**Track progress.** Shows your current baseline statistics grouped by rule, giving you a clear overview of your remaining technical debt and which rules are violated the most.

## 🔄 Recommended Workflow

To get the most out of Gradual, integrate it into your daily development cycle:

1. **Pre-commit (Husky / lint-staged):**
   Run `gradual check` on pre-commit to ensure developers cannot commit new violations.
2. **Continuous Integration (GitHub Actions):**
   Use `gradual check` as a mandatory status check in your PR pipelines.
3. **Tech Debt Sprints:**
   When a developer fixes legacy errors, have them run `gradual update` and include the baseline changes in their pull request.

## 📄 License

MIT