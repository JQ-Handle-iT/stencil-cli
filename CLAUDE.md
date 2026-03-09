# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**Stencil CLI** is a BigCommerce development tool that provides a local server emulator for theme development. Users can develop themes locally with live reload, test theme functionality, and bundle themes for upload to BigCommerce.

- **Language:** JavaScript (Node.js, ESM)
- **Node versions:** 20.x, 22.x
- **Main entry point:** `bin/stencil.js`

## Common Development Commands

```bash
# Install dependencies
npm install

# Linting and formatting
npm run lint                # Check for lint/format issues
npm run lint-and-fix        # Fix lint and format issues

# Testing
npm test                    # Run all tests with Jest
npm run test-with-coverage  # Run tests with coverage report
npm test -- --testNamePattern="pattern"  # Run specific test

# Release (handled by semantic-release)
npm run release             # Creates version bump and publishes to npm
```

## High-Level Architecture

### Directory Structure

```
├── bin/                    # CLI command entry points (thin wrappers)
├── lib/                    # Core implementation classes and utilities
│   ├── css/               # CSS/SCSS processing
│   ├── graphql/           # GraphQL utilities
│   ├── lang/              # Language/region handling
│   ├── nodeSass/          # Sass compilation
│   ├── release/           # Release utilities
│   ├── schemas/           # JSON Schema validators
│   └── *.js               # Command classes, managers, validators
├── server/                # Hapi.js local development server
│   ├── config.js          # Server configuration
│   ├── index.js           # Server setup
│   ├── manifest.js        # Hapi manifest
│   └── plugins/
│       ├── renderer/      # Template rendering logic
│       ├── router/        # HTTP routing
│       └── theme-assets/  # Static asset serving
├── test/                  # Test utilities and fixtures
└── constants.js           # Global constants (API host, theme paths, etc.)
```

### CLI Command Pattern

Each CLI command follows a standardized pattern:

1. **Entry point** (`bin/stencil-*.js`): Thin wrapper that:
   - Parses command-line arguments using `commander.js`
   - Extracts options and creates service instances
   - Calls the main method of a command class

2. **Command class** (`lib/stencil-*.js` or similar): Contains:
   - Constructor with dependency injection (fs, logger, managers, etc.)
   - `run()` method as the main entry point
   - Helper methods for specific tasks
   - Private methods prefixed with underscore (`_methodName`)

Example structure:
```javascript
// bin/stencil-bundle.js
const options = extractOptions(program);
new StencilBundle({ fs, logger }).run(options);

// lib/stencil-bundle.js
class StencilBundle {
  constructor({ fs, logger }) {
    this.fs = fs;
    this.logger = logger;
  }

  run(options) {
    // Main logic, calls helper methods
    this._validateTheme();
    this._bundleFiles();
  }

  _validateTheme() { /* private */ }
  _bundleFiles() { /* private */ }
}
```

### Manager Classes

Key manager classes handle configuration and build state:

- **`BuildConfigManager`**: Manages build-time configuration, timeout, and initialization
- **`StencilConfigManager`**: Loads and manages `.stencil` configuration file
- **`StencilContextAnalyzer`**: Analyzes theme structure and generates context for rendering
- **`StencilCLISettings`**: Stores CLI-wide settings

These are initialized at the start of commands and passed to service classes via dependency injection.

### Server Architecture

The Hapi.js server (`server/`) has three main plugin groups:

- **Router plugin**: Maps HTTP routes to handlers
- **Renderer plugin**: Handles Handlebars template rendering using Stencil Paper
- **Theme Assets plugin**: Serves theme CSS, JS, images, etc. with proper caching and compilation

Plugins only contain controller logic; business logic is moved to lib classes.

### Template Engine

Stencil uses **Handlebars** (via `@bigcommerce/stencil-paper` package) for server-side template rendering. The context object passed to templates contains:

- Theme configuration and settings
- Product/cart/customer data from the BigCommerce API
- Custom regions and layout overrides
- Handlebars helpers for Stencil syntax

The `StencilContextAnalyzer` class is responsible for building this context.

## Key Technologies and Dependencies

- **Hapi.js**: Server framework for local development
- **Commander.js**: CLI argument and option parsing
- **BrowserSync**: Live reload during development
- **Handlebars/Stencil Paper**: Template engine for theme rendering
- **PostCSS/node-sass**: SCSS and CSS processing with Autoprefixer
- **Jest**: Testing framework
- **ESLint/Prettier**: Code quality and formatting
- **Semantic Release**: Automated versioning and npm publishing

## Commit Message Style

Follow **Conventional Commits** format:

```
<type>[optional scope]: <description>

[optional body]

[optional footer(s)]
```

**Types that trigger releases:**
- `fix:` - Bug fix (PATCH)
- `feat:` - New feature (MINOR)
- `perf:` - Performance improvement (MINOR)
- `BREAKING CHANGE:` or `!` after type - Breaking change (MAJOR)

**Types that don't trigger releases:**
- `build:`, `ci:`, `docs:`, `refactor:`, `style:`, `test:`

Example: `fix(renderer): handle missing context variables gracefully`

## Code Style Guide

- **JavaScript standard:** ESLint with Airbnb config (see `.eslintrc`)
- **Async patterns:** Prefer `async/await` over callbacks and promises
- **Dependency injection:** Pass external dependencies (fs, logger) to constructors, not as globals
- **Private methods:** Mark with underscore prefix (`_methodName`) and `@private` JSDoc tag
- **Testing:** Co-locate `.spec.js` files with implementation

## Testing Strategy

- Tests use **Jest** with ESM support (note: `node --experimental-vm-modules` flag in npm script)
- Test files are alongside implementation files (e.g., `StencilBundle.js` and `StencilBundle.spec.js`)
- Mock external dependencies injected via constructor
- Use `describe()` for grouping, `it()` for test cases

Run a single test file:
```bash
npm test -- lib/StencilBundle.spec.js
```

## Debugging

For debugging locally:

**VSCode setup:**
1. Create `.vscode/launch.json` with Node debugger configuration
2. Run: `node --inspect bin/stencil.js start`
3. In VSCode, open Run and Debug, select "Attach" configuration
4. Pick the Node process

**Chrome DevTools:**
1. Run: `node --inspect bin/stencil.js start`
2. Open `localhost:9230` in Chrome
3. Use DevTools to set breakpoints

## Release Process

Releases are automated via **Semantic Release** triggered by merging PRs to master:

1. Merge PR with Conventional Commits message to master
2. Semantic Release automatically:
   - Bumps version in package.json
   - Updates CHANGELOG.md
   - Creates GitHub release
   - Publishes to npm registry

**Important:** PR title must match the commit message structure for correct release notes.

## Important Files to Know

- **`.stencil`** - User config file (created by `stencil init`), contains API credentials and store info
- **`config.json`** - Theme configuration (settings, variations, custom layouts)
- **`.eslintrc`** - Linting rules
- **`.releaserc`** - Semantic Release configuration
- **`jest.config.js`** - Jest test configuration

## Common Patterns

### Adding a New CLI Command

1. Create `lib/StencilNewCommand.js` with class implementing `run(options)` method
2. Create `bin/stencil-new-command.js` that imports the class and calls it
3. Register command in `bin/stencil.js` using `.command()` method
4. Add corresponding npm script if needed for direct execution
5. Add tests in `lib/StencilNewCommand.spec.js`

### Working with the Server

The server is started by the `start` command. To test server changes:

```bash
npm test -- server/  # Run server-related tests
# Then manually test with: node bin/stencil-start.js
```

### Handling Theme Files

- Theme files are read from current working directory (process.cwd())
- Use `StencilContextAnalyzer` to discover and analyze theme structure
- CSS/SCSS files in `assets/scss/` are compiled via PostCSS/node-sass
- Use `bundle-validator.js` to validate theme structure before bundling

