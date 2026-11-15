# ADR-0001 — Dioxus all-Rust Web/Desktop

Date: 2026-11-14 • Status: Accepted for starter

**Context**: Browser and Desktop app should share frontend logic and Rust stack. Maintaining React + Tauri or Leptos + Tauri increases integration/code duplication for this goal.

**Decision**: Dioxus 0.7.x; one RSX UI codebase. Web compiled to WASM; desktop compiled native and rendered by OS WebView. Axum service for server-hosted/Web use cases. CSS allowed.

**Consequences**: Strong source reuse; ecosystem learning curve; verify runtime/CLI patch compatibility and WebView OS dependencies before release. This is not pixel-native desktop UI, but Rust native application logic and system WebView presentation.

**Alternatives**: Leptos CSR + Tauri 2; React + Tauri 2; Electron; native-only Iced. Reconsider only with evidence and a new ADR.
