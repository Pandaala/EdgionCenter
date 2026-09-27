#!/usr/bin/env bash
# Shared isolation for native integration runners. Callers provide log and WORK_DIR.

cleanup_owned_runtime() {
  log "Stopping this run's child processes..."
  local pid attempt
  for pid in $(jobs -pr); do kill -TERM "$pid" 2>/dev/null || true; done
  for attempt in 1 2 3 4 5; do
    [[ -z "$(jobs -pr)" ]] && break
    sleep 1
  done
  for pid in $(jobs -pr); do kill -KILL "$pid" 2>/dev/null || true; done
  wait 2>/dev/null || true
  if [[ -n "$WORK_DIR" && -d "$WORK_DIR" ]]; then
    log "Run artifacts retained: $WORK_DIR"
  fi
}

assert_runtime_ports_free() {
  python3 - "$@" <<'PYPORTS'
import socket, sys
sockets = []
try:
    for port in sys.argv[1:]:
        # macOS may permit a wildcard bind beside a specific-address listener
        # when address reuse is enabled. Check the actual loopback endpoint too.
        with socket.socket() as probe:
            probe.settimeout(0.2)
            if probe.connect_ex(('127.0.0.1', int(port))) == 0:
                raise OSError('loopback endpoint already accepts connections')
        sock = socket.socket()
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        sockets.append(sock)
        sock.bind(('0.0.0.0', int(port)))
        sock.listen(1)
except OSError as error:
    sys.exit(f'Port {port} unavailable; no existing process was stopped: {error}')
finally:
    for sock in sockets:
        sock.close()
PYPORTS
}
