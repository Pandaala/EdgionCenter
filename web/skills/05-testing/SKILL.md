---
name: dashboard-testing
description: Edgion Center testing guide — starting the backend, loading data, development verification workflow
---

# Testing Guide

## Starting the Backend Test Environment

Frontend development requires the backend API to return real data. Use Edgion's integration test infrastructure to start it.

### Option 1: One-command Startup (Recommended)

```bash
# in the edgion project directory
cd /Users/caohao/ws2/edgion

# start Controller + Gateway and load all test data
./examples/test/scripts/utils/start_all_with_conf.sh

# or load data manually after startup
./examples/test/scripts/utils/start_all_with_conf.sh --no-load
./examples/test/scripts/utils/load_conf.sh all          # load all
./examples/test/scripts/utils/load_conf.sh http          # load HTTPRoute only
```

### Option 2: Manual Startup

```bash
# Run from the EdgionCenter repo root

# 1. Build
cargo build -p edgion-center-standalone

# 2. Start Center backend (Center Admin API on :12201)
cargo run -p edgion-center-standalone -- --config-file config/edgion-center.yaml
```

## Test Ports

| Service | Port | Purpose |
|------|------|------|
| Center Admin API | 12201 | Frontend API backend (Vite proxy target) |
| Controller gRPC | 12151 | Gateway config sync |
| Gateway HTTP | 10080 | Data-plane HTTP proxy |
| Gateway HTTPS | 10443 | Data-plane HTTPS proxy |
| Gateway Admin | 12001 | Gateway management API |
| Frontend Dev Server | 5173 | Vite dev server |

## Verification Workflow

1. **Start backend**: `start_all_with_conf.sh`
2. **Load data**: `load_conf.sh all`
3. **Start frontend**: `cd edgion-dashboard && npm run dev`
4. **Browser check**: http://localhost:5173
5. **Verify API**: http://localhost:12201/api/v1/namespaced/httproute (direct API test)

## Test Data Directory

```
edgion/examples/test/conf/
├── base/                  # GatewayClass, Gateway, EdgionGatewayConfig, TLS secrets
├── HTTPRoute/             # HTTPRoute test cases
├── GRPCRoute/             # GRPCRoute test cases
├── TCPRoute/              # TCPRoute test cases
├── UDPRoute/              # UDPRoute test cases
├── TLSRoute/              # TLSRoute test cases
├── EdgionPlugins/         # plugin test cases
├── EdgionTls/             # TLS config test cases
├── Status/                # status update tests
└── LinkSys/               # external integration tests
```

## Verification Checklist

After completing a new page:
- [ ] List page loads correctly and displays test data
- [ ] Search/filter works correctly
- [ ] Create new resource (Form mode)
- [ ] Create new resource (YAML mode)
- [ ] View resource details
- [ ] Edit resource (Form mode)
- [ ] Edit resource (YAML mode)
- [ ] Delete a single resource
- [ ] Batch delete resources
- [ ] Refresh button works
- [ ] Sidebar navigation highlight is correct
- [ ] No TypeScript compilation errors
- [ ] No console errors
