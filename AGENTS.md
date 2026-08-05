# Repository Guidelines

## Project Overview
Web platform built in V language using a specialized web framework (`veb`). Purpose: Display and manage rich data about collectible assets (Cookies, Pets), functioning as a modular content management system with SSR capabilities. Core function is to provide structured, localized content viewing for users.

## Architecture & Data Flow
The architecture follows an MVC-like pattern adapted for V's module structure and Server-Side Rendering (SSR).

1.  **Request Handling:** Incoming requests hit the application modules (`app/*.v`), which act as controllers/entry points.
2.  **Service Layer:** Modules in `database/` encapsulate all data logic. They handle connection management (`wapp.db`) and define business invariants, abstracting database access from the controllers.
3.  **Data Persistence:** Domain models defined in `database/models/*.v`. Data integrity is enforced using ORM-like annotations (`@[]`) for primary keys (PK), foreign keys (FK), and unique constraints.
4.  **Context Preparation:** Controllers prepare a structured context object, including fetched data and localization metadata (`available_languages`).
5.  **View Rendering:** The `veb` framework renders the view templates (`templates/views/*.html`) by consuming this prepared context, generating final HTML output.

*Data Flow:* Request $\to$ Controller (`app/*`) $\to$ Model Logic $\to$ Context $\to$ View Template $\to$ Output.

## Key Directories
*   `database/models/`: Defines core domain entities (User, Pet, Cookie). **Critical for data structure and invariants.**
*   `database/create.v`: Contains high-level business logic services (e.g., user registration flow). Demonstrates service layer pattern; controllers must call these functions.
*   `app/*.v`: Application entry points (controllers). Orchestrates the request lifecycle: fetching context, calling service methods, and returning a `veb.Result`.
*   `templates/views/`: Presentation templates (`.html`). Consume structured context data.
*   `uno.config.ts`: Central styling configuration for UnoCSS; defines global design tokens and CSS variables.

## Development Commands
| Action | Command | Details |
| :--- | :--- | :--- |
| **Development/Run** | `v -d sqlite_fts5 -d new_veb run .` | Primary command for running the application, using SQLite with FTS5 and a dedicated VEB environment. Must be used until all runtime errors are resolved. |
| **Test (Recommended)** | *(TBD)* | No formal test suite found. Integration testing of API endpoints is required. Unit tests must mock database calls to `database/` modules. |
| **Linting/Formatting** | *(TBD)* | Standard V tooling commands should be added here. The project does not currently expose lint/format scripts in `package.json`. |

## Code Conventions & Common Patterns
*   **Language:** V (V Language). Use type safety and explicit error handling (`veb.Result`).
*   **Error Handling:** Use the `veb.Result` type for all public API functions to manage success/failure paths explicitly. Failure should result in a controlled HTTP response (e.g., `ctx.not_found()`).
*   **State Management:** State is managed via the `Context` object (`mut ctx Context`), which is passed through the entire request lifecycle, ensuring data locality and explicit dependency tracking. Global state reliance must be minimized.
*   **Security:** User passwords MUST use secure hashing (e.g., argon2) during creation functions in service layers. All user input must be treated as untrusted.
*   **Localization (i18n):** Use a dedicated `available_languages` list and map (`veb.tr`) for all displayed text. Context handlers must fetch this list early to populate the view template's language selector.

## Important Files
*   `database/models/*.v`: Source of truth for schema definition, data invariants, and ORM definitions.
*   `app/cookies.v`: Example controller demonstrating how to handle GET (view) vs POST (submission) methods and managing session state (`ctx.error_message`).
*   `templates/cookies.html`: Example view template showing the required integration of language selectors and conditional form rendering based on context variables.

## Runtime/Tooling Preferences
*   **Runtime:** V compiler (`v`) is mandatory.
*   **Database:** SQLite (with FTS5). Persistence layer uses specialized `database/*` modules.
*   **Web Framework:** `veb`. Requires the associated tooling for SSR and routing.
*   **Styling:** UnoCSS utility-first CSS stack managed by `uno.config.ts`.

## Testing & QA
No test suite found. **Action: Implement Unit/Integration Tests.**
*   **Testing Focus:** Test critical API endpoints (e.g., `/api/set-lang`, `/cookies`).
*   **Isolation Requirement:** ALL tests must mock the database connection (`wapp.db`) to prevent external I/O dependencies, ensuring unit test determinism.
*   **Coverage Goal:** Achieve high coverage on business logic validation paths and state transitions (e.g., user login $\to$ role check $\to$ dashboard render).
*   **Accessibility (A11y):** All view templates must be audited against WCAG standards, focusing on semantic HTML and ARIA attributes, especially for dynamic/modal content like the cookie submission modal.