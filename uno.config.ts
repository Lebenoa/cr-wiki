import { defineConfig, presetAttributify, presetWind4 } from "unocss";

export default defineConfig({
    cli: {
        entry: {
            patterns: ["./**/*.html", "./app/app.v"],
            outFile: "./static/styles.css",
            rewrite: true,
        },
    },
    presets: [presetWind4(), presetAttributify()],
    preflights: [
        {
            getCSS: () => `:root {
					--font-headline:
						"Sora",
						"Noto Sans",
						"Noto Sans Thai",
						sans-serif;

					--font-body:
						"Hanken Grotesk",
						"Noto Sans",
						"Noto Sans Thai",
						sans-serif;

					--font-label:
						"JetBrains Mono",
						"Noto Sans Mono",
						monospace;

					--global-font: "Noto Sans Thai", sans-serif;
				}
				* {
					font-family: var(--global-font)
				}
				@view-transition {
					navigation: auto;
				}
				html {
					width: 100%;
					height: 100%;
					display: table;
					scrollbar-gutter: stable both-edges;

					--background: 0.1874 0.0124 300.42;
					--surface: var(--background);
					--border: 0.35 0.01 260;
					--primary: 0.5808 0.1298 294.32;
					--secondary: 0.5744 0.0557 297.35;
					--accent: 0.7413 0.1156 88.58;
					--muted: 0.30 0.01 260;
					--foreground: 0.97 0 0;
					--foreground-muted: 0.70 0.01 260;
					--success: 0.78 0.16 145;
					--warning: 0.86 0.16 85;
					--error: 0.70 0.22 25;
				}
				body {
					width: 100%;
					display: table-cell;
					background-color: oklch(var(--background, 0.99 0 0));
					color: oklch(var(--foreground, 0.15 0 0));
					transition: background-color 0.3s ease, color 0.3s ease;
				}
				html, body {
					margin: 0;
					padding: 0;
				}
				button {
					padding: .5rem 1rem;
					cursor: pointer;
				}
				div[popover] {
					position-area: bottom center;
					opacity: 0;
					translate: 0 -10px;
					&:popover-open {
						opacity: 1;
						translate: 0 0;
						@starting-style {
							opacity: 0;
							translate: 0 -10px;
						}
					}
				}`,
        },
    ],
    shortcuts: {
        btn: "inline-flex items-center justify-center px-4 py-2 rounded transition-colors duration-300",
        "btn-primary": "btn bg-primary text-foreground hover:bg-primary/90",
        "btn-secondary": "btn bg-secondary text-foreground hover:bg-secondary/90",
        "btn-success": "btn bg-success text-foreground hover:bg-success/90",
        "btn-warning": "btn bg-warning text-background hover:bg-warning/90",
        "btn-error": "btn bg-error text-foreground hover:bg-error/90",
        "index-card":
            "p-6 rounded-xl flex flex-col gap-4 cursor-pointer h-full bg-surface/25 backdrop-blur-md hover:-translate-y-1 hover:border-primary transition-all border border-accent/30",
    },
    theme: {
        colors: {
            // Semantic colors
            background: "oklch(var(--background, 0.99 0 0))",
            surface: "oklch(var(--surface, 0.97 0 0))",
            border: "oklch(var(--border, 0.88 0.01 260))",

            // Brand colors
            primary: "oklch(var(--primary, 0.62 0.22 260))",
            secondary: "oklch(var(--secondary, 0.72 0.12 220))",
            accent: "oklch(var(--accent, 0.70 0.18 330))",
            muted: "oklch(var(--muted, 0.92 0.01 260))",

            foreground: "oklch(var(--foreground, 0.15 0 0))",
            "foreground-muted": "oklch(var(--foreground-muted, 0.4 0 0))",

            // Status colors
            success: "oklch(var(--success, 0.72 0.18 145))",
            warning: "oklch(var(--warning, 0.82 0.17 85))",
            error: "oklch(var(--error, 0.62 0.24 25))",
        },
        radius: {
            none: "0",
            sm: "0.25rem",
            DEFAULT: "0.5rem",
            md: "0.75rem",
            lg: "1rem",
            xl: "1.25rem",
            full: "9999px",
        },
        font: {
            headline: "var(--font-headline)",
            body: "var(--font-body)",
            label: "var(--font-label)",
        },
    },
});
