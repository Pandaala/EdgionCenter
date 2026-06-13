---
name: center-testing
description: Test guidance for EdgionCenter. Center-specific notes here; the shared framework lives upstream.
---

# 05 Testing

Test guidance for EdgionCenter.

## Running tests

TODO: document Center's test entry points (`cargo test -p edgion-center`, integration
harness location). Capture commands as they stabilize.

## Center-specific scenarios

TODO: federation sync (register → reverse Watch), aggregator merge correctness, controller
offline/online transitions, CenterDb persistence.

## External dependency — Edgion testing framework

Unit/integration testing patterns are shared and defined upstream:
https://github.com/Pandaala/Edgion/tree/main/skills/05-testing
