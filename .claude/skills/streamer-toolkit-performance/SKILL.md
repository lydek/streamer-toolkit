---
name: streamer-toolkit-performance
description: Review and optimize Vue 3, Tauri WebView and Rust desktop resource usage in Streamer Toolkit. Use for memory leaks, long-running sessions, event rendering, module lifecycle, profiling and performance regression tests.
---

# Streamer Toolkit Performance

## Goals

Prioritize bounded memory, low idle CPU/GPU usage, predictable latency and long-running stability. Preserve Vue 3, existing backend contracts, module boundaries and data integrity.

## Workflow

1. Read project architecture, frontend conventions and applicable performance requirements.
2. Establish a reproducible Release/production-build baseline before modifying code.
3. Identify bottlenecks with static review and profiling. Distinguish confirmed findings from hypotheses.
4. Prefer minimal, measurable, reversible changes. Do not introduce dependencies or redesign architecture without justification and required approval.
5. Run relevant correctness tests and repeat the same benchmark after changes.
6. Report before/after results, trade-offs, remaining risks and untested scenarios.

## Vue 3 review rules

- Bound growing arrays, logs, event queues, caches and subscriptions by count and/or bytes; define overflow behavior.
- Keep complete historical records in backend storage; paginate or stream them on demand. Do not silently discard durable business events.
- Use virtual scrolling for large lists where measurements justify it; separately bound retained data.
- Prefer shallowRef/markRaw for large immutable or non-reactive data when semantics permit. Avoid blanket conversion and unnecessary deep watchers.
- Batch high-frequency UI updates while preserving ordering and required real-time triggers.
- Subscribe only to relevant WebSocket topics; unsubscribe and cancel work on unmount/deactivation. Handle reconnects without duplicate subscriptions.
- Clean up timers, DOM listeners, observers, animation frames, requests, Blob URLs, audio and GPU resources. Check stale async callbacks and detached DOM.
- Review v-if, v-show, KeepAlive, Pinia state, lazy imports and route/module lifecycle. Do not assume unmount releases imported modules or global stores.
- Avoid excessive cloning, serialization, deep reactive traversal and unnecessary component renders.
- Pause unnecessary background rendering and animation when hidden; preserve intended OBS Overlay behavior and audio/effect timing.
- Keep admin UI and OBS Overlay entry points/resource requirements distinct where appropriate.

## Desktop constraints

- Measure Rust Core and WebView process tree separately and together.
- Closing/hiding a window is not proof of WebView resource release. Verify actual window destruction and process/resource behavior.
- Track JavaScript heap, DOM nodes, listeners, private bytes, private working set, CPU, GPU and VRAM where available.
- Do not optimize memory at the expense of significant CPU/GPU usage, latency, correctness or accessibility without documented trade-offs.
- Browser-only measurements are not equivalent to Tauri WebView2 or OBS Browser Source measurements.

## Regression scenarios

- Idle for 30 minutes, module switch 100 times, repeated connect/disconnect and window open/close.
- Replay a deterministic synthetic event stream; measure queue growth and retained objects.
- Run 8–24 hour soak tests when requested and feasible; do not claim completion before tests actually finish.
- Compare heap snapshots at comparable lifecycle points; distinguish leaks from allocator/cache warm-up.
- Verify event ordering, persistence, module isolation, UI functionality and resource cleanup.
- Use synthetic data; never send production chat logs, credentials or personal data to external tools.

## Reporting

For each finding include severity, evidence, source location, proposed change, expected impact and actual benchmark results if available. Label unmeasured estimates explicitly. Report failures and regressions. Avoid arbitrary memory targets unless agreed upon and backed by baseline data.
