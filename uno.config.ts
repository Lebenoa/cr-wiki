import { defineConfig, presetAttributify, presetWind4 } from "unocss";

export default defineConfig({
    cli: {
        entry: {
            patterns: ["./**/*.html", "./app/*.v"],
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
					overflow-y: auto;
					scrollbar-gutter: stable;

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
						--on-primary: 0.9612 0.0000 89.88;
						--on-secondary: 0.9612 0.0000 89.88;
						--on-accent: 0.2090 0.0000 89.88;
						--on-success: 0.2090 0.0000 89.88;
						--on-warning: 0.2090 0.0000 89.88;
						--on-error: 0.2090 0.0000 89.88;
						--on-surface: 0.9612 0.0000 89.88;
						--on-muted: 0.9612 0.0000 89.88;
				}
					html[data-theme="light"] {
						--background: 0.9851 0.0000 89.88;
						--border: 0.8711 0.0055 286.29;
						--primary: 0.4907 0.2412 292.58;
						--secondary: 0.6056 0.2189 292.72;
						--accent: 0.6482 0.1754 131.68;
						--muted: 0.9197 0.0040 286.32;
						--foreground: 0.2542 0.0111 254.04;
						--foreground-muted: 0.4419 0.0146 285.79;
						--success: 0.6271 0.1699 149.21;
						--warning: 0.6658 0.1574 58.32;
						--error: 0.5771 0.2152 27.33;
						--on-primary: 0.9612 0.0000 89.88;
						--on-secondary: 0.2090 0.0000 89.88;
						--on-accent: 0.2090 0.0000 89.88;
						--on-success: 0.2090 0.0000 89.88;
						--on-warning: 0.2090 0.0000 89.88;
						--on-error: 0.9612 0.0000 89.88;
						--on-surface: 0.2090 0.0000 89.88;
						--on-muted: 0.2090 0.0000 89.88;
					}
					html[data-theme="tokyo_night"] {
						--background: 0.2263 0.0214 280.49;
						--border: 0.3867 0.0537 273.88;
						--primary: 0.7190 0.1322 264.20;
						--secondary: 0.7515 0.1344 299.50;
						--accent: 0.7953 0.1395 130.14;
						--muted: 0.4955 0.0682 274.37;
						--foreground: 0.8456 0.0611 274.76;
						--foreground-muted: 0.7666 0.0537 275.49;
						--success: 0.7953 0.1395 130.14;
						--warning: 0.7839 0.1057 75.43;
						--error: 0.7227 0.1589 10.28;
						--on-primary: 0.2090 0.0000 89.88;
						--on-secondary: 0.2090 0.0000 89.88;
						--on-accent: 0.2090 0.0000 89.88;
						--on-success: 0.2090 0.0000 89.88;
						--on-warning: 0.2090 0.0000 89.88;
						--on-error: 0.2090 0.0000 89.88;
						--on-surface: 0.9612 0.0000 89.88;
						--on-muted: 0.9612 0.0000 89.88;
					}
					html[data-theme="cappuccino"] {
						--background: 0.2997 0.0335 62.19;
						--border: 0.3877 0.0420 66.06;
						--primary: 0.7458 0.1097 70.03;
						--secondary: 0.6262 0.0929 62.54;
						--accent: 0.8049 0.0952 75.67;
						--muted: 0.4579 0.0513 67.06;
						--foreground: 0.9182 0.0330 79.27;
						--foreground-muted: 0.7776 0.0492 76.32;
						--success: 0.7456 0.0999 116.93;
						--warning: 0.7707 0.1145 73.92;
						--error: 0.6596 0.1068 41.66;
						--on-primary: 0.2090 0.0000 89.88;
						--on-secondary: 0.2090 0.0000 89.88;
						--on-accent: 0.2090 0.0000 89.88;
						--on-success: 0.2090 0.0000 89.88;
						--on-warning: 0.2090 0.0000 89.88;
						--on-error: 0.2090 0.0000 89.88;
						--on-surface: 0.9612 0.0000 89.88;
						--on-muted: 0.9612 0.0000 89.88;
					}
					html[data-theme="dracula"] {
						--background: 0.2882 0.0221 277.51;
						--border: 0.4028 0.0322 277.83;
						--primary: 0.7420 0.1485 301.88;
						--secondary: 0.5598 0.0803 270.09;
						--accent: 0.8710 0.2195 148.02;
						--muted: 0.4028 0.0322 277.83;
						--foreground: 0.9775 0.0079 106.55;
						--foreground-muted: 0.7254 0.0231 280.13;
						--success: 0.8710 0.2195 148.02;
						--warning: 0.9553 0.1342 112.76;
						--error: 0.6822 0.2063 24.43;
						--on-primary: 0.2090 0.0000 89.88;
						--on-secondary: 0.9612 0.0000 89.88;
						--on-accent: 0.2090 0.0000 89.88;
						--on-success: 0.2090 0.0000 89.88;
						--on-warning: 0.2090 0.0000 89.88;
						--on-error: 0.2090 0.0000 89.88;
						--on-surface: 0.9612 0.0000 89.88;
						--on-muted: 0.9612 0.0000 89.88;
					}
					html[data-theme="nord"] {
						--background: 0.3244 0.0229 264.18;
						--border: 0.3792 0.0290 266.47;
						--primary: 0.7746 0.0622 217.47;
						--secondary: 0.6965 0.0591 248.69;
						--accent: 0.7683 0.0749 131.06;
						--muted: 0.4157 0.0324 264.13;
						--foreground: 0.8993 0.0164 262.75;
						--foreground-muted: 0.7184 0.0264 257.66;
						--success: 0.7683 0.0749 131.06;
						--warning: 0.8549 0.0892 84.09;
						--error: 0.6061 0.1206 15.34;
						--on-primary: 0.2090 0.0000 89.88;
						--on-secondary: 0.2090 0.0000 89.88;
						--on-accent: 0.2090 0.0000 89.88;
						--on-success: 0.2090 0.0000 89.88;
						--on-warning: 0.2090 0.0000 89.88;
						--on-error: 0.2090 0.0000 89.88;
						--on-surface: 0.9612 0.0000 89.88;
						--on-muted: 0.9612 0.0000 89.88;
					}
					html[data-theme="gruvbox"] {
						--background: 0.2768 0.0000 89.88;
						--border: 0.3441 0.0066 48.52;
						--primary: 0.6927 0.0420 169.77;
						--secondary: 0.7054 0.0976 2.19;
						--accent: 0.7652 0.1581 110.83;
						--muted: 0.4110 0.0115 51.87;
						--foreground: 0.8941 0.0566 89.24;
						--foreground-muted: 0.6903 0.0346 76.31;
						--success: 0.7652 0.1581 110.83;
						--warning: 0.8325 0.1595 82.99;
						--error: 0.6597 0.2175 30.39;
						--on-primary: 0.2090 0.0000 89.88;
						--on-secondary: 0.2090 0.0000 89.88;
						--on-accent: 0.2090 0.0000 89.88;
						--on-success: 0.2090 0.0000 89.88;
						--on-warning: 0.2090 0.0000 89.88;
						--on-error: 0.2090 0.0000 89.88;
						--on-surface: 0.9612 0.0000 89.88;
						--on-muted: 0.9612 0.0000 89.88;
					}						html[data-theme="rose_pine"] {
							--background: 0.2134 0.0255 291.13;
							--surface: 0.2413 0.0322 289.14;
							--border: 0.3717 0.0358 291.10;
							--primary: 0.7760 0.0945 304.99;
							--secondary: 0.8363 0.0544 21.14;
							--accent: 0.8219 0.0543 209.56;
							--muted: 0.2708 0.0423 289.23;
							--foreground: 0.9088 0.0299 289.97;
							--foreground-muted: 0.6539 0.0444 291.23;
							--success: 0.5277 0.0793 227.72;
							--warning: 0.8429 0.1100 74.60;
							--error: 0.6977 0.1565 4.22;
							--on-primary: 0.2090 0.0000 89.88;
							--on-secondary: 0.2090 0.0000 89.88;
							--on-accent: 0.2090 0.0000 89.88;
							--on-success: 0.9612 0.0000 89.88;
							--on-warning: 0.2090 0.0000 89.88;
							--on-error: 0.2090 0.0000 89.88;
							--on-surface: 0.9612 0.0000 89.88;
							--on-muted: 0.9612 0.0000 89.88;
						}
					.theme-swatch[data-theme="default"] {
						background: conic-gradient(from 0deg, oklch(0.5808 0.1298 294.32), oklch(0.7413 0.1156 88.58), oklch(0.1874 0.0124 300.42), oklch(0.5808 0.1298 294.32));
					}
					.theme-swatch[data-theme="light"] {
						background: conic-gradient(from 0deg, oklch(0.4907 0.2412 292.58), oklch(0.6482 0.1754 131.68), oklch(0.9851 0.0000 89.88), oklch(0.4907 0.2412 292.58));
					}
					.theme-swatch[data-theme="tokyo_night"] {
						background: conic-gradient(from 0deg, oklch(0.7190 0.1322 264.20), oklch(0.7953 0.1395 130.14), oklch(0.2263 0.0214 280.49), oklch(0.7190 0.1322 264.20));
					}
					.theme-swatch[data-theme="cappuccino"] {
						background: conic-gradient(from 0deg, oklch(0.7458 0.1097 70.03), oklch(0.8049 0.0952 75.67), oklch(0.2997 0.0335 62.19), oklch(0.7458 0.1097 70.03));
					}
					.theme-swatch[data-theme="dracula"] {
						background: conic-gradient(from 0deg, oklch(0.7420 0.1485 301.88), oklch(0.8710 0.2195 148.02), oklch(0.2882 0.0221 277.51), oklch(0.7420 0.1485 301.88));
					}
					.theme-swatch[data-theme="nord"] {
						background: conic-gradient(from 0deg, oklch(0.7746 0.0622 217.47), oklch(0.7683 0.0749 131.06), oklch(0.3244 0.0229 264.18), oklch(0.7746 0.0622 217.47));
					}
					.theme-swatch[data-theme="gruvbox"] {
						background: conic-gradient(from 0deg, oklch(0.6927 0.0420 169.77), oklch(0.7652 0.1581 110.83), oklch(0.2768 0.0000 89.88), oklch(0.6927 0.0420 169.77));
					}
					.theme-swatch[data-theme="rose_pine"] {
						background: conic-gradient(from 0deg, oklch(0.7760 0.0945 304.99), oklch(0.8219 0.0543 209.56), oklch(0.2134 0.0255 291.13), oklch(0.7760 0.0945 304.99));
					}
				body {
					width: 100%;
					display: table-cell;
					max-height: 100vh;
					background-color: oklch(var(--background, 0.99 0 0));
					color: oklch(var(--foreground, 0.15 0 0));
					transition: background-color 0.3s ease, color 0.3s ease;
				}					html, body {
						margin: 0;
						padding: 0;
					}
					button {
						padding: .5rem 1rem;
						cursor: pointer;
					}
					@keyframes combo-in {
						from { opacity: 0; translate: 0 -6px; }
						to { opacity: 1; translate: 0 0; }
					}
				/* /gacha pool tabs: the open card is styled from its ARIA state,
				   not a JS-toggled class — UnoCSS only scans templates and
				   app/*.v, so a class that lives solely in static/js never gets
				   generated. */
				[data-gacha-tab] {
					transition: box-shadow 0.2s ease, border-color 0.2s ease, translate 0.2s ease;
				}
				[data-gacha-tab]:hover {
					translate: 0 -2px;
				}
				[data-gacha-tab][aria-selected="true"] {
					border-color: oklch(var(--primary, 0.62 0.22 260));
					box-shadow: 0 0 0 2px oklch(var(--primary, 0.62 0.22 260) / 0.45);
				}
				/* the panel is toggled by the hidden attribute, which cannot be
				   transitioned — so the reveal is a keyframe that runs when the
				   element starts rendering again. Closing stays instant. */
				@keyframes gacha-panel-in {
					from { opacity: 0; translate: 0 -8px; }
					to { opacity: 1; translate: 0 0; }
				}
				[role="tabpanel"]:not([hidden]) {
					animation: gacha-panel-in 0.22s ease-out both;
				}
				/* the prize cards trail in behind the panel, capped so a 138-card
				   pool does not stagger for seconds */
				[role="tabpanel"]:not([hidden])[data-revealing] [data-cr-card] {
					animation: gacha-panel-in 0.24s ease-out both;
					animation-delay: calc(min(var(--card-index, 0), 12) * 18ms);
				}
				@media (prefers-reduced-motion: reduce) {
					[data-gacha-tab],
					[role="tabpanel"]:not([hidden]),
					[role="tabpanel"]:not([hidden])[data-revealing] [data-cr-card] {
						animation: none;
						transition: none;
					}
					[data-gacha-tab]:hover {
						translate: none;
					}
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
				}
				/* Without the Popover API the trigger button is inert, which
				   would strand every wiki section behind a dead control — they
				   have no other navbar entry. Fall back to rendering the
				   dropdown inline, always open. */
				@supports not selector(:popover-open) {
					#wiki-dropdown,
					#wiki-dropdown-mobile {
						display: flex;
						position: static;
						opacity: 1;
						translate: none;
						margin-block-start: 0;
						border: 0;
						padding: 0;
						background: transparent;
					}
					#wiki-trigger,
					#wiki-trigger-mobile {
						display: none;
					}
				}
				#theme-trigger {
					anchor-name: --theme-trigger;
				}
				#theme-popover {
					position-anchor: --theme-trigger;
					position-area: left center;
				}
				.level-slider {
					--fill: 100%;
					accent-color: oklch(var(--primary, 0.62 0.22 260));
				}
				.level-slider::-webkit-slider-runnable-track {
					height: 0.375rem;
					border-radius: 9999px;
					background-image: linear-gradient(to right,
						oklch(var(--primary)) 0%,
						oklch(var(--primary)) var(--fill, 100%),
						oklch(var(--secondary)) var(--fill, 100%));
				}
				.level-slider::-moz-range-track {
					height: 0.375rem;
					border-radius: 9999px;
					background-image: linear-gradient(to right,
						oklch(var(--primary)) 0%,
						oklch(var(--primary)) var(--fill, 100%),
						oklch(var(--secondary)) var(--fill, 100%));
				}
				.level-slider::-webkit-slider-thumb {
					-webkit-appearance: none;
					appearance: none;
					width: 1rem;
					height: 1rem;
					border-radius: 9999px;
					background-color: oklch(var(--primary));
					border: 2px solid oklch(var(--surface, 0.97 0 0));
					box-shadow: 0 4px 6px -1px rgb(0 0 0 / 0.3);
					cursor: pointer;
					transition: all 0.2s ease;
				}
				.level-slider::-webkit-slider-thumb:hover {
					background-color: oklch(var(--accent));
				}
				.level-slider::-webkit-slider-thumb:active {
					transform: scale(1.1);
				}
				.level-slider::-moz-range-thumb {
					width: 1rem;
					height: 1rem;
					border-radius: 9999px;
					background-color: oklch(var(--primary));
					border: 2px solid oklch(var(--surface, 0.97 0 0));
					box-shadow: 0 4px 6px -1px rgb(0 0 0 / 0.3);
					cursor: pointer;
					transition: all 0.2s ease;
				}
				.level-slider::-moz-range-thumb:hover {
					background-color: oklch(var(--accent));
				}
				.level-slider::-moz-range-thumb:active {
					transform: scale(1.1);
				}`,
        },
    ],
    shortcuts: {
        // Page scaffolding. Every catalog page is a title, an optional filter
        // bar and one grid; every detail page is a back link inside the same
        // shell. Keeping those as shortcuts means a page cannot drift into its
        // own spacing or breakpoints by copy-paste.
        "page-title": "text-4xl font-bold my-4 text-center",
        "page-shell": "max-w-screen-2xl mx-auto px-8 py-8",
        "section-title": "text-xl font-headline font-bold text-primary mb-4",
        "back-link":
            "text-sm text-foreground-muted hover:text-primary transition-colors",
        "admin-actions": "flex justify-end px-8 max-w-screen-2xl mx-auto",
        "filter-bar": "flex flex-wrap items-center justify-center gap-3 my-6 px-8",
        "filter-input":
            "bg-surface border-2 border-secondary/50 focus:border-primary focus:ring-0 rounded-full py-2 pl-4 pr-6 w-72 transition-all outline-none text-sm text-foreground",

        // Catalog grids: one ladder for icon cards, one for the wider episode
        // cards. Nothing else should define its own column counts.
        "catalog-grid":
            "grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 gap-4 p-8 max-w-screen-2xl mx-auto",
        "catalog-grid-wide":
            "grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4 p-8 max-w-screen-2xl mx-auto",
        "card-grid":
            "grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 gap-4",

        // Cards. entity-card is the one card shell; -sm is the denser variant
        // used inside a detail page's sub-grids.
        "entity-card":
            "rounded-2xl border-2 border-primary/20 bg-surface/50 p-4 flex flex-col items-center gap-2 hover:border-primary transition-colors",
        "entity-card-sm":
            "rounded-2xl border-2 border-primary/20 bg-surface/50 p-3 flex flex-col items-center gap-1 hover:border-primary transition-colors",
        "card-title": "text-center text-sm font-bold text-on-surface leading-tight",
        "card-title-sm": "text-center text-xs font-bold text-on-surface leading-tight",
        "card-note": "text-center text-xs text-foreground-muted leading-snug",
        "card-meta": "text-[10px] font-label uppercase text-foreground-muted",
        "pill-accent":
            "text-[10px] font-bold text-accent uppercase border border-accent/40 rounded-full px-2 py-0.5",
        "pill-primary":
            "text-[10px] font-bold text-primary uppercase border border-primary/40 rounded-full px-2 py-0.5",

        btn: "inline-flex items-center justify-center px-4 py-2 rounded transition-colors duration-300",
        "btn-primary": "btn bg-primary text-on-primary hover:bg-primary/90",
        "btn-secondary": "btn bg-secondary text-on-secondary hover:bg-secondary/90",
        "btn-success": "btn bg-success text-on-success hover:bg-success/90",
        "btn-warning": "btn bg-warning text-on-warning hover:bg-warning/90",
        "btn-error": "btn bg-error text-on-error hover:bg-error/90",
        "index-card":
            "p-6 rounded-xl flex flex-col gap-4 cursor-pointer h-full bg-surface/25 backdrop-blur-md hover:-translate-y-1 hover:border-primary transition-all border border-accent/30",

        // The build planner's picker modals (templates/components/
        // picker_dialog_*.html). One shell for all three so a dialog cannot
        // drift into its own size or transition.
        "picker-dialog":
            "relative m-auto p-6 w-[min(80rem,95vw)] max-h-[85vh] hidden open:flex flex-col bg-surface text-foreground rounded-xl border border-accent/30 shadow-2xl transition-discrete transition-all duration-300 opacity-0 open:starting:opacity-0 open:opacity-100 backdrop:transition-all backdrop:transition-discrete backdrop:opacity-0 backdrop:bg-black backdrop:open:opacity-40 backdrop:open:backdrop-blur-sm backdrop:open:blur-sm backdrop:open:starting:opacity-0",
        "picker-search":
            "w-full shrink-0 bg-surface border-2 border-primary/30 rounded-xl py-2 px-3 text-foreground mb-4",
        "picker-tab":
            "state-tab rounded-full px-4 py-2 font-bold transition-colors border-2 border-secondary/50 text-secondary",
        "picker-tab-on":
            "state-tab active rounded-full px-4 py-2 font-bold transition-colors bg-primary text-foreground",
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
            "on-primary": "oklch(var(--on-primary, 0.96 0 0))",
            "on-secondary": "oklch(var(--on-secondary, 0.96 0 0))",
            "on-accent": "oklch(var(--on-accent, 0.96 0 0))",
            "on-success": "oklch(var(--on-success, 0.96 0 0))",
            "on-warning": "oklch(var(--on-warning, 0.96 0 0))",
            "on-error": "oklch(var(--on-error, 0.96 0 0))",
            "on-surface": "oklch(var(--on-surface, 0.96 0 0))",
            "on-muted": "oklch(var(--on-muted, 0.96 0 0))",
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
