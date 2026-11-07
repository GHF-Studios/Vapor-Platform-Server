# Vapor Public Homepage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the root homepage into a concise introduction that sends visitors to the existing authoritative setup flow.

**Architecture:** Keep the implementation in the Homepage service's existing Axum route and static HTML response. Do not duplicate the dynamic release handling or setup content from `/downloads`.

**Tech Stack:** Rust, Axum, semantic HTML, inline CSS.

**Spec:** `docs/superpowers/specs/2026-11-07-public-homepage.md`

## Global Constraints

- Homepage service owns public product and legal copy; Platform owns routing.
- Keep the service stateless and independently rebuildable.
- `/downloads` remains authoritative for installer status and setup.
- Do not include proprietary game files or invite users to upload them.

## Review Focus

- Primary CTA routes to `/downloads` rather than a nonexistent release asset.
- Copy does not imply a stable release is currently available.
- Layout remains usable on narrow viewports and with keyboard navigation.
- The homepage introduces no external asset, script, or service dependency.

## Tasks

### Task 1: Replace the link-only homepage

**Files:** `Vapor-Homepage-Server/src/lib.rs`

- [x] Replace only `home_page()` content with responsive semantic markup and
  styles consistent with `/downloads`.
- [x] Include one prominent `/downloads` CTA, short platform description,
  local-content boundary, and links to Docs, License, and Impressum.
- [x] Keep other routes and service ownership unchanged.
- [x] Run `cargo check --locked` for the Homepage crate and inspect the diff.
- [x] Commit the Homepage service change, then update the Platform submodule pin
  and record the public-page change in the deployment status note.
