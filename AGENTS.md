# Repository Guidelines

## Project Overview
The repository implements a web platform written in V (V Language) using a specialized web framework called `veb`. The purpose is to display and manage rich data about collectible assets—specifically cookies and pets—serving as a modular content management system with server-side rendering (SSR) capabilities.

## Architecture & Data Flow
- **Overall pattern**: MVC‑like architecture adapted for V.
- **Request lifecycle**:
  1. Incoming HTTP request hits an entry point in `app/` controllers.
  2. Controllers delegate business logic to a service layer located in `database/`.
  3. Data is persisted through domain models defined in `database/models/`.
  4. A structured context (including fetched data and local state) is prepared for rendering.
  5. The view template (`templates/views/*.html`) consumes this context, producing the final HTML response.
- **Key modules**:
  - *Database layer*: entities, repositories, and domain logic.
  - *Controller layer*: request handling and orchestration.
  - *Service layer*: business rules (exposed as functions in `app/` or `database/`).
  - *Presentation layer*: view templates.

## Key Directories
- **`database/models/*.v`**: Persistent domain entities (e.g., `User`, `Pet`, `Cookie`). Defined with ORM‑like annotations (`@[]`) for primary keys, foreign keys, and constraints.
- **`database/`**: Contains database schema definitions and model mappings.
- **`app/`**: Controllers and high‑level request handlers.
- **`templates/views/`**: HTML view templates that reference the context.
- **`libraries/`** (if any): Shared utilities or external libraries.

## Development Commands
- **Build/test**: Not yet configured; V compiler (`v`) handles type checking and compilation. Run application with `v -d sqlite_fts5 -d new_veb run .`
- **Lint / format**: Not implemented at this time.
- **Run development tools** (if any): not specified.

## Code Conventions & Common Patterns
- Use explicit imports rather than implicit paths.
- Follow structured results for error handling (return controlled HTTP responses).
- Avoid unnecessary abstractions; prefer straightforward functions and modules.
- Error handling: return status codes via the result type, not generic exceptions.
- Asynchronous operations use `await` in JavaScript or equivalent in V\'s async primitives.

## Important Files
- **`database/models/*.v`**: Source of truth for schema definition and invariants.
- **`app/cookies.v`**: Example controller handling GET (view) vs POST (submission).
- **`templates/views/cookies.html`**: Entry point for cookie management UI.
- **`README.md`** (if present): Project setup instructions.

## Runtime / Tooling Preferences
- **Runtime**: V compiler mandatory; SQLite with FTS5 as persistence layer.
- **Package manager**: Not specified in repository; assume standard Node/npm/V package resolution.
- **Tooling constraints**: No explicit linting or formatting pipeline yet; future work may introduce these.

## Testing & QA
- Test frameworks: None currently defined; a default test suite will be added after initial development.
- Coverage expectation: High coverage on business logic validation paths and state transitions.
